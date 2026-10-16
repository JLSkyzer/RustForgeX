//! C-16 : ce qu'est une tache du graphe.
//!
//! Cahier des charges : PARTIE 5.16, modele `TaskNode`. Maturite : `STABLE`.
//!
//! Le graphe ne sait pas executer : il decrit. Une tache porte ce qu'il faut pour
//! l'ordonner (dependances, priorite, echeance, cout estime) et une charge utile
//! opaque que l'executant interprete. C'est ce qui permet de le tester sans
//! ordonnanceur, et a l'ordonnanceur de ne rien savoir de ce qu'il execute.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rfx_model::WorkId;

/// Identifiant d'une tache dans son graphe.
///
/// Stable pendant toute la vie du graphe : une tache fusionnee disparait, mais son
/// identifiant n'est jamais reattribue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub(crate) u32);

impl TaskId {
    /// Rang de la tache dans l'ordre de creation.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Nature d'une tache (PARTIE 5.16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskKind {
    /// Calcul.
    Compute,
    /// Prise d'instantane de l'etat a lire.
    Snapshot,
    /// Preparation d'un commit.
    CommitPrep,
    /// Execution fantome, pour comparer sans appliquer.
    Shadow,
    /// Analyse interne du runtime.
    Analysis,
}

/// Priorite d'une tache (PARTIE 5.17.4).
///
/// L'ordre de declaration est celui de l'urgence croissante : `max` donne la plus
/// urgente de deux priorites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Priority {
    /// Analyse, persistance, apprentissage : hors fenetre critique seulement.
    Background,
    /// Execution fantome : seulement si des workers sont libres.
    Low,
    /// Lots, preparation.
    Normal,
    /// Offloads actifs du tick.
    High,
    /// Necessaire au commit de ce tick.
    Critical,
}

/// Echeance d'une tache.
///
/// L'ordre de declaration va de la plus proche a la plus lointaine : `min` donne la
/// plus contraignante de deux echeances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Deadline {
    /// Doit etre terminee a la fenetre de drain de ce tick.
    ThisTick,
    /// Doit etre terminee dans `n` ticks, `n >= 1`.
    Ticks(u32),
    /// Aucune echeance.
    None,
}

/// Jeton d'annulation partage, verifie aux points de controle (PARTIE 5.17.7).
///
/// Cloner le jeton partage le meme drapeau : annuler l'un annule tous ses clones.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// Cree un jeton non annule.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Demande l'annulation. Idempotent.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// `true` si l'annulation a ete demandee.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// `true` si les deux jetons partagent le meme drapeau.
    #[must_use]
    pub fn same_as(&self, other: &CancelToken) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Charge utile d'une tache, interpretee par l'executant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskPayload {
    /// Travail indivisible, designe par une poignee opaque.
    Unit(u64),
    /// Lot d'elements independants `start..start + len` d'un meme travail. Divisible :
    /// l'executant garantit que l'ordre de traitement des elements est indifferent.
    Batch {
        /// Poignee du travail.
        handle: u64,
        /// Premier element.
        start: u32,
        /// Nombre d'elements.
        len: u32,
    },
    /// Taches fusionnees, a executer dans cet ordre.
    Sequence(Vec<TaskPayload>),
}

/// Ce qu'il faut pour creer une tache ; ses dependances s'ajoutent ensuite.
#[derive(Debug, Clone)]
pub struct TaskSpec {
    /// Unite de travail dont la tache execute une part.
    pub work: WorkId,
    /// Nature.
    pub kind: TaskKind,
    /// Priorite.
    pub priority: Priority,
    /// Echeance.
    pub deadline: Deadline,
    /// Cout estime, en nanosecondes.
    pub estimated_ns: u64,
    /// Charge utile.
    pub payload: TaskPayload,
    /// Jeton d'annulation.
    pub cancel: CancelToken,
}

/// Tache du graphe.
#[derive(Debug, Clone)]
pub struct TaskNode {
    /// Identifiant.
    pub id: TaskId,
    /// Unite de travail.
    pub work: WorkId,
    /// Nature.
    pub kind: TaskKind,
    /// Taches qui doivent etre terminees avant celle-ci.
    pub deps: Vec<TaskId>,
    /// Priorite.
    pub priority: Priority,
    /// Echeance.
    pub deadline: Deadline,
    /// Cout estime, en nanosecondes.
    pub estimated_ns: u64,
    /// Charge utile.
    pub payload: TaskPayload,
    /// Jeton d'annulation.
    pub cancel: CancelToken,
}

impl TaskNode {
    pub(crate) fn from_spec(id: TaskId, spec: TaskSpec) -> Self {
        Self {
            id,
            work: spec.work,
            kind: spec.kind,
            deps: Vec::new(),
            priority: spec.priority,
            deadline: spec.deadline,
            estimated_ns: spec.estimated_ns,
            payload: spec.payload,
            cancel: spec.cancel,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priorities_and_deadlines_order_by_urgency() {
        assert_eq!(Priority::Low.max(Priority::Critical), Priority::Critical);
        assert_eq!(Deadline::None.min(Deadline::ThisTick), Deadline::ThisTick);
        assert_eq!(
            Deadline::Ticks(3).min(Deadline::Ticks(1)),
            Deadline::Ticks(1)
        );
    }

    #[test]
    fn a_cloned_token_shares_the_cancellation() {
        let token = CancelToken::new();
        let clone = token.clone();
        assert!(token.same_as(&clone));
        assert!(!token.same_as(&CancelToken::new()));
        clone.cancel();
        assert!(token.is_cancelled());
    }
}
