//! DM-06 a DM-12 : faits d'analyse, drapeaux de classification et confiance.
//!
//! Cahier des charges : PARTIE 4.6 a 4.12, R-206, R-211, R-214 a R-218, T-011.
//! Decision : ADR-054. Maturite : `STABLE`.
//!
//! # Inconnu n'est pas neutre
//!
//! Un fait qu'aucune analyse n'a produit n'a pas de valeur par defaut : il est absent
//! (`Option::None`, [`Tri::Unknown`], [`Precision::Unknown`]), et le moteur le traite
//! comme le pire cas (R-206, P-02). Une liste vide de dependances dirait
//! « independant », ce que R-216 interdit ; c'est pourquoi les parties de
//! [`DependencySet`] que produit C-10 sont des `Option`.
//!
//! Deux champs du cahier sont reportes jusqu'a leur producteur : les regions fines
//! d'un [`AccessSet`] (C-11) et les roles de fil observes de [`ThreadFacts`] (C-12).

use serde::{Deserialize, Serialize};

use crate::WorkId;

/// Valeur a trois etats : `UNKNOWN` est traite comme le pire cas (P-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Tri {
    /// Oui.
    Yes,
    /// Non.
    No,
    /// Inconnu.
    #[default]
    Unknown,
}

/// DM-06 : domaine memoire abstrait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Domain {
    /// Blocs du monde.
    WorldBlocks = 0,
    /// Fluides.
    WorldFluids = 1,
    /// Eclairage.
    WorldLighting = 2,
    /// Donnees de chunk.
    ChunkData = 3,
    /// Etat de chargement des chunks.
    ChunkStatus = 4,
    /// Etat des entites.
    EntityState = 5,
    /// Inventaires d'entites.
    EntityInventory = 6,
    /// Etat des block entities.
    BlockEntityState = 7,
    /// Globaux du niveau.
    LevelGlobals = 8,
    /// Registres.
    Registries = 9,
    /// Capabilities.
    Capabilities = 10,
    /// Statiques des mods.
    ModStatics = 11,
    /// Statiques Java.
    JavaStatics = 12,
    /// Variables locales de fil.
    ThreadLocals = 13,
    /// Reseau.
    Network = 14,
    /// Systeme de fichiers.
    FileSystem = 15,
    /// Code natif externe.
    NativeExternal = 16,
    /// Generateurs pseudo-aleatoires partages (R-213).
    Random = 17,
    /// Horloge.
    Time = 18,
    /// Inconnu.
    Unknown = 19,
}

/// DM-06 : ensemble de domaines, un bit par [`Domain`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Domains(pub u32);

impl Domains {
    /// Aucun domaine.
    pub const EMPTY: Self = Self(0);

    /// `true` si `domain` est dans l'ensemble.
    #[must_use]
    pub fn contains(self, domain: Domain) -> bool {
        self.0 & (1 << domain as u8) != 0
    }

    /// L'ensemble augmente de `domain`.
    #[must_use]
    pub fn with(self, domain: Domain) -> Self {
        Self(self.0 | 1 << domain as u8)
    }

    /// `true` si l'ensemble est vide.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// `true` si l'ensemble a un domaine en commun avec `other`.
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

/// DM-06 : precision d'un ensemble d'acces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Precision {
    /// Exactement les acces reels.
    Exact,
    /// Un surensemble des acces reels (R-212).
    OverApprox,
    /// Inconnu : en conflit avec tout (R-211).
    #[default]
    Unknown,
}

/// DM-06 : acces d'une unite de travail, par domaine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AccessSet {
    /// Domaines touches.
    pub domains: Domains,
    /// Precision.
    pub precision: Precision,
}

