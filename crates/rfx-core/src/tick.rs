//! SM-04 : fenetre de tick.
//!
//! Composant : C-01 (ouverture), C-05 (mesure). Interface : IF-02.
//! Cahier des charges : PARTIE 3.5 et PARTIE 7.4. Exigences : R-706 (fermeture
//! implicite d'un tick non termine), R-707 (aucun hook au-dela de la deadline),
//! R-732 (chaque operation dans sa phase). Maturite : `STABLE`.
//!
//! ```text
//! CLOSED --tick_begin--> OPEN_PRE --phase(VANILLA)--> OPEN_VANILLA
//!    ^                                                      |
//!    |                                               phase(DRAIN)
//!    |                                                      v
//!    +---tick_end--- OPEN_POST <--phase(POST)-- DRAINING <---+
//! ```

use std::time::{Duration, Instant};

use rfx_model::Side;

/// Duree maximale par defaut d'un hook de tick (`tick.max_hook_ns`, R-707).
pub const DEFAULT_MAX_HOOK_NS: u64 = 500_000;

/// Phase du tick, telle que Java la declare (IF-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TickPhase {
    /// Avant le tick du jeu : publication des decisions, preparation des snapshots.
    Pre,
    /// Tick Minecraft et mods : les sondes travaillent.
    Vanilla,
    /// Fin de tick : recuperation des taches, verification des conflits, commit.
    Drain,
    /// Apres le tick : collecte des metriques, traitement des resultats.
    Post,
}

impl TickPhase {
    /// Convertit le code entier transmis par Java.
    ///
    /// Renvoie `None` pour toute autre valeur : un code inconnu est un defaut de
    /// programmation, jamais une phase devinee.
    #[must_use]
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::Pre),
            1 => Some(Self::Vanilla),
            2 => Some(Self::Drain),
            3 => Some(Self::Post),
            _ => None,
        }
    }

    /// Code entier transmis a travers la frontiere.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Self::Pre => 0,
            Self::Vanilla => 1,
            Self::Drain => 2,
            Self::Post => 3,
        }
    }

    /// Libelle publie dans les diagnostics.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Pre => "PRE",
            Self::Vanilla => "VANILLA",
            Self::Drain => "DRAIN",
            Self::Post => "POST",
        }
    }
}

/// Etat de la fenetre de tick (SM-04).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    /// Aucun tick en cours.
    Closed,
    /// Tick ouvert, phase courante.
    Open(TickPhase),
}

impl WindowState {
    /// Indique si un snapshot peut etre pris (R-732 : uniquement en `OPEN_PRE`).
    #[must_use]
    pub fn allows_snapshot(self) -> bool {
        matches!(self, Self::Open(TickPhase::Pre))
    }

    /// Indique si un commit peut avoir lieu (R-732 : uniquement en `DRAINING`).
    #[must_use]
    pub fn allows_commit(self) -> bool {
        matches!(self, Self::Open(TickPhase::Drain))
    }

    /// Phase courante, ou `None` si la fenetre est fermee.
    #[must_use]
    pub fn phase(self) -> Option<TickPhase> {
        match self {
            Self::Closed => None,
            Self::Open(p) => Some(p),
        }
    }
}

/// Compteurs de la fenetre de tick, exposes par la telemetrie (C-34).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TickMetrics {
    /// Ticks ouverts depuis le demarrage.
    pub ticks: u64,
    /// Ticks fermes implicitement faute de `tick_end` (R-706, `rfx.tick.unbalanced`).
    pub unbalanced: u64,
    /// Transitions de phase refusees parce qu'elles ne suivaient pas SM-04.
    pub invalid_transitions: u64,
    /// Hooks ayant depasse la deadline (R-707).
    pub hook_budget_exceeded: u64,
    /// Duree cumulee passee dans la fenetre, en nanosecondes.
    pub total_window_ns: u64,
    /// Duree de la derniere fenetre fermee, en nanosecondes.
    pub last_window_ns: u64,
}

/// Fenetre de tick : suit l'etat, mesure la duree, compte les anomalies.
///
/// Elle n'interrompt jamais le jeu : une transition non conforme est comptee et
/// ignoree, jamais transformee en erreur remontant jusqu'a Forge.
#[derive(Debug)]
pub struct TickWindow {
    state: WindowState,
    tick: u64,
    side: Side,
    opened_at: Option<Instant>,
    max_hook: Duration,
    metrics: TickMetrics,
}

impl Default for TickWindow {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_HOOK_NS)
    }
}

