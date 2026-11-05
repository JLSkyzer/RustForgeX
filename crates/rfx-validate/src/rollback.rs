//! C-26 : Rollback — ramener un workload a une execution sure, sans attendre.
//!
//! Cahier des charges : PARTIE 5.26 (R-510, R-511, R-512). Tests : T-350, T-351, T-352.
//! Maturite : `STABLE` pour la logique ; l'annulation effective des taches en vol et le
//! retrait des buffers sont faits par l'appelant, qui suit le [`RollbackPlan`].
//!
//! La revocation est visible des la demande, au prochain point de soumission (R-510) ;
//! elle n'annule jamais un effet deja commite (R-511). Un motif de correction bannit la
//! strategie pour l'empreinte courante ; trois motifs de correction mettent le workload
//! en `JAVA_ONLY` jusqu'au changement d'empreinte (R-512).

use std::collections::{HashMap, VecDeque};

use rfx_model::{StrategyKind, WorkId};

/// Incidents de correction avant le `JAVA_ONLY` permanent (R-512).
pub const PERMANENT_AFTER: u32 = 3;

/// Depassements de delai tolerables : au-dela de 2 dans la fenetre, retour arriere.
pub const MAX_TIMEOUTS: usize = 3;

/// Fenetre des depassements de delai, en ticks.
pub const TIMEOUT_WINDOW_TICKS: u64 = 100;

/// Part de buffers rejetes au-dela de laquelle le workload revient en arriere, en %.
pub const MAX_REJECTED_PCT: u64 = 5;

/// Fenetre d'observation d'une regression de performance, en ticks.
pub const REGRESSION_WINDOW_TICKS: u64 = 300;

/// Declencheur d'un retour arriere (PARTIE 5.26).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollbackReason {
    /// Divergence de validation.
    Mismatch,
    /// Exception ou panic dans le chemin optimise.
    Fault,
    /// Depassements de delai repetes.
    Timeouts,
    /// Conflits de commit repetes.
    Conflicts,
    /// Gain mesure negatif.
    Regression,
    /// Corruption suspectee (audit, invariant viole).
    Corruption,
    /// Demande de l'utilisateur ou changement de configuration.
    User,
}

impl RollbackReason {
    /// `true` pour un motif de correction : le resultat lui-meme est en cause.
    #[must_use]
    pub fn is_correctness(self) -> bool {
        matches!(self, Self::Mismatch | Self::Fault | Self::Corruption)
    }
}

/// Ce que l'appelant doit faire, dans l'ordre (PARTIE 5.26, etapes 1 a 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollbackPlan {
    /// Workload.
    pub work: WorkId,
    /// Motif.
    pub reason: RollbackReason,
    /// Annuler les taches en vol de ce workload.
    pub cancel_in_flight: bool,
    /// Jeter leurs buffers non appliques.
    pub discard_buffers: bool,
    /// Strategie restauree.
    pub restore: StrategyKind,
    /// Strategie bannie pour l'empreinte courante, si le motif est de correction.
    pub banned: Option<StrategyKind>,
    /// `true` si le workload passe en `JAVA_ONLY` jusqu'au changement d'empreinte.
    pub permanent: bool,
}

#[derive(Debug, Default, Clone)]
struct WorkState {
    revoked: bool,
    correctness_incidents: u32,
    permanent: bool,
    banned: Vec<StrategyKind>,
}

/// Registre des retours arriere, par workload, pour l'empreinte courante de chacun.
#[derive(Debug, Default)]
pub struct Ledger {
    works: HashMap<WorkId, WorkState>,
    rollbacks: u64,
    permanent_bans: u64,
}

impl Ledger {
    /// Registre vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Retour arriere du workload `work`, qui executait `strategy`. La revocation est
    /// immediate (R-510).
    pub fn rollback(
        &mut self,
        work: WorkId,
        strategy: StrategyKind,
        reason: RollbackReason,
    ) -> RollbackPlan {
        let state = self.works.entry(work).or_default();
        state.revoked = true;
        let mut banned = None;
        if reason.is_correctness() {
            state.correctness_incidents = state.correctness_incidents.saturating_add(1);
            if strategy != StrategyKind::JavaOnly && !state.banned.contains(&strategy) {
                state.banned.push(strategy);
            }
            banned = (strategy != StrategyKind::JavaOnly).then_some(strategy);
            if state.correctness_incidents >= PERMANENT_AFTER && !state.permanent {
                state.permanent = true;
                self.permanent_bans = self.permanent_bans.saturating_add(1);
            }
        }
        self.rollbacks = self.rollbacks.saturating_add(1);
        RollbackPlan {
            work,
            reason,
            cancel_in_flight: true,
            discard_buffers: true,
            restore: StrategyKind::JavaOnly,
            banned,
            permanent: state.permanent,
        }
    }

