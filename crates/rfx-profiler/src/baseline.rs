//! Auto-mesure par mise en pause : ce que le runtime coute reellement.
//!
//! Composant : C-05. Cahier des charges : PARTIE 12.4. Exigences : R-770 (le runtime
//! DOIT toujours pouvoir repondre « combien est-ce que je coute ? » avec un chiffre
//! mesure, jamais estime), H-07, INV-10. Tests : T-140. Maturite : `STABLE`.
//!
//! # Pourquoi des compteurs ne suffisent pas
//!
//! [`crate::overhead::OverheadMeter`] additionne le temps passe dans le code du
//! profiler : vidage des tampons, agregation, consolidation. C'est une mesure honnete
//! de ce qu'elle mesure, et elle rate l'essentiel.
//!
//! Les appels `RfxProbes.enter` et `exit` injectes dans le bytecode s'executent dans la
//! JVM, sur le fil du serveur, hors de toute zone chronometree par le natif. Aucun
//! compteur de ce cote de la frontiere ne les voit. Sur un modpack ou des dizaines de
//! milliers de methodes sont sondees, c'est la depense la plus probable — et un budget
//! aveugle a sa depense dominante ne borne rien.
//!
//! # Le principe
//!
//! Une fois toutes les [`BASELINE_PERIOD_TICKS`], le profiler s'eteint pendant
//! [`BASELINE_PAUSE_TICKS`] ticks. On compare la duree des ticks pendant la pause a
//! celle des ticks qui la precedent immediatement. La difference est le cout de
//! RUSTFORGE-X, mesure de l'exterieur, sans rien supposer de ce qui le compose.
//!
//! Les deux fenetres sont **adjacentes dans le temps** : c'est la seule facon de
//! comparer deux charges de jeu comparables. Une moyenne depuis le demarrage ne dirait
//! rien, la charge d'un serveur variant bien plus vite que cela.
//!
//! La mediane, jamais la moyenne : un seul tick contamine par une pause du ramasse-
//! miettes suffirait a rendre la comparaison absurde (PARTIE 21.3, point 6).
//!
//! # Ce que la pause ne mesure pas
//!
//! Elle mesure ce qui est **retirable** : les sondes actives, le vidage, la
//! consolidation. Elle ne mesure pas le cout structurel de l'instrumentation — un
//! `enter` qui lit son niveau, le trouve a `OFF` et rend la main coute encore quelques
//! nanosecondes, et aucune pause ne peut l'annuler. Seul le retrait du transformateur
//! le ferait, et cela ne se decide pas en cours de partie.

use crate::overhead::TickCost;

/// Ticks entre deux mesures de ligne de base (PARTIE 12.4, `N = 6000`).
///
/// Cinq minutes de jeu a vingt ticks par seconde.
pub const BASELINE_PERIOD_TICKS: u64 = 6_000;

/// Duree d'une pause de mesure, en ticks (PARTIE 12.4).
pub const BASELINE_PAUSE_TICKS: u32 = 20;

/// Taille des deux fenetres comparees.
///
/// Autant d'echantillons de chaque cote : comparer vingt ticks a deux cents rendrait
/// les deux medianes incomparables, la seconde lissant une charge que la premiere
/// subit.
const WINDOW: usize = BASELINE_PAUSE_TICKS as usize;

/// Resultat d'une mesure de ligne de base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaselineMeasurement {
    /// Mediane des ticks precedant la pause, en nanosecondes.
    pub active_median_ns: u64,
    /// Mediane des ticks pendant la pause, en nanosecondes.
    pub paused_median_ns: u64,
    /// Difference des deux medianes, bornee a zero, en nanosecondes.
    ///
    /// Bornee parce qu'un tick pause plus long qu'un tick actif ne signifie pas que le
    /// profilage fait gagner du temps : il signifie que la charge a bouge. La mesure
    /// est alors nulle, jamais negative.
    pub overhead_ns: u64,
    /// Tick auquel la mesure s'est achevee.
    pub tick: u64,
}

impl BaselineMeasurement {
    /// Cout du runtime en pourcentage de la duree d'un tick sans lui.
    ///
    /// Rapporte au tick **pause** : c'est la duree de reference, celle qu'aurait le
    /// serveur si RUSTFORGE-X ne faisait rien.
    #[must_use]
    pub fn overhead_pct(&self) -> f32 {
        if self.paused_median_ns == 0 {
            return 0.0;
        }
        ((self.overhead_ns as f64) * 100.0 / (self.paused_median_ns as f64)) as f32
    }
}

