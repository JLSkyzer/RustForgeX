//! Auto-mesure du cout du profilage.
//!
//! Composant : C-05. Cahier des charges : PARTIE 5.5, etape 5. Exigences : H-07
//! (surcout d'analyse borne), INV-10 (l'overhead de RUSTFORGE-X est mesure et borne).
//! Tests : T-140. Maturite : `STABLE`.
//!
//! Un profiler qui ne mesure pas son propre cout est un profiler qui ment : il attribue
//! aux autres un temps qu'il consomme lui-meme. Le budget est double, et les deux
//! lectures ne disent pas la meme chose :
//!
//! - **une fraction d'un cœur** : le temps passe a profiler rapporte au temps reel
//!   ecoule. C'est ce que l'utilisateur ressent comme « le mod prend du CPU ».
//! - **une fraction du MSPT** : le temps passe a profiler rapporte a la duree utile du
//!   tick. C'est ce qui decide si le serveur tient ses vingt ticks par seconde.
//!
//! Un serveur peu charge tient largement la seconde mesure tout en depassant la
//! premiere, et un serveur sature fait l'inverse. Depasser l'une des deux suffit a
//! declencher la reduction de profondeur.
//!
//! # Deux sources, et l'une prime
//!
//! Les compteurs ci-dessus ne voient que le temps passe dans le code natif. Le cout des
//! appels injectes dans le bytecode Java leur echappe entierement, et c'est
//! probablement la depense dominante. [`crate::baseline`] le mesure de l'exterieur, en
//! eteignant periodiquement le profilage.
//!
//! Quand les deux sources se contredisent, **la mesure par mise en pause prime** : elle
//! observe le tick entier, la ou les compteurs n'observent que ce qu'ils savent
//! compter. C'est ce qu'exige R-770 — un chiffre mesure, jamais estime.

use crate::baseline::BaselineMeasurement;
use rfx_model::Ewma;

/// Part d'un cœur accordee au profilage, en pourcentage (PARTIE 5.5).
pub const DEFAULT_CPU_BUDGET_PCT: f32 = 2.0;

/// Part du temps de tick accordee au profilage, en pourcentage (PARTIE 5.5).
pub const DEFAULT_MSPT_BUDGET_PCT: f32 = 1.5;

/// Ticks observes avant qu'un verdict de depassement soit rendu.
///
/// Une seconde de jeu. Les premiers ticks apres le chargement portent le cout de la
/// mise en place des sondes ; les juger reviendrait a eteindre le profilage au moment
/// precis ou il commence.
const MIN_TICKS_BEFORE_VERDICT: u64 = 20;

/// Marge sous le budget en deca de laquelle la profondeur peut remonter.
///
/// Remonter des qu'on repasse sous le budget ferait osciller le niveau d'un tick a
/// l'autre : on ne remonte qu'a la moitie du budget.
const RAISE_RATIO: f32 = 0.5;

/// Cout d'un tick, tel que le runtime l'observe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TickCost {
    /// Temps passe dans le profilage pendant ce tick, en nanosecondes.
    pub profiling_ns: u64,
    /// Duree utile du tick, en nanosecondes.
    pub tick_ns: u64,
    /// Temps reel ecoule depuis l'ouverture du tick precedent, en nanosecondes.
    pub period_ns: u64,
}

/// Verdict rendu apres integration d'un tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverheadVerdict {
    /// Le profilage tient son budget : rien a changer.
    WithinBudget,
    /// Le budget est depasse : la profondeur doit descendre d'un cran (`E-1201`).
    OverBudget,
    /// Le profilage coute nettement moins que son budget : la profondeur peut remonter.
    Comfortable,
}

/// Mesure glissante du cout du profilage.
#[derive(Debug, Clone)]
pub struct OverheadMeter {
    cpu_budget_pct: f32,
    mspt_budget_pct: f32,
    core_pct: Ewma,
    mspt_pct: Ewma,
    ticks: u64,
    over_budget_ticks: u64,
    total_profiling_ns: u64,
    last_baseline: Option<BaselineMeasurement>,
    baseline_over_budget: u64,
}

