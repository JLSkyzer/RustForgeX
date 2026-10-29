//! C-14 : Risk Engine — le risque d'une unite de travail, independant du gain attendu.
//!
//! Cahier des charges : PARTIE 5.14. Decision : ADR-066. Tests : T-230, T-231, T-232.
//! Maturite : `STABLE`.
//!
//! Quatre composantes, chacune entre 0 et 1 : correction, stabilite, compatibilite,
//! donnees. Les regles R1..R10 posent chacune un plancher, et les planchers se cumulent ;
//! R10 (`1 - confiance`) est toujours un plancher de la correction. C'est la seule lecture
//! ou ajouter un fait defavorable n'abaisse jamais le risque (T-231, ADR-066).
//!
//! Le *blast radius* borne le niveau de transformation : il dit jusqu'ou les degats
//! iraient si la transformation etait fausse.

use rfx_model::{
    AccessSet, ClassificationFlags, Confidence, DeterminismFacts, Domain, Domains, Precision,
    SideEffectSet, ThreadFacts, TransformationLevel, Tri,
};

/// Domaines du monde : une ecriture la est visible du jeu entier.
const WORLD: [Domain; 9] = [
    Domain::WorldBlocks,
    Domain::WorldFluids,
    Domain::WorldLighting,
    Domain::ChunkData,
    Domain::ChunkStatus,
    Domain::EntityState,
    Domain::EntityInventory,
    Domain::BlockEntityState,
    Domain::LevelGlobals,
];

/// Portee maximale des degats si une transformation est incorrecte, de la plus etroite
/// a la plus large.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Blast {
    /// La tache seule : ni lecture ni ecriture partagee.
    Task,
    /// L'unite de travail : lectures partagees, aucune ecriture partagee.
    Workload,
    /// Un sous-systeme : ecritures partagees hors du monde, verrou partage, appel natif.
    Subsystem,
    /// Le monde : ecritures du monde, effets visibles du jeu, ou inconnu.
    World,
}

impl Blast {
    /// Niveau de transformation maximal autorise (PARTIE 5.14).
    #[must_use]
    pub fn max_level(self) -> TransformationLevel {
        match self {
            Self::Task => TransformationLevel::Tl6,
            Self::Workload => TransformationLevel::Tl5,
            Self::Subsystem => TransformationLevel::Tl4,
            Self::World => TransformationLevel::Tl2,
        }
    }
}

/// Regle qui a pose un plancher (PARTIE 5.14), pour l'explication (C-43).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RiskReason {
    /// R1 : drapeau `UNKNOWN`.
    Unknown,
    /// R2 : effets de bord en partie inconnus.
    UnknownEffects,
    /// R3 : pas prouve deterministe.
    NonDeterministic,
    /// R4 : le fil autoritatif n'est pas exclu.
    MainThread,
    /// R5 : ecrit des blocs ou des entites, hors snapshot.
    WorldWrite,
    /// R6 : graphe tronque ou appel non resolu.
    Incomplete,
    /// R7 : une regle de compatibilite vise l'unite.
    Compatibility,
    /// R8 : des retours arriere ont deja eu lieu.
    Rollbacks,
    /// R9 : une execution comparee a diverge.
    Mismatch,
    /// R10 : risque de base, `1 - confiance`.
    Confidence,
}

impl RiskReason {
    /// Toutes les raisons, dans l'ordre des regles.
    pub const ALL: [Self; 10] = [
        Self::Unknown,
        Self::UnknownEffects,
        Self::NonDeterministic,
        Self::MainThread,
        Self::WorldWrite,
        Self::Incomplete,
        Self::Compatibility,
        Self::Rollbacks,
        Self::Mismatch,
        Self::Confidence,
    ];

    /// Identifiant de la regle, `R1` a `R10`.
    #[must_use]
    pub fn rule(self) -> &'static str {
        match self {
            Self::Unknown => "R1",
            Self::UnknownEffects => "R2",
            Self::NonDeterministic => "R3",
            Self::MainThread => "R4",
            Self::WorldWrite => "R5",
            Self::Incomplete => "R6",
            Self::Compatibility => "R7",
            Self::Rollbacks => "R8",
            Self::Mismatch => "R9",
            Self::Confidence => "R10",
        }
    }
}

