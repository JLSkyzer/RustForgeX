//! C-18 : contrat d'execution d'une tache, et groupe de taches attendues ensemble.
//!
//! Cahier des charges : PARTIE 5.18 (IF-13). Exigences : R-434 (verifier l'annulation et
//! l'echeance), R-524 (une panic n'empoisonne pas le pool). Maturite : `STABLE` pour ce
//! qui est ici ; l'arene, l'instantane et le tampon de commandes du contexte arrivent
//! avec C-31 et C-19/C-21 (jalon M4).

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crossbeam_deque::Worker;
use rfx_taskgraph::{CancelToken, Priority};

use crate::pool::{Job, Shared};

/// Pourquoi une tache n'a pas produit de resultat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// Annulee avant ou pendant son execution.
    Cancelled,
    /// Budget de la tache depasse (arene, par exemple).
    BudgetExceeded,
}

/// Issue d'une tache (IF-13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskOutcome {
    /// Terminee avec succes.
    Success,
    /// Refusee ou abandonnee.
    Rejected(RejectReason),
    /// Echeance depassee.
    Timeout,
    /// Resultat invalide : a ignorer.
    Invalid,
    /// La tache a panique ; le worker a survecu (R-524).
    Panicked,
}

impl TaskOutcome {
    fn slot(self) -> usize {
        match self {
            TaskOutcome::Success => 0,
            TaskOutcome::Rejected(_) => 1,
            TaskOutcome::Timeout => 2,
            TaskOutcome::Invalid => 3,
            TaskOutcome::Panicked => 4,
        }
    }
}

/// Ce qu'une tache voit de son execution.
pub struct TaskContext<'a> {
    /// Jeton d'annulation de la tache.
    pub cancel: &'a CancelToken,
    /// Echeance, si la tache en a une.
    pub deadline: Option<Instant>,
    /// Priorite, heritee par les sous-taches.
    pub(crate) priority: Priority,
    /// Profondeur d'imbrication : 0 pour une tache soumise, 1 pour sa sous-tache.
    pub(crate) depth: u8,
    pub(crate) shared: &'a Shared,
    /// File du worker qui execute la tache ; `None` en `FORCE_SERIAL`.
    pub(crate) local: Option<&'a Worker<Job>>,
}

impl std::fmt::Debug for TaskContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskContext")
            .field("cancel", self.cancel)
            .field("deadline", &self.deadline)
            .field("priority", &self.priority)
            .field("depth", &self.depth)
            .finish_non_exhaustive()
    }
}

impl TaskContext<'_> {
    /// Point de controle (R-434) : `true` si la tache doit s'arreter, parce qu'elle est
    /// annulee ou que son echeance est passee.
    #[must_use]
    pub fn should_stop(&self) -> bool {
        self.cancel.is_cancelled() || self.deadline.is_some_and(|at| Instant::now() >= at)
    }
}

/// Une tache executable (IF-13).
pub trait Task: Send {
    /// Execute la tache.
    fn run(&mut self, cx: &mut TaskContext<'_>) -> TaskOutcome;
}

impl<F> Task for F
where
    F: FnMut(&mut TaskContext<'_>) -> TaskOutcome + Send,
{
    fn run(&mut self, cx: &mut TaskContext<'_>) -> TaskOutcome {
        self(cx)
    }
}

/// Bilan d'un groupe : nombre de taches par issue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GroupTally {
    /// Terminees avec succes.
    pub success: u64,
    /// Refusees ou abandonnees.
    pub rejected: u64,
    /// Echeance depassee.
    pub timeout: u64,
    /// Resultat invalide.
    pub invalid: u64,
    /// Ont panique.
    pub panicked: u64,
}

impl GroupTally {
    /// Nombre total de taches terminees, quelle qu'en soit l'issue.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.success + self.rejected + self.timeout + self.invalid + self.panicked
    }
}

#[derive(Debug, Default)]
pub(crate) struct GroupState {
    pending: AtomicUsize,
    outcomes: [AtomicU64; 5],
    lock: Mutex<()>,
    done: Condvar,
}

impl GroupState {
    pub(crate) fn enter(&self) {
        self.pending.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn finish(&self, outcome: TaskOutcome) {
        self.outcomes[outcome.slot()].fetch_add(1, Ordering::Relaxed);
        if self.pending.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.notify();
        }
    }

    fn notify(&self) {
        // Le verrou ordonne la notification avec la verification du compteur dans
        // `wait` : sans lui, un reveil pourrait tomber entre les deux et se perdre.
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.done.notify_all();
    }
}

/// Groupe de taches attendues ensemble.
///
/// Cloner le groupe partage le meme etat.
#[derive(Debug, Clone, Default)]
pub struct TaskGroup(pub(crate) Arc<GroupState>);

impl TaskGroup {
    /// Groupe vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Taches soumises et non terminees.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.0.pending.load(Ordering::Acquire)
    }

    /// Attend que toutes les taches soumises soient terminees, au plus `timeout`.
    ///
    /// Renvoie `true` si le groupe est vide au retour. Ne bloque jamais au-dela du
    /// delai : le fil autoritatif n'attend pas un worker au-dela de son echeance
    /// (INV-08).
    pub fn wait(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut guard = self
            .0
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if self.pending() == 0 {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            guard = self
                .0
                .done
                .wait_timeout(guard, deadline - now)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }

    /// Bilan des taches terminees.
    #[must_use]
    pub fn tally(&self) -> GroupTally {
        let read = |slot: usize| self.0.outcomes[slot].load(Ordering::Relaxed);
        GroupTally {
            success: read(0),
            rejected: read(1),
            timeout: read(2),
            invalid: read(3),
            panicked: read(4),
        }
    }
}
