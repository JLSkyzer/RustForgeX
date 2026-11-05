//! C-43 : Explainability Engine — la chaine causale d'une decision, en raisons a codes
//! stables.
//!
//! Cahier des charges : PARTIE 5.41 (R-640, R-641), INV-11. Decision : ADR-070. Tests :
//! T-460, T-461. Maturite : `STABLE` pour le perimetre de M3.
//!
//! Une decision de C-15 se lit en trois parts :
//!
//! - **les contraintes bloquantes**, une raison par contrainte (R-640) : chaque motif de
//!   rejet DM-15 de chaque genre, chaque genre ecarte par le niveau du mode ou du jalon,
//!   l'absence de speedup mesure (R-410), un score sous le seuil d'activation, un verdict
//!   de compatibilite restrictif, chaque regle de risque qui a pose un plancher ;
//! - **les contributions au score**, les trois plus fortes de chaque signe : gain
//!   attendu, cout de soumission, penalite de risque ;
//! - **l'issue** : pourquoi cette strategie, et pas une autre.
//!
//! Les codes ne changent jamais de numero ni de nom (R-641) : l'interface et les tests
//! les lisent, et la traduction se fait par leur nom. Un code retire n'est jamais
//! reutilise ; un code nouveau prend un numero libre.

use rfx_model::{RejectReason, RuntimeMode, StrategyKind};

use crate::compat::{CompatVerdict, Source, Verdict};
use crate::decision::{Decision, ModePolicy};
use crate::risk::{RiskAssessment, RiskReason};

/// Contributions gardees par signe (R-640).
pub const CONTRIBUTIONS_PER_SIGN: usize = 3;

/// Code d'une raison : numero et nom stables (R-641).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ReasonCode {
    /// DM-15 : effets de bord inconnus.
    UnknownSideEffect,
    /// DM-15 : etat partage non protege.
    UnsafeSharedState,
    /// DM-15 : l'ordre relatif est observable.
    OrderDependency,
    /// DM-15 : le fil autoritatif est requis.
    ThreadAffinity,
    /// DM-15 : comportement non deterministe.
    NonDeterministicBehavior,
    /// DM-15 : cout de transfert trop eleve.
    HighTransferCost,
    /// DM-15 : cout de synchronisation trop eleve.
    HighSynchronizationCost,
    /// DM-15 : dependance JNI non resolue.
    UnresolvedJniDependency,
    /// DM-15 : appel natif non modelise.
    UnmodeledNativeCall,
    /// DM-15 : confiance insuffisante.
    InsufficientConfidence,
    /// DM-15 : budget depasse.
    BudgetExceeded,
    /// DM-15 : regle de compatibilite.
    CompatibilityRule,
    /// DM-15 : validation echouee.
    ValidationFailed,
    /// DM-15 : regression detectee.
    RegressionDetected,
    /// DM-15 : materiel insuffisant.
    HardwareUnsuitable,
    /// DM-15 : cache invalide.
    CacheInvalidated,
    /// DM-15 : desactive par l'utilisateur.
    UserDisabled,
    /// Le niveau du genre depasse celui que le mode autorise.
    LevelAboveMode,
    /// Le niveau du genre depasse celui du jalon en cours.
    LevelAboveMilestone,
    /// Aucun speedup mesure : le gain vaut zero (R-410).
    NoMeasuredSpeedup,
    /// Score sous le seuil d'activation du mode.
    BelowActivationThreshold,
    /// Le verdict de compatibilite restreint l'unite (C-33).
    CompatibilityVerdict,
    /// R1 : drapeau `UNKNOWN`.
    RiskUnknown,
    /// R2 : effets de bord en partie inconnus.
    RiskUnknownEffects,
    /// R3 : pas prouve deterministe.
    RiskNonDeterministic,
    /// R4 : le fil autoritatif n'est pas exclu.
    RiskMainThread,
    /// R5 : ecrit des blocs ou des entites.
    RiskWorldWrite,
    /// R6 : graphe tronque ou appel non resolu.
    RiskIncomplete,
    /// R7 : une regle de compatibilite vise l'unite.
    RiskCompatibility,
    /// R8 : des retours arriere ont deja eu lieu.
    RiskRollbacks,
    /// R9 : une execution comparee a diverge.
    RiskMismatch,
    /// R10 : risque de base, `1 - confiance`.
    RiskConfidence,
    /// Contribution positive : gain attendu d'un genre.
    ExpectedGain,
    /// Contribution negative : cout de soumission des taches.
    SchedulingCost,
    /// Contribution negative : penalite de risque.
    RiskPenalty,
    /// Issue : le meilleur score au-dessus du seuil.
    BestScore,
    /// Issue : aucun genre au-dessus du seuil, le code Java reste tel quel.
    JavaOnlyFallback,
}