    /// `true` si le workload a ete revoque : rien ne doit plus lui etre soumis hors de
    /// `JAVA_ONLY` tant que la decision n'a pas ete reprise.
    #[must_use]
    pub fn is_revoked(&self, work: WorkId) -> bool {
        self.works.get(&work).is_some_and(|s| s.revoked)
    }

    /// Reprise : une nouvelle decision peut etre soumise. Les bannissements restent.
    pub fn reinstate(&mut self, work: WorkId) {
        if let Some(state) = self.works.get_mut(&work) {
            state.revoked = false;
        }
    }

    /// `true` si `strategy` peut etre essayee pour ce workload. `JAVA_ONLY` l'est
    /// toujours.
    #[must_use]
    pub fn allows(&self, work: WorkId, strategy: StrategyKind) -> bool {
        if strategy == StrategyKind::JavaOnly {
            return true;
        }
        self.works
            .get(&work)
            .is_none_or(|s| !s.revoked && !s.permanent && !s.banned.contains(&strategy))
    }

    /// Nouvelle empreinte du code du workload : bannissements et incidents valaient pour
    /// l'ancien code (R-512, SM-03).
    pub fn on_fingerprint(&mut self, work: WorkId) {
        self.works.remove(&work);
    }

    /// Retours arriere depuis le demarrage (`rfx.rollback.count`).
    #[must_use]
    pub fn rollbacks(&self) -> u64 {
        self.rollbacks
    }

    /// Workloads passes en `JAVA_ONLY` permanent (`rfx.rollback.permanent_bans`).
    #[must_use]
    pub fn permanent_bans(&self) -> u64 {
        self.permanent_bans
    }
}

/// Depassements de delai d'un workload : declenche au [`MAX_TIMEOUTS`]-ieme dans
/// [`TIMEOUT_WINDOW_TICKS`] ticks.
#[derive(Debug, Default)]
pub struct TimeoutWatch {
    ticks: VecDeque<u64>,
}

impl TimeoutWatch {
    /// Enregistre un depassement a `tick` ; `true` s'il faut revenir en arriere.
    pub fn record(&mut self, tick: u64) -> bool {
        while self
            .ticks
            .front()
            .is_some_and(|t| tick.saturating_sub(*t) >= TIMEOUT_WINDOW_TICKS)
        {
            self.ticks.pop_front();
        }
        self.ticks.push_back(tick);
        self.ticks.len() >= MAX_TIMEOUTS
    }
}

/// Part des buffers d'un workload rejetes au commit.
#[derive(Debug, Default)]
pub struct ConflictWatch {
    committed: u64,
    rejected: u64,
}

impl ConflictWatch {
    /// Enregistre l'issue d'un buffer ; `true` s'il faut revenir en arriere. Sans
    /// echantillon minimal : revenir trop tot est sur, trop tard ne l'est pas.
    pub fn record(&mut self, rejected: bool) -> bool {
        self.committed = self.committed.saturating_add(1);
        if rejected {
            self.rejected = self.rejected.saturating_add(1);
        }
        self.rejected.saturating_mul(100) > self.committed.saturating_mul(MAX_REJECTED_PCT)
    }
}

/// Gain mesure d'un workload actif, par tick : declenche quand le gain cumule sur
/// [`REGRESSION_WINDOW_TICKS`] ticks est negatif.
#[derive(Debug, Default)]
pub struct RegressionWatch {
    window: VecDeque<(u64, f64)>,
}

