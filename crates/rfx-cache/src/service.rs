//! C-09, C-48 : le fil `RFX-IO`, qui detient la base, la relit et l'ecrit.
//!
//! Cahier des charges : PARTIE 5.9 (R-360 a R-362), PARTIE 5.46 (R-690 : jamais sur le
//! fil autoritatif ; R-692 : un echec desactive la persistance, le runtime continue),
//! R-361 (ecriture asynchrone, hors tick, atomique). Tests : T-184, T-500, T-502.
//! Decisions : ADR-048, ADR-049. Maturite : `STABLE`.
//!
//! # Ce que fait l'appelant, et rien de plus
//!
//! La base vit sur ce fil. L'appelant — le runtime, sous son verrou — ne lui confie que
//! des lots ([`SyncBatch`]) : les unites de travail decouvertes depuis le lot precedent
//! et les dynamiques qui ont change. Il ne copie jamais la base entiere : cette copie,
//! faite sous le verrou que le tick partage, le ferait attendre.
//!
//! La file est bornee et ne fait jamais attendre : pleine, elle rend le lot a
//! l'appelant ([`Rejected`]), qui le garde pour le cycle suivant.
//!
//! # Au demarrage
//!
//! Le fil relit d'abord le fichier ([`crate::store::load`]), puis sert les lots. Le jeu
//! n'attend pas cette relecture : un lot confie entre-temps attend son tour dans la file.
//!
//! # Quand le disque refuse
//!
//! Au premier echec d'ecriture — disque plein, permission —, l'ecriture s'arrete pour de
//! bon (R-692, FM-18). Les lots continuent d'etre appliques a la base en memoire : le
//! runtime ne voit aucune difference, sinon que rien n'est plus ecrit. Un echec de
//! rotation est compte mais ne desactive rien : un journal verrouille par un autre
//! programme ne doit pas arreter l'ecriture de la base.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use rfx_model::{Fingerprint, WorkId, WorkloadDynamics};

use crate::db::WorkloadDb;
use crate::file;
use crate::fs::FileSystem;
use crate::record::{HistoryEvent, HistoryKind, WorkloadRecord};
use crate::rotation::{self, RotationPolicy};
use crate::store::{self, LoadOutcome};

/// Nom du fil. Il commence par `RFX-` : le capteur de charge l'exclut.
pub const THREAD_NAME: &str = "RFX-IO";

/// Attente au plus d'une fermeture par `Drop` : un disque qui ne repond plus ne doit pas
/// bloquer l'arret du jeu.
pub const DROP_TIMEOUT: Duration = Duration::from_secs(10);

/// Demandes en attente au plus. Au-dela, une demande est refusee : l'appelant n'attend
/// jamais le disque.
pub const QUEUE_CAPACITY: usize = 4;

/// Ce que le fil doit savoir de la base qu'il detient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbConfig {
    /// Chemin de `workload.db`.
    pub path: PathBuf,
    /// Empreinte de l'environnement courant (R-360).
    pub environment: Fingerprint,
    /// Unites de travail gardees au plus (R-362, `max_workloads`).
    pub max_entries: usize,
    /// Octets estimes gardes au plus (R-362, `cache.max_memory_mb`).
    pub max_bytes: usize,
}

/// Nouvelles dynamiques d'une unite de travail.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DynamicsUpdate {
    /// Unite de travail.
    pub id: WorkId,
    /// Ses dynamiques, telles que le profileur les tient.
    pub dynamics: WorkloadDynamics,
}

/// Ce que le runtime confie au fil a chaque cycle.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncBatch {
    /// Unites de travail decouvertes depuis le lot precedent.
    pub new_records: Vec<WorkloadRecord>,
    /// Dynamiques qui ont change depuis le lot precedent.
    pub updates: Vec<DynamicsUpdate>,
    /// Nouveau plafond d'octets, si le budget memoire l'a change.
    pub max_bytes: Option<usize>,
}

impl SyncBatch {
    /// `true` si le lot n'apporte rien.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.new_records.is_empty() && self.updates.is_empty() && self.max_bytes.is_none()
    }
}

/// Pourquoi une demande n'a pas ete prise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// La file est pleine : le disque est en retard.
    QueueFull,
    /// Le fil est arrete.
    Stopped,
}