/// Ce que la cloture d'un tick change pour la mesure de ligne de base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineEvent {
    /// Rien a signaler.
    None,
    /// La pause commence : les sondes doivent etre eteintes au tick suivant.
    PauseBegan,
    /// La pause s'acheve. La mesure manque si les fenetres n'etaient pas completes.
    PauseEnded(Option<BaselineMeasurement>),
}

/// Phase du cycle de mesure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Le profilage tourne ; les durees de tick alimentent la fenetre active.
    Active,
    /// Le profilage est eteint ; les durees de tick alimentent la fenetre de reference.
    Paused,
}

/// Cycle de mesure par mise en pause (PARTIE 12.4).
#[derive(Debug, Clone)]
pub struct BaselineSampler {
    period_ticks: u64,
    pause_ticks: u32,
    phase: Phase,
    ticks_until_pause: u64,
    pause_left: u32,

    /// Anneau des dernieres durees de tick actives.
    active: [u64; WINDOW],
    active_len: usize,
    active_next: usize,

    /// Durees de tick relevees pendant la pause courante.
    paused: [u64; WINDOW],
    paused_len: usize,

    last: Option<BaselineMeasurement>,
    measurements: u64,
    pauses: u64,
}

impl BaselineSampler {
    /// Construit un cycle aux constantes de la PARTIE 12.4.
    #[must_use]
    pub fn new() -> Self {
        Self::with_period(BASELINE_PERIOD_TICKS, BASELINE_PAUSE_TICKS)
    }

    /// Construit un cycle a la periode donnee.
    ///
    /// Une periode nulle ou une pause nulle ramenent aux constantes normatives : une
    /// pause de zero tick ne mesurerait rien, et une periode de zero tick eteindrait
    /// le profilage en permanence.
    #[must_use]
    pub fn with_period(period_ticks: u64, pause_ticks: u32) -> Self {
        let period = if period_ticks == 0 {
            BASELINE_PERIOD_TICKS
        } else {
            period_ticks
        };
        let pause = if pause_ticks == 0 {
            BASELINE_PAUSE_TICKS
        } else {
            pause_ticks.min(WINDOW as u32)
        };
        Self {
            period_ticks: period,
            pause_ticks: pause,
            phase: Phase::Active,
            ticks_until_pause: period,
            pause_left: 0,
            active: [0; WINDOW],
            active_len: 0,
            active_next: 0,
            paused: [0; WINDOW],
            paused_len: 0,
            last: None,
            measurements: 0,
            pauses: 0,
        }
    }

    /// Integre la duree du tick qui vient de s'achever.
    ///
    /// `tick_ns` est la duree utile du tick, celle que mesure la fenetre de C-27 : ni
    /// la periode entre deux ticks, qui inclut l'attente, ni le temps passe dans le
    /// natif, qui n'est qu'une part de la depense.
    ///
    /// N'alloue pas.
    pub fn record_tick(&mut self, tick: u64, cost: TickCost) -> BaselineEvent {
        let tick_ns = cost.tick_ns;
        if tick_ns == 0 {
            // Un tick de duree nulle n'a pas ete mesure. Le compter fausserait les deux
            // medianes dans le meme sens, mais pas de la meme quantite.
            return BaselineEvent::None;
        }

        match self.phase {
            Phase::Active => {
                self.active[self.active_next] = tick_ns;
                self.active_next = (self.active_next + 1) % WINDOW;
                self.active_len = (self.active_len + 1).min(WINDOW);

                self.ticks_until_pause = self.ticks_until_pause.saturating_sub(1);
                if self.ticks_until_pause == 0 {
                    self.phase = Phase::Paused;
                    self.pause_left = self.pause_ticks;
                    self.paused_len = 0;
                    self.pauses = self.pauses.saturating_add(1);
                    return BaselineEvent::PauseBegan;
                }
                BaselineEvent::None
            }
            Phase::Paused => {
                if self.paused_len < WINDOW {
                    self.paused[self.paused_len] = tick_ns;
                    self.paused_len += 1;
                }
                self.pause_left = self.pause_left.saturating_sub(1);
                if self.pause_left > 0 {
                    return BaselineEvent::None;
                }

                self.phase = Phase::Active;
                self.ticks_until_pause = self.period_ticks;
                let measurement = self.measure(tick);
                if measurement.is_some() {
                    self.measurements = self.measurements.saturating_add(1);
                    self.last = measurement;
                }
                // La fenetre active repart de zero : les ticks d'avant la pause
                // decrivent une charge que la prochaine comparaison n'aura pas connue.
                self.active_len = 0;
                self.active_next = 0;
                BaselineEvent::PauseEnded(measurement)
            }
        }
    }

