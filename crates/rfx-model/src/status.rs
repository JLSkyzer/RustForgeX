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

/// Etat d'un composant, tel qu'affiche par `/rfx status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentStatus {
    /// Identifiant normatif du composant, par exemple `"C-27"`.
    pub id: String,
    /// Nom lisible du composant.
    pub nom: String,
    /// Niveau de maturite declare (contrat agent 4.1).
    pub maturite: Maturity,
    /// `true` si le composant est actif dans cette execution.
    pub actif: bool,
}

/// Statut global du runtime natif.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStatus {
    /// Version du schema du blob.
    pub schema: u32,
    /// Version de l'ABI implementee par le binaire natif charge (IF-01).
    pub abi_version: u32,
    /// Version du paquet natif (`CARGO_PKG_VERSION`).
    pub version_native: String,
    /// Etat courant du runtime : `INIT`, `RUNNING`, `DEGRADED` ou `HALTED`.
    pub etat: String,
    /// Nombre de panics capturees a la frontiere FFI depuis le demarrage (R-523).
    pub panics: u64,
    /// Classe materielle mesuree par C-45.
    pub materiel: HardwareClass,
    /// Champs de `materiel` reellement mesures.
    pub couverture_sonde: ProbeCoverage,
    /// Etat des composants natifs.
    pub composants: Vec<ComponentStatus>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_statut_fait_un_aller_retour_cbor() {
        let s = RuntimeStatus {
            schema: crate::MODEL_SCHEMA_VERSION,
            abi_version: 1,
            version_native: "0.1.0".to_owned(),
            etat: "RUNNING".to_owned(),
            panics: 0,
            materiel: HardwareClass::default(),
            couverture_sonde: ProbeCoverage::default(),
            composants: vec![ComponentStatus {
                id: "C-27".to_owned(),
                nom: "Rust Runtime Core".to_owned(),
                maturite: Maturity::Stable,
                actif: true,
            }],
        };
        let bytes = crate::to_cbor(&s).expect("encodage");
        let back: RuntimeStatus = crate::from_cbor(&bytes).expect("decodage");
        assert_eq!(s, back);
    }
}
