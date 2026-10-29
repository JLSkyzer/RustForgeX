//! C-09, C-48 : le service qui detient la base, la relit et l'ecrit.
//!
//! Cahier des charges : PARTIE 5.9 (R-360 a R-362), PARTIE 5.46 (R-690 : jamais sur le
//! fil autoritatif ; R-692 : un echec desactive la persistance, le runtime continue),
//! R-361 (ecriture asynchrone, hors tick, atomique). Tests : T-184, T-500, T-502.
//! Decisions : ADR-048, ADR-049, ADR-061. Maturite : `STABLE`.
//!
//! # Qui l'execute
//!
//! Un [`Executor`] : en jeu, l'ordonnanceur (taches `BACKGROUND`) ; a defaut, un fil
//! dedie, `RFX-IO` ([`DbService::start`]). La base garde un seul detenteur : une seule
//! tache de vidage est en cours a la fois, qui sert les demandes dans l'ordre puis se
//! retire. Aucune demande n'est executee sur le fil qui la confie.
//!
//! # Ce que fait l'appelant, et rien de plus
//!
//! La base vit dans le service. L'appelant — le runtime, sous son verrou — ne lui confie que
//! des lots ([`SyncBatch`]) : les unites de travail decouvertes depuis le lot precedent
//! et les dynamiques qui ont change. Il ne copie jamais la base entiere : cette copie,
//! faite sous le verrou que le tick partage, le ferait attendre.
//!
//! La file est bornee et ne fait jamais attendre : pleine, elle rend le lot a
//! l'appelant ([`Rejected`]), qui le garde pour le cycle suivant.
//!
//! # Au demarrage
//!
//! La premiere tache relit d'abord le fichier ([`crate::store::load_with`], blocs decodes
//! en sous-travaux), puis sert les lots. Le jeu n'attend pas cette relecture : un lot
//! confie entre-temps attend son tour dans la file.
//!
//! # Quand le disque refuse
//!
//! Au premier echec d'ecriture — disque plein, permission —, l'ecriture s'arrete pour de
//! bon (R-692, FM-18). Les lots continuent d'etre appliques a la base en memoire : le
//! runtime ne voit aucune difference, sinon que rien n'est plus ecrit. Un echec de
//! rotation est compte mais ne desactive rien : un journal verrouille par un autre
//! programme ne doit pas arreter l'ecriture de la base.

use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use rfx_model::{Fingerprint, PersistedAnalysis, WorkId, WorkloadDynamics};

use crate::db::WorkloadDb;
use crate::exec::{Executor, Fork, InlineFork, ThreadExecutor};
use crate::file;
use crate::fs::FileSystem;
use crate::record::{HistoryEvent, HistoryKind, WorkloadRecord};
use crate::rotation::{self, RotationPolicy};
use crate::store::{self, LoadOutcome};

/// Nom du fil de secours. Il commence par `RFX-` : le capteur de charge l'exclut.
pub const THREAD_NAME: &str = "RFX-IO";

/// Attente au plus d'une fermeture par `Drop` : un disque qui ne repond plus ne doit pas
/// bloquer l'arret du jeu.
pub const DROP_TIMEOUT: Duration = Duration::from_secs(10);

/// Tranche d'attente de [`DbService::flush`] : entre deux, il verifie qu'une tache de
/// vidage est confiee, et rend la main si l'executeur les refuse.
const FLUSH_SLICE: Duration = Duration::from_millis(10);

/// Demandes en attente au plus, hors celle en cours. Au-dela, une demande est refusee :
/// l'appelant n'attend jamais le disque.
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

/// Faits d'analyse d'une unite de travail, nouveaux ou recalcules (ADR-064).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisUpdate {
    /// Unite de travail.
    pub id: WorkId,
    /// Ses faits ; `None` les efface (descripteur perime, a recalculer).
    pub analysis: Option<PersistedAnalysis>,
}

/// Ce que le runtime confie au fil a chaque cycle.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncBatch {
    /// Unites de travail decouvertes depuis le lot precedent.
    pub new_records: Vec<WorkloadRecord>,
    /// Dynamiques qui ont change depuis le lot precedent.
    pub updates: Vec<DynamicsUpdate>,
    /// Faits d'analyse qui ont change depuis le lot precedent (ADR-064).
    pub analyses: Vec<AnalysisUpdate>,
    /// Nouveau plafond d'octets, si le budget memoire l'a change.
    pub max_bytes: Option<usize>,
}

