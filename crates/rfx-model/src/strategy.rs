//! DM-13 : strategie d'execution d'une unite de travail (genre et niveau), et DM-15 :
//! motif d'un rejet.
//!
//! Cahier des charges : PARTIE 4.13, PARTIE 4.15. Maturite : `STABLE` pour ce qui est
//! defini ici.
//!
//! `Strategy` (parametres, gains attendus et mesures) viendra avec son premier
//! producteur, quand une decision sera appliquee. Genre et niveau existent des
//! maintenant : le moteur de risque (C-14) borne le niveau (ADR-066), le moteur de
//! decision (C-15) choisit le genre (ADR-068).

use serde::{Deserialize, Serialize};

/// Niveau de transformation, de TL0 (code Java tel quel) a TL6 (implementation native).
///
/// L'ordre des variantes est celui des niveaux : `TL0 < TL1 < ... < TL6`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TransformationLevel {
    /// `JavaOnly` : rien n'est change.
    Tl0,
    /// Plafond d'une unite `UNKNOWN` (R-214).
    Tl1,
    /// `JavaBatch` : appels regroupes, ordre preserve.
    Tl2,
    /// `JavaParallel`.
    Tl3,
    /// `RustOffload`, `RustBatch`, `RustParallel`.
    Tl4,
    /// `RustTransformed`.
    Tl5,
    /// `RustNative`.
    Tl6,
}

impl TransformationLevel {
    /// Tous les niveaux, du plus sur au plus transforme.
    pub const ALL: [Self; 7] = [
        Self::Tl0,
        Self::Tl1,
        Self::Tl2,
        Self::Tl3,
        Self::Tl4,
        Self::Tl5,
        Self::Tl6,
    ];

    /// Le numero du niveau, de 0 a 6.
    #[must_use]
    pub fn number(self) -> u8 {
        self as u8
    }
}

/// DM-13 : genre de strategie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StrategyKind {
    /// Le code Java, tel quel (TL0).
    JavaOnly,
    /// Appels regroupes en Java, ordre preserve (TL2).
    JavaBatch,
    /// Parallelise en Java (TL3).
    JavaParallel,
    /// Deporte vers un worker Rust (TL4).
    RustOffload,
    /// Deporte et regroupe (TL4).
    RustBatch,
    /// Deporte et parallelise (TL4).
    RustParallel,
    /// Transforme par l'IR (TL5).
    RustTransformed,
    /// Implementation native fournie (TL6).
    RustNative,
    /// Rejete : motif obligatoire (DM-15).
    Reject,
}

impl StrategyKind {
    /// Les strategies executables, du plus sur au plus transforme (sans `Reject`).
    pub const CANDIDATES: [Self; 8] = [
        Self::JavaOnly,
        Self::JavaBatch,
        Self::JavaParallel,
        Self::RustOffload,
        Self::RustBatch,
        Self::RustParallel,
        Self::RustTransformed,
        Self::RustNative,
    ];

    /// Niveau de transformation (PARTIE 4.13) ; `None` pour `Reject`.
    #[must_use]
    pub fn level(self) -> Option<TransformationLevel> {
        match self {
            Self::JavaOnly => Some(TransformationLevel::Tl0),
            Self::JavaBatch => Some(TransformationLevel::Tl2),
            Self::JavaParallel => Some(TransformationLevel::Tl3),
            Self::RustOffload | Self::RustBatch | Self::RustParallel => {
                Some(TransformationLevel::Tl4)
            }
            Self::RustTransformed => Some(TransformationLevel::Tl5),
            Self::RustNative => Some(TransformationLevel::Tl6),
            Self::Reject => None,
        }
    }

    /// Nom normatif, tel que la fiche d'explication l'affiche (PARTIE 5.33).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::JavaOnly => "JAVA_ONLY",
            Self::JavaBatch => "JAVA_BATCH",
            Self::JavaParallel => "JAVA_PARALLEL",
            Self::RustOffload => "RUST_OFFLOAD",
            Self::RustBatch => "RUST_BATCH",
            Self::RustParallel => "RUST_PARALLEL",
            Self::RustTransformed => "RUST_TRANSFORMED",
            Self::RustNative => "RUST_NATIVE",
            Self::Reject => "REJECT",
        }
    }
}

/// DM-15 : motif d'un rejet. Tout rejet en porte un (R-219).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RejectReason {
    /// Effets de bord inconnus.
    UnknownSideEffect,
    /// Etat partage non protege.
    UnsafeSharedState,
    /// L'ordre relatif est observable.
    OrderDependency,
    /// Le fil autoritatif est requis.
    ThreadAffinity,
    /// Comportement non deterministe.
    NonDeterministicBehavior,
    /// Cout de transfert trop eleve.
    HighTransferCost,
    /// Cout de synchronisation trop eleve.
    HighSynchronizationCost,
    /// Dependance JNI non resolue.
    UnresolvedJniDependency,
    /// Appel natif non modelise.
    UnmodeledNativeCall,
    /// Confiance insuffisante.
    InsufficientConfidence,
    /// Budget depasse.
    BudgetExceeded,
    /// Regle de compatibilite.
    CompatibilityRule,
    /// Validation echouee.
    ValidationFailed,
    /// Regression detectee.
    RegressionDetected,
    /// Materiel insuffisant.
    HardwareUnsuitable,
    /// Cache invalide.
    CacheInvalidated,
    /// Desactive par l'utilisateur.
    UserDisabled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_levels_follow_the_table_of_dm13() {
        let levels: Vec<_> = StrategyKind::CANDIDATES
            .iter()
            .map(|s| s.level().expect("executable").number())
            .collect();
        assert_eq!(levels, vec![0, 2, 3, 4, 4, 4, 5, 6]);
        assert_eq!(StrategyKind::Reject.level(), None);
    }

    #[test]
    fn levels_are_ordered_by_their_number() {
        for (i, level) in TransformationLevel::ALL.iter().enumerate() {
            assert_eq!(usize::from(level.number()), i);
        }
        assert!(TransformationLevel::ALL.windows(2).all(|w| w[0] < w[1]));
    }
}