impl TickWindow {
    /// Construit une fenetre fermee.
    #[must_use]
    pub fn new(max_hook_ns: u64) -> Self {
        Self {
            state: WindowState::Closed,
            tick: 0,
            side: Side::Common,
            opened_at: None,
            max_hook: Duration::from_nanos(max_hook_ns),
            metrics: TickMetrics::default(),
        }
    }

    /// Ouvre un tick.
    ///
    /// R-706 : si la fenetre precedente est restee ouverte — un autre mod ayant
    /// interrompu le tick avant le `POST` —, elle est fermee implicitement et
    /// comptee. Le jeu n'en sait rien ; c'est exactement ce qu'on veut.
    pub fn begin(&mut self, tick: u64, side: Side, now: Instant) {
        if self.state != WindowState::Closed {
            self.metrics.unbalanced = self.metrics.unbalanced.saturating_add(1);
            self.close(now);
        }
        self.state = WindowState::Open(TickPhase::Pre);
        self.tick = tick;
        self.side = side;
        self.opened_at = Some(now);
        self.metrics.ticks = self.metrics.ticks.saturating_add(1);
    }

    /// Declare une transition de phase.
    ///
    /// Renvoie `true` si la transition est conforme a SM-04. Une transition non
    /// conforme est comptee et refusee, sans effet sur l'etat : accepter un ordre
    /// arbitraire reviendrait a autoriser un commit hors de la phase de drain.
    pub fn phase(&mut self, phase: TickPhase) -> bool {
        let accepted = match (self.state, phase) {
            (WindowState::Open(TickPhase::Pre), TickPhase::Vanilla)
            | (WindowState::Open(TickPhase::Vanilla), TickPhase::Drain)
            | (WindowState::Open(TickPhase::Drain), TickPhase::Post) => true,
            // `PRE` est deja pose par `begin` : le redeclarer est sans effet mais
            // n'est pas une anomalie, Java pouvant l'emettre explicitement.
            (WindowState::Open(TickPhase::Pre), TickPhase::Pre) => true,
            _ => false,
        };

        if accepted {
            self.state = WindowState::Open(phase);
        } else {
            self.metrics.invalid_transitions = self.metrics.invalid_transitions.saturating_add(1);
        }
        accepted
    }

    /// Ferme le tick.
    ///
    /// Renvoie `true` si un tick etait effectivement ouvert. Un `tick_end` sans
    /// `tick_begin` est compte comme transition invalide.
    pub fn end(&mut self, now: Instant) -> bool {
        if self.state == WindowState::Closed {
            self.metrics.invalid_transitions = self.metrics.invalid_transitions.saturating_add(1);
            return false;
        }
        self.close(now);
        true
    }

    /// Ferme la fenetre et enregistre sa duree.
    fn close(&mut self, now: Instant) {
        if let Some(opened) = self.opened_at.take() {
            let elapsed = now.saturating_duration_since(opened);
            let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
            self.metrics.last_window_ns = nanos;
            self.metrics.total_window_ns = self.metrics.total_window_ns.saturating_add(nanos);
        }
        self.state = WindowState::Closed;
    }

    /// Comptabilise la duree d'un hook et signale un depassement de deadline.
    ///
    /// Renvoie `true` si le hook a tenu dans son budget (R-707). Un depassement est
    /// compte : c'est le signal qui fera reduire l'activite du runtime, pas une
    /// erreur.
    pub fn record_hook(&mut self, duration: Duration) -> bool {
        if duration > self.max_hook {
            self.metrics.hook_budget_exceeded = self.metrics.hook_budget_exceeded.saturating_add(1);
            return false;
        }
        true
    }

    /// Etat courant de la fenetre.
    #[must_use]
    pub fn state(&self) -> WindowState {
        self.state
    }

    /// Numero du tick courant, ou du dernier tick ouvert.
    #[must_use]
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Cote sur lequel le tick courant s'execute.
    #[must_use]
    pub fn side(&self) -> Side {
        self.side
    }

