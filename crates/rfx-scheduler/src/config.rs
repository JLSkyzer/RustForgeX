//! C-17 : dimensionnement et reglages de l'ordonnanceur.
//!
//! Cahier des charges : PARTIE 5.17.2 (dimensionnement), 5.17.5 (contre-pression),
//! PARTIE 5.17 « Fallback » (`FORCE_SERIAL`). Exigence : R-420. Maturite : `STABLE`.

/// Nombre de workers par defaut pour une machine a `logical_cores` coeurs logiques.
///
/// `clamp(coeurs - reserve, 1, 32)`, la reserve laissant la place au fil autoritatif et,
/// a partir de six coeurs, au rendu et au reseau (PARTIE 5.17.2).
#[must_use]
pub fn default_workers(logical_cores: usize) -> usize {
    let reserve = if logical_cores >= 6 { 2 } else { 1 };
    logical_cores.saturating_sub(reserve).clamp(1, 32)
}

/// Reglages de l'ordonnanceur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerConfig {
    /// Workers demandes. Borne a `[1, logical_cores]` au demarrage (R-420).
    pub workers: usize,
    /// Coeurs logiques de la machine.
    pub logical_cores: usize,
    /// Au-dela, les soumissions `LOW` et `BACKGROUND` sont refusees (`E-2401`).
    pub high_watermark: usize,
    /// Au-dela, les soumissions `NORMAL` le sont aussi.
    pub hard_limit: usize,
    /// `FORCE_SERIAL` : tout s'execute sur le fil qui soumet. Plus lent, jamais faux.
    pub force_serial: bool,
    /// Profondeur d'imbrication au-dela de laquelle une sous-tache s'execute en ligne,
    /// dans la tache qui la lance (PARTIE 5.17.9).
    pub max_nesting: u8,
}

impl SchedulerConfig {
    /// Reglages par defaut pour une machine a `logical_cores` coeurs logiques.
    #[must_use]
    pub fn for_machine(logical_cores: usize) -> Self {
        let logical_cores = logical_cores.max(1);
        Self {
            workers: default_workers(logical_cores),
            logical_cores,
            high_watermark: 4_096,
            hard_limit: 8_192,
            force_serial: false,
            max_nesting: 4,
        }
    }

    /// Workers effectivement demarres : jamais plus que de coeurs logiques (R-420).
    #[must_use]
    pub fn effective_workers(&self) -> usize {
        self.workers.clamp(1, self.logical_cores.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_leaves_room_for_the_authoritative_thread() {
        assert_eq!(default_workers(1), 1);
        assert_eq!(default_workers(2), 1);
        assert_eq!(default_workers(4), 3);
        assert_eq!(default_workers(6), 4);
        assert_eq!(default_workers(16), 14);
        assert_eq!(default_workers(128), 32);
    }

    /// R-420 : jamais plus de workers actifs que de coeurs logiques.
    #[test]
    fn never_more_workers_than_logical_cores() {
        let mut config = SchedulerConfig::for_machine(4);
        config.workers = 64;
        assert_eq!(config.effective_workers(), 4);
        config.workers = 0;
        assert_eq!(config.effective_workers(), 1);
    }
}
