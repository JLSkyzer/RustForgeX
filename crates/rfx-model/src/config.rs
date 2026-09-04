//! Configuration transmise au runtime natif.
//!
//! Composant : C-37. Cahier des charges : PARTIE 28.
//!
//! Ce blob ne transporte que les sections dont le runtime **natif** a besoin au jalon
//! courant. Les autres sections de la PARTIE 28.2 pilotent des composants Java ou des
//! composants non encore implementes : elles restent cote Java et rejoindront ce blob
//! au jalon qui les mobilise. Toute evolution incompatible incremente
//! [`MODEL_SCHEMA_VERSION`](crate::MODEL_SCHEMA_VERSION).

use serde::{Deserialize, Serialize};

/// Mode de fonctionnement global (PARTIE 28.3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeMode {
    /// Risque de correction maximal 0.05, aucune exploration.
    Safe,
    /// Defaut : risque de correction maximal 0.15, exploration 2 %.
    #[default]
    Balanced,
    /// Risque de correction maximal 0.30, exploration 5 %.
    Performance,
    /// Active les niveaux de transformation eleves et l'IR.
    Experimental,
    /// Assertions d'invariants, journalisation par tick, instrumentation maximale.
    /// N'est pas destine au jeu normal (R-920).
    Debug,
}

impl RuntimeMode {
    /// Analyse le libelle d'un mode tel qu'ecrit dans `rustforgex.toml`.
    ///
    /// Renvoie `None` si le libelle est inconnu : l'appelant applique alors le defaut
    /// et signale la valeur rejetee (PARTIE 28.5).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "safe" => Some(Self::Safe),
            "balanced" => Some(Self::Balanced),
            "performance" => Some(Self::Performance),
            "experimental" => Some(Self::Experimental),
            "debug" => Some(Self::Debug),
            _ => None,
        }
    }

    /// Libelle canonique du mode, tel qu'il apparait dans le fichier de configuration.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
            Self::Experimental => "experimental",
            Self::Debug => "debug",
        }
    }
}

impl core::fmt::Display for RuntimeMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.label())
    }
}

/// Configuration effective transmise a `rfx_init` sous forme de blob CBOR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Version du schema du blob. Doit valoir [`MODEL_SCHEMA_VERSION`](crate::MODEL_SCHEMA_VERSION).
    pub schema: u32,
    /// `general.enabled` : `false` signifie que RUSTFORGE-X ne fait rien du tout.
    pub enabled: bool,
    /// `general.mode`.
    pub mode: RuntimeMode,
    /// `memory.max_native_mb` : plafond de memoire native, en mebioctets.
    pub max_native_mb: u32,
    /// `telemetry.enabled`.
    pub telemetry_enabled: bool,
    /// `runtime.panic_threshold` : nombre de panics tolerees pour un sous-systeme
    /// avant sa desactivation (R-523).
    pub panic_threshold: u32,
}

impl Default for RuntimeConfig {
    /// Valeurs par defaut de la PARTIE 28.2. Tout defaut est sur (R-590).
    fn default() -> Self {
        Self {
            schema: crate::MODEL_SCHEMA_VERSION,
            enabled: true,
            mode: RuntimeMode::Balanced,
            max_native_mb: 512,
            telemetry_enabled: true,
            panic_threshold: 3,
        }
    }
}

impl RuntimeConfig {
    /// Verifie que le blob decode porte un schema lisible par cette version.
    ///
    /// # Erreurs
    ///
    /// Renvoie [`CodecError::UnsupportedSchema`](crate::CodecError::UnsupportedSchema)
    /// si le schema differe de [`MODEL_SCHEMA_VERSION`](crate::MODEL_SCHEMA_VERSION).
    pub fn check_schema(&self) -> Result<(), crate::CodecError> {
        if self.schema == crate::MODEL_SCHEMA_VERSION {
            Ok(())
        } else {
            Err(crate::CodecError::UnsupportedSchema {
                found: self.schema,
                expected: crate::MODEL_SCHEMA_VERSION,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_part_28() {
        let c = RuntimeConfig::default();
        assert!(c.enabled);
        assert_eq!(c.mode, RuntimeMode::Balanced);
        assert_eq!(c.max_native_mb, 512);
        assert!(c.telemetry_enabled);
        assert_eq!(c.panic_threshold, 3);
    }

    #[test]
    fn every_mode_round_trips_through_text() {
        for m in [
            RuntimeMode::Safe,
            RuntimeMode::Balanced,
            RuntimeMode::Performance,
            RuntimeMode::Experimental,
            RuntimeMode::Debug,
        ] {
            assert_eq!(RuntimeMode::parse(m.label()), Some(m));
        }
    }

    #[test]
    fn unknown_mode_is_rejected_never_guessed() {
        assert_eq!(RuntimeMode::parse("turbo"), None);
        assert_eq!(RuntimeMode::parse("SAFE"), None);
        assert_eq!(RuntimeMode::parse(""), None);
    }

    #[test]
    fn unknown_schema_is_refused() {
        let mut c = RuntimeConfig::default();
        assert!(c.check_schema().is_ok());
        c.schema = crate::MODEL_SCHEMA_VERSION + 1;
        assert!(c.check_schema().is_err());
    }
}