/// Ce que le moteur de risque lit : les faits d'un descripteur (C-08), et ce que
/// d'autres composants apportent quand ils existent.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RiskInputs {
    /// Drapeaux (DM-11).
    pub flags: ClassificationFlags,
    /// Lectures partagees (DM-06).
    pub reads: AccessSet,
    /// Ecritures partagees (DM-06).
    pub writes: AccessSet,
    /// Effets de bord (DM-07).
    pub side_effects: SideEffectSet,
    /// Un verrou est pris sur un objet partage.
    pub locks: bool,
    /// Faits de fil (DM-09).
    pub thread: ThreadFacts,
    /// Faits de determinisme (DM-10).
    pub determinism: DeterminismFacts,
    /// Le graphe d'appel est tronque (DM-08).
    pub truncated: bool,
    /// Appels non resolus (DM-08).
    pub unresolved_calls: u32,
    /// Confiance (DM-12).
    pub confidence: Confidence,
    /// Risque de compatibilite fixe par une regle de C-33 ; `None` sans regle.
    pub compat_risk: Option<f32>,
    /// Retours arriere deja subis par cette unite.
    pub rollbacks: u32,
    /// Divergences relevees par la validation (C-29), en plus de celles du
    /// determinisme.
    pub validation_mismatches: u32,
}

/// Risque d'une unite de travail (PARTIE 5.14).
#[derive(Debug, Clone, PartialEq)]
pub struct RiskAssessment {
    /// Probabilite de changer le comportement.
    pub correctness_risk: f32,
    /// Crash, interblocage, depassement de delai.
    pub stability_risk: f32,
    /// Conflit avec un autre mod ou un transformateur.
    pub compat_risk: f32,
    /// Corruption d'etat ou de sauvegarde.
    pub data_risk: f32,
    /// Portee maximale des degats.
    pub max_blast_radius: Blast,
    /// Regles qui ont pose un plancher, dans l'ordre des regles.
    pub reasons: Vec<RiskReason>,
}

impl RiskAssessment {
    /// Niveau de transformation maximal que le blast radius autorise.
    #[must_use]
    pub fn max_level(&self) -> TransformationLevel {
        self.max_blast_radius.max_level()
    }
}