    /// Compteurs de la fenetre.
    #[must_use]
    pub fn metrics(&self) -> TickMetrics {
        self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(offset_ms: u64) -> Instant {
        // Base fixe pour que les durees soient deterministes.
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(Instant::now) + Duration::from_millis(offset_ms)
    }

    #[test]
    fn phase_codes_round_trip() {
        for phase in [
            TickPhase::Pre,
            TickPhase::Vanilla,
            TickPhase::Drain,
            TickPhase::Post,
        ] {
            assert_eq!(TickPhase::from_code(phase.code()), Some(phase));
        }
        assert_eq!(TickPhase::from_code(-1), None);
        assert_eq!(TickPhase::from_code(4), None);
    }

    #[test]
    fn a_nominal_tick_follows_the_state_machine() {
        let mut w = TickWindow::default();
        assert_eq!(w.state(), WindowState::Closed);

        w.begin(1, Side::Server, at(0));
        assert_eq!(w.state(), WindowState::Open(TickPhase::Pre));
        assert!(w.state().allows_snapshot(), "R-732 : snapshot en PRE");
        assert!(!w.state().allows_commit());

        assert!(w.phase(TickPhase::Vanilla));
        assert!(w.phase(TickPhase::Drain));
        assert!(w.state().allows_commit(), "R-732 : commit en DRAIN");
        assert!(!w.state().allows_snapshot());

        assert!(w.phase(TickPhase::Post));
        assert!(w.end(at(50)));

        assert_eq!(w.state(), WindowState::Closed);
        assert_eq!(w.metrics().ticks, 1);
        assert_eq!(w.metrics().unbalanced, 0);
        assert_eq!(w.metrics().invalid_transitions, 0);
        assert_eq!(w.metrics().last_window_ns, 50_000_000);
    }

    /// R-706 : un tick sans `tick_end` est ferme implicitement au suivant.
    #[test]
    fn an_unterminated_tick_is_closed_implicitly() {
        let mut w = TickWindow::default();

        w.begin(1, Side::Server, at(0));
        w.phase(TickPhase::Vanilla);
        // Un autre mod a interrompu le tick : ni DRAIN, ni POST, ni tick_end.

        w.begin(2, Side::Server, at(50));

        assert_eq!(
            w.metrics().unbalanced,
            1,
            "la fermeture implicite doit etre comptee"
        );
        assert_eq!(w.metrics().ticks, 2);
        assert_eq!(
            w.state(),
            WindowState::Open(TickPhase::Pre),
            "le nouveau tick est ouvert"
        );
        assert_eq!(w.tick(), 2);
    }

    #[test]
    fn an_out_of_order_transition_is_refused_and_counted() {
        let mut w = TickWindow::default();
        w.begin(1, Side::Server, at(0));

        // Sauter directement au drain contournerait la phase du jeu.
        assert!(!w.phase(TickPhase::Drain));
        assert_eq!(
            w.state(),
            WindowState::Open(TickPhase::Pre),
            "l'etat ne bouge pas"
        );
        assert_eq!(w.metrics().invalid_transitions, 1);

        // Revenir en arriere est tout aussi refuse.
        assert!(w.phase(TickPhase::Vanilla));
        assert!(!w.phase(TickPhase::Pre));
        assert_eq!(w.metrics().invalid_transitions, 2);
    }

    #[test]
    fn a_phase_outside_any_tick_is_refused() {
        let mut w = TickWindow::default();
        assert!(!w.phase(TickPhase::Vanilla));
        assert_eq!(w.metrics().invalid_transitions, 1);
        assert_eq!(w.state(), WindowState::Closed);
    }

    #[test]
    fn an_end_without_a_begin_is_counted() {
        let mut w = TickWindow::default();
        assert!(!w.end(at(0)));
        assert_eq!(w.metrics().invalid_transitions, 1);
        assert_eq!(w.metrics().ticks, 0);
    }

    /// R-707 : un hook trop long est signale, jamais transforme en erreur.
    #[test]
    fn a_hook_over_budget_is_reported() {
        let mut w = TickWindow::new(DEFAULT_MAX_HOOK_NS);

        assert!(w.record_hook(Duration::from_nanos(1_000)));
        assert_eq!(w.metrics().hook_budget_exceeded, 0);

        assert!(w.record_hook(Duration::from_nanos(DEFAULT_MAX_HOOK_NS)));
        assert_eq!(
            w.metrics().hook_budget_exceeded,
            0,
            "la deadline elle-meme tient"
        );

        assert!(!w.record_hook(Duration::from_nanos(DEFAULT_MAX_HOOK_NS + 1)));
        assert_eq!(w.metrics().hook_budget_exceeded, 1);
    }

    #[test]
    fn the_window_records_the_side_and_the_tick_number() {
        let mut w = TickWindow::default();
        w.begin(1234, Side::Client, at(0));
        assert_eq!(w.tick(), 1234);
        assert_eq!(w.side(), Side::Client);
    }

    #[test]
    fn window_durations_accumulate() {
        let mut w = TickWindow::default();

        w.begin(1, Side::Server, at(0));
        w.end(at(10));
        w.begin(2, Side::Server, at(20));
        w.end(at(45));

        assert_eq!(w.metrics().last_window_ns, 25_000_000);
        assert_eq!(w.metrics().total_window_ns, 35_000_000);
    }
}