    /// Compare les deux fenetres.
    ///
    /// Rend `None` si l'une des deux est incomplete : une comparaison de trois ticks
    /// contre vingt ne dit rien, et rendre un chiffre faux serait pire que n'en rendre
    /// aucun (R-770 exige un chiffre **mesure**, pas un chiffre a tout prix).
    fn measure(&self, tick: u64) -> Option<BaselineMeasurement> {
        if self.active_len < WINDOW || self.paused_len < self.pause_ticks as usize {
            return None;
        }
        let active_median_ns = median(&self.active[..self.active_len]);
        let paused_median_ns = median(&self.paused[..self.paused_len]);
        Some(BaselineMeasurement {
            active_median_ns,
            paused_median_ns,
            overhead_ns: active_median_ns.saturating_sub(paused_median_ns),
            tick,
        })
    }

    /// `true` si le profilage doit rester eteint.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.phase == Phase::Paused
    }

    /// Derniere mesure aboutie, si elle existe.
    #[must_use]
    pub fn last(&self) -> Option<BaselineMeasurement> {
        self.last
    }

    /// Mesures abouties depuis le demarrage.
    #[must_use]
    pub fn measurements(&self) -> u64 {
        self.measurements
    }

    /// Pauses engagees depuis le demarrage, mesure aboutie ou non.
    #[must_use]
    pub fn pauses(&self) -> u64 {
        self.pauses
    }

    /// Ticks restants avant la prochaine pause.
    #[must_use]
    pub fn ticks_until_pause(&self) -> u64 {
        self.ticks_until_pause
    }
}

impl Default for BaselineSampler {
    fn default() -> Self {
        Self::new()
    }
}

