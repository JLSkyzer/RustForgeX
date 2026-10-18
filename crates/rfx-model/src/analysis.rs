//! Blobs de l'analyse statique (C-07) echanges avec Java.
//!
//! Cahier des charges : PARTIE 5.7. Decision : ADR-051. Maturite : `STABLE`.

use serde::{Deserialize, Serialize};

/// Une classe de la hierarchie de l'installation, sans son bytecode :
/// `[nom interne, super-classe ("" si aucune), drapeaux d'acces]`.
///
/// Forme de tableau, et non de table : la hierarchie compte des dizaines de milliers
/// de classes, et un nom de champ repete par entree doublerait le blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HierarchyEntry(pub String, pub String, pub u32);

/// Etat de l'analyse, publie dans le statut (`rfx.callgraph.*`).
///
/// Tous les champs sont nuls tant que la hierarchie n'est pas chargee (R-660).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisStatus {
    /// `analysis.enabled`.
    pub enabled: bool,
    /// Classes declarees dans la hierarchie.
    pub hierarchy_classes: u64,
    /// Classes dont le bytecode a ete fourni.
    pub classes_provided: u64,
    /// Classes declarees absentes (cibles de mixin, illisibles).
    pub classes_absent: u64,
    /// Graphes construits (`rfx.callgraph.graphs`).
    pub graphs: u64,
    /// Tours de construction, relances comprises.
    pub rounds: u64,
    /// Noeuds, sommes sur tous les graphes.
    pub nodes: u64,
    /// Sites d'appel resolus, sommes sur tous les graphes.
    pub resolved_calls: u64,
    /// Sites d'appel non resolus, sommes sur tous les graphes.
    pub unresolved_calls: u64,
    /// Graphes arretes par une borne (R-340).
    pub truncated: u64,
    /// Graphes ou un appel reflexif a ete rencontre.
    pub reflection: u64,
    /// Duree de la derniere construction, en nanosecondes (`rfx.callgraph.build_ns`).
    pub last_build_ns: u64,
    /// Unites de travail en attente d'analyse.
    pub queue: u64,
    /// Refus du budget memoire natif : l'analyse s'est alors arretee (R-533).
    pub budget_refusals: u64,
    /// Descripteurs tenus a jour (C-08, `rfx.analysis.descriptors`).
    #[serde(default)]
    pub descriptors: u64,
    /// Descripteurs calcules, recalculs compris (`rfx.analysis.completed`).
    #[serde(default)]
    pub completed: u64,
    /// Descripteurs dont l'analyse a ete tronquee : borne ou budget (FM-16,
    /// `rfx.analysis.truncated`).
    #[serde(default)]
    pub descriptors_truncated: u64,
    /// Descripteurs `UNKNOWN` en ce moment (P-02).
    #[serde(default)]
    pub descriptors_unknown: u64,
    /// Changements significatifs d'un descripteur, a notifier a C-15.
    #[serde(default)]
    pub descriptor_changes: u64,
    /// Recalculs sur changement du bytecode de la methode racine (R-350).
    #[serde(default)]
    pub recomputes: u64,
    /// Temps passe dans les pas d'analyse, en nanosecondes, horloge monotone du fil
    /// d'analyse (`rfx.analysis.cpu_ms`) : une borne haute du temps processeur.
    #[serde(default)]
    pub step_ns: u64,
    /// Descripteurs dont les ecritures sont `EXACT` en ce moment (C-11,
    /// `rfx.dataflow.exact_ratio`).
    pub dataflow_exact: u64,
    /// Temps passe a composer les effets des unites, en nanosecondes, horloge
    /// monotone du fil d'analyse (C-11, `rfx.dataflow.analysis_ns`).
    pub dataflow_ns: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy_entries_are_compact_arrays() {
        let entry = HierarchyEntry("a/B".to_owned(), "a/A".to_owned(), 0x21);
        let bytes = crate::to_cbor(&entry).unwrap();
        // Tableau de trois elements (0x83), sans aucun nom de champ.
        assert_eq!(bytes[0], 0x83);
        assert_eq!(crate::from_cbor::<HierarchyEntry>(&bytes).unwrap(), entry);
    }

    #[test]
    fn the_status_is_readable_by_java() {
        let status = AnalysisStatus {
            enabled: true,
            graphs: 3,
            nodes: 120,
            ..AnalysisStatus::default()
        };
        crate::assert_java_readable(&crate::to_cbor(&status).unwrap()).unwrap();
    }
}
