//! SM-06 : etat du runtime natif.
//!
//! Composant : C-27. Cahier des charges : PARTIE 5.27 et PARTIE 7.6.

/// Etat courant du runtime natif.
///
/// ```text
/// INIT ──▶ RUNNING ──▶ DEGRADED
///            │            │
///            └──────┬─────┘
///                   ▼
///                HALTED        (terminal : le jeu continue en Java pur)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    /// Le runtime est construit mais n'a pas encore accepte de travail.
    Init,
    /// Fonctionnement nominal.
    Running,
    /// Fonctionnement reduit : le jeu tourne, le runtime n'entreprend plus rien.
    Degraded,
    /// Arret d'urgence. Toutes les decisions retombent en `JAVA_ONLY`, le jeu
    /// continue en Java pur. Etat terminal pour le processus courant.
    Halted,
}

impl RuntimeState {
    /// Libelle publie dans `/rfx status` et les diagnostics.
    #[must_use]
    pub fn libelle(self) -> &'static str {
        match self {
            Self::Init => "INIT",
            Self::Running => "RUNNING",
            Self::Degraded => "DEGRADED",
            Self::Halted => "HALTED",
        }
    }

    /// Indique si le runtime peut encore accepter du travail.
    ///
    /// `HALTED` et `DEGRADED` ne l'acceptent plus : dans ces etats, toute decision
    /// retombe sur le chemin Java (contrat agent 6.2).
    #[must_use]
    pub fn accepte_du_travail(self) -> bool {
        matches!(self, Self::Running)
    }

    /// Indique si l'etat est terminal pour le processus courant.
    #[must_use]
    pub fn est_terminal(self) -> bool {
        matches!(self, Self::Halted)
    }
}

impl core::fmt::Display for RuntimeState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.libelle())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seul_running_accepte_du_travail() {
        assert!(RuntimeState::Running.accepte_du_travail());
        assert!(!RuntimeState::Init.accepte_du_travail());
        assert!(!RuntimeState::Degraded.accepte_du_travail());
        assert!(!RuntimeState::Halted.accepte_du_travail());
    }

    #[test]
    fn seul_halted_est_terminal() {
        assert!(RuntimeState::Halted.est_terminal());
        assert!(!RuntimeState::Degraded.est_terminal());
    }
}