impl RegressionWatch {
    /// Enregistre le gain mesure a `tick`, en ns ; `true` s'il faut revenir en arriere.
    /// Une mesure qui n'est pas un nombre compte comme une perte.
    pub fn record(&mut self, tick: u64, gain_ns: f64) -> bool {
        let gain = if gain_ns.is_finite() { gain_ns } else { -1.0 };
        self.window.push_back((tick, gain));
        while self
            .window
            .front()
            .is_some_and(|(t, _)| tick.saturating_sub(*t) >= REGRESSION_WINDOW_TICKS)
        {
            self.window.pop_front();
        }
        let first = self.window.front().map_or(tick, |(t, _)| *t);
        let covered = tick.saturating_sub(first) + 1 >= REGRESSION_WINDOW_TICKS;
        covered && self.window.iter().map(|(_, g)| g).sum::<f64>() < 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: WorkId = WorkId(7);

    /// T-350 et T-351 : la revocation est immediate, et le plan annule les taches en
    /// vol, jette leurs buffers et restaure `JAVA_ONLY`.
    #[test]
    fn a_rollback_is_immediate_and_complete() {
        let mut ledger = Ledger::new();
        assert!(ledger.allows(W, StrategyKind::RustOffload));
        let plan = ledger.rollback(W, StrategyKind::RustOffload, RollbackReason::Timeouts);
        assert!(ledger.is_revoked(W), "visible des la demande");
        assert!(!ledger.allows(W, StrategyKind::RustOffload));
        assert!(ledger.allows(W, StrategyKind::JavaOnly));
        assert!(plan.cancel_in_flight && plan.discard_buffers);
        assert_eq!(plan.restore, StrategyKind::JavaOnly);
        assert_eq!(plan.banned, None, "un depassement de delai ne bannit pas");
        ledger.reinstate(W);
        assert!(ledger.allows(W, StrategyKind::RustOffload));
    }

    /// T-352 : trois incidents de correction mettent le workload en `JAVA_ONLY`
    /// permanent, jusqu'au changement d'empreinte.
    #[test]
    fn three_correctness_incidents_ban_permanently() {
        let mut ledger = Ledger::new();
        let first = ledger.rollback(W, StrategyKind::RustOffload, RollbackReason::Mismatch);
        assert_eq!(first.banned, Some(StrategyKind::RustOffload));
        assert!(!first.permanent);
        ledger.reinstate(W);
        assert!(
            !ledger.allows(W, StrategyKind::RustOffload),
            "bannie pour ce code"
        );
        assert!(
            ledger.allows(W, StrategyKind::RustBatch),
            "une autre reste possible"
        );
        ledger.rollback(W, StrategyKind::RustBatch, RollbackReason::Fault);
        ledger.reinstate(W);
        // Un motif hors correction ne compte pas.
        ledger.rollback(W, StrategyKind::JavaBatch, RollbackReason::Conflicts);
        ledger.reinstate(W);
        assert!(ledger.allows(W, StrategyKind::JavaBatch));
        let third = ledger.rollback(W, StrategyKind::JavaBatch, RollbackReason::Corruption);
        assert!(third.permanent);
        ledger.reinstate(W);
        assert!(!ledger.allows(W, StrategyKind::RustParallel), "permanent");
        assert_eq!(ledger.permanent_bans(), 1);
        assert_eq!(ledger.rollbacks(), 4);
        ledger.on_fingerprint(W);
        assert!(
            ledger.allows(W, StrategyKind::RustOffload),
            "code change : rehabilite"
        );
    }

    #[test]
    fn timeouts_trigger_at_three_within_a_hundred_ticks() {
        let mut watch = TimeoutWatch::default();
        assert!(!watch.record(0));
        assert!(!watch.record(50));
        assert!(!watch.record(100), "le premier est sorti de la fenetre");
        assert!(watch.record(120), "50, 100, 120");
    }

    #[test]
    fn rejected_buffers_trigger_beyond_five_percent() {
        let mut watch = ConflictWatch::default();
        for _ in 0..19 {
            assert!(!watch.record(false));
        }
        assert!(!watch.record(true), "1 sur 20 : 5 %, pas au-dela");
        assert!(watch.record(true), "2 sur 21");
    }

    #[test]
    fn a_negative_gain_over_the_window_triggers() {
        let mut watch = RegressionWatch::default();
        for tick in 0..REGRESSION_WINDOW_TICKS - 1 {
            assert!(!watch.record(tick, -1.0), "fenetre pas encore couverte");
        }
        assert!(watch.record(REGRESSION_WINDOW_TICKS - 1, -1.0));
        let mut positive = RegressionWatch::default();
        for tick in 0..2 * REGRESSION_WINDOW_TICKS {
            assert!(!positive.record(tick, if tick % 2 == 0 { 3.0 } else { -2.0 }));
        }
        let mut nan = RegressionWatch::default();
        for tick in 0..REGRESSION_WINDOW_TICKS {
            nan.record(tick, f64::NAN);
        }
        assert!(
            nan.record(REGRESSION_WINDOW_TICKS, f64::NAN),
            "NaN compte comme perte"
        );
    }
}