impl ReasonCode {
    /// Tous les codes, dans l'ordre de leur numero.
    pub const ALL: [Self; 37] = [
        Self::UnknownSideEffect,
        Self::UnsafeSharedState,
        Self::OrderDependency,
        Self::ThreadAffinity,
        Self::NonDeterministicBehavior,
        Self::HighTransferCost,
        Self::HighSynchronizationCost,
        Self::UnresolvedJniDependency,
        Self::UnmodeledNativeCall,
        Self::InsufficientConfidence,
        Self::BudgetExceeded,
        Self::CompatibilityRule,
        Self::ValidationFailed,
        Self::RegressionDetected,
        Self::HardwareUnsuitable,
        Self::CacheInvalidated,
        Self::UserDisabled,
        Self::LevelAboveMode,
        Self::LevelAboveMilestone,
        Self::NoMeasuredSpeedup,
        Self::BelowActivationThreshold,
        Self::CompatibilityVerdict,
        Self::RiskUnknown,
        Self::RiskUnknownEffects,
        Self::RiskNonDeterministic,
        Self::RiskMainThread,
        Self::RiskWorldWrite,
        Self::RiskIncomplete,
        Self::RiskCompatibility,
        Self::RiskRollbacks,
        Self::RiskMismatch,
        Self::RiskConfidence,
        Self::ExpectedGain,
        Self::SchedulingCost,
        Self::RiskPenalty,
        Self::BestScore,
        Self::JavaOnlyFallback,
    ];

    /// Numero stable : centaine par famille (1 rejet DM-15, 2 politique, 3 risque,
    /// 5 contribution, 6 issue).
    #[must_use]
    pub fn id(self) -> u16 {
        match self {
            Self::UnknownSideEffect => 101,
            Self::UnsafeSharedState => 102,
            Self::OrderDependency => 103,
            Self::ThreadAffinity => 104,
            Self::NonDeterministicBehavior => 105,
            Self::HighTransferCost => 106,
            Self::HighSynchronizationCost => 107,
            Self::UnresolvedJniDependency => 108,
            Self::UnmodeledNativeCall => 109,
            Self::InsufficientConfidence => 110,
            Self::BudgetExceeded => 111,
            Self::CompatibilityRule => 112,
            Self::ValidationFailed => 113,
            Self::RegressionDetected => 114,
            Self::HardwareUnsuitable => 115,
            Self::CacheInvalidated => 116,
            Self::UserDisabled => 117,
            Self::LevelAboveMode => 201,
            Self::LevelAboveMilestone => 202,
            Self::NoMeasuredSpeedup => 203,
            Self::BelowActivationThreshold => 204,
            Self::CompatibilityVerdict => 205,
            Self::RiskUnknown => 301,
            Self::RiskUnknownEffects => 302,
            Self::RiskNonDeterministic => 303,
            Self::RiskMainThread => 304,
            Self::RiskWorldWrite => 305,
            Self::RiskIncomplete => 306,
            Self::RiskCompatibility => 307,
            Self::RiskRollbacks => 308,
            Self::RiskMismatch => 309,
            Self::RiskConfidence => 310,
            Self::ExpectedGain => 501,
            Self::SchedulingCost => 502,
            Self::RiskPenalty => 503,
            Self::BestScore => 601,
            Self::JavaOnlyFallback => 602,
        }
    }