/// Un lot refuse, rendu a l'appelant pour qu'il le confie de nouveau plus tard.
#[derive(Debug, Clone, PartialEq)]
pub struct Rejected {
    /// Pourquoi.
    pub reason: Refused,
    /// Le lot, intact.
    pub batch: SyncBatch,
}

/// Issue de la relecture du demarrage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum LoadState {
    /// Pas encore terminee.
    #[default]
    Pending = 0,
    /// Aucun fichier : premiere execution.
    Missing = 1,
    /// Fichier d'un autre environnement, ignore (R-360).
    EnvironmentChanged = 2,
    /// Fichier illisible, mis de cote en `.bak` (R-223, FM-17).
    SetAside = 3,
    /// Fichier relu.
    Loaded = 4,
    /// Le systeme a refuse la lecture : la base part vide.
    Failed = 5,
}

impl LoadState {
    /// Code publie dans le statut.
    #[must_use]
    pub fn code(self) -> u8 {
        self as u8
    }

    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Missing,
            2 => Self::EnvironmentChanged,
            3 => Self::SetAside,
            4 => Self::Loaded,
            5 => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// Compteurs du fil.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DbStats {
    /// Issue de la relecture.
    pub load: LoadState,
    /// Unites de travail relues du fichier.
    pub loaded_entries: u64,
    /// Entrees ecartees a la relecture (`rfx.db.load_rejected_entries`).
    pub rejected_entries: u64,
    /// Unites de travail dans la base (`rfx.db.entries`).
    pub entries: u64,
    /// Memoire estimee de la base, en octets.
    pub estimated_bytes: u64,
    /// Unites de travail evincees (`rfx.db.evictions`).
    pub evictions: u64,
    /// Fichiers ecrits.
    pub writes: u64,
    /// Echecs d'ecriture (le premier desactive l'ecriture).
    pub write_failures: u64,
    /// Echecs de rotation.
    pub rotation_failures: u64,
    /// Fichiers supprimes par rotation.
    pub rotated: u64,
    /// Demandes refusees, file pleine.
    pub refused: u64,
    /// Duree du dernier encodage et de sa derniere ecriture, en ms (`rfx.db.persist_ms`).
    pub last_persist_ms: u64,
    /// `true` si l'ecriture est desactivee.
    pub disabled: bool,
}

enum Request {
    Sync(SyncBatch),
    Rotate {
        dir: PathBuf,
        policy: RotationPolicy,
    },
    Flush(mpsc::Sender<()>),
}

#[derive(Default)]
struct State {
    load: AtomicU8,
    loaded_entries: AtomicU64,
    rejected_entries: AtomicU64,
    entries: AtomicU64,
    estimated_bytes: AtomicU64,
    evictions: AtomicU64,
    writes: AtomicU64,
    write_failures: AtomicU64,
    rotation_failures: AtomicU64,
    rotated: AtomicU64,
    refused: AtomicU64,
    last_persist_ms: AtomicU64,
    disabled: AtomicBool,
    last_error: Mutex<Option<String>>,
}

impl State {
    fn note_error(&self, error: &io::Error) {
        *self
            .last_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(error.to_string());
    }

    fn publish(&self, db: &WorkloadDb) {
        self.entries.store(db.len() as u64, Ordering::Relaxed);
        self.estimated_bytes
            .store(db.estimated_bytes() as u64, Ordering::Relaxed);
        self.evictions.store(db.evictions(), Ordering::Relaxed);
    }
}

/// Le fil `RFX-IO` et la base qu'il detient.
pub struct DbService {
    sender: Option<SyncSender<Request>>,
    thread: Option<JoinHandle<()>>,
    state: Arc<State>,
}

impl std::fmt::Debug for DbService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbService")
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

