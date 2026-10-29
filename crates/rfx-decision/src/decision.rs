//! C-15 : Decision Engine — la strategie d'execution d'une unite de travail, sous
//! contrainte de risque, de compatibilite et de mode.
//!
//! Cahier des charges : PARTIE 5.15, PARTIE 28.3. Decision : ADR-068. Tests : T-240 a
//! T-245. Maturite : `STABLE` pour le perimetre de M3 (TL0..TL2).
//!
//! Trois temps : un filtre dur (un genre interdit l'est avec ses motifs, DM-15), un
//! score (gain economise moins couts et penalite de risque), puis la stabilite (R-412) :
//! une strategie tenue ne change pas sans raison de surete avant 600 ticks. Sans
//! speedup mesure, le gain est nul et rien ne s'active (R-410) : `JAVA_ONLY` est
//! toujours une decision valide.

use rfx_model::{
    ClassificationFlags, RejectReason, RuntimeMode, StrategyKind, TransformationLevel, Tri,
};

use crate::compat::CompatVerdict;
use crate::risk::RiskAssessment;

/// Niveau maximal applique au jalon M3 (ADR-068, decision 5).
pub const MILESTONE_MAX_LEVEL: TransformationLevel = TransformationLevel::Tl2;

/// Duree minimale de tenue d'une strategie, en ticks (R-412).
pub const MIN_HOLD_TICKS: u64 = 600;

/// Fenetre de detection d'oscillation, en ticks : dix minutes (FM-21).
pub const OSCILLATION_WINDOW_TICKS: u64 = 12_000;

/// Changements toleres dans la fenetre ; au-dela, verrouillage (FM-21).
pub const MAX_CHANGES_IN_WINDOW: u32 = 3;

/// Une nanoseconde retiree du fil autoritatif vaut ce poids ailleurs (PARTIE 5.15).
pub const MAINTHREAD_WEIGHT: f64 = 3.0;

/// Cout d'une soumission de tache, en nanosecondes (PARTIE 5.15, defaut a remesurer).
pub const TASK_SUBMISSION_NS: f64 = 120.0;

/// Ce que le mode impose (PARTIE 28.3, ADR-068 decision 3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModePolicy {
    /// Risque de correction maximal accepte.
    pub max_correctness_risk: f32,
    /// Score minimal pour activer une strategie, en ns par tick.
    pub activation_threshold_ns: f64,
    /// Niveau de transformation maximal du mode.
    pub max_level: TransformationLevel,
}

impl ModePolicy {
    /// La politique d'un mode.
    #[must_use]
    pub fn of(mode: RuntimeMode) -> Self {
        let (risk, threshold, level) = match mode {
            RuntimeMode::Safe => (0.05, 200_000.0, TransformationLevel::Tl3),
            RuntimeMode::Balanced | RuntimeMode::Debug => {
                (0.15, 50_000.0, TransformationLevel::Tl4)
            }
            RuntimeMode::Performance => (0.30, 20_000.0, TransformationLevel::Tl4),
            RuntimeMode::Experimental => (0.30, 20_000.0, TransformationLevel::Tl6),
        };
        Self {
            max_correctness_risk: risk,
            activation_threshold_ns: threshold,
            max_level: level,
        }
    }

    /// Niveau maximal des candidats : celui du mode, borne par le jalon.
    #[must_use]
    pub fn candidate_level(&self) -> TransformationLevel {
        self.max_level.min(MILESTONE_MAX_LEVEL)
    }
}

/// Ce que le moteur lit pour une unite.
#[derive(Debug, Clone, Copy)]
pub struct DecisionInputs<'a> {
    /// Drapeaux (DM-11).
    pub flags: ClassificationFlags,
    /// Le fil autoritatif est-il requis (DM-09).
    pub main_thread_required: Tri,
    /// Risque (C-14).
    pub risk: &'a RiskAssessment,
    /// Verdict de compatibilite (C-33).
    pub compat: CompatVerdict,
    /// Mode du runtime.
    pub mode: RuntimeMode,
    /// Coeurs logiques de la machine (C-45).
    pub logical_cores: u32,
    /// Appels par tick (C-05).
    pub calls_per_tick: f64,
    /// Duree mediane d'un appel, en nanosecondes (C-05).
    pub cpu_ns_p50: f64,
    /// Speedups mesures pour cette unite et cette empreinte ; un genre absent n'a
    /// aucune mesure (R-410).
    pub measured_speedups: &'a [(StrategyKind, f64)],
}

