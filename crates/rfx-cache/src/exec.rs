//! C-09, C-48 : ou s'execute le travail de la base (ADR-061).
//!
//! Cahier des charges : PARTIE 5.46 (R-690 : jamais sur le fil autoritatif), PARTIE 5.17
//! (`BACKGROUND`). Maturite : `STABLE`.
//!
//! La base ne sait pas qui l'execute. En jeu, c'est l'ordonnanceur (C-17), realise dans
//! `rfx-core` ; a defaut, et dans les tests, un fil dedie ([`ThreadExecutor`]). Les deux
//! promettent la meme chose : un travail confie ne s'execute **jamais** sur le fil qui
//! le confie, car ce fil tient le verrou que le tick partage.

use std::io;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, PoisonError};

/// Sous-travaux d'un travail, et leur attente.
pub trait Fork {
    /// Lance un sous-travail.
    fn spawn(&self, work: Box<dyn FnOnce() + Send>);
    /// Attend la fin de tous les sous-travaux lances, en en executant au besoin : jamais
    /// d'interblocage, meme sans autre fil libre.
    fn join(&self);
}

/// Un travail confie a un [`Executor`]. Il attend lui-meme ([`Fork::join`]) les
/// sous-travaux qu'il lance, avant de rendre la main.
pub type Work = Box<dyn FnOnce(&dyn Fork) + Send>;

/// Execute des travaux ailleurs que sur le fil qui les confie.
pub trait Executor: Send + Sync + std::fmt::Debug {
    /// Confie un travail. Ne l'execute jamais sur le fil appelant.
    ///
    /// # Errors
    ///
    /// Le travail, intact, s'il ne peut pas etre lance maintenant (contre-pression,
    /// execution en serie, executeur arrete).
    fn execute(&self, work: Work) -> Result<(), Work>;
}

/// Sous-travaux executes a la suite, sur le fil du travail.
#[derive(Debug, Default, Clone, Copy)]
pub struct InlineFork;

impl Fork for InlineFork {
    fn spawn(&self, work: Box<dyn FnOnce() + Send>) {
        work();
    }

    fn join(&self) {}
}

/// Un fil dedie qui execute les travaux dans l'ordre ou ils sont confies ; leurs
/// sous-travaux s'executent a la suite ([`InlineFork`]). Secours quand l'ordonnanceur
/// n'a pas demarre, et executeur des tests.
///
/// Le detruire ferme sa file sans l'attendre : le fil finit ses travaux puis s'arrete.
/// Un disque qui ne repond plus ne bloque donc pas l'arret du jeu.
#[derive(Debug)]
pub struct ThreadExecutor {
    sender: Mutex<Option<Sender<Work>>>,
}

impl ThreadExecutor {
    /// Demarre le fil `name`.
    ///
    /// # Errors
    ///
    /// L'erreur du systeme si le fil ne peut pas etre cree.
    pub fn start(name: &str) -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel::<Work>();
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                for work in receiver {
                    // Un travail qui panique ne fait pas taire le fil : les suivants
                    // s'executent (R-692 : la base continue).
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        work(&InlineFork);
                    }));
                }
            })?;
        Ok(Self {
            sender: Mutex::new(Some(sender)),
        })
    }

    /// Ferme la file : les travaux deja confies s'executent, les suivants sont refuses.
    pub fn stop(&self) {
        self.sender
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }
}

impl Executor for ThreadExecutor {
    fn execute(&self, work: Work) -> Result<(), Work> {
        let sender = self.sender.lock().unwrap_or_else(PoisonError::into_inner);
        match sender.as_ref() {
            Some(sender) => sender.send(work).map_err(|error| error.0),
            None => Err(work),
        }
    }
}

impl Drop for ThreadExecutor {
    fn drop(&mut self) {
        self.stop();
    }
}
