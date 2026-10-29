//! Fiche d'une unite de travail : ce qui est mesure, ce que l'analyse en sait
//! (`/rfx why`, `/rfx workload`).
//!
//! Composant : C-35 (Diagnostics), C-38. Cahier des charges : PARTIE 5.33, PARTIE 5.36.
//! Decision : ADR-063. Maturite : `STABLE`.
//!
//! Le bloc `DECISION` de la PARTIE 5.33 n'existe pas ici : aucun moteur de decision ne
//! tourne encore, et le remplir serait inventer (R-660).
//!
//! Comme tout ce qui traverse vers Java : entiers, booleens et textes seulement. Le
//! lecteur CBOR de Java ne lit du type majeur 7 que les booleens : ni reel, ni `null`.
//! La confiance passe donc en milliemes, et une valeur absente est une cle omise.

use serde::{Deserialize, Serialize};

use crate::TopEntry;

/// Fiche d'une unite de travail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkloadCard {
    /// Version du format.
    pub schema: u32,
    /// Identifiant demande, en hexadecimal sur seize caracteres.
    pub work_id_hex: String,
    /// Duree du dernier tick observe, en nanosecondes ; zero si aucun.
    pub tick_ns: u64,
    /// Mesure du profileur ; omise si l'unite n'est pas suivie.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<TopEntry>,
    /// Ce que l'analyse en sait ; omis si l'unite n'est pas encore decrite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis: Option<AnalysisCard>,
    /// L'analyse construisait un graphe au moment de la demande : rien n'a ete attendu,
    /// redemander.
    pub analysis_busy: bool,
}

impl WorkloadCard {
    /// Version courante du format.
    pub const SCHEMA: u32 = 1;
}

/// Bloc `ANALYSIS` : le descripteur de l'unite (C-08), resume.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisCard {
    /// Noeuds du graphe d'appel (C-07).
    pub graph_nodes: u32,
    /// Appels resolus.
    pub resolved_calls: u32,
    /// Appels non resolus.
    pub unresolved_calls: u32,
    /// Graphe tronque.
    pub truncated: bool,
    /// Un appel reflexif a ete rencontre.
    pub reflection: bool,
    /// Domaines lus (DM-06), par nom.
    pub reads: Vec<String>,
    /// Precision des lectures : `Exact`, `OverApprox` ou `Unknown`.
    pub reads_precision: String,
    /// Domaines ecrits, par nom.
    pub writes: Vec<String>,
    /// Precision des ecritures.
    pub writes_precision: String,
    /// Effets de bord poses (DM-07), par nom.
    pub side_effects: Vec<String>,
    /// Une partie des effets est inconnue.
    pub side_effects_unknown: bool,
    /// Producteurs de donnees (`data_deps`) ; omis si inconnu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_deps: Option<u32>,
    /// Unites dont l'ordre relatif est observable (`order_deps`) ; omis si inconnu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order_deps: Option<u32>,
    /// Verrous externes connus (une liste, eventuellement vide).
    pub external_locks_known: bool,
    /// La racine peut etre un rappel qu'un repartiteur decide d'appeler (ADR-060).
    pub may_be_dispatched: bool,
    /// Fil autoritatif requis : `Yes`, `No` ou `Unknown` (DM-09).
    pub main_thread_required: String,
    /// Une API non sure entre fils est touchee.
    pub touches_non_threadsafe_api: bool,
    /// Deterministe : `Yes`, `No` ou `Unknown` (DM-10).
    pub deterministic: String,
    /// Sources de non-determinisme trouvees, par nom.
    pub nondet_sources: Vec<String>,
    /// Sensible a l'ordre : `Yes`, `No` ou `Unknown`.
    pub order_sensitive: String,
    /// Arithmetique flottante non associative.
    pub float_sensitive: bool,
    /// Drapeaux (DM-11), par nom.
    pub flags: Vec<String>,
    /// Confiance combinee (DM-12), en milliemes.
    pub confidence_x1000: u32,
    /// Le bytecode de la racine a change depuis : a recalculer (R-350).
    pub stale: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_is_readable_by_java_with_absent_parts_omitted() {
        let card = WorkloadCard {
            schema: WorkloadCard::SCHEMA,
            work_id_hex: "ffffffffffffffff".to_owned(),
            tick_ns: 3_000_000,
            measured: None,
            analysis: Some(AnalysisCard {
                reads: vec!["EntityState".to_owned()],
                data_deps: Some(2),
                order_deps: None,
                confidence_x1000: 750,
                ..AnalysisCard::default()
            }),
            analysis_busy: false,
        };
        let bytes = crate::to_cbor(&card).unwrap();
        crate::assert_java_readable(&bytes).unwrap();
        assert_eq!(crate::from_cbor::<WorkloadCard>(&bytes).unwrap(), card);

        let empty = WorkloadCard {
            analysis: None,
            ..card
        };
        let bytes = crate::to_cbor(&empty).unwrap();
        crate::assert_java_readable(&bytes).unwrap();
        assert_eq!(crate::from_cbor::<WorkloadCard>(&bytes).unwrap(), empty);
    }
}
