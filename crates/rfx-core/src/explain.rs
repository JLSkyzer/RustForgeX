//! C-43 (premier pas) : le bloc `DECISION` de la fiche d'une unite (`/rfx why`).
//!
//! Cahier des charges : PARTIE 5.33, PARTIE 5.41. Decisions : ADR-063, ADR-066 a
//! ADR-068. Maturite : `STABLE` pour ce qui est assemble ici.
//!
//! Le risque (C-14), le verdict de compatibilite (C-33) et la decision (C-15) sont
//! calcules a la demande, a partir du descripteur et de la mesure de l'unite, et
//! expliques ; rien n'est applique (M3). Les sources de C-33 qui ne sont pas encore
//! branchees (regles de l'utilisateur, quarantaines, transformateurs concurrents) sont
//! absentes : le verdict ne voit que la reflexion. En M3, cela ne change aucune issue,
//! le plafond TL2 du jalon valant deja `FORCE_SERIAL`.

use rfx_decision::{
    assess, decide, verdict, Blast, CompatInputs, DecisionInputs, DetectedProperties, RiskInputs,
};
use rfx_model::{DecisionCard, Fingerprint, RejectedCard, RuntimeMode, TopEntry, WorkDescriptor};

use crate::analysis::Descriptor;

/// Une probabilite en milliemes, pour le lecteur CBOR de Java qui ne lit pas les reels.
fn per_mille(probability: f32) -> u32 {
    if probability.is_nan() {
        return 1_000;
    }
    // Borne a [0, 1000] : la conversion est exacte.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let value = (probability.clamp(0.0, 1.0) * 1_000.0).round() as u32;
    value
}

fn blast_name(blast: Blast) -> &'static str {
    match blast {
        Blast::Task => "TASK",
        Blast::Workload => "WORKLOAD",
        Blast::Subsystem => "SUBSYSTEM",
        Blast::World => "WORLD",
    }
}

