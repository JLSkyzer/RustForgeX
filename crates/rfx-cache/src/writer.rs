//! C-48 : le fil d'ecriture de la persistance.
//!
//! Cahier des charges : PARTIE 5.46 (R-690 : jamais sur le fil autoritatif ; R-692 :
//! un echec desactive la persistance, le runtime continue), R-361 (ecriture
//! asynchrone, hors tick, atomique). Tests : T-184, T-500, T-502. Decision : ADR-048.
//! Maturite : `STABLE`.
//!
//! # Ce que fait l'appelant, et rien de plus
//!
//! Il confie une copie des enregistrements a une file bornee, sans attendre : si la
//! file est pleine, la demande est refusee et comptee, plutot que d'attendre le disque.
//! L'encodage, l'ecriture atomique et la rotation ont lieu sur le fil `RFX-IO`.
//!
//! # Quand le disque refuse
//!
//! Au premier echec d'ecriture — disque plein, permission —, la persistance se
//! desactive : les demandes suivantes sont refusees sans toucher au disque, et le
//! runtime continue en memoire (R-692, FM-18). Un echec de rotation est compte mais ne
//! desactive rien : un fichier verrouille par un autre programme ne doit pas arreter
//! l'ecriture de la base.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use rfx_model::Fingerprint;

use crate::file;
use crate::fs::FileSystem;
use crate::record::WorkloadRecord;
use crate::rotation::{self, RotationPolicy};

/// Nom du fil d'ecriture. Il commence par `RFX-` : le capteur de charge l'exclut.
pub const THREAD_NAME: &str = "RFX-IO";

/// Demandes en attente au plus. Au-dela, une demande est refusee : l'appelant n'attend
/// jamais le disque.
pub const QUEUE_CAPACITY: usize = 4;

/// Pourquoi une demande n'a pas ete prise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// La persistance est desactivee apres un echec d'ecriture (R-692).
    Disabled,
    /// La file est pleine : le disque est en retard.
    QueueFull,
}

/// Compteurs du fil d'ecriture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriterStats {
    /// Fichiers ecrits.
    pub writes: u64,
    /// Echecs d'ecriture (le premier desactive la persistance).
    pub write_failures: u64,
    /// Echecs de rotation.
    pub rotation_failures: u64,
    /// Fichiers supprimes par rotation.
    pub rotated: u64,
    /// Demandes refusees : file pleine, ou persistance desactivee.
    pub refused: u64,
    /// Duree du dernier encodage et de sa derniere ecriture, en ms (`rfx.db.persist_ms`).
    pub last_persist_ms: u64,
    /// `true` si la persistance est desactivee.
    pub disabled: bool,
}

enum Request {
    Persist {
        path: PathBuf,
        fingerprint: Fingerprint,
        records: Vec<WorkloadRecord>,
    },
    Rotate {
        dir: PathBuf,
        policy: RotationPolicy,
    },
    Flush(mpsc::Sender<()>),
}

#[derive(Default)]
struct State {
    disabled: AtomicBool,
    writes: AtomicU64,
    write_failures: AtomicU64,
    rotation_failures: AtomicU64,
    rotated: AtomicU64,
    refused: AtomicU64,
    last_persist_ms: AtomicU64,
    last_error: Mutex<Option<String>>,
}

impl State {
    fn note_error(&self, error: &io::Error) {
        *self
            .last_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(error.to_string());
    }
}

/// Fil d'ecriture de la persistance.
pub struct PersistenceWriter {
    sender: Option<SyncSender<Request>>,
    thread: Option<JoinHandle<()>>,
    state: Arc<State>,
}

impl std::fmt::Debug for PersistenceWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistenceWriter")
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

impl PersistenceWriter {
    /// Demarre le fil d'ecriture.
    ///
    /// # Errors
    ///
    /// L'erreur du systeme si le fil ne peut pas etre cree.
    pub fn start(fs: Arc<dyn FileSystem>) -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let state = Arc::new(State::default());
        let worker_state = Arc::clone(&state);
        let thread = std::thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || run(&receiver, fs.as_ref(), &worker_state))?;
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
            state,
        })
    }

    fn submit(&self, request: Request) -> Result<(), Refused> {
        if self.state.disabled.load(Ordering::Acquire) {
            self.state.refused.fetch_add(1, Ordering::Relaxed);
            return Err(Refused::Disabled);
        }
        let Some(sender) = &self.sender else {
            return Err(Refused::Disabled);
        };
        match sender.try_send(request) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.state.refused.fetch_add(1, Ordering::Relaxed);
                Err(Refused::QueueFull)
            }
            Err(TrySendError::Disconnected(_)) => Err(Refused::Disabled),
        }
    }

    /// Confie l'ecriture de `records` dans `path`, avec l'en-tete de `fingerprint`.
    /// Rend la main aussitot (R-361).
    ///
    /// # Errors
    ///
    /// [`Refused`] si la persistance est desactivee ou la file pleine.
    pub fn persist(
        &self,
        path: impl Into<PathBuf>,
        fingerprint: Fingerprint,
        records: Vec<WorkloadRecord>,
    ) -> Result<(), Refused> {
        self.submit(Request::Persist {
            path: path.into(),
            fingerprint,
            records,
        })
    }

    /// Confie la rotation de `dir` selon `policy` (R-691).
    ///
    /// # Errors
    ///
    /// [`Refused`] si la persistance est desactivee ou la file pleine.
    pub fn rotate(&self, dir: impl Into<PathBuf>, policy: RotationPolicy) -> Result<(), Refused> {
        self.submit(Request::Rotate {
            dir: dir.into(),
            policy,
        })
    }

    /// Attend, au plus `timeout`, que tout ce qui a ete confie avant cet appel soit
    /// traite. Pour l'arret et les tests : jamais depuis le fil autoritatif.
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

    /// Compteurs.
    #[must_use]
    pub fn stats(&self) -> WriterStats {
        let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        WriterStats {
            writes: read(&self.state.writes),
            write_failures: read(&self.state.write_failures),
            rotation_failures: read(&self.state.rotation_failures),
            rotated: read(&self.state.rotated),
            refused: read(&self.state.refused),
            last_persist_ms: read(&self.state.last_persist_ms),
            disabled: self.state.disabled.load(Ordering::Acquire),
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

impl Drop for PersistenceWriter {
    /// Ferme la file ; le fil termine ce qui y reste, puis s'arrete.
    fn drop(&mut self) {
        self.sender = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(receiver: &Receiver<Request>, fs: &dyn FileSystem, state: &State) {
    for request in receiver {
        match request {
            Request::Flush(done) => {
                let _ = done.send(());
            }
            _ if state.disabled.load(Ordering::Acquire) => {
                state.refused.fetch_add(1, Ordering::Relaxed);
            }
            Request::Persist {
                path,
                fingerprint,
                records,
            } => {
                let started = Instant::now();
                let written = file::encode(&fingerprint, &records)
                    .map_err(io::Error::other)
                    .and_then(|bytes| fs.write_atomic(&path, &bytes));
                match written {
                    Ok(()) => {
                        state.writes.fetch_add(1, Ordering::Relaxed);
                        let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                        state.last_persist_ms.store(ms, Ordering::Relaxed);
                    }
                    Err(error) => {
                        // R-692 : la persistance s'arrete, le runtime continue en memoire.
                        state.write_failures.fetch_add(1, Ordering::Relaxed);
                        state.note_error(&error);
                        state.disabled.store(true, Ordering::Release);
                    }
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
}