impl SyncBatch {
    /// `true` si le lot n'apporte rien.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.new_records.is_empty()
            && self.updates.is_empty()
            && self.analyses.is_empty()
            && self.max_bytes.is_none()
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
    /// Analyses relues au demarrage, a remettre une fois au runtime (ADR-064).
    restored: Mutex<Option<Vec<(WorkId, PersistedAnalysis)>>>,
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

/// Ce que la base garde entre deux taches de vidage.
struct Holder {
    db: Option<WorkloadDb>,
    max_bytes: usize,
}

/// Le service, partage avec la tache de vidage en cours.
struct Inner {
    executor: Arc<dyn Executor>,
    fs: Arc<dyn FileSystem>,
    config: DbConfig,
    state: State,
    queue: Mutex<VecDeque<Request>>,
    /// Une tache de vidage est confiee ou en cours.
    scheduled: AtomicBool,
    closed: AtomicBool,
    holder: Mutex<Holder>,
}

/// La base des unites de travail et le service qui la detient.
pub struct DbService {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for DbService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbService")
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

impl DbService {
    /// Demarre le service sur un fil dedie (`RFX-IO`) : le secours quand l'ordonnanceur
    /// n'a pas demarre. Il commence par relire `config.path`. Rend la main aussitot.
    ///
    /// # Errors
    ///
    /// L'erreur du systeme si le fil ne peut pas etre cree.
    pub fn start(fs: Arc<dyn FileSystem>, config: DbConfig) -> io::Result<Self> {
        let executor = ThreadExecutor::start(THREAD_NAME)?;
        Ok(Self::start_on(fs, config, Arc::new(executor)))
    }

    /// Demarre le service sur `executor` (ADR-061). Il commence par relire
    /// `config.path`. Rend la main aussitot ; si l'executeur refuse, la relecture attend
    /// la premiere demande.
    #[must_use]
    pub fn start_on(
        fs: Arc<dyn FileSystem>,
        config: DbConfig,
        executor: Arc<dyn Executor>,
    ) -> Self {
        let max_bytes = config.max_bytes;
        let inner = Arc::new(Inner {
            executor,
            fs,
            config,
            state: State::default(),
            queue: Mutex::new(VecDeque::with_capacity(QUEUE_CAPACITY)),
            scheduled: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            holder: Mutex::new(Holder {
                db: None,
                max_bytes,
            }),
        });
        schedule(&inner);
        Self { inner }
    }

    fn submit(&self, request: Request) -> Result<(), (Refused, Request)> {
        let inner = &self.inner;
        if inner.closed.load(Ordering::Acquire) {
            return Err((Refused::Stopped, request));
        }
        {
            let mut queue = inner.queue.lock().unwrap_or_else(PoisonError::into_inner);
            if queue.len() >= QUEUE_CAPACITY {
                inner.state.refused.fetch_add(1, Ordering::Relaxed);
                return Err((Refused::QueueFull, request));
            }
            queue.push_back(request);
        }
        schedule(inner);
        Ok(())
    }