/// Une probabilite ramenee dans `[0, 1]` ; une valeur qui n'est pas un nombre vaut le
/// pire cas (UNKNOWN = CONSERVATIVE).
fn probability(value: f32) -> f32 {
    if value.is_nan() {
        1.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

/// Domaines du monde et l'inconnu, qui peut les contenir.
fn world_or_unknown() -> Domains {
    WORLD
        .iter()
        .fold(Domains::EMPTY.with(Domain::Unknown), |set, d| set.with(*d))
}

/// Portee maximale des degats (ADR-066, decision 3).
#[must_use]
pub fn blast_radius(inputs: &RiskInputs) -> Blast {
    let e = &inputs.side_effects;
    let writes = inputs.writes;
    let world = inputs.flags.contains(ClassificationFlags::UNKNOWN)
        || e.unknown
        || e.mutates_world
        || e.spawns_or_removes_entity
        || e.sends_packets
        || e.io
        || e.fires_events
        || e.schedules_tasks
        || e.cancels_event
        || (writes.precision == Precision::Unknown && !writes.domains.is_empty())
        || writes.domains.intersects(world_or_unknown());
    if world {
        return Blast::World;
    }
    if !writes.domains.is_empty() || inputs.locks || e.native_call || e.throws_observable {
        return Blast::Subsystem;
    }
    if !inputs.reads.domains.is_empty() {
        return Blast::Workload;
    }
    Blast::Task
}

/// Evalue le risque d'une unite (PARTIE 5.14, ADR-066). Fonction pure.
#[must_use]
pub fn assess(inputs: &RiskInputs) -> RiskAssessment {
    let mut correctness = 0.0_f32;
    let mut stability = 0.0_f32;
    let mut compat = 0.0_f32;
    let mut data = 0.0_f32;
    let mut reasons = Vec::new();
    let mut floor = |value: &mut f32, to: f32, reason: RiskReason, applies: bool| {
        if applies {
            *value = value.max(to);
            reasons.push(reason);
        }
    };

    let writes_world = inputs.writes.domains.intersects(
        Domains::EMPTY
            .with(Domain::WorldBlocks)
            .with(Domain::EntityState)
            .with(Domain::Unknown),
    );
    let mismatches = inputs
        .determinism
        .mismatches
        .saturating_add(inputs.validation_mismatches);

    floor(
        &mut correctness,
        1.0,
        RiskReason::Unknown,
        inputs.flags.contains(ClassificationFlags::UNKNOWN),
    );
    floor(
        &mut correctness,
        0.9,
        RiskReason::UnknownEffects,
        inputs.side_effects.unknown,
    );
    floor(
        &mut correctness,
        0.8,
        RiskReason::NonDeterministic,
        inputs.determinism.deterministic != Tri::Yes,
    );
    floor(
        &mut stability,
        0.8,
        RiskReason::MainThread,
        inputs.thread.main_thread_required != Tri::No,
    );
    floor(&mut data, 1.0, RiskReason::WorldWrite, writes_world);
    floor(
        &mut correctness,
        0.7,
        RiskReason::Incomplete,
        inputs.truncated || inputs.unresolved_calls > 0,
    );
    if let Some(rule) = inputs.compat_risk {
        floor(
            &mut compat,
            probability(rule),
            RiskReason::Compatibility,
            true,
        );
    }
    // R8 ajoute 0,3 par retour arriere : un plancher a la valeur courante augmentee,
    // ce qui revient au meme et garde chaque regle sous la meme forme.
    let after_rollbacks = (stability + 0.3 * inputs.rollbacks.min(4) as f32).min(1.0);
    floor(
        &mut stability,
        after_rollbacks,
        RiskReason::Rollbacks,
        inputs.rollbacks > 0,
    );
    floor(&mut correctness, 1.0, RiskReason::Mismatch, mismatches > 0);

    // R10 : toujours un plancher ; nomme seulement si aucune autre regle de correction
    // ne s'applique (ADR-066, decision 2).
    let base = probability(1.0 - inputs.confidence.combined);
    let named_correction = reasons.iter().any(|r| {
        matches!(
            r,
            RiskReason::Unknown
                | RiskReason::UnknownEffects
                | RiskReason::NonDeterministic
                | RiskReason::Incomplete
                | RiskReason::Mismatch
        )
    });
    correctness = correctness.max(base);
    if !named_correction {
        reasons.push(RiskReason::Confidence);
    }

    RiskAssessment {
        correctness_risk: probability(correctness),
        stability_risk: probability(stability),
        compat_risk: probability(compat),
        data_risk: probability(data),
        max_blast_radius: blast_radius(inputs),
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une unite sans aucun fait defavorable : pure, deterministe, hors du fil
    /// autoritatif, analysee entierement, confiance pleine.
    fn clean() -> RiskInputs {
        RiskInputs {
            determinism: DeterminismFacts {
                deterministic: Tri::Yes,
                ..DeterminismFacts::default()
            },
            thread: ThreadFacts {
                main_thread_required: Tri::No,
                ..ThreadFacts::default()
            },
            reads: AccessSet {
                domains: Domains::EMPTY,
                precision: Precision::Exact,
            },
            writes: AccessSet {
                domains: Domains::EMPTY,
                precision: Precision::Exact,
            },
            confidence: Confidence::new(1.0, 1.0, 1.0),
            ..RiskInputs::default()
        }
    }

    /// Les faits defavorables, un ou deux par regle, tels que la fiche les enonce.
    const TOGGLES: usize = 11;

    /// Bits des faits qui relevent d'une regle de correction : R1, R2, R3, R6 (deux
    /// faits) et R9.
    const CORRECTION_TOGGLES: u32 = 0b10_0110_0111;

    fn worsen(inputs: &mut RiskInputs, toggle: usize) {
        match toggle {
            0 => inputs.flags = inputs.flags.set(ClassificationFlags::UNKNOWN, true),
            1 => inputs.side_effects.unknown = true,
            2 => inputs.determinism.deterministic = Tri::Unknown,
            3 => inputs.thread.main_thread_required = Tri::Unknown,
            4 => inputs.writes.domains = inputs.writes.domains.with(Domain::EntityState),
            5 => inputs.truncated = true,
            6 => inputs.unresolved_calls += 2,
            7 => inputs.compat_risk = Some(0.6),
            8 => inputs.rollbacks += 1,
            9 => inputs.determinism.mismatches += 1,
            // Base 0,9 : au-dessus des planchers de R3 et R6, pour qu'une lecture ou R10
            // ne vaudrait que « sinon » se voie (ADR-066, decision 2).
            10 => inputs.confidence = Confidence::new(0.1, 0.1, 0.0),
            _ => unreachable!(),
        }
    }

    fn with(mask: u32) -> RiskInputs {
        let mut inputs = clean();
        for toggle in 0..TOGGLES {
            if mask & (1 << toggle) != 0 {
                worsen(&mut inputs, toggle);
            }
        }
        inputs
    }

    /// Ce que la fiche impose, recalcule a part de l'implementation : chaque fait pose
    /// son plancher sur sa composante, les planchers se cumulent, R10 toujours.
    fn expected(mask: u32) -> (f32, f32, f32, f32) {
        let on = |t: usize| mask & (1 << t) != 0;
        let base = 1.0 - with(mask).confidence.combined;
        let mut correctness = base.max(0.0);
        for (t, value) in [(0, 1.0), (1, 0.9), (2, 0.8), (5, 0.7), (6, 0.7), (9, 1.0)] {
            if on(t) {
                correctness = correctness.max(value);
            }
        }
        let mut stability = if on(3) { 0.8_f32 } else { 0.0 };
        if on(8) {
            stability = (stability + 0.3).min(1.0);
        }
        let compat = if on(7) { 0.6 } else { 0.0 };
        let data = if on(4) { 1.0 } else { 0.0 };
        (correctness, stability, compat, data)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    /// T-230 : table de verite des regles R1..R10, sur toutes les combinaisons des
    /// onze faits defavorables (2 048).
    #[test]
    fn t230_every_combination_follows_the_rules() {
        for mask in 0..(1_u32 << TOGGLES) {
            let got = assess(&with(mask));
            let (c, s, k, d) = expected(mask);
            assert!(
                close(got.correctness_risk, c),
                "{mask:#b} correction {got:?}"
            );
            assert!(close(got.stability_risk, s), "{mask:#b} stabilite {got:?}");
            assert!(close(got.compat_risk, k), "{mask:#b} compatibilite {got:?}");
            assert!(close(got.data_risk, d), "{mask:#b} donnees {got:?}");
        }
    }

    /// Chaque regle se nomme quand son fait est present, et seulement alors ; R10 est
    /// nomme quand aucune autre regle de correction ne l'est.
    #[test]
    fn each_rule_names_itself_exactly_when_it_applies() {
        let rule_of = [
            RiskReason::Unknown,
            RiskReason::UnknownEffects,
            RiskReason::NonDeterministic,
            RiskReason::MainThread,
            RiskReason::WorldWrite,
            RiskReason::Incomplete,
            RiskReason::Incomplete,
            RiskReason::Compatibility,
            RiskReason::Rollbacks,
            RiskReason::Mismatch,
        ];
        for mask in 0..(1_u32 << TOGGLES) {
            let reasons = assess(&with(mask)).reasons;
            for rule in RiskReason::ALL {
                let applies = match rule {
                    RiskReason::Incomplete => mask & 0b110_0000 != 0,
                    RiskReason::Confidence => mask & CORRECTION_TOGGLES == 0,
                    other => {
                        let i = rule_of.iter().position(|r| *r == other).unwrap();
                        mask & (1 << i) != 0
                    }
                };
                assert_eq!(reasons.contains(&rule), applies, "{mask:#b} {rule:?}");
            }
        }
    }

    /// T-231 : ajouter un fait defavorable n'abaisse jamais aucune composante, ni le
    /// blast radius.
    #[test]
    fn t231_adding_an_unfavourable_fact_never_lowers_the_risk() {
        for mask in 0..(1_u32 << TOGGLES) {
            let before = assess(&with(mask));
            for toggle in 0..TOGGLES {
                let mut worse = with(mask);
                worsen(&mut worse, toggle);
                let after = assess(&worse);
                assert!(
                    after.correctness_risk >= before.correctness_risk,
                    "{mask:#b}+{toggle}"
                );
                assert!(
                    after.stability_risk >= before.stability_risk,
                    "{mask:#b}+{toggle}"
                );
                assert!(
                    after.compat_risk >= before.compat_risk,
                    "{mask:#b}+{toggle}"
                );
                assert!(after.data_risk >= before.data_risk, "{mask:#b}+{toggle}");
                assert!(
                    after.max_blast_radius >= before.max_blast_radius,
                    "{mask:#b}+{toggle}"
                );
            }
        }
    }

    /// T-232 : une divergence rend le risque de correction definitif, quels que soient
    /// les autres faits.
    #[test]
    fn t232_a_mismatch_makes_the_correctness_risk_final() {
        for mask in 0..(1_u32 << TOGGLES) {
            let mut inputs = with(mask);
            inputs.validation_mismatches = 1;
            assert!(close(assess(&inputs).correctness_risk, 1.0), "{mask:#b}");
        }
        let mut perfect = clean();
        assert!(close(assess(&perfect).correctness_risk, 0.0));
        perfect.determinism.mismatches = 1;
        let risk = assess(&perfect);
        assert!(close(risk.correctness_risk, 1.0));
        assert!(risk.reasons.contains(&RiskReason::Mismatch));
    }

    /// Une confiance qui n'est pas un nombre vaut le pire cas.
    #[test]
    fn a_nan_confidence_is_the_worst_case() {
        let mut inputs = clean();
        inputs.confidence.combined = f32::NAN;
        assert!(close(assess(&inputs).correctness_risk, 1.0));
        inputs.compat_risk = Some(f32::NAN);
        assert!(close(assess(&inputs).compat_risk, 1.0));
    }

    /// ADR-066, decision 3 : la portee des degats, de la tache au monde.
    #[test]
    fn the_blast_radius_widens_with_shared_effects() {
        let mut inputs = clean();
        assert_eq!(blast_radius(&inputs), Blast::Task);
        assert_eq!(blast_radius(&inputs).max_level(), TransformationLevel::Tl6);

        inputs.reads.domains = Domains::EMPTY.with(Domain::Registries);
        assert_eq!(blast_radius(&inputs), Blast::Workload);
        assert_eq!(blast_radius(&inputs).max_level(), TransformationLevel::Tl5);

        inputs.writes.domains = Domains::EMPTY.with(Domain::ModStatics);
        assert_eq!(blast_radius(&inputs), Blast::Subsystem);
        assert_eq!(blast_radius(&inputs).max_level(), TransformationLevel::Tl4);

        for world in WORLD.iter().chain([Domain::Unknown].iter()) {
            let mut w = clean();
            w.writes.domains = Domains::EMPTY.with(*world);
            assert_eq!(blast_radius(&w), Blast::World, "{world:?}");
            assert_eq!(blast_radius(&w).max_level(), TransformationLevel::Tl2);
        }
        let effects: [fn(&mut SideEffectSet); 8] = [
            |e| e.unknown = true,
            |e| e.mutates_world = true,
            |e| e.spawns_or_removes_entity = true,
            |e| e.sends_packets = true,
            |e| e.io = true,
            |e| e.fires_events = true,
            |e| e.schedules_tasks = true,
            |e| e.cancels_event = true,
        ];
        for (i, set) in effects.iter().enumerate() {
            let mut w = clean();
            set(&mut w.side_effects);
            assert_eq!(blast_radius(&w), Blast::World, "effet {i}");
        }
        let subsystem: [fn(&mut RiskInputs); 3] = [
            |i| i.locks = true,
            |i| i.side_effects.native_call = true,
            |i| i.side_effects.throws_observable = true,
        ];
        for (i, set) in subsystem.iter().enumerate() {
            let mut s = clean();
            set(&mut s);
            assert_eq!(blast_radius(&s), Blast::Subsystem, "fait {i}");
        }
        let mut unknown = clean();
        unknown.flags = unknown.flags.set(ClassificationFlags::UNKNOWN, true);
        assert_eq!(blast_radius(&unknown), Blast::World);
    }
}
