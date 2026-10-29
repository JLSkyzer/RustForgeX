//! DM-13 : strategie d'execution d'une unite de travail — pour l'instant, son niveau de
//! transformation.
//!
//! Cahier des charges : PARTIE 4.13. Maturite : `STABLE` pour ce qui est defini ici.
//!
//! Le reste de DM-13 (`Strategy`, `StrategyKind`, parametres) viendra avec le moteur de
//! decision (C-15), son premier producteur. Le niveau existe des maintenant parce que le
//! moteur de risque (C-14) en borne le maximum (ADR-066).

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_ordered_by_their_number() {
        for (i, level) in TransformationLevel::ALL.iter().enumerate() {
            assert_eq!(usize::from(level.number()), i);
        }
        assert!(TransformationLevel::ALL.windows(2).all(|w| w[0] < w[1]));
    }
}