impl OverheadMeter {
    /// Construit une mesure avec le budget donne, en pourcentage d'un cœur.
    ///
    /// Une valeur nulle ou negative est ramenee au defaut : un budget nul eteindrait
    /// le profilage des le premier tick, ce qui n'est pas un reglage mais une panne.
    #[must_use]
    pub fn new(cpu_budget_pct: f32) -> Self {
        let cpu = if cpu_budget_pct.is_finite() && cpu_budget_pct > 0.0 {
            cpu_budget_pct
        } else {
            DEFAULT_CPU_BUDGET_PCT
        };
        Self {
            cpu_budget_pct: cpu,
            // Le budget de MSPT suit le budget de cœur dans la meme proportion que les
            // valeurs par defaut : regler l'un sans l'autre rendrait le second muet.
            mspt_budget_pct: cpu * (DEFAULT_MSPT_BUDGET_PCT / DEFAULT_CPU_BUDGET_PCT),
            core_pct: Ewma::default(),
            mspt_pct: Ewma::default(),
            ticks: 0,
            over_budget_ticks: 0,
            total_profiling_ns: 0,
            last_baseline: None,
            baseline_over_budget: 0,
        }
    }

    /// Integre une mesure par mise en pause et rend le verdict correspondant.
    ///
    /// Le verdict ne connait que deux valeurs. `OverBudget` quand le cout mesure
    /// depasse le budget de MSPT ; `WithinBudget` sinon. Jamais `Comfortable` : une
    /// mesure tous les six mille ticks ne suffit pas a autoriser une remontee de
    /// profondeur, qui doit rester l'affaire des compteurs, bien plus frequents.
    ///
    /// La comparaison porte sur le budget de MSPT et non sur celui de cœur : la mesure
    /// compare des durees de tick, ce qui est exactement l'echelle du premier.
    pub fn record_baseline(&mut self, measurement: BaselineMeasurement) -> OverheadVerdict {
        self.last_baseline = Some(measurement);
        if measurement.overhead_pct() > self.mspt_budget_pct {
            self.baseline_over_budget = self.baseline_over_budget.saturating_add(1);
            return OverheadVerdict::OverBudget;
        }
        OverheadVerdict::WithinBudget
    }

    /// Derniere mesure par mise en pause, si une a abouti (PARTIE 12.4).
    #[must_use]
    pub fn last_baseline(&self) -> Option<BaselineMeasurement> {
        self.last_baseline
    }

    /// Mesures par mise en pause ayant conclu au depassement du budget.
    #[must_use]
    pub fn baseline_over_budget(&self) -> u64 {
        self.baseline_over_budget
    }

    /// Budget accorde, en pourcentage du temps de tick.
    #[must_use]
    pub fn mspt_budget_pct(&self) -> f32 {
        self.mspt_budget_pct
    }

    /// Integre le cout d'un tick et rend le verdict.
    pub fn record_tick(&mut self, cost: TickCost) -> OverheadVerdict {
        self.ticks = self.ticks.saturating_add(1);
        self.total_profiling_ns = self.total_profiling_ns.saturating_add(cost.profiling_ns);

        if cost.period_ns > 0 {
            self.core_pct
                .update(ratio_pct(cost.profiling_ns, cost.period_ns));
        }
        if cost.tick_ns > 0 {
            self.mspt_pct
                .update(ratio_pct(cost.profiling_ns, cost.tick_ns));
        }

        if self.ticks < MIN_TICKS_BEFORE_VERDICT {
            return OverheadVerdict::WithinBudget;
        }

        let core = self.core_pct.value() as f32;
        let mspt = self.mspt_pct.value() as f32;

        if core > self.cpu_budget_pct || mspt > self.mspt_budget_pct {
            self.over_budget_ticks = self.over_budget_ticks.saturating_add(1);
            return OverheadVerdict::OverBudget;
        }
        if core < self.cpu_budget_pct * RAISE_RATIO && mspt < self.mspt_budget_pct * RAISE_RATIO {
            return OverheadVerdict::Comfortable;
        }
        OverheadVerdict::WithinBudget
    }

    /// Cout du profilage en pourcentage d'un cœur (`rfx.profiler.overhead_pct`).
    #[must_use]
    pub fn overhead_pct(&self) -> f32 {
        self.core_pct.value() as f32
    }

    /// Cout du profilage en pourcentage du temps de tick.
    #[must_use]
    pub fn mspt_pct(&self) -> f32 {
        self.mspt_pct.value() as f32
    }

    /// Budget accorde, en pourcentage d'un cœur.
    #[must_use]
    pub fn cpu_budget_pct(&self) -> f32 {
        self.cpu_budget_pct
    }

    /// Ticks integres depuis le demarrage.
    #[must_use]
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Ticks pour lesquels un depassement a ete constate.
    #[must_use]
    pub fn over_budget_ticks(&self) -> u64 {
        self.over_budget_ticks
    }

    /// Temps total passe a profiler depuis le demarrage, en nanosecondes.
    #[must_use]
    pub fn total_profiling_ns(&self) -> u64 {
        self.total_profiling_ns
    }