/// Mediane d'un echantillon, sans allocation.
///
/// Le tri se fait sur une copie de pile bornee par [`WINDOW`]. Pour un echantillon de
/// taille paire, la mediane est la demi-somme des deux valeurs centrales.
fn median(values: &[u64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut buffer = [0u64; WINDOW];
    let len = values.len().min(WINDOW);
    buffer[..len].copy_from_slice(&values[..len]);
    let sample = &mut buffer[..len];
    sample.sort_unstable();
    if len % 2 == 1 {
        sample[len / 2]
    } else {
        // Demi-somme sans risque de debordement.
        let low = sample[len / 2 - 1];
        let high = sample[len / 2];
        low + (high - low) / 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(tick_ns: u64) -> TickCost {
        TickCost {
            profiling_ns: 0,
            tick_ns,
            period_ns: 50_000_000,
        }
    }

    /// Fait tourner le cycle et rend la premiere mesure aboutie.
    ///
    /// `active_ns` est la duree d'un tick profile, `paused_ns` celle d'un tick sans
    /// profilage.
    fn run_one_cycle(
        sampler: &mut BaselineSampler,
        active_ns: u64,
        paused_ns: u64,
    ) -> Option<BaselineMeasurement> {
        let mut tick = 0;
        loop {
            tick += 1;
            let paused = sampler.is_paused();
            let value = if paused { paused_ns } else { active_ns };
            match sampler.record_tick(tick, cost(value)) {
                BaselineEvent::PauseEnded(measurement) => return measurement,
                _ => {
                    assert!(tick < 100_000, "le cycle ne s'acheve jamais");
                }
            }
        }
    }

    #[test]
    fn t140_la_pause_mesure_la_difference_des_deux_fenetres() {
        let mut sampler = BaselineSampler::with_period(50, 20);
        let measurement =
            run_one_cycle(&mut sampler, 12_000_000, 10_000_000).expect("mesure aboutie");

        assert_eq!(measurement.active_median_ns, 12_000_000);
        assert_eq!(measurement.paused_median_ns, 10_000_000);
        assert_eq!(measurement.overhead_ns, 2_000_000);
        // Deux millisecondes sur dix : vingt pour cent.
        assert!((measurement.overhead_pct() - 20.0).abs() < 0.01);
    }

    #[test]
    fn t140_un_tick_pause_plus_long_ne_produit_pas_un_gain() {
        let mut sampler = BaselineSampler::with_period(50, 20);
        // La charge a monte pendant la pause : la mediane pausee depasse l'active.
        let measurement =
            run_one_cycle(&mut sampler, 10_000_000, 14_000_000).expect("mesure aboutie");

        assert_eq!(
            measurement.overhead_ns, 0,
            "un cout ne peut pas etre negatif"
        );
        assert_eq!(measurement.overhead_pct(), 0.0);
    }

    #[test]
    fn t140_la_pause_dure_exactement_le_nombre_de_ticks_demande() {
        let mut sampler = BaselineSampler::with_period(50, 20);
        let mut paused_ticks = 0;
        for tick in 1..=70u64 {
            if sampler.is_paused() {
                paused_ticks += 1;
            }
            sampler.record_tick(tick, cost(10_000_000));
        }
        assert_eq!(paused_ticks, 20);
    }

    #[test]
    fn t140_une_fenetre_active_incomplete_ne_produit_aucune_mesure() {
        // Periode plus courte que la fenetre : les ticks actifs ne suffisent jamais.
        let mut sampler = BaselineSampler::with_period(5, 20);
        let measurement = run_one_cycle(&mut sampler, 12_000_000, 10_000_000);

        assert!(
            measurement.is_none(),
            "aucun chiffre ne vaut mieux qu'un chiffre tire de trois ticks"
        );
        assert_eq!(sampler.measurements(), 0);
        assert_eq!(sampler.pauses(), 1, "la pause a bien eu lieu");
    }

    #[test]
    fn t140_les_ticks_de_duree_nulle_sont_ecartes() {
        let mut sampler = BaselineSampler::with_period(50, 20);
        for tick in 1..=200u64 {
            assert_eq!(sampler.record_tick(tick, cost(0)), BaselineEvent::None);
        }
        assert!(
            !sampler.is_paused(),
            "un tick non mesure ne fait pas avancer le cycle"
        );
        assert_eq!(sampler.ticks_until_pause(), 50);
    }

    #[test]
    fn t140_la_mediane_ignore_un_tick_contamine() {
        let mut sampler = BaselineSampler::with_period(50, 20);
        let mut tick = 0;
        let mut result = None;
        while result.is_none() {
            tick += 1;
            let paused = sampler.is_paused();
            // Un tick sur dix dure une seconde : une pause du ramasse-miettes.
            let contaminated = tick % 10 == 0;
            let value = match (paused, contaminated) {
                (_, true) => 1_000_000_000,
                (true, false) => 10_000_000,
                (false, false) => 12_000_000,
            };
            if let BaselineEvent::PauseEnded(m) = sampler.record_tick(tick, cost(value)) {
                result = Some(m);
            }
        }
        let measurement = result.flatten().expect("mesure aboutie");
        // Les medianes restent celles des ticks ordinaires : la contamination est
        // conservee dans l'echantillon, mais ne le decide pas.
        assert_eq!(measurement.active_median_ns, 12_000_000);
        assert_eq!(measurement.paused_median_ns, 10_000_000);
    }

    #[test]
    fn t140_les_mesures_s_enchainent() {
        let mut sampler = BaselineSampler::with_period(50, 20);
        for _ in 0..3 {
            run_one_cycle(&mut sampler, 12_000_000, 10_000_000).expect("mesure aboutie");
        }
        assert_eq!(sampler.measurements(), 3);
        assert_eq!(sampler.pauses(), 3);
        assert_eq!(
            sampler.last().expect("derniere mesure").overhead_ns,
            2_000_000
        );
    }

    #[test]
    fn la_mediane_paire_est_la_demi_somme_des_valeurs_centrales() {
        assert_eq!(median(&[1, 2, 3, 4]), 2);
        assert_eq!(median(&[1, 3, 5, 9]), 4);
        assert_eq!(median(&[7]), 7);
        assert_eq!(median(&[]), 0);
    }

    #[test]
    fn des_reglages_absurdes_reviennent_aux_constantes_normatives() {
        let sampler = BaselineSampler::with_period(0, 0);
        assert_eq!(sampler.ticks_until_pause(), BASELINE_PERIOD_TICKS);
        assert!(!sampler.is_paused());
    }
}
