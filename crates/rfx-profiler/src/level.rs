//! Niveaux de sondage et etat du profiler.
//!
//! Composant : C-05. Cahier des charges : PARTIE 5.4 (niveaux par methode) et
//! PARTIE 5.5 (machine a etats du profiler). Maturite : `STABLE`.
//!
//! Deux echelles distinctes cohabitent, et les confondre serait une faute :
//!
//! - [`ProbeLevel`] est la profondeur d'**une** methode sondee. Elle suit la chaleur
//!   de cette methode : une methode froide ne merite pas qu'on lise l'horloge a chaque
//!   appel.
//! - [`ProfilerLevel`] est l'etat **global** du profiler. Il suit le cout du profilage
//!   lui-meme et sert de plafond : quand l'observation devient trop chere, elle se
//!   restreint partout a la fois.

use rfx_model::Heat;

/// Profondeur de sondage d'une methode (PARTIE 5.4).
///
/// Les codes sont ceux transmis a Java dans la table des niveaux (ADR-016) : ils
/// doivent rester alignes sur l'enumeration `dev.rustforgex.instrument.ProbeLevel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ProbeLevel {
    /// Sonde eteinte : la methode reste transformee, mais ne mesure rien.
    #[default]
    Off,
    /// Compteur d'appels seul, sans lecture d'horloge.
    Counter,
    /// Compteur et duree.
    Timed,
    /// Duree, contexte d'appel et allocation approchee.
    Deep,
}

impl ProbeLevel {
    /// Code transmis a Java dans la table des niveaux.
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Counter => 1,
            Self::Timed => 2,
            Self::Deep => 3,
        }
    }

    /// Decode un niveau depuis son code.
    ///
    /// Un code inconnu rend `None` : le principe UNKNOWN = CONSERVATIVE interdit de
    /// deviner une profondeur qu'on ne comprend pas.
    #[must_use]
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Off),
            1 => Some(Self::Counter),
            2 => Some(Self::Timed),
            3 => Some(Self::Deep),
            _ => None,
        }
    }

    /// Indique si ce niveau demande de mesurer une duree.
    #[must_use]
    pub fn measures_duration(self) -> bool {
        matches!(self, Self::Timed | Self::Deep)
    }

    /// Profondeur souhaitable pour une chaleur donnee (PARTIE 5.5, etape 4).
    ///
    /// `COLD` reste au compteur : mesurer finement ce qui ne coute rien ne rapporte
    /// rien et coute, precisement, quelque chose.
    #[must_use]
    pub fn for_heat(heat: Heat) -> Self {
        match heat {
            Heat::Cold => Self::Counter,
            Heat::Warm | Heat::Hot => Self::Timed,
            Heat::Critical => Self::Deep,
        }
    }

    /// Libelle publie dans les rapports et `/rfx why`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Counter => "COUNTER",
            Self::Timed => "TIMED",
            Self::Deep => "DEEP",
        }
    }
}

impl core::fmt::Display for ProbeLevel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.label())
    }
}

/// Indique si la chaleur justifie de capturer le contexte d'appel.
///
/// La PARTIE 5.5 associe le contexte a `HOT` et au-dela. Le contexte est une donnee
/// supplementaire par enregistrement, pas une profondeur : il ne merite donc pas un
/// niveau a lui seul.
#[must_use]
pub fn wants_call_context(heat: Heat) -> bool {
    heat >= Heat::Hot
}

/// Etat global du profiler (SM de la PARTIE 5.5).
///
/// `OFF -> LIGHT -> NORMAL -> DEEP -> THROTTLED -> OFF`
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ProfilerLevel {
    /// Profilage arrete. Aucune sonde n'est armee, aucun echantillon n'est pris.
    #[default]
    Off,
    /// Compteurs seuls : le profilage le moins cher qui mesure encore quelque chose.
    Light,
    /// Durees, la profondeur nominale.
    Normal,
    /// Durees, contexte et allocation sur les unites les plus couteuses.
    Deep,
    /// Sondes eteintes, echantillonnage seul : le profilage coutait trop cher.
    Throttled,
}

impl ProfilerLevel {
    /// Plafond de profondeur impose aux sondes a cet etat.
    ///
    /// `Throttled` eteint toutes les sondes : c'est ce qui distingue cet etat de
    /// `Light`. L'observation continue par echantillonnage, qui ne passe pas par les
    /// sondes et ne coute donc rien au chemin chaud.
    #[must_use]
    pub fn max_probe_level(self) -> ProbeLevel {
        match self {
            Self::Off | Self::Throttled => ProbeLevel::Off,
            Self::Light => ProbeLevel::Counter,
            Self::Normal => ProbeLevel::Timed,
            Self::Deep => ProbeLevel::Deep,
        }
    }