/// DM-07 : effets de bord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // la forme du cahier (PARTIE 4.7)
pub struct SideEffectSet {
    /// Modifie le monde.
    pub mutates_world: bool,
    /// Fait apparaitre ou retire une entite.
    pub spawns_or_removes_entity: bool,
    /// Envoie des paquets.
    pub sends_packets: bool,
    /// Planifie des taches.
    pub schedules_tasks: bool,
    /// Declenche des evenements.
    pub fires_events: bool,
    /// Entrees-sorties.
    pub io: bool,
    /// Appel natif.
    pub native_call: bool,
    /// Exception qui remonte hors de l'unite de travail.
    pub throws_observable: bool,
    /// Annule un evenement.
    pub cancels_event: bool,
    /// Au moins un appel non analysable (R-214).
    pub unknown: bool,
}

impl SideEffectSet {
    /// `true` si au moins un effet de bord connu est present.
    #[must_use]
    pub fn any(&self) -> bool {
        self.mutates_world
            || self.spawns_or_removes_entity
            || self.sends_packets
            || self.schedules_tasks
            || self.fires_events
            || self.io
            || self.native_call
            || self.throws_observable
            || self.cancels_event
    }
}

/// DM-08 : dependances d'une unite de travail.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DependencySet {
    /// Appels sortants resolus, `classe.methode(descripteur)` (C-07).
    pub calls: Vec<String>,
    /// Appels non resolus : virtuels, reflexion, `invokedynamic` (C-07).
    pub unresolved_calls: u32,
    /// Unites produisant des donnees consommees (C-10) ; `None` : non analyse.
    pub data_deps: Option<Vec<WorkId>>,
    /// Unites dont l'ordre relatif est observable (C-10) ; `None` : non analyse.
    pub order_deps: Option<Vec<WorkId>>,
    /// Moniteurs et verrous rencontres (C-12) ; `None` : non analyse.
    pub external_locks: Option<Vec<String>>,
    /// Profondeur d'analyse atteinte.
    pub depth: u16,
    /// Analyse arretee par une borne ou par le budget (R-340, FM-16).
    pub truncated: bool,
}

/// DM-09 : faits de fil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // la forme du cahier (PARTIE 4.9)
pub struct ThreadFacts {
    /// Le fil autoritatif est requis ; `UNKNOWN` vaut `YES` (DM-09).
    pub main_thread_required: Tri,
    /// Utilise `synchronized`.
    pub uses_synchronized: bool,
    /// Utilise des verrous.
    pub uses_locks: bool,
    /// Utilise des variables locales de fil.
    pub uses_thread_locals: bool,
    /// Touche une API non thread-safe.
    pub touches_non_threadsafe_api: bool,
    /// Reentrant.
    pub reentrant: Tri,
}

/// DM-10 : source de non-determinisme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum NonDetSource {
    /// Generateur aleatoire partage.
    SharedRandom = 0,
    /// Horloge systeme.
    SystemTime = 1,
    /// Hachage d'identite.
    IdentityHash = 2,
    /// Ordre d'iteration d'une table de hachage.
    HashMapIteration = 3,
    /// Ordonnancement des fils.
    ThreadScheduling = 4,
    /// Entrees-sorties externes.
    ExternalIo = 5,
    /// Appel natif.
    NativeCall = 6,
    /// Reflexion.
    Reflection = 7,
    /// Inconnue.
    Unknown = 8,
}

/// DM-10 : faits de determinisme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DeterminismFacts {
    /// Deterministe.
    pub deterministic: Tri,
    /// Sources de non-determinisme, un bit par [`NonDetSource`].
    pub sources_of_nondeterminism: u16,
    /// Sensible a l'ordre.
    pub order_sensitive: Tri,
    /// Arithmetique flottante non associative (R-217).
    pub float_sensitive: bool,
    /// Executions comparees identiques.
    pub verified_runs: u32,
    /// Executions comparees divergentes.
    pub mismatches: u32,
}

/// DM-11 : drapeaux de classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClassificationFlags(pub u32);