    /// Nom stable, cle de traduction (`rustforgex.reason.<nom>`).
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::UnknownSideEffect => "unknown_side_effect",
            Self::UnsafeSharedState => "unsafe_shared_state",
            Self::OrderDependency => "order_dependency",
            Self::ThreadAffinity => "thread_affinity",
            Self::NonDeterministicBehavior => "non_deterministic_behavior",
            Self::HighTransferCost => "high_transfer_cost",
            Self::HighSynchronizationCost => "high_synchronization_cost",
            Self::UnresolvedJniDependency => "unresolved_jni_dependency",
            Self::UnmodeledNativeCall => "unmodeled_native_call",
            Self::InsufficientConfidence => "insufficient_confidence",
            Self::BudgetExceeded => "budget_exceeded",
            Self::CompatibilityRule => "compatibility_rule",
            Self::ValidationFailed => "validation_failed",
            Self::RegressionDetected => "regression_detected",
            Self::HardwareUnsuitable => "hardware_unsuitable",
            Self::CacheInvalidated => "cache_invalidated",
            Self::UserDisabled => "user_disabled",
            Self::LevelAboveMode => "level_above_mode",
            Self::LevelAboveMilestone => "level_above_milestone",
            Self::NoMeasuredSpeedup => "no_measured_speedup",
            Self::BelowActivationThreshold => "below_activation_threshold",
            Self::CompatibilityVerdict => "compatibility_verdict",
            Self::RiskUnknown => "risk_unknown",
            Self::RiskUnknownEffects => "risk_unknown_effects",
            Self::RiskNonDeterministic => "risk_non_deterministic",
            Self::RiskMainThread => "risk_main_thread",
            Self::RiskWorldWrite => "risk_world_write",
            Self::RiskIncomplete => "risk_incomplete",
            Self::RiskCompatibility => "risk_compatibility",
            Self::RiskRollbacks => "risk_rollbacks",
            Self::RiskMismatch => "risk_mismatch",
            Self::RiskConfidence => "risk_confidence",
            Self::ExpectedGain => "expected_gain",
            Self::SchedulingCost => "scheduling_cost",
            Self::RiskPenalty => "risk_penalty",
            Self::BestScore => "best_score",
            Self::JavaOnlyFallback => "java_only_fallback",
        }
    }

    /// `true` pour une contrainte bloquante (familles 1 a 3).
    #[must_use]
    pub fn is_blocking(self) -> bool {
        self.id() < 500
    }

    fn of_reject(reason: RejectReason) -> Self {
        match reason {
            RejectReason::UnknownSideEffect => Self::UnknownSideEffect,
            RejectReason::UnsafeSharedState => Self::UnsafeSharedState,
            RejectReason::OrderDependency => Self::OrderDependency,
            RejectReason::ThreadAffinity => Self::ThreadAffinity,
            RejectReason::NonDeterministicBehavior => Self::NonDeterministicBehavior,
            RejectReason::HighTransferCost => Self::HighTransferCost,
            RejectReason::HighSynchronizationCost => Self::HighSynchronizationCost,
            RejectReason::UnresolvedJniDependency => Self::UnresolvedJniDependency,
            RejectReason::UnmodeledNativeCall => Self::UnmodeledNativeCall,
            RejectReason::InsufficientConfidence => Self::InsufficientConfidence,
            RejectReason::BudgetExceeded => Self::BudgetExceeded,
            RejectReason::CompatibilityRule => Self::CompatibilityRule,
            RejectReason::ValidationFailed => Self::ValidationFailed,
            RejectReason::RegressionDetected => Self::RegressionDetected,
            RejectReason::HardwareUnsuitable => Self::HardwareUnsuitable,
            RejectReason::CacheInvalidated => Self::CacheInvalidated,
            RejectReason::UserDisabled => Self::UserDisabled,
        }
    }

    fn of_risk(reason: RiskReason) -> Self {
        match reason {
            RiskReason::Unknown => Self::RiskUnknown,
            RiskReason::UnknownEffects => Self::RiskUnknownEffects,
            RiskReason::NonDeterministic => Self::RiskNonDeterministic,
            RiskReason::MainThread => Self::RiskMainThread,
            RiskReason::WorldWrite => Self::RiskWorldWrite,
            RiskReason::Incomplete => Self::RiskIncomplete,
            RiskReason::Compatibility => Self::RiskCompatibility,
            RiskReason::Rollbacks => Self::RiskRollbacks,
            RiskReason::Mismatch => Self::RiskMismatch,
            RiskReason::Confidence => Self::RiskConfidence,
        }
    }
}

/// Ce sur quoi porte une raison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    /// Un genre d'execution candidat.
    Strategy(StrategyKind),
    /// Une regle de risque.
    Rule(RiskReason),
    /// Le verdict de compatibilite, et sa source.
    Compatibility(Verdict, Source),
}