    /// Indique si l'echantillonnage doit continuer a cet etat.
    ///
    /// `Throttled` conserve l'echantillonnage : c'est le dernier moyen d'observer
    /// quand les sondes ont ete jugees trop cheres (R-322).
    #[must_use]
    pub fn samples(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Descend d'un cran, comme l'exige le depassement de budget (PARTIE 5.5).
    ///
    /// `Off` est terminal : voir [`Self::raise`].
    #[must_use]
    pub fn reduce(self) -> Self {
        match self {
            Self::Deep => Self::Normal,
            Self::Normal => Self::Light,
            Self::Light => Self::Throttled,
            Self::Throttled | Self::Off => Self::Off,
        }
    }

    /// Remonte d'un cran quand le budget le permet a nouveau.
    ///
    /// `Off` ne remonte **jamais** de lui-meme. Le cahier des charges range l'arret du
    /// profilage parmi les mesures persistantes (RISK-04) : y etre descendu signifie
    /// que meme l'echantillonnage seul depassait le budget, et reessayer
    /// automatiquement rejouerait exactement le scenario qui a mene la.
    #[must_use]
    pub fn raise(self) -> Self {
        match self {
            Self::Off => Self::Off,
            Self::Throttled => Self::Light,
            Self::Light => Self::Normal,
            Self::Normal | Self::Deep => Self::Deep,
        }
    }

    /// Code publie dans le statut et vers Java.
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Light => 1,
            Self::Normal => 2,
            Self::Deep => 3,
            Self::Throttled => 4,
        }
    }

    /// Libelle publie dans les rapports (`rfx.profiler.level`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Light => "LIGHT",
            Self::Normal => "NORMAL",
            Self::Deep => "DEEP",
            Self::Throttled => "THROTTLED",
        }
    }
}

impl core::fmt::Display for ProfilerLevel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_level_codes_match_the_java_enumeration() {
        assert_eq!(ProbeLevel::Off.code(), 0);
        assert_eq!(ProbeLevel::Counter.code(), 1);
        assert_eq!(ProbeLevel::Timed.code(), 2);
        assert_eq!(ProbeLevel::Deep.code(), 3);
    }

    #[test]
    fn an_unknown_probe_code_is_refused_rather_than_guessed() {
        for code in 0..=3_u8 {
            let level = ProbeLevel::from_code(code).expect("code connu");
            assert_eq!(level.code(), code);
        }
        assert_eq!(ProbeLevel::from_code(4), None);
        assert_eq!(ProbeLevel::from_code(255), None);
    }

    #[test]
    fn only_timed_and_deep_read_the_clock() {
        assert!(!ProbeLevel::Off.measures_duration());
        assert!(!ProbeLevel::Counter.measures_duration());
        assert!(ProbeLevel::Timed.measures_duration());
        assert!(ProbeLevel::Deep.measures_duration());
    }

    #[test]
    fn depth_follows_heat() {
        assert_eq!(ProbeLevel::for_heat(Heat::Cold), ProbeLevel::Counter);
        assert_eq!(ProbeLevel::for_heat(Heat::Warm), ProbeLevel::Timed);
        assert_eq!(ProbeLevel::for_heat(Heat::Hot), ProbeLevel::Timed);
        assert_eq!(ProbeLevel::for_heat(Heat::Critical), ProbeLevel::Deep);
    }

    #[test]
    fn the_call_context_is_captured_from_hot_upwards() {
        assert!(!wants_call_context(Heat::Cold));
        assert!(!wants_call_context(Heat::Warm));
        assert!(wants_call_context(Heat::Hot));
        assert!(wants_call_context(Heat::Critical));
    }

    /// La descente decrite en PARTIE 5.5 : un cran a la fois, jusqu'a `OFF`.
    #[test]
    fn the_profiler_steps_down_one_level_at_a_time() {
        assert_eq!(ProfilerLevel::Deep.reduce(), ProfilerLevel::Normal);
        assert_eq!(ProfilerLevel::Normal.reduce(), ProfilerLevel::Light);
        assert_eq!(ProfilerLevel::Light.reduce(), ProfilerLevel::Throttled);
        assert_eq!(ProfilerLevel::Throttled.reduce(), ProfilerLevel::Off);
        assert_eq!(ProfilerLevel::Off.reduce(), ProfilerLevel::Off);
    }

    #[test]
    fn a_profiler_stopped_for_cost_never_restarts_on_its_own() {
        assert_eq!(
            ProfilerLevel::Off.raise(),
            ProfilerLevel::Off,
            "reessayer rejouerait le scenario qui a mene a l'arret"
        );
        assert_eq!(ProfilerLevel::Throttled.raise(), ProfilerLevel::Light);
        assert_eq!(ProfilerLevel::Light.raise(), ProfilerLevel::Normal);
        assert_eq!(ProfilerLevel::Normal.raise(), ProfilerLevel::Deep);
        assert_eq!(ProfilerLevel::Deep.raise(), ProfilerLevel::Deep);
    }

    #[test]
    fn throttled_turns_every_probe_off_but_keeps_sampling() {
        assert_eq!(ProfilerLevel::Throttled.max_probe_level(), ProbeLevel::Off);
        assert!(ProfilerLevel::Throttled.samples());

        assert_eq!(ProfilerLevel::Off.max_probe_level(), ProbeLevel::Off);
        assert!(!ProfilerLevel::Off.samples(), "arrete signifie arrete");
    }

    #[test]
    fn the_profiler_level_caps_the_probe_level() {
        assert_eq!(ProfilerLevel::Light.max_probe_level(), ProbeLevel::Counter);
        assert_eq!(ProfilerLevel::Normal.max_probe_level(), ProbeLevel::Timed);
        assert_eq!(ProfilerLevel::Deep.max_probe_level(), ProbeLevel::Deep);
    }
}