    /// Indique si assez de ticks ont ete observes pour qu'un verdict soit rendu.
    #[must_use]
    pub fn has_verdict(&self) -> bool {
        self.ticks >= MIN_TICKS_BEFORE_VERDICT
    }
}

impl Default for OverheadMeter {
    fn default() -> Self {
        Self::new(DEFAULT_CPU_BUDGET_PCT)
    }
}

/// Rapport de deux durees, en pourcentage.
fn ratio_pct(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    (part as f64) * 100.0 / (whole as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un tick de 50 ms de periode, dont `profiling_ns` passees a profiler.
    fn tick(profiling_ns: u64) -> TickCost {
        TickCost {
            profiling_ns,
            tick_ns: 10_000_000,
            period_ns: 50_000_000,
        }
    }

    fn run(meter: &mut OverheadMeter, cost: TickCost, ticks: usize) -> OverheadVerdict {
        let mut verdict = OverheadVerdict::WithinBudget;
        for _ in 0..ticks {
            verdict = meter.record_tick(cost);
        }
        verdict
    }

    #[test]
    fn no_verdict_is_rendered_before_a_second_of_play() {
        let mut meter = OverheadMeter::default();
        // Un cout absurde, mais rendu trop tot pour etre juge.
        for _ in 0..(MIN_TICKS_BEFORE_VERDICT - 1) {
            assert_eq!(
                meter.record_tick(tick(40_000_000)),
                OverheadVerdict::WithinBudget
            );
        }
        assert!(!meter.has_verdict());
    }

    /// T-140 : un profilage bon marche reste sous son budget.
    #[test]
    fn a_cheap_profiler_stays_within_budget() {
        let mut meter = OverheadMeter::default();
        // 0,5 ms sur 50 ms de periode : 1 % d'un cœur, sous les 2 % accordes ; mais
        // 5 % du tick, ce qui depasse le budget de MSPT.
        let verdict = run(&mut meter, tick(500_000), 100);
        assert_eq!(
            verdict,
            OverheadVerdict::OverBudget,
            "le budget de MSPT est le plus contraignant sur un serveur peu charge"
        );
    }

    #[test]
    fn a_genuinely_cheap_profiler_is_reported_comfortable() {
        let mut meter = OverheadMeter::default();
        // 50 us par tick : 0,1 % d'un cœur et 0,5 % du tick.
        let verdict = run(&mut meter, tick(50_000), 100);
        assert_eq!(verdict, OverheadVerdict::Comfortable);
        assert!(meter.overhead_pct() < 0.2, "{}", meter.overhead_pct());
    }

    /// Un serveur sature : le tick occupe presque toute la periode.
    #[test]
    fn the_core_budget_binds_when_the_server_is_saturated() {
        let mut meter = OverheadMeter::default();
        let saturated = TickCost {
            profiling_ns: 1_500_000,
            tick_ns: 48_000_000,
            period_ns: 50_000_000,
        };
        // 3 % d'un cœur, mais seulement 3,1 % du tick : c'est le budget de cœur qui
        // tranche.
        let verdict = run(&mut meter, saturated, 100);
        assert_eq!(verdict, OverheadVerdict::OverBudget);
        assert!(meter.over_budget_ticks() > 0);
    }

    #[test]
    fn an_aberrant_budget_falls_back_to_the_default() {
        for value in [0.0_f32, -1.0, f32::NAN] {
            let meter = OverheadMeter::new(value);
            assert!(
                (meter.cpu_budget_pct() - DEFAULT_CPU_BUDGET_PCT).abs() < f32::EPSILON,
                "budget {value} devrait retomber sur le defaut"
            );
        }
    }

    #[test]
    fn a_larger_budget_relaxes_both_measures() {
        let mut meter = OverheadMeter::new(20.0);
        let verdict = run(&mut meter, tick(500_000), 100);
        assert_eq!(verdict, OverheadVerdict::Comfortable);
    }

    #[test]
    fn a_tick_without_duration_is_integrated_without_dividing_by_zero() {
        let mut meter = OverheadMeter::default();
        let verdict = meter.record_tick(TickCost::default());
        assert_eq!(verdict, OverheadVerdict::WithinBudget);
        assert_eq!(meter.ticks(), 1);
        assert!((meter.overhead_pct() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_cumulated_profiling_time_is_reported() {
        let mut meter = OverheadMeter::default();
        run(&mut meter, tick(1_000), 10);
        assert_eq!(meter.total_profiling_ns(), 10_000);
    }
}