    /// Confie un lot. Rend la main aussitot (R-361).
    ///
    /// # Errors
    ///
    /// [`Rejected`], avec le lot intact, si la file est pleine ou le service ferme.
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
    /// [`Refused`] si la file est pleine ou le service ferme.
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
        // La demande d'attente ne doit pas etre perdue : on reessaie tant que la file
        // est pleine, jusqu'au delai.
        let deadline = Instant::now() + timeout;
        let mut request = Request::Flush(done);
        loop {
            match self.submit(request) {
                Ok(()) => break,
                Err((Refused::Stopped, _)) => return true,
                Err((Refused::QueueFull, back)) => {
                    if Instant::now() >= deadline {
                        return false;
                    }
                    request = back;
                    schedule(&self.inner);
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        // Attente par tranches : si aucune tache de vidage n'est confiee et qu'on ne peut
        // pas en confier (executeur en serie ou arrete), inutile d'attendre le delai.
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match wait.recv_timeout(left.min(FLUSH_SLICE)) {
                Ok(()) => return true,
                Err(mpsc::RecvTimeoutError::Disconnected) => return false,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if Instant::now() >= deadline {
                return false;
            }
            schedule(&self.inner);
            if !self.inner.scheduled.load(Ordering::Acquire) {
                return false;
            }
        }
    }

    /// Ferme le service : attend au plus `timeout` que tout ce qui a ete confie soit
    /// ecrit. Si aucune tache n'a pu etre lancee entre-temps (executeur en serie ou
    /// arrete), ce qui reste est servi sur le fil appelant, qui n'est jamais le fil
    /// autoritatif ni sous le verrou du runtime. Renvoie `false` si le travail n'a pas pu
    /// etre fini : un disque qui ne repond plus ne bloque pas l'arret du jeu.
    pub fn close(self, timeout: Duration) -> bool {
        self.shut(timeout)
    }

    fn shut(&self, timeout: Duration) -> bool {
        let inner = &self.inner;
        if inner.closed.load(Ordering::Acquire) {
            return true;
        }
        let drained = self.flush(timeout);
        inner.closed.store(true, Ordering::Release);
        if drained {
            return true;
        }
        // Une tache en cours, peut-etre bloquee sur le disque, garde la base : on ne
        // l'attend pas.
        let Ok(mut holder) = inner.holder.try_lock() else {
            return false;
        };
        serve(inner, &mut holder, &InlineFork);
        drop(holder);
        inner
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    }

    /// Compteurs.
    #[must_use]
    pub fn stats(&self) -> DbStats {
        let s = &self.inner.state;
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
        self.inner
            .state
            .last_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Analyses relues au demarrage, rendues une seule fois (ADR-064) ; `None` tant que
    /// la relecture n'est pas terminee, ou une fois rendues.
    #[must_use]
    pub fn take_restored(&self) -> Option<Vec<(WorkId, PersistedAnalysis)>> {
        self.inner
            .state
            .restored
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

impl Drop for DbService {
    /// Ferme comme [`DbService::close`], avec [`DROP_TIMEOUT`].
    fn drop(&mut self) {
        self.shut(DROP_TIMEOUT);
    }
}

/// Confie une tache de vidage, s'il n'y en a pas deja une. Refusee, la demande reste en
/// file : la prochaine demande, ou la fermeture, relance le vidage.
fn schedule(inner: &Arc<Inner>) {
    if inner
        .scheduled
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let task = Arc::clone(inner);
    if inner
        .executor
        .execute(Box::new(move |fork: &dyn Fork| drain(&task, fork)))
        .is_err()
    {
        inner.scheduled.store(false, Ordering::Release);
    }
}

/// La tache de vidage : sert la file dans l'ordre, puis se retire. Une demande arrivee
/// pendant le retrait est servie par la meme tache.
fn drain(inner: &Arc<Inner>, fork: &dyn Fork) {
    /// Libere le drapeau meme si le vidage panique : sinon plus aucune tache ne serait
    /// jamais confiee, et la base ne serait plus ecrite.
    struct Release<'a>(&'a AtomicBool);
    impl Drop for Release<'_> {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    loop {
        {
            let _release = Release(&inner.scheduled);
            let mut holder = inner.holder.lock().unwrap_or_else(PoisonError::into_inner);
            serve(inner, &mut holder, fork);
        }
        let waiting = !inner
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty();
        if !waiting
            || inner
                .scheduled
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }
    }
}

/// Relit la base si ce n'est pas fait, puis sert la file jusqu'a la vider.
fn serve(inner: &Inner, holder: &mut Holder, fork: &dyn Fork) {
    let db = holder
        .db
        .get_or_insert_with(|| load_db(inner.fs.as_ref(), &inner.config, &inner.state, fork));
    loop {
        let request = inner
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front();
        let Some(request) = request else {
            return;
        };
        handle(inner, db, &mut holder.max_bytes, request);
    }
}

/// Une demande.
fn handle(inner: &Inner, db: &mut WorkloadDb, max_bytes: &mut usize, request: Request) {
    let (fs, config, state) = (inner.fs.as_ref(), &inner.config, &inner.state);
    match request {
        Request::Flush(done) => {
            let _ = done.send(());
        }
        Request::Sync(batch) => {
            if let Some(bytes) = batch.max_bytes {
                *max_bytes = bytes;
            }
            let mut dirty = apply(db, batch);
            dirty |= db.evict(config.max_entries, *max_bytes) > 0;
            state.publish(db);
            if dirty && !state.disabled.load(Ordering::Acquire) {
                persist(fs, config, db, state);
            }
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

fn load_db(fs: &dyn FileSystem, config: &DbConfig, state: &State, fork: &dyn Fork) -> WorkloadDb {
    // Les empreintes courantes des unites de travail ne sont pas encore connues : le
    // filtre par entree se fera a leur observation (`WorkloadDb::observe`).
    let (load, db) = match store::load_with(fs, &config.path, &config.environment, |_| None, fork) {
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
    let restored: Vec<(WorkId, PersistedAnalysis)> = db
        .iter()
        .filter_map(|record| {
            record
                .analysis
                .clone()
                .map(|analysis| (record.id, analysis))
        })
        .collect();
    *state
        .restored
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(restored);
    state.load.store(load.code(), Ordering::Release);
    db
}

/// Applique un lot. Renvoie `true` si la base a change.
fn apply(db: &mut WorkloadDb, batch: SyncBatch) -> bool {
    let mut changed = !batch.new_records.is_empty() || !batch.updates.is_empty();
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
    // Une unite evincee ou inconnue de la base n'a pas d'enregistrement ou ranger ses
    // faits : ils sont perdus, l'unite sera analysee de nouveau.
    for update in batch.analyses {
        if let Some(record) = db.get_mut(update.id) {
            if record.analysis != update.analysis {
                record.analysis = update.analysis;
                changed = true;
            }
        }
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