/// Bloc `DECISION` d'une unite decrite.
#[must_use]
pub fn decision_card(
    descriptor: &Descriptor,
    measured: Option<&TopEntry>,
    mode: RuntimeMode,
    logical_cores: u16,
) -> DecisionCard {
    let d = descriptor;
    let risk = assess(&RiskInputs {
        flags: d.flags,
        reads: d.reads,
        writes: d.writes,
        side_effects: d.side_effects,
        locks: d.locks,
        thread: d.thread,
        determinism: d.determinism,
        truncated: d.deps.truncated,
        unresolved_calls: d.summary.unresolved_calls,
        confidence: d.confidence,
        compat_risk: None,
        rollbacks: 0,
        validation_mismatches: 0,
    });
    // Sans regle utilisateur ni quarantaine, l'identite et l'empreinte ne sont pas lues.
    let (identity, fingerprint) = (WorkDescriptor::default(), Fingerprint::default());
    let compat = verdict(&CompatInputs {
        descriptor: &identity,
        fingerprint: &fingerprint,
        user: &[],
        quarantines: &[],
        detected: DetectedProperties {
            reflection: d.summary.reflection,
            ..DetectedProperties::default()
        },
        learned: None,
    });
    #[allow(clippy::cast_precision_loss)] // des comptes, loin de 2^52
    let (calls, p50) = measured.map_or((0.0, 0.0), |m| {
        (m.calls_per_tick_x100 as f64 / 100.0, m.cpu_ns_p50 as f64)
    });
    let decision = decide(&DecisionInputs {
        flags: d.flags,
        main_thread_required: d.thread.main_thread_required,
        risk: &risk,
        compat,
        mode,
        logical_cores: u32::from(logical_cores),
        calls_per_tick: calls,
        cpu_ns_p50: p50,
        measured_speedups: &[],
    });
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // borne a >= 0
    let gain = decision.expected_gain_ns.max(0.0).round() as u64;
    DecisionCard {
        mode: format!("{mode:?}"),
        strategy: decision.strategy.name().to_owned(),
        level: decision.level.number(),
        expected_gain_ns: gain,
        correctness_x1000: per_mille(risk.correctness_risk),
        stability_x1000: per_mille(risk.stability_risk),
        compat_x1000: per_mille(risk.compat_risk),
        data_x1000: per_mille(risk.data_risk),
        blast: blast_name(risk.max_blast_radius).to_owned(),
        max_level: risk.max_level().number(),
        risk_rules: risk.reasons.iter().map(|r| r.rule().to_owned()).collect(),
        compat_verdict: compat.verdict.name().to_owned(),
        compat_source: format!("{:?}", compat.source),
        rejected: decision
            .rejected
            .iter()
            .map(|r| RejectedCard {
                strategy: r.strategy.name().to_owned(),
                reasons: r
                    .reasons
                    .iter()
                    .map(|reason| format!("{reason:?}"))
                    .collect(),
            })
            .collect(),
        requires_validation: decision.requires_validation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probabilities_become_per_mille_bounded() {
        assert_eq!(per_mille(0.0), 0);
        assert_eq!(per_mille(0.1234), 123);
        assert_eq!(per_mille(1.0), 1_000);
        assert_eq!(per_mille(7.0), 1_000);
        assert_eq!(per_mille(-1.0), 0);
        assert_eq!(per_mille(f32::NAN), 1_000);
    }

    use crate::analysis::tests::{drive, fixtures, hierarchy, hot};
    use crate::analysis::{Analysis, StepState, MAX_ROUNDS};
    use rfx_analyzer::MethodRef;
    use rfx_model::{RuntimeConfig, Side, WorkId};

    /// Une unite pure et deterministe, sans mesure de speedup : `JAVA_ONLY`, et chaque
    /// genre candidat rejete l'est avec ses motifs (R-219, R-410).
    #[test]
    fn a_pure_unit_stays_java_only_without_a_measurement() {
        let classes = fixtures();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        let pure = WorkId(15);
        analysis.note(
            pure,
            MethodRef::new("fixtures/Chance", "pure", "(II)I"),
            Side::Server,
        );
        analysis.declare(hierarchy(&classes));
        assert_eq!(
            drive(&mut analysis, &[hot(pure)], &classes),
            StepState::Finished
        );
        let card = decision_card(
            analysis.descriptor(pure).unwrap(),
            None,
            RuntimeMode::Balanced,
            8,
        );
        assert_eq!(card.strategy, "JAVA_ONLY");
        assert_eq!(card.level, 0);
        assert_eq!(card.expected_gain_ns, 0, "R-410 : aucun gain sans mesure");
        assert_eq!(card.mode, "Balanced");
        assert_eq!(card.compat_verdict, "ALLOW");
        assert!(!card.requires_validation);
        assert!(card.rejected.iter().all(|r| !r.reasons.is_empty()));
        assert!(card.correctness_x1000 < 1_000, "{card:?}");
    }

    /// Une unite dont l'analyse a epuise son budget est `UNKNOWN` : risque de
    /// correction maximal, R1 nomme, blast radius monde.
    #[test]
    fn an_unknown_unit_has_the_maximal_risk() {
        let classes = fixtures();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        let id = WorkId(7);
        analysis.note(
            id,
            MethodRef::new("fixtures/Calls", "vehicle", "(Lfixtures/Vehicle;)I"),
            Side::Common,
        );
        analysis.declare(hierarchy(&classes));
        for _ in 0..MAX_ROUNDS {
            analysis.step(&[hot(id)]);
        }
        let card = decision_card(analysis.descriptor(id).unwrap(), None, RuntimeMode::Safe, 8);
        assert_eq!(card.strategy, "JAVA_ONLY");
        assert_eq!(card.correctness_x1000, 1_000);
        assert!(card.risk_rules.contains(&"R1".to_owned()), "{card:?}");
        assert_eq!(card.blast, "WORLD");
        assert_eq!(card.max_level, 2);
        assert!(card
            .rejected
            .iter()
            .any(|r| r.reasons.contains(&"InsufficientConfidence".to_owned())));
    }
}