/// Un genre rejete, avec tous ses motifs (R-219).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    /// Le genre.
    pub strategy: StrategyKind,
    /// Ses motifs, sans doublon, dans l'ordre de DM-15.
    pub reasons: Vec<RejectReason>,
}

/// Un genre faisable, et son evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scored {
    /// Le genre.
    pub strategy: StrategyKind,
    /// Gain attendu, en ns par tick.
    pub gain_ns: f64,
    /// Score : gain, moins couts et penalite de risque.
    pub score_ns: f64,
}

/// Decision de C-15 pour une unite.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// Strategie retenue.
    pub strategy: StrategyKind,
    /// Son niveau.
    pub level: TransformationLevel,
    /// Son score, en ns par tick ; zero pour `JAVA_ONLY`.
    pub score_ns: f64,
    /// Son gain attendu, en ns par tick.
    pub expected_gain_ns: f64,
    /// Genres candidats rejetes, avec leurs motifs.
    pub rejected: Vec<Rejected>,
    /// Genres candidats faisables, evalues.
    pub scored: Vec<Scored>,
    /// La strategie demande une validation avant d'etre active.
    pub requires_validation: bool,
    /// Ticks avant revision (R-411).
    pub revisit_after_ticks: u32,
}

/// Genres executes hors du fil autoritatif.
fn off_thread(s: StrategyKind) -> bool {
    !matches!(
        s,
        StrategyKind::JavaOnly | StrategyKind::JavaBatch | StrategyKind::Reject
    )
}

/// Genres paralleles.
fn parallel(s: StrategyKind) -> bool {
    matches!(s, StrategyKind::JavaParallel | StrategyKind::RustParallel)
}

/// Motifs qui interdisent ce genre pour cette unite ; vide s'il est faisable
/// (PARTIE 5.15, ADR-068 decision 4). `JAVA_ONLY` est toujours faisable.
#[must_use]
pub fn feasibility(strategy: StrategyKind, inputs: &DecisionInputs<'_>) -> Vec<RejectReason> {
    let Some(level) = strategy.level() else {
        return Vec::new();
    };
    if strategy == StrategyKind::JavaOnly {
        return Vec::new();
    }
    let flags = inputs.flags;
    let policy = ModePolicy::of(inputs.mode);
    let mut reasons = Vec::new();
    let mut forbid = |applies: bool, reason: RejectReason| {
        if applies {
            reasons.push(reason);
        }
    };
    forbid(
        flags.contains(ClassificationFlags::UNKNOWN),
        RejectReason::InsufficientConfidence,
    );
    forbid(
        off_thread(strategy) && inputs.main_thread_required != Tri::No,
        RejectReason::ThreadAffinity,
    );
    forbid(
        off_thread(strategy) && !flags.contains(ClassificationFlags::DETERMINISTIC),
        RejectReason::NonDeterministicBehavior,
    );
    forbid(
        parallel(strategy) && flags.contains(ClassificationFlags::ORDER_SENSITIVE),
        RejectReason::OrderDependency,
    );
    forbid(
        inputs
            .compat
            .verdict
            .max_level()
            .is_none_or(|allowed| level > allowed),
        RejectReason::CompatibilityRule,
    );
    forbid(
        inputs.risk.correctness_risk > policy.max_correctness_risk,
        RejectReason::InsufficientConfidence,
    );
    forbid(
        level > inputs.risk.max_level(),
        RejectReason::UnsafeSharedState,
    );
    forbid(
        parallel(strategy) && inputs.logical_cores < 2,
        RejectReason::HardwareUnsuitable,
    );
    forbid(
        strategy == StrategyKind::JavaBatch && !flags.contains(ClassificationFlags::BATCHABLE),
        RejectReason::UnsafeSharedState,
    );
    reasons.sort_unstable();
    reasons.dedup();
    reasons
}