impl ClassificationFlags {
    /// Lecture seule.
    pub const READ_ONLY: Self = Self(1 << 0);
    /// Pure.
    pub const PURE: Self = Self(1 << 1);
    /// Intensive en calcul.
    pub const CPU_INTENSIVE: Self = Self(1 << 2);
    /// Intensive en memoire.
    pub const MEMORY_INTENSIVE: Self = Self(1 << 3);
    /// Parallelisable.
    pub const PARALLELIZABLE: Self = Self(1 << 4);
    /// Regroupable par lots.
    pub const BATCHABLE: Self = Self(1 << 5);
    /// Deterministe.
    pub const DETERMINISTIC: Self = Self(1 << 6);
    /// Sure entre fils.
    pub const THREAD_SAFE: Self = Self(1 << 7);
    /// Requiert le fil autoritatif.
    pub const MAIN_THREAD_REQUIRED: Self = Self(1 << 8);
    /// Sensible a l'ordre.
    pub const ORDER_SENSITIVE: Self = Self(1 << 9);
    /// Mise en cache possible.
    pub const CACHEABLE: Self = Self(1 << 10);
    /// Candidate a Rust.
    pub const RUST_CANDIDATE: Self = Self(1 << 11);
    /// Dangereuse a transformer.
    pub const UNSAFE: Self = Self(1 << 12);
    /// Un fait manque ou n'est pas sur (P-02).
    pub const UNKNOWN: Self = Self(1 << 13);
    /// Client uniquement.
    pub const CLIENT_ONLY: Self = Self(1 << 14);
    /// Serveur uniquement.
    pub const SERVER_ONLY: Self = Self(1 << 15);
    /// Garde un etat entre appels.
    pub const STATEFUL: Self = Self(1 << 16);
    /// Effet de bord.
    pub const SIDE_EFFECT: Self = Self(1 << 17);
    /// Ecrit.
    pub const WRITE: Self = Self(1 << 18);
    /// Chaude.
    pub const HOT: Self = Self(1 << 19);
    /// Eligible a la RF-IR.
    pub const IR_ELIGIBLE: Self = Self(1 << 20);

    /// Aucun drapeau.
    pub const EMPTY: Self = Self(0);

    /// `true` si tous les drapeaux de `other` sont poses.
    #[must_use]
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Les drapeaux, avec `other` pose si `on`.
    #[must_use]
    pub fn set(self, other: Self, on: bool) -> Self {
        if on {
            Self(self.0 | other.0)
        } else {
            Self(self.0 & !other.0)
        }
    }

    /// Premiere regle de coherence de T-011 violee, ou `None`.
    ///
    /// ```text
    /// PURE                 => READ_ONLY && DETERMINISTIC && !SIDE_EFFECT
    /// MAIN_THREAD_REQUIRED => !PARALLELIZABLE
    /// UNKNOWN              => !RUST_CANDIDATE && !PARALLELIZABLE && !CACHEABLE && !IR_ELIGIBLE
    /// CACHEABLE            => PURE
    /// IR_ELIGIBLE          => PURE && !UNKNOWN
    /// ORDER_SENSITIVE      => !PARALLELIZABLE
    /// ```
    #[must_use]
    pub fn incoherence(self) -> Option<&'static str> {
        let has = |flag| self.contains(flag);
        if has(Self::PURE)
            && !(has(Self::READ_ONLY) && has(Self::DETERMINISTIC) && !has(Self::SIDE_EFFECT))
        {
            return Some("PURE => READ_ONLY && DETERMINISTIC && !SIDE_EFFECT");
        }
        if has(Self::MAIN_THREAD_REQUIRED) && has(Self::PARALLELIZABLE) {
            return Some("MAIN_THREAD_REQUIRED => !PARALLELIZABLE");
        }
        if has(Self::UNKNOWN)
            && (has(Self::RUST_CANDIDATE)
                || has(Self::PARALLELIZABLE)
                || has(Self::CACHEABLE)
                || has(Self::IR_ELIGIBLE))
        {
            return Some(
                "UNKNOWN => !RUST_CANDIDATE && !PARALLELIZABLE && !CACHEABLE && !IR_ELIGIBLE",
            );
        }
        if has(Self::CACHEABLE) && !has(Self::PURE) {
            return Some("CACHEABLE => PURE");
        }
        if has(Self::IR_ELIGIBLE) && (!has(Self::PURE) || has(Self::UNKNOWN)) {
            return Some("IR_ELIGIBLE => PURE && !UNKNOWN");
        }
        if has(Self::ORDER_SENSITIVE) && has(Self::PARALLELIZABLE) {
            return Some("ORDER_SENSITIVE => !PARALLELIZABLE");
        }
        None
    }
}

