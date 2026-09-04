//! Niveaux de maturite des fonctionnalites.
//!
//! Cahier des charges : PARTIE 0.3. Obligation : contrat agent 4.1 — tout composant
//! implemente expose son etat de maturite.

use serde::{Deserialize, Serialize};

/// Niveau de maturite d'une fonctionnalite. Chaque fonctionnalite en porte
/// exactement un, expose dans le code, dans la configuration et dans les diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Maturity {
    /// Specifie, implemente, teste, benchmarke, valide. Actif par defaut.
    ///
    /// Ne contient aucun `TODO`, `FIXME`, `unimplemented!()`, `todo!()`, placeholder
    /// ou faux benchmark : verifie mecaniquement par le job CI `lint-no-fiction`.
    Stable,
    /// Implemente, partiellement valide, comportement pouvant changer.
    /// Jamais actif par defaut.
    Experimental,
    /// Code present mais desactive par compilation ou configuration.
    Disabled,
    /// Uniquement specifie : interfaces et tests de contrat existent,
    /// implementation absente ou minimale refusant l'activation.
    Future,
}

impl Maturity {
    /// Indique si une fonctionnalite de ce niveau peut etre active par defaut.
    #[must_use]
    pub fn active_par_defaut(self) -> bool {
        matches!(self, Self::Stable)
    }

    /// Libelle court, utilise par `/rfx status` et les diagnostics.
    #[must_use]
    pub fn libelle(self) -> &'static str {
        match self {
            Self::Stable => "STABLE",
            Self::Experimental => "EXPERIMENTAL",
            Self::Disabled => "DISABLED",
            Self::Future => "FUTURE",
        }
    }
}

impl core::fmt::Display for Maturity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.libelle())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seul_stable_est_actif_par_defaut() {
        assert!(Maturity::Stable.active_par_defaut());
        assert!(!Maturity::Experimental.active_par_defaut());
        assert!(!Maturity::Disabled.active_par_defaut());
        assert!(!Maturity::Future.active_par_defaut());
    }
}