/// Gain attendu et score d'un genre faisable (ADR-068, decisions 1 et 2).
#[must_use]
pub fn score(strategy: StrategyKind, inputs: &DecisionInputs<'_>) -> Scored {
    let cost = inputs.calls_per_tick.max(0.0) * inputs.cpu_ns_p50.max(0.0);
    let measured = inputs
        .measured_speedups
        .iter()
        .find(|(kind, _)| *kind == strategy)
        .map(|(_, speedup)| *speedup)
        .filter(|speedup| speedup.is_finite() && *speedup > 0.0);
    // R-410 : sans mesure, le speedup vaut 1,0, et le gain est nul.
    let speedup = measured.unwrap_or(1.0);
    let saved = cost * (1.0 - 1.0 / speedup);
    // Le fil autoritatif soulage ne compte que mesure, comme le reste.
    let main_thread = if off_thread(strategy) && measured.is_some() {
        cost * (MAINTHREAD_WEIGHT - 1.0)
    } else {
        0.0
    };
    let gain = if strategy == StrategyKind::JavaOnly {
        0.0
    } else {
        saved + main_thread
    };
    let scheduling = if off_thread(strategy) {
        inputs.calls_per_tick.max(0.0) * TASK_SUBMISSION_NS
    } else {
        0.0
    };
    let risk_penalty = f64::from(inputs.risk.correctness_risk) * gain.max(0.0);
    Scored {
        strategy,
        gain_ns: gain,
        score_ns: gain - scheduling - risk_penalty,
    }
}

/// Decide la strategie d'une unite (PARTIE 5.15, ADR-068). Fonction pure.
#[must_use]
pub fn decide(inputs: &DecisionInputs<'_>) -> Decision {
    let policy = ModePolicy::of(inputs.mode);
    let mut rejected = Vec::new();
    let mut scored = Vec::new();
    for strategy in StrategyKind::CANDIDATES {
        if strategy
            .level()
            .is_none_or(|level| level > policy.candidate_level())
        {
            continue;
        }
        let reasons = feasibility(strategy, inputs);
        if reasons.is_empty() {
            scored.push(score(strategy, inputs));
        } else {
            rejected.push(Rejected { strategy, reasons });
        }
    }
    // Le meilleur score au-dessus du seuil ; a egalite, le niveau le plus bas.
    let best = scored
        .iter()
        .filter(|s| s.strategy != StrategyKind::JavaOnly)
        .filter(|s| s.score_ns > policy.activation_threshold_ns)
        .fold(None::<Scored>, |best, s| match best {
            Some(b) if b.score_ns >= s.score_ns => Some(b),
            _ => Some(*s),
        });
    let (strategy, score_ns, gain) = match best {
        Some(s) => (s.strategy, s.score_ns, s.gain_ns),
        None => (StrategyKind::JavaOnly, 0.0, 0.0),
    };
    let level = strategy.level().unwrap_or(TransformationLevel::Tl0);
    Decision {
        strategy,
        level,
        score_ns,
        expected_gain_ns: gain,
        rejected,
        scored,
        requires_validation: level > TransformationLevel::Tl2,
        revisit_after_ticks: u32::try_from(MIN_HOLD_TICKS).unwrap_or(u32::MAX),
    }
}

/// Strategie tenue par une unite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    /// La strategie.
    pub strategy: StrategyKind,
    /// Tick ou elle a ete adoptee.
    pub since_tick: u64,
    /// Changements de strategie dans la fenetre d'oscillation (FM-21).
    pub changes_in_window: u32,
}

/// Issue de la stabilisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stability {
    /// La strategie tenue reste.
    Kept,
    /// La strategie change.
    Changed,
    /// Oscillation : verrouillage sur `JAVA_ONLY` (FM-21, `E-2201`).
    Locked,
}