impl Subject {
    /// Libelle technique, non traduit : nom de genre, de regle ou de verdict.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Strategy(strategy) => strategy.name().to_owned(),
            Self::Rule(rule) => rule.rule().to_owned(),
            Self::Compatibility(verdict, source) => format!("{} ({source:?})", verdict.name()),
        }
    }
}

/// D'ou vient ce qui fonde la raison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// Une mesure du profileur.
    Measurement,
    /// Un fait de l'analyse statique.
    StaticFact,
    /// Un incident passe (retour arriere, divergence, quarantaine).
    Incident,
    /// La configuration de l'utilisateur, ou le mode.
    Configuration,
    /// Une limite du jalon en cours.
    Milestone,
}

impl Evidence {
    /// Nom stable.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Measurement => "MEASUREMENT",
            Self::StaticFact => "STATIC_FACT",
            Self::Incident => "INCIDENT",
            Self::Configuration => "CONFIGURATION",
            Self::Milestone => "MILESTONE",
        }
    }
}

/// Une raison (PARTIE 5.41).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reason {
    /// Code stable.
    pub code: ReasonCode,
    /// Ce sur quoi elle porte.
    pub subject: Subject,
    /// Contribution : en ns par tick pour une contribution au score (negative pour un
    /// cout), 1 pour une contrainte bloquante, le score pour l'issue.
    pub weight: f32,
    /// Ce qui la fonde.
    pub evidence: Evidence,
}

/// Ce que l'explication lit, en plus de la decision.
#[derive(Debug, Clone, Copy)]
pub struct Explained<'a> {
    /// Mode du runtime.
    pub mode: RuntimeMode,
    /// Risque de l'unite (C-14).
    pub risk: &'a RiskAssessment,
    /// Verdict de compatibilite (C-33).
    pub compat: CompatVerdict,
    /// Decision (C-15).
    pub decision: &'a Decision,
}

fn blocking(code: ReasonCode, subject: Subject, evidence: Evidence) -> Reason {
    Reason {
        code,
        subject,
        weight: 1.0,
        evidence,
    }
}

/// Fondement d'un motif de rejet DM-15.
fn reject_evidence(reason: RejectReason) -> Evidence {
    match reason {
        RejectReason::ValidationFailed
        | RejectReason::RegressionDetected
        | RejectReason::CacheInvalidated => Evidence::Incident,
        RejectReason::HighTransferCost
        | RejectReason::HighSynchronizationCost
        | RejectReason::BudgetExceeded
        | RejectReason::HardwareUnsuitable => Evidence::Measurement,
        RejectReason::UserDisabled | RejectReason::CompatibilityRule => Evidence::Configuration,
        _ => Evidence::StaticFact,
    }
}

fn risk_evidence(reason: RiskReason) -> Evidence {
    match reason {
        RiskReason::Rollbacks | RiskReason::Mismatch => Evidence::Incident,
        RiskReason::Compatibility => Evidence::Configuration,
        _ => Evidence::StaticFact,
    }
}

fn compat_evidence(source: Source) -> Evidence {
    match source {
        Source::User | Source::Default => Evidence::Configuration,
        Source::Quarantine => Evidence::Incident,
        Source::Detected => Evidence::StaticFact,
        Source::Learned => Evidence::Measurement,
    }
}

/// Les trois plus fortes contributions d'un signe, de la plus forte a la plus faible ;
/// a egalite, dans l'ordre des genres candidats.
fn strongest(mut contributions: Vec<Reason>) -> Vec<Reason> {
    contributions.retain(|r| r.weight != 0.0 && r.weight.is_finite());
    contributions.sort_by(|a, b| b.weight.abs().total_cmp(&a.weight.abs()));
    contributions.truncate(CONTRIBUTIONS_PER_SIGN);
    contributions
}