impl DbService {
    /// Demarre le fil, qui commence par relire `config.path`. Rend la main aussitot.
    ///
    /// # Errors
    ///
    /// L'erreur du systeme si le fil ne peut pas etre cree.
    pub fn start(fs: Arc<dyn FileSystem>, config: DbConfig) -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let state = Arc::new(State::default());
        let worker_state = Arc::clone(&state);
        let thread = std::thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || run(&receiver, fs.as_ref(), &config, &worker_state))?;
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
            state,
        })
    }

    fn submit(&self, request: Request) -> Result<(), (Refused, Request)> {
        let Some(sender) = &self.sender else {
            return Err((Refused::Stopped, request));
        };
        sender.try_send(request).map_err(|error| match error {
            TrySendError::Full(request) => {
                self.state.refused.fetch_add(1, Ordering::Relaxed);
                (Refused::QueueFull, request)
            }
            TrySendError::Disconnected(request) => (Refused::Stopped, request),
        })
    }

    /// Confie un lot. Rend la main aussitot (R-361).
    ///
    /// # Errors
    ///
    /// [`Rejected`], avec le lot intact, si la file est pleine ou le fil arrete.
    pub fn sync(&self, batch: SyncBatch) -> Result<(), Rejected> {
        self.submit(Request::Sync(batch))
            .map_err(|(reason, request)| match request {
                Request::Sync(batch) => Rejected { reason, batch },
                _ => Rejected {
                    reason,
                    batch: SyncBatch::default(),
                },
            })
    }

    /// Confie la rotation de `dir` selon `policy` (R-691). Jamais sur le repertoire de
    /// `workload.db` : la taille y ferait partir la base elle-meme.
    ///
    /// # Errors
    ///
    /// [`Refused`] si la file est pleine ou le fil arrete.
    pub fn rotate(&self, dir: impl Into<PathBuf>, policy: RotationPolicy) -> Result<(), Refused> {
        self.submit(Request::Rotate {
            dir: dir.into(),
            policy,
        })
        .map_err(|(reason, _)| reason)
    }

    /// Attend, au plus `timeout`, que tout ce qui a ete confie avant cet appel soit
    /// traite, relecture du demarrage comprise. Pour l'arret et les tests : jamais
    /// depuis le fil autoritatif, ni sous le verrou du runtime.
    pub fn flush(&self, timeout: Duration) -> bool {
        let (done, wait) = mpsc::channel();
        let Some(sender) = &self.sender else {
            return true;
        };
        // La demande d'attente ne doit pas etre perdue : on reessaie tant que la file
        // est pleine, jusqu'au delai.
        let deadline = Instant::now() + timeout;
        let mut request = Request::Flush(done);
        loop {
            match sender.try_send(request) {
                Ok(()) => break,
                Err(TrySendError::Disconnected(_)) => return true,
                Err(TrySendError::Full(back)) => {
                    if Instant::now() >= deadline {
                        return false;
                    }
                    request = back;
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        wait.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .is_ok()
    }

    /// Ferme le fil : attend au plus `timeout` que tout ce qui a ete confie soit ecrit,
    /// puis le rejoint. Renvoie `false` si le delai est depasse : le fil est alors
    /// laisse a lui-meme, et l'arret du jeu n'attend pas un disque qui ne repond plus.
    pub fn close(mut self, timeout: Duration) -> bool {
        self.shut(timeout)
    }

    fn shut(&mut self, timeout: Duration) -> bool {
        let drained = self.flush(timeout);
        self.sender = None;
        if let Some(thread) = self.thread.take() {
            if drained {
                let _ = thread.join();
            }
        }
        drained
    }

    /// Compteurs.
    #[must_use]
    pub fn stats(&self) -> DbStats {
        let s = &self.state;
        let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        DbStats {
            load: LoadState::from_code(s.load.load(Ordering::Acquire)),
            loaded_entries: read(&s.loaded_entries),
            rejected_entries: read(&s.rejected_entries),
            entries: read(&s.entries),
            estimated_bytes: read(&s.estimated_bytes),
            evictions: read(&s.evictions),
            writes: read(&s.writes),
            write_failures: read(&s.write_failures),
            rotation_failures: read(&s.rotation_failures),
            rotated: read(&s.rotated),
            refused: read(&s.refused),
            last_persist_ms: read(&s.last_persist_ms),
            disabled: s.disabled.load(Ordering::Acquire),
        }
    }

    /// Derniere erreur du disque, pour le diagnostic.
    #[must_use]
    pub fn last_error(&self) -> Option<String> {
        self.state
            .last_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Drop for DbService {
    /// Ferme comme [`DbService::close`], avec [`DROP_TIMEOUT`].
    fn drop(&mut self) {
        self.shut(DROP_TIMEOUT);
    }
}

fn load_db(fs: &dyn FileSystem, config: &DbConfig, state: &State) -> WorkloadDb {
    // Les empreintes courantes des unites de travail ne sont pas encore connues : le
    // filtre par entree se fera a leur observation (`WorkloadDb::observe`).
    let (load, db) = match store::load(fs, &config.path, &config.environment, |_| None) {
        Ok(LoadOutcome::Missing) => (LoadState::Missing, WorkloadDb::new()),
        Ok(LoadOutcome::EnvironmentChanged) => (LoadState::EnvironmentChanged, WorkloadDb::new()),
        Ok(LoadOutcome::SetAside { .. }) => (LoadState::SetAside, WorkloadDb::new()),
        Ok(LoadOutcome::Loaded {
            db,
            rejected_entries,
        }) => {
            state
                .rejected_entries
                .store(rejected_entries as u64, Ordering::Relaxed);
            state
                .loaded_entries
                .store(db.len() as u64, Ordering::Relaxed);
            (LoadState::Loaded, db)
        }
        Err(error) => {
            state.note_error(&error);
            (LoadState::Failed, WorkloadDb::new())
        }
    };
    state.publish(&db);
    state.load.store(load.code(), Ordering::Release);
    db
}

/// Applique un lot. Renvoie `true` si la base a change.
fn apply(db: &mut WorkloadDb, batch: SyncBatch) -> bool {
    let changed = !batch.new_records.is_empty() || !batch.updates.is_empty();
    for record in batch.new_records {
        db.observe(record);
    }
    for update in batch.updates {
        let Some(record) = db.get_mut(update.id) else {
            continue;
        };
        let (from, to) = (record.dynamics.heat, update.dynamics.heat);
        if from != to {
            record.history.push(HistoryEvent {
                tick: update.dynamics.last_seen_tick,
                kind: HistoryKind::HeatChanged { from, to },
            });
        }
        record.dynamics = update.dynamics;
    }
    changed
}

fn persist(fs: &dyn FileSystem, config: &DbConfig, db: &WorkloadDb, state: &State) {
    let started = Instant::now();
    let written = file::encode(&config.environment, db.iter())
        .map_err(io::Error::other)
        .and_then(|bytes| fs.write_atomic(&config.path, &bytes));
    match written {
        Ok(()) => {
            state.writes.fetch_add(1, Ordering::Relaxed);
            let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            state.last_persist_ms.store(ms, Ordering::Relaxed);
        }
        Err(error) => {
            // R-692 : l'ecriture s'arrete, la base continue en memoire.
            state.write_failures.fetch_add(1, Ordering::Relaxed);
            state.note_error(&error);
            state.disabled.store(true, Ordering::Release);
        }
    }
}

fn run(receiver: &Receiver<Request>, fs: &dyn FileSystem, config: &DbConfig, state: &State) {
    let mut db = load_db(fs, config, state);
    let mut max_bytes = config.max_bytes;
    let mut dirty = false;
    let write_if_dirty = |db: &WorkloadDb, dirty: &mut bool| {
        if *dirty && !state.disabled.load(Ordering::Acquire) {
            persist(fs, config, db, state);
        }
        *dirty = false;
    };
    for request in receiver {
        match request {
            Request::Flush(done) => {
                let _ = done.send(());
            }
            Request::Sync(batch) => {
                if let Some(bytes) = batch.max_bytes {
                    max_bytes = bytes;
                }
                dirty |= apply(&mut db, batch);
                dirty |= db.evict(config.max_entries, max_bytes) > 0;
                state.publish(&db);
                write_if_dirty(&db, &mut dirty);
            }
            Request::Rotate { dir, policy } => {
                match rotation::rotate(fs, &dir, &policy, SystemTime::now()) {
                    Ok(report) => {
                        state
                            .rotated
                            .fetch_add(report.removed as u64, Ordering::Relaxed);
                    }
                    Err(error) => {
                        state.rotation_failures.fetch_add(1, Ordering::Relaxed);
                        state.note_error(&error);
                    }
                }
            }
        }
    }
    write_if_dirty(&db, &mut dirty);
}
