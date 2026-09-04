//! Blob de statut publie par le runtime natif vers Java.
//!
//! Composants : C-27 (etat du runtime), C-45 (couverture de sonde), C-34/C-35 pour
//! l'exposition. Consommateur : `/rfx status` (C-38).

use serde::{Deserialize, Serialize};

use crate::{HardwareClass, Maturity};

/// Champs de [`HardwareClass`] reellement mesures sur cette plateforme.
///
/// Publiee a cote de la classe materielle pour qu'un `0` ne puisse jamais etre
/// confondu avec une mesure (voir la convention documentee sur [`HardwareClass`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeCoverage {
    /// Le nombre de coeurs physiques et logiques a ete mesure.
    pub cores: bool,
    /// La topologie des coeurs a ete determinee.
    pub topology: bool,
    /// La taille du cache L3 a ete mesuree.
    pub l3: bool,
    /// Le nombre de noeuds NUMA a ete mesure.
    pub numa: bool,
    /// Les capacites SIMD ont ete detectees.
    pub simd: bool,
    /// La memoire physique totale a ete mesuree.
    pub mem_total: bool,
    /// Le cout d'un aller-retour FFI a ete mesure.
    pub ffi_call: bool,
    /// Le debit de transfert Java vers natif a ete mesure.
    pub ffi_transfer: bool,
}

/// Compteurs de la fenetre de tick (IF-02), publies vers Java.
///
/// Definis ici plutot que dans `rfx-core` parce que `rfx-model` est la couche la plus
/// basse : le sens des dependances interdit l'inverse (INV-13).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickStatus {
    /// Ticks ouverts depuis le demarrage.
    pub ticks: u64,
    /// Ticks fermes implicitement faute de `tick_end` (R-706).
    pub unbalanced: u64,
    /// Transitions de phase refusees car non conformes a SM-04.
    pub invalid_transitions: u64,
    /// Hooks ayant depasse la deadline (R-707).
    pub hook_budget_exceeded: u64,
    /// Duree de la derniere fenetre fermee, en nanosecondes.
    pub last_window_ns: u64,
}

/// Compteurs du flux de profilage (IF-03) et de la memoire native (C-31).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeStatus {
    /// Enregistrements de sonde consommes depuis le demarrage.
    pub records_consumed: u64,
    /// Enregistrements perdus par saturation des tampons (R-709).
    pub records_lost: u64,
    /// Octets natifs actuellement reserves, tous pools confondus (R-534).
    pub native_bytes: u64,
    /// Plafond de memoire native, en octets (R-533).
    pub native_limit_bytes: u64,
}

/// Compteurs du profiler (C-05), publies vers Java.
///
/// Toutes les valeurs sont entieres : le lecteur CBOR de Java ne decode
/// volontairement aucun flottant, ce qui lui evite un pan entier de la specification
/// CBOR pour un blob de statut. Les fractions traversent donc en centiemes de
/// pourcent — `250` se lit « 2,50 % ».
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProfilerStatus {
    /// Etat courant du profiler : `OFF`, `LIGHT`, `NORMAL`, `DEEP` ou `THROTTLED`.
    pub level: String,
    /// Unites de travail suivies (`rfx.profiler.workloads_tracked`).
    pub workloads_tracked: u64,
    /// Identifiants de sonde attribues, evinces compris.
    pub probes_allocated: u64,
    /// Cout du profilage en centiemes de pourcent d'un cœur
    /// (`rfx.profiler.overhead_pct`).
    pub overhead_pct_x100: u64,
    /// Cout du profilage en centiemes de pourcent du temps de tick.
    pub mspt_pct_x100: u64,
    /// Enregistrements agreges depuis le demarrage.
    pub records_ingested: u64,
    /// Enregistrements portant un identifiant de sonde inconnu.
    pub records_unknown: u64,
    /// Enregistrements reconnus mais non interpretes : evenements Forge (C-06) et
    /// natures inconnues, conserves au compteur plutot que devines.
    pub records_ignored: u64,
    /// Echantillons de pile agreges (`rfx.profiler.samples`).
    pub samples_ingested: u64,
    /// Unites de travail evincees (`rfx.db.evictions`, R-321).
    pub evictions: u64,
    /// Collisions de `WorkId` signalees (`E-2101`, R-202).
    pub collisions: u64,
    /// Sorties de duree nulle : horloge non monotone ou trop grossiere (FM-12).
    pub zero_duration_exits: u64,
    /// Changements de profondeur decides par l'auto-mesure.
    pub level_changes: u64,
}

/// Etat d'un composant, tel qu'affiche par `/rfx status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentStatus {
    /// Identifiant normatif du composant, par exemple `"C-27"`.
    pub id: String,
    /// Nom lisible du composant.
    pub name: String,
    /// Niveau de maturite declare (contrat agent 4.1).
    pub maturity: Maturity,
    /// `true` si le composant est actif dans cette execution.
    pub active: bool,
}

/// Statut global du runtime natif.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStatus {
    /// Version du schema du blob.
    pub schema: u32,
    /// Version de l'ABI implementee par le binaire natif charge (IF-01).
    pub abi_version: u32,
    /// Version du paquet natif (`CARGO_PKG_VERSION`).
    pub native_version: String,
    /// Etat courant du runtime : `INIT`, `RUNNING`, `DEGRADED` ou `HALTED`.
    pub state: String,
    /// Nombre de panics capturees a la frontiere FFI depuis le demarrage (R-523).
    pub panics: u64,
    /// Classe materielle mesuree par C-45.
    pub hardware: HardwareClass,
    /// Champs de `hardware` reellement mesures.
    pub probe_coverage: ProbeCoverage,
    /// Etat des composants natifs.
    pub components: Vec<ComponentStatus>,
    /// Compteurs de la fenetre de tick (IF-02).
    pub tick: TickStatus,
    /// Compteurs du flux de profilage et de la memoire native.
    pub probes: ProbeStatus,
    /// Compteurs du profiler (C-05).
    pub profiler: ProfilerStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trips_through_cbor() {
        let s = RuntimeStatus {
            schema: crate::MODEL_SCHEMA_VERSION,
            abi_version: 1,
            native_version: "0.1.0".to_owned(),
            state: "RUNNING".to_owned(),
            panics: 0,
            hardware: HardwareClass::default(),
            probe_coverage: ProbeCoverage::default(),
            components: vec![ComponentStatus {
                id: "C-27".to_owned(),
                name: "Rust Runtime Core".to_owned(),
                maturity: Maturity::Stable,
                active: true,
            }],
            tick: TickStatus::default(),
            probes: ProbeStatus::default(),
            profiler: ProfilerStatus::default(),
        };
        let bytes = crate::to_cbor(&s).expect("encodage");
        let back: RuntimeStatus = crate::from_cbor(&bytes).expect("decodage");
        assert_eq!(s, back);
    }
}