/// Applique la regle de stabilite (R-412, FM-21) a une strategie proposee.
/// `held_still_feasible` dit si la strategie tenue passe encore le filtre : sinon,
/// changer est une demotion pour surete, toujours permise.
#[must_use]
pub fn stabilize(
    held: Option<Held>,
    proposed: StrategyKind,
    now_tick: u64,
    held_still_feasible: bool,
) -> (StrategyKind, Stability) {
    let Some(held) = held else {
        return (proposed, Stability::Changed);
    };
    if held.changes_in_window > MAX_CHANGES_IN_WINDOW {
        return (StrategyKind::JavaOnly, Stability::Locked);
    }
    if held.strategy == proposed {
        return (proposed, Stability::Kept);
    }
    let held_for = now_tick.saturating_sub(held.since_tick);
    if held_for < MIN_HOLD_TICKS && held_still_feasible {
        return (held.strategy, Stability::Kept);
    }
    (proposed, Stability::Changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::{Source, Verdict};
    use crate::risk::{Blast, RiskAssessment};

    fn risk(correctness: f32, blast: Blast) -> RiskAssessment {
        RiskAssessment {
            correctness_risk: correctness,
            stability_risk: 0.0,
            compat_risk: 0.0,
            data_risk: 0.0,
            max_blast_radius: blast,
            reasons: Vec::new(),
        }
    }

    fn allow() -> CompatVerdict {
        CompatVerdict {
            verdict: Verdict::Allow,
            source: Source::Default,
        }
    }

    const MODES: [RuntimeMode; 5] = [
        RuntimeMode::Safe,
        RuntimeMode::Balanced,
        RuntimeMode::Performance,
        RuntimeMode::Experimental,
        RuntimeMode::Debug,
    ];

    const VERDICTS: [Verdict; 6] = [
        Verdict::Allow,
        Verdict::ForceParallel,
        Verdict::ForceBatch,
        Verdict::ForceSerial,
        Verdict::ForceJava,
        Verdict::Disable,
    ];

    /// Un cas du filtre : les faits qu'il lit.
    #[derive(Debug, Clone, Copy)]
    struct Case {
        flags: ClassificationFlags,
        main: Tri,
        verdict: Verdict,
        correctness: f32,
        blast: Blast,
        cores: u32,
        mode: RuntimeMode,
    }

    /// Les motifs que la fiche impose, recalcules a part (ADR-068, decision 4).
    fn expected_reasons(s: StrategyKind, c: Case) -> Vec<RejectReason> {
        if s == StrategyKind::JavaOnly {
            return Vec::new();
        }
        let level = s.level().unwrap();
        let off = !matches!(s, StrategyKind::JavaOnly | StrategyKind::JavaBatch);
        let par = matches!(s, StrategyKind::JavaParallel | StrategyKind::RustParallel);
        let mut r = Vec::new();
        if c.flags.contains(ClassificationFlags::UNKNOWN) {
            r.push(RejectReason::InsufficientConfidence);
        }
        if off && c.main != Tri::No {
            r.push(RejectReason::ThreadAffinity);
        }
        if off && !c.flags.contains(ClassificationFlags::DETERMINISTIC) {
            r.push(RejectReason::NonDeterministicBehavior);
        }
        if par && c.flags.contains(ClassificationFlags::ORDER_SENSITIVE) {
            r.push(RejectReason::OrderDependency);
        }
        if c.verdict.max_level().is_none_or(|m| level > m) {
            r.push(RejectReason::CompatibilityRule);
        }
        if c.correctness > ModePolicy::of(c.mode).max_correctness_risk {
            r.push(RejectReason::InsufficientConfidence);
        }
        if level > c.blast.max_level() {
            r.push(RejectReason::UnsafeSharedState);
        }
        if par && c.cores < 2 {
            r.push(RejectReason::HardwareUnsuitable);
        }
        if s == StrategyKind::JavaBatch && !c.flags.contains(ClassificationFlags::BATCHABLE) {
            r.push(RejectReason::UnsafeSharedState);
        }
        r.sort_unstable();
        r.dedup();
        r
    }

    fn all_cases() -> Vec<Case> {
        let flag_bits = [
            ClassificationFlags::UNKNOWN,
            ClassificationFlags::DETERMINISTIC,
            ClassificationFlags::ORDER_SENSITIVE,
            ClassificationFlags::BATCHABLE,
        ];
        let mut cases = Vec::new();
        for mask in 0..16_u32 {
            let flags = flag_bits
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .fold(ClassificationFlags::EMPTY, |f, (_, b)| f.set(*b, true));
            for main in [Tri::Yes, Tri::No, Tri::Unknown] {
                for verdict in VERDICTS {
                    for correctness in [0.0_f32, 0.2, 1.0] {
                        for blast in [Blast::Task, Blast::Workload, Blast::Subsystem, Blast::World]
                        {
                            for cores in [1, 8] {
                                for mode in MODES {
                                    cases.push(Case {
                                        flags,
                                        main,
                                        verdict,
                                        correctness,
                                        blast,
                                        cores,
                                        mode,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        cases
    }

    /// T-240 : le filtre de faisabilite, exhaustif sur les faits qu'il lit et sur
    /// les huit genres.
    #[test]
    fn t240_the_feasibility_filter_is_exhaustive() {
        let cases = all_cases();
        assert_eq!(cases.len(), 16 * 3 * 6 * 3 * 4 * 2 * 5);
        for c in cases {
            let r = risk(c.correctness, c.blast);
            let inputs = DecisionInputs {
                flags: c.flags,
                main_thread_required: c.main,
                risk: &r,
                compat: CompatVerdict {
                    verdict: c.verdict,
                    source: Source::User,
                },
                mode: c.mode,
                logical_cores: c.cores,
                calls_per_tick: 10.0,
                cpu_ns_p50: 1_000.0,
                measured_speedups: &[],
            };
            for s in StrategyKind::CANDIDATES {
                assert_eq!(
                    feasibility(s, &inputs),
                    expected_reasons(s, c),
                    "{s:?} {c:?}"
                );
            }
        }
    }

    fn batchable_inputs<'a>(
        r: &'a RiskAssessment,
        speedups: &'a [(StrategyKind, f64)],
        mode: RuntimeMode,
    ) -> DecisionInputs<'a> {
        DecisionInputs {
            flags: ClassificationFlags::EMPTY
                .set(ClassificationFlags::BATCHABLE, true)
                .set(ClassificationFlags::DETERMINISTIC, true),
            main_thread_required: Tri::Yes,
            risk: r,
            compat: allow(),
            mode,
            logical_cores: 8,
            calls_per_tick: 400.0,
            cpu_ns_p50: 2_000.0,
            measured_speedups: speedups,
        }
    }

    /// T-241 : le score croit avec le gain mesure et la charge, decroit avec le risque.
    #[test]
    fn t241_the_score_is_monotone_in_gain_and_cost() {
        let r_low = risk(0.0, Blast::World);
        let r_high = risk(0.5, Blast::World);
        let mut previous = f64::NEG_INFINITY;
        for speedup in [1.0, 1.1, 1.5, 2.0, 4.0, 10.0] {
            let speedups = [(StrategyKind::JavaBatch, speedup)];
            let low = batchable_inputs(&r_low, &speedups, RuntimeMode::Balanced);
            let s = score(StrategyKind::JavaBatch, &low);
            assert!(s.score_ns >= previous, "speedup {speedup}");
            previous = s.score_ns;
            let high = batchable_inputs(&r_high, &speedups, RuntimeMode::Balanced);
            let riskier = score(StrategyKind::JavaBatch, &high).score_ns;
            if s.gain_ns > 0.0 {
                // Un gain attendu pese moins quand on a plus de chances d'avoir tort.
                assert!(riskier < s.score_ns, "speedup {speedup}");
            } else {
                assert!(riskier <= s.score_ns, "speedup {speedup}");
            }
        }
        let speedups = [(StrategyKind::JavaBatch, 2.0)];
        let mut lighter = batchable_inputs(&r_low, &speedups, RuntimeMode::Balanced);
        lighter.calls_per_tick = 10.0;
        let heavier = batchable_inputs(&r_low, &speedups, RuntimeMode::Balanced);
        assert!(
            score(StrategyKind::JavaBatch, &heavier).score_ns
                > score(StrategyKind::JavaBatch, &lighter).score_ns
        );
        // Un genre hors du fil paie ses soumissions de taches.
        let off = score(StrategyKind::JavaParallel, &heavier);
        assert!(off.score_ns < off.gain_ns);
    }

    /// T-242 : sans speedup mesure, aucune strategie ne s'active, quels que soient la
    /// charge et le mode.
    #[test]
    fn t242_nothing_activates_without_a_measurement() {
        let r = risk(0.0, Blast::World);
        for mode in MODES {
            let mut inputs = batchable_inputs(&r, &[], mode);
            inputs.calls_per_tick = 1.0e6;
            inputs.cpu_ns_p50 = 1.0e6;
            let d = decide(&inputs);
            assert_eq!(d.strategy, StrategyKind::JavaOnly, "{mode:?}");
            assert!(d.scored.iter().all(|s| s.gain_ns == 0.0), "{mode:?}");
        }
        // Avec une mesure, la meme unite passe a JAVA_BATCH.
        let speedups = [(StrategyKind::JavaBatch, 2.0)];
        let d = decide(&batchable_inputs(&r, &speedups, RuntimeMode::Balanced));
        assert_eq!(d.strategy, StrategyKind::JavaBatch);
        assert!(!d.requires_validation, "TL2 : actif sans validation");
        // Sous le seuil du mode, non.
        let mut small = batchable_inputs(&r, &speedups, RuntimeMode::Safe);
        small.calls_per_tick = 10.0;
        assert_eq!(decide(&small).strategy, StrategyKind::JavaOnly);
    }

    /// T-243 : pas de changement avant 600 ticks sauf demotion pour surete ; plus de
    /// trois changements dans la fenetre verrouillent sur JAVA_ONLY.
    #[test]
    fn t243_a_strategy_does_not_oscillate() {
        let held = Held {
            strategy: StrategyKind::JavaBatch,
            since_tick: 1_000,
            changes_in_window: 1,
        };
        assert_eq!(
            stabilize(
                Some(held),
                StrategyKind::JavaOnly,
                1_000 + MIN_HOLD_TICKS - 1,
                true
            ),
            (StrategyKind::JavaBatch, Stability::Kept)
        );
        assert_eq!(
            stabilize(
                Some(held),
                StrategyKind::JavaOnly,
                1_000 + MIN_HOLD_TICKS,
                true
            ),
            (StrategyKind::JavaOnly, Stability::Changed)
        );
        assert_eq!(
            stabilize(Some(held), StrategyKind::JavaOnly, 1_001, false),
            (StrategyKind::JavaOnly, Stability::Changed),
            "demotion pour surete"
        );
        assert_eq!(
            stabilize(Some(held), StrategyKind::JavaBatch, 1_001, true),
            (StrategyKind::JavaBatch, Stability::Kept)
        );
        let oscillating = Held {
            changes_in_window: MAX_CHANGES_IN_WINDOW + 1,
            ..held
        };
        assert_eq!(
            stabilize(Some(oscillating), StrategyKind::JavaBatch, 1_000_000, true),
            (StrategyKind::JavaOnly, Stability::Locked)
        );
        assert_eq!(
            stabilize(None, StrategyKind::JavaBatch, 0, true),
            (StrategyKind::JavaBatch, Stability::Changed)
        );
    }

    /// T-244 : le mode SAFE ne depasse jamais TL3 ; et au jalon M3, rien ne depasse TL2,
    /// ni la decision ni les genres evalues.
    #[test]
    fn t244_safe_never_exceeds_tl3_and_m3_never_exceeds_tl2() {
        assert_eq!(
            ModePolicy::of(RuntimeMode::Safe).max_level,
            TransformationLevel::Tl3
        );
        let r = risk(0.0, Blast::Task);
        let all: Vec<(StrategyKind, f64)> = StrategyKind::CANDIDATES
            .iter()
            .map(|s| (*s, 100.0))
            .collect();
        for mode in MODES {
            let mut inputs = batchable_inputs(&r, &all, mode);
            inputs.main_thread_required = Tri::No;
            let d = decide(&inputs);
            assert!(d.level <= ModePolicy::of(mode).max_level, "{mode:?}");
            assert!(d.level <= MILESTONE_MAX_LEVEL, "{mode:?}");
            let evaluated = d
                .scored
                .iter()
                .map(|s| s.strategy)
                .chain(d.rejected.iter().map(|r| r.strategy));
            for s in evaluated {
                assert!(s.level().unwrap() <= MILESTONE_MAX_LEVEL, "{mode:?} {s:?}");
            }
        }
    }

    /// R-219 : tout genre candidat rejete porte au moins un motif.
    #[test]
    fn every_rejection_has_a_reason() {
        let r = risk(1.0, Blast::World);
        let mut inputs = batchable_inputs(&r, &[], RuntimeMode::Balanced);
        inputs.flags = ClassificationFlags::EMPTY.set(ClassificationFlags::UNKNOWN, true);
        let d = decide(&inputs);
        assert_eq!(d.strategy, StrategyKind::JavaOnly);
        assert!(!d.rejected.is_empty());
        assert!(d.rejected.iter().all(|r| !r.reasons.is_empty()));
    }
}
