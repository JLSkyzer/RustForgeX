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
    /// `profiler.max_workloads` : plafond d'unites de travail suivies (R-321).
    ///
    /// Un defaut est declare pour que l'absence du champ dans un blob plus ancien
    /// n'empeche pas le decodage : le runtime prend alors la valeur du cahier des
    /// charges, jamais une valeur indefinie.
    #[serde(default = "default_max_workloads")]
    pub profiler_max_workloads: u32,
    /// `profiler.cpu_budget_pct` : part d'un cœur accordee au profilage, en pourcent
    /// entier (H-07).
    #[serde(default = "default_cpu_budget_pct")]
    pub profiler_cpu_budget_pct: u32,
}

/// Plafond d'unites de travail par defaut (PARTIE 5.5).
fn default_max_workloads() -> u32 {
    20_000
}

/// Part d'un cœur accordee au profilage par defaut, en pourcent (H-07).
fn default_cpu_budget_pct() -> u32 {
    2
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
            profiler_max_workloads: default_max_workloads(),
            profiler_cpu_budget_pct: default_cpu_budget_pct(),
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

    /// Le blob vient de Java, qui est toujours de la meme version que le natif. Mais
    /// un champ absent ne doit jamais empecher le decodage : le runtime prendrait alors
    /// une valeur indefinie, ou refuserait de demarrer pour une option de confort.
    #[test]
    fn les_options_de_profilage_absentes_prennent_le_defaut_du_cahier_des_charges() {
        // Un blob CBOR forme comme celui que Java produisait avant ces deux options.
        #[derive(serde::Serialize)]
        struct Older {
            schema: u32,
            enabled: bool,
            mode: RuntimeMode,
            max_native_mb: u32,
            telemetry_enabled: bool,
            panic_threshold: u32,
        }
        let blob = crate::to_cbor(&Older {
            schema: crate::MODEL_SCHEMA_VERSION,
            enabled: true,
            mode: RuntimeMode::Balanced,
            max_native_mb: 512,
            telemetry_enabled: true,
            panic_threshold: 3,
        })
        .expect("encodage");
        let config: RuntimeConfig = crate::from_cbor(&blob).expect("decodage");

        assert_eq!(config.profiler_max_workloads, 20_000);
        assert_eq!(config.profiler_cpu_budget_pct, 2);
    }

    #[test]
    fn unknown_schema_is_refused() {
        let mut c = RuntimeConfig::default();
        assert!(c.check_schema().is_ok());
        c.schema = crate::MODEL_SCHEMA_VERSION + 1;
        assert!(c.check_schema().is_err());
    }
}