/// DM-12 : confiance, chaque composante entre 0 et 1.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Confidence {
    /// Preuve de structure : l'analyse statique.
    pub static_analysis: f32,
    /// Preuve d'observation.
    pub dynamic_evidence: f32,
    /// Preuve d'execution comparee (shadow runs).
    pub validation: f32,
    /// `min(static, max(dynamic, validation))` (R-218).
    pub combined: f32,
}

impl Confidence {
    /// Confiance de ces composantes, ramenees entre 0 et 1, combinee par R-218 : une
    /// preuve d'execution ne remplace jamais une preuve de structure.
    #[must_use]
    pub fn new(static_analysis: f32, dynamic_evidence: f32, validation: f32) -> Self {
        let clamp = |value: f32| {
            if value.is_nan() {
                0.0
            } else {
                value.clamp(0.0, 1.0)
            }
        };
        let (static_analysis, dynamic_evidence, validation) = (
            clamp(static_analysis),
            clamp(dynamic_evidence),
            clamp(validation),
        );
        Self {
            static_analysis,
            dynamic_evidence,
            validation,
            combined: static_analysis.min(dynamic_evidence.max(validation)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coherent_and_incoherent_flag_sets() {
        let f = ClassificationFlags::EMPTY;
        assert_eq!(f.incoherence(), None);
        let pure = f
            .set(ClassificationFlags::PURE, true)
            .set(ClassificationFlags::READ_ONLY, true)
            .set(ClassificationFlags::DETERMINISTIC, true)
            .set(ClassificationFlags::CACHEABLE, true);
        assert_eq!(pure.incoherence(), None);
        assert!(pure
            .set(ClassificationFlags::SIDE_EFFECT, true)
            .incoherence()
            .is_some());
        assert!(pure
            .set(ClassificationFlags::UNKNOWN, true)
            .incoherence()
            .is_some());
        let parallel = f
            .set(ClassificationFlags::PARALLELIZABLE, true)
            .set(ClassificationFlags::MAIN_THREAD_REQUIRED, true);
        assert!(parallel.incoherence().is_some());
        assert!(f
            .set(ClassificationFlags::CACHEABLE, true)
            .incoherence()
            .is_some());
        assert!(f
            .set(ClassificationFlags::IR_ELIGIBLE, true)
            .incoherence()
            .is_some());
        assert!(f
            .set(ClassificationFlags::ORDER_SENSITIVE, true)
            .set(ClassificationFlags::PARALLELIZABLE, true)
            .incoherence()
            .is_some());
    }

    #[test]
    fn confidence_never_exceeds_the_static_proof() {
        let c = Confidence::new(0.4, 0.9, 1.0);
        assert!((c.combined - 0.4).abs() < f32::EPSILON);
        let c = Confidence::new(0.9, 0.3, 0.6);
        assert!((c.combined - 0.6).abs() < f32::EPSILON);
        let c = Confidence::new(f32::NAN, 2.0, -1.0);
        assert_eq!(
            (c.static_analysis, c.dynamic_evidence, c.validation),
            (0.0, 1.0, 0.0)
        );
        assert!(c.combined.abs() < f32::EPSILON);
    }

    #[test]
    fn domains_are_bits() {
        let d = Domains::EMPTY.with(Domain::ModStatics).with(Domain::Random);
        assert!(d.contains(Domain::ModStatics) && d.contains(Domain::Random));
        assert!(!d.contains(Domain::WorldBlocks));
        assert!(d.intersects(Domains::EMPTY.with(Domain::Random)));
        assert!(!d.is_empty() && Domains::EMPTY.is_empty());
    }
}