/// Chaine causale d'une decision (R-640) : contraintes bloquantes, contributions au
/// score, issue. Jamais vide (T-460). Fonction pure.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // des ns par tick, loin des bornes de f32
pub fn explain(input: &Explained<'_>) -> Vec<Reason> {
    let decision = input.decision;
    let policy = ModePolicy::of(input.mode);
    let mut reasons = Vec::new();

    for rejected in &decision.rejected {
        for reason in &rejected.reasons {
            reasons.push(blocking(
                ReasonCode::of_reject(*reason),
                Subject::Strategy(rejected.strategy),
                reject_evidence(*reason),
            ));
        }
    }
    for strategy in &decision.capped {
        let above_mode = strategy
            .level()
            .is_none_or(|level| level > policy.max_level);
        let (code, evidence) = if above_mode {
            (ReasonCode::LevelAboveMode, Evidence::Configuration)
        } else {
            (ReasonCode::LevelAboveMilestone, Evidence::Milestone)
        };
        reasons.push(blocking(code, Subject::Strategy(*strategy), evidence));
    }
    for scored in &decision.scored {
        if scored.strategy == StrategyKind::JavaOnly {
            continue;
        }
        if !scored.measured {
            reasons.push(blocking(
                ReasonCode::NoMeasuredSpeedup,
                Subject::Strategy(scored.strategy),
                Evidence::Measurement,
            ));
        } else if scored.score_ns <= policy.activation_threshold_ns {
            reasons.push(blocking(
                ReasonCode::BelowActivationThreshold,
                Subject::Strategy(scored.strategy),
                Evidence::Configuration,
            ));
        }
    }
    if input.compat.verdict != Verdict::Allow {
        reasons.push(blocking(
            ReasonCode::CompatibilityVerdict,
            Subject::Compatibility(input.compat.verdict, input.compat.source),
            compat_evidence(input.compat.source),
        ));
    }
    for rule in &input.risk.reasons {
        reasons.push(blocking(
            ReasonCode::of_risk(*rule),
            Subject::Rule(*rule),
            risk_evidence(*rule),
        ));
    }

    let mut positive = Vec::new();
    let mut negative = Vec::new();
    for scored in &decision.scored {
        let subject = Subject::Strategy(scored.strategy);
        let evidence = if scored.measured {
            Evidence::Measurement
        } else {
            Evidence::StaticFact
        };
        positive.push(Reason {
            code: ReasonCode::ExpectedGain,
            subject,
            weight: scored.gain_ns as f32,
            evidence,
        });
        negative.push(Reason {
            code: ReasonCode::SchedulingCost,
            subject,
            weight: -(scored.scheduling_ns as f32),
            evidence: Evidence::StaticFact,
        });
        negative.push(Reason {
            code: ReasonCode::RiskPenalty,
            subject,
            weight: -(scored.risk_penalty_ns as f32),
            evidence: Evidence::StaticFact,
        });
    }
    reasons.extend(strongest(positive));
    reasons.extend(strongest(negative));

    reasons.push(if decision.strategy == StrategyKind::JavaOnly {
        Reason {
            code: ReasonCode::JavaOnlyFallback,
            subject: Subject::Strategy(StrategyKind::JavaOnly),
            weight: 0.0,
            evidence: Evidence::Configuration,
        }
    } else {
        Reason {
            code: ReasonCode::BestScore,
            subject: Subject::Strategy(decision.strategy),
            weight: decision.score_ns as f32,
            evidence: Evidence::Measurement,
        }
    });
    reasons
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::{decide, DecisionInputs};
    use crate::risk::Blast;
    use rfx_model::{ClassificationFlags, Tri};

    /// T-461 : les codes ne changent ni de numero ni de nom. Cet instantane ne se met a
    /// jour que pour **ajouter** un code ; modifier ou retirer une ligne casse l'interface
    /// et les traductions (R-641).
    #[test]
    fn reason_codes_are_stable() {
        let snapshot: Vec<(u16, &str)> =
            ReasonCode::ALL.iter().map(|c| (c.id(), c.key())).collect();
        let expected = [
            (101, "unknown_side_effect"),
            (102, "unsafe_shared_state"),
            (103, "order_dependency"),
            (104, "thread_affinity"),
            (105, "non_deterministic_behavior"),
            (106, "high_transfer_cost"),
            (107, "high_synchronization_cost"),
            (108, "unresolved_jni_dependency"),
            (109, "unmodeled_native_call"),
            (110, "insufficient_confidence"),
            (111, "budget_exceeded"),
            (112, "compatibility_rule"),
            (113, "validation_failed"),
            (114, "regression_detected"),
            (115, "hardware_unsuitable"),
            (116, "cache_invalidated"),
            (117, "user_disabled"),
            (201, "level_above_mode"),
            (202, "level_above_milestone"),
            (203, "no_measured_speedup"),
            (204, "below_activation_threshold"),
            (205, "compatibility_verdict"),
            (301, "risk_unknown"),
            (302, "risk_unknown_effects"),
            (303, "risk_non_deterministic"),
            (304, "risk_main_thread"),
            (305, "risk_world_write"),
            (306, "risk_incomplete"),
            (307, "risk_compatibility"),
            (308, "risk_rollbacks"),
            (309, "risk_mismatch"),
            (310, "risk_confidence"),
            (501, "expected_gain"),
            (502, "scheduling_cost"),
            (503, "risk_penalty"),
            (601, "best_score"),
            (602, "java_only_fallback"),
        ];
        assert_eq!(snapshot, expected);
    }

    #[test]
    fn every_reject_and_risk_reason_has_its_own_code() {
        let mut ids: Vec<u16> = ReasonCode::ALL.iter().map(|c| c.id()).collect();
        ids.dedup();
        assert_eq!(
            ids.len(),
            ReasonCode::ALL.len(),
            "numeros uniques et croissants"
        );
        let mut keys: Vec<&str> = ReasonCode::ALL.iter().map(|c| c.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), ReasonCode::ALL.len(), "noms uniques");
        for rule in RiskReason::ALL {
            assert!(ReasonCode::of_risk(rule).key().starts_with("risk_"));
            assert!(ReasonCode::of_risk(rule).is_blocking());
        }
        assert!(!ReasonCode::ExpectedGain.is_blocking());
    }

    /// Chaque code a son texte dans les deux langues obligatoires : une cle ajoutee dans
    /// l'une et pas dans l'autre est un defaut.
    #[test]
    fn every_code_is_translated_in_both_languages() {
        let languages = [
            include_str!("../../../src/main/resources/assets/rustforgex/lang/en_us.json"),
            include_str!("../../../src/main/resources/assets/rustforgex/lang/fr_fr.json"),
        ];
        for code in ReasonCode::ALL {
            let key = format!("\"rustforgex.reason.{}\"", code.key());
            for language in languages {
                assert!(language.contains(&key), "{key} manque");
            }
        }
    }

    /// Un risque construit a la main : la correction, la portee, les regles citees.
    fn risk_of(correctness: f32, blast: Blast, reasons: &[RiskReason]) -> RiskAssessment {
        RiskAssessment {
            correctness_risk: correctness,
            stability_risk: 0.0,
            compat_risk: 0.0,
            data_risk: 0.0,
            max_blast_radius: blast,
            reasons: reasons.to_vec(),
        }
    }

    fn evaluate(
        flags: ClassificationFlags,
        main_thread: Tri,
        risk: RiskAssessment,
        mode: RuntimeMode,
        speedups: &[(StrategyKind, f64)],
        compat: CompatVerdict,
    ) -> (RiskAssessment, Decision) {
        let decision = decide(&DecisionInputs {
            flags,
            main_thread_required: main_thread,
            risk: &risk,
            compat,
            mode,
            logical_cores: 8,
            calls_per_tick: 2_000.0,
            cpu_ns_p50: 400.0,
            measured_speedups: speedups,
        });
        (risk, decision)
    }

    /// T-460 : toute decision a des raisons ; chaque contrainte bloquante en a une ;
    /// au plus trois contributions par signe ; l'issue est dite une fois.
    #[test]
    fn every_decision_has_its_reasons() {
        let batchable = flags(&[
            ClassificationFlags::PURE,
            ClassificationFlags::BATCHABLE,
            ClassificationFlags::DETERMINISTIC,
            ClassificationFlags::THREAD_SAFE,
            ClassificationFlags::PARALLELIZABLE,
        ]);
        let flag_sets = [
            ClassificationFlags::EMPTY,
            batchable,
            ClassificationFlags::UNKNOWN,
            flags(&[
                ClassificationFlags::READ_ONLY,
                ClassificationFlags::DETERMINISTIC,
            ]),
            flags(&[
                ClassificationFlags::ORDER_SENSITIVE,
                ClassificationFlags::MAIN_THREAD_REQUIRED,
            ]),
        ];
        let risks = [
            risk_of(0.01, Blast::Task, &[RiskReason::Confidence]),
            risk_of(
                0.5,
                Blast::Workload,
                &[RiskReason::NonDeterministic, RiskReason::Confidence],
            ),
            risk_of(1.0, Blast::World, &RiskReason::ALL),
        ];
        let modes = [
            RuntimeMode::Safe,
            RuntimeMode::Balanced,
            RuntimeMode::Performance,
            RuntimeMode::Experimental,
        ];
        let measured = [
            (StrategyKind::JavaBatch, 4.0),
            (StrategyKind::JavaParallel, 3.0),
            (StrategyKind::RustParallel, 1.0001),
        ];
        // Un speedup mesure mais faible : un gain sous le seuil de tous les modes.
        let weak = [(StrategyKind::JavaBatch, 1.01)];
        let verdicts = [
            CompatVerdict {
                verdict: Verdict::Allow,
                source: Source::Default,
            },
            CompatVerdict {
                verdict: Verdict::ForceSerial,
                source: Source::Detected,
            },
            CompatVerdict {
                verdict: Verdict::ForceJava,
                source: Source::User,
            },
        ];
        let mut cases = 0;
        let mut below = 0;
        for flags in flag_sets {
            for main in [Tri::Yes, Tri::No, Tri::Unknown] {
                for risk in &risks {
                    for mode in modes {
                        for speedups in [&[][..], &measured[..], &weak[..]] {
                            for compat in verdicts {
                                let (risk, decision) =
                                    evaluate(flags, main, risk.clone(), mode, speedups, compat);
                                let reasons = explain(&Explained {
                                    mode,
                                    risk: &risk,
                                    compat,
                                    decision: &decision,
                                });
                                check(&reasons, &risk, compat, &decision);
                                below += reasons
                                    .iter()
                                    .filter(|r| r.code == ReasonCode::BelowActivationThreshold)
                                    .count();
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 5 * 3 * 3 * 4 * 3 * 3);
        assert!(below > 0, "le cas sous le seuil a ete explore");
    }

    fn flags(all: &[ClassificationFlags]) -> ClassificationFlags {
        ClassificationFlags(all.iter().fold(0, |acc, f| acc | f.0))
    }

    fn batchable() -> ClassificationFlags {
        flags(&[
            ClassificationFlags::PURE,
            ClassificationFlags::BATCHABLE,
            ClassificationFlags::DETERMINISTIC,
            ClassificationFlags::THREAD_SAFE,
            ClassificationFlags::PARALLELIZABLE,
        ])
    }

    fn count(reasons: &[Reason], code: ReasonCode, subject: Subject) -> usize {
        reasons
            .iter()
            .filter(|r| r.code == code && r.subject == subject)
            .count()
    }

    fn check(
        reasons: &[Reason],
        risk: &RiskAssessment,
        compat: CompatVerdict,
        decision: &Decision,
    ) {
        assert!(!reasons.is_empty(), "{decision:?}");
        for rejected in &decision.rejected {
            for reason in &rejected.reasons {
                assert_eq!(
                    count(
                        reasons,
                        ReasonCode::of_reject(*reason),
                        Subject::Strategy(rejected.strategy)
                    ),
                    1,
                    "{reason:?} de {:?}",
                    rejected.strategy
                );
            }
        }
        for strategy in &decision.capped {
            let capped = reasons.iter().filter(|r| {
                r.subject == Subject::Strategy(*strategy)
                    && matches!(
                        r.code,
                        ReasonCode::LevelAboveMode | ReasonCode::LevelAboveMilestone
                    )
            });
            assert_eq!(capped.count(), 1, "{strategy:?}");
        }
        for rule in &risk.reasons {
            assert_eq!(
                count(reasons, ReasonCode::of_risk(*rule), Subject::Rule(*rule)),
                1
            );
        }
        let compat_reasons = reasons
            .iter()
            .filter(|r| r.code == ReasonCode::CompatibilityVerdict)
            .count();
        assert_eq!(
            compat_reasons,
            usize::from(compat.verdict != Verdict::Allow)
        );
        for scored in decision
            .scored
            .iter()
            .filter(|s| s.strategy != StrategyKind::JavaOnly && s.strategy != decision.strategy)
        {
            let subject = Subject::Strategy(scored.strategy);
            assert_eq!(
                count(reasons, ReasonCode::NoMeasuredSpeedup, subject)
                    + count(reasons, ReasonCode::BelowActivationThreshold, subject),
                1,
                "un genre faisable non retenu dit pourquoi : {scored:?}"
            );
        }
        let positives = reasons
            .iter()
            .filter(|r| r.code == ReasonCode::ExpectedGain)
            .count();
        let negatives = reasons
            .iter()
            .filter(|r| matches!(r.code, ReasonCode::SchedulingCost | ReasonCode::RiskPenalty))
            .count();
        assert!(positives <= CONTRIBUTIONS_PER_SIGN && negatives <= CONTRIBUTIONS_PER_SIGN);
        let outcomes: Vec<&Reason> = reasons
            .iter()
            .filter(|r| matches!(r.code, ReasonCode::BestScore | ReasonCode::JavaOnlyFallback))
            .collect();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].subject, Subject::Strategy(decision.strategy));
    }

    /// R-640 : les trois plus fortes de chaque signe, les plus fortes d'abord.
    #[test]
    fn the_three_strongest_contributions_of_each_sign_are_kept() {
        let reason = |weight| Reason {
            code: ReasonCode::ExpectedGain,
            subject: Subject::Strategy(StrategyKind::JavaBatch),
            weight,
            evidence: Evidence::Measurement,
        };
        let kept = strongest(vec![
            reason(5.0),
            reason(0.0),
            reason(9.0),
            reason(1.0),
            reason(7.0),
        ]);
        let weights: Vec<f32> = kept.iter().map(|r| r.weight).collect();
        assert_eq!(weights, vec![9.0, 7.0, 5.0]);
        let negative = strongest(vec![reason(-1.0), reason(-8.0), reason(f32::NAN)]);
        let weights: Vec<f32> = negative.iter().map(|r| r.weight).collect();
        assert_eq!(weights, vec![-8.0, -1.0]);
    }

    /// Une unite sans mesure : le seul genre faisable au-dessus de JAVA_ONLY attend son
    /// speedup ; les genres au-dessus du jalon le disent ; l'issue est le repli.
    #[test]
    fn an_unmeasured_unit_explains_why_it_stays_in_java() {
        let compat = CompatVerdict {
            verdict: Verdict::Allow,
            source: Source::Default,
        };
        let (risk, decision) = evaluate(
            batchable(),
            Tri::No,
            risk_of(0.01, Blast::Task, &[RiskReason::Confidence]),
            RuntimeMode::Balanced,
            &[],
            compat,
        );
        let reasons = explain(&Explained {
            mode: RuntimeMode::Balanced,
            risk: &risk,
            compat,
            decision: &decision,
        });
        assert_eq!(decision.strategy, StrategyKind::JavaOnly);
        assert!(reasons
            .iter()
            .any(|r| r.code == ReasonCode::NoMeasuredSpeedup
                && r.subject == Subject::Strategy(StrategyKind::JavaBatch)));
        assert!(reasons
            .iter()
            .any(|r| r.code == ReasonCode::LevelAboveMilestone));
        assert_eq!(
            reasons.last().map(|r| r.code),
            Some(ReasonCode::JavaOnlyFallback)
        );
    }

    /// Un speedup mesure qui passe le seuil : l'issue nomme le genre retenu, et son gain
    /// est la plus forte contribution positive.
    #[test]
    fn a_measured_gain_is_named_as_the_outcome() {
        let compat = CompatVerdict {
            verdict: Verdict::Allow,
            source: Source::Default,
        };
        let speedups = [(StrategyKind::JavaBatch, 4.0)];
        let (risk, decision) = evaluate(
            batchable(),
            Tri::No,
            risk_of(0.01, Blast::Task, &[RiskReason::Confidence]),
            RuntimeMode::Balanced,
            &speedups,
            compat,
        );
        assert_eq!(decision.strategy, StrategyKind::JavaBatch, "{decision:?}");
        let reasons = explain(&Explained {
            mode: RuntimeMode::Balanced,
            risk: &risk,
            compat,
            decision: &decision,
        });
        let gain = reasons
            .iter()
            .find(|r| r.code == ReasonCode::ExpectedGain)
            .expect("gain");
        assert_eq!(gain.subject, Subject::Strategy(StrategyKind::JavaBatch));
        assert!(gain.weight > 0.0);
        assert_eq!(reasons.last().map(|r| r.code), Some(ReasonCode::BestScore));
    }
}
