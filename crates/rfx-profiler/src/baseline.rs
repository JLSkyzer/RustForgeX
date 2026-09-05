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
//! Toutes les [`BASELINE_PERIOD_TICKS`], le profiler s'eteint pendant
//! [`BASELINE_PAUSE_TICKS`] ticks. On compare la duree des ticks pendant la pause a
//! celle des ticks qui la precedent immediatement : leur difference est le cout de
//! RUSTFORGE-X pour ce cycle, mesure de l'exterieur.
//!
//! Les deux fenetres sont **adjacentes dans le temps** : c'est la seule facon de
//! comparer deux charges de jeu comparables. Une moyenne depuis le demarrage ne dirait
//! rien, la charge d'un serveur variant bien plus vite que cela.
//!
//! La mediane, jamais la moyenne : un seul tick contamine par une pause du ramasse-
//! miettes suffirait a rendre la comparaison absurde (PARTIE 21.3, point 6).
//!
//! # Pourquoi un seul cycle ne suffit pas
//!
//! La premiere version rendait un verdict par cycle. La campagne C-36 du 2026-09-05 l'a
//! refutee : cinq mesures ont donne `13,3 %  0,0 %  0,5 %  0,0 %  0,0 %` alors que la
//! comparaison de deux installations donnait un ecart stable de **+36,4 %**. Un zero
//! n'y etait pas un cout nul mais une borne : la fenetre en pause avait dure *plus*
//! longtemps que la fenetre active, la charge du jeu ayant bouge entre les deux.
//!
//! Vingt ticks contre vingt ne sortent pas 0,9 ms d'un signal qui varie de plusieurs
//! millisecondes d'un tick a l'autre. Le rapport signal sur bruit etait le probleme,
//! pas le principe.
//!
//! D'ou la forme actuelle : **des cycles courts, souvent, et une mediane des
//! differences**. Chaque cycle rend une difference *signee* — negative quand le bruit
//! l'emporte — et [`BASELINE_CYCLES`] differences donnent une mediane ou ce bruit
//! s'annule au lieu de s'accumuler dans une borne. Borner chaque cycle a zero, comme le
//! faisait la premiere version, biaisait le resultat vers le haut avant meme de le
//! resumer.
//!
//! # Ce que la pause ne mesure pas
//!
//! Elle mesure ce qui est **retirable** : les sondes actives, le vidage, la
//! consolidation. Elle ne mesure pas le cout structurel de l'instrumentation — un
//! `enter` qui lit son niveau, le trouve a `OFF` et rend la main coute encore quelques
//! nanosecondes, et aucune pause ne peut l'annuler. Seul le retrait du transformateur
//! le ferait, et cela ne se decide pas en cours de partie.

use crate::overhead::TickCost;

/// Ticks profiles entre deux pauses.
///
/// Quinze secondes de jeu. La PARTIE 12.4 ecrit `N = 6000`, soit cinq minutes ; cette
/// cadence a ete **mesuree insuffisante** (voir l'en-tete du module et ADR-020). Le
/// principe reste celui du cahier des charges, sa cadence seule change.
pub const BASELINE_PERIOD_TICKS: u64 = 300;

/// Duree d'une pause de mesure, en ticks (PARTIE 12.4).
pub const BASELINE_PAUSE_TICKS: u32 = 20;

/// Cycles agreges avant qu'une mesure soit rendue.
///
/// Trente cycles de 320 ticks : une mesure toutes les huit minutes environ, batie sur
/// trente differences appariees au lieu d'une seule. C'est la seule facon d'extraire un
/// ecart plus petit que le bruit d'un tick isole.
pub const BASELINE_CYCLES: u32 = 30;

/// Plafond de cycles agreges, qui borne la taille des tableaux de travail.
const MAX_CYCLES: usize = 64;

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
    /// Mediane des differences par cycle, bornee a zero, en nanosecondes.
    ///
    /// Bornee **une seule fois, a la fin** : chaque cycle rend une difference signee,
    /// et c'est leur mediane qui est ramenee a zero si elle reste negative. Borner
    /// chaque cycle biaiserait le resultat vers le haut, puisque le bruit ne pourrait
    /// jouer que dans un sens.
    pub overhead_ns: u64,
    /// Nombre de cycles agreges dans cette mesure.
    pub cycles: u32,
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
    cycles_per_measurement: u32,
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

    /// Differences signees des cycles deja acheves, en nanosecondes.
    deltas: [i64; MAX_CYCLES],
    /// Medianes des fenetres actives, pour exprimer le resultat en pourcentage.
    active_medians: [u64; MAX_CYCLES],
    /// Medianes des fenetres en pause : la duree de reference d'un tick sans nous.
    paused_medians: [u64; MAX_CYCLES],
    cycles_done: usize,

    last: Option<BaselineMeasurement>,
    measurements: u64,
    pauses: u64,
}

impl BaselineSampler {
    /// Construit un cycle aux constantes de la PARTIE 12.4.
    #[must_use]
    pub fn new() -> Self {
        Self::with_cadence(BASELINE_PERIOD_TICKS, BASELINE_PAUSE_TICKS, BASELINE_CYCLES)
    }

    /// Construit un cycle a la cadence donnee.
    ///
    /// Une periode nulle, une pause nulle ou zero cycle ramenent aux constantes de ce
    /// module : une pause de zero tick ne mesurerait rien, une periode de zero tick
    /// eteindrait le profilage en permanence, et zero cycle ne rendrait jamais rien.
    #[must_use]
    pub fn with_cadence(period_ticks: u64, pause_ticks: u32, cycles: u32) -> Self {
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
        let cycles = if cycles == 0 {
            BASELINE_CYCLES
        } else {
            cycles.min(MAX_CYCLES as u32)
        };
        Self {
            period_ticks: period,
            pause_ticks: pause,
            cycles_per_measurement: cycles,
            phase: Phase::Active,
            ticks_until_pause: period,
            pause_left: 0,
            active: [0; WINDOW],
            active_len: 0,
            active_next: 0,
            paused: [0; WINDOW],
            paused_len: 0,
            deltas: [0; MAX_CYCLES],
            active_medians: [0; MAX_CYCLES],
            paused_medians: [0; MAX_CYCLES],
            cycles_done: 0,
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
                self.close_cycle();
                // La fenetre active repart de zero : les ticks d'avant la pause
                // decrivent une charge que la prochaine comparaison n'aura pas connue.
                self.active_len = 0;
                self.active_next = 0;

                let measurement = if self.cycles_done >= self.cycles_per_measurement as usize {
                    let m = self.conclude(tick);
                    self.cycles_done = 0;
                    self.measurements = self.measurements.saturating_add(1);
                    self.last = Some(m);
                    Some(m)
                } else {
                    None
                };
                BaselineEvent::PauseEnded(measurement)
            }
        }
    }

    /// Clot le cycle courant : retient sa difference signee.
    ///
    /// Un cycle dont l'une des deux fenetres est incomplete n'est pas retenu : une
    /// comparaison de trois ticks contre vingt ne dit rien, et un chiffre faux est pire
    /// que pas de chiffre (R-770 exige un chiffre **mesure**).
    fn close_cycle(&mut self) {
        if self.active_len < WINDOW
            || self.paused_len < self.pause_ticks as usize
            || self.cycles_done >= MAX_CYCLES
        {
            return;
        }
        let active_median_ns = median(&self.active[..self.active_len]);
        let paused_median_ns = median(&self.paused[..self.paused_len]);

        // Difference SIGNEE : quand le bruit l'emporte, elle est negative, et c'est
        // ainsi qu'elle doit entrer dans la mediane pour s'y annuler.
        self.deltas[self.cycles_done] = active_median_ns as i64 - paused_median_ns as i64;
        self.active_medians[self.cycles_done] = active_median_ns;
        self.paused_medians[self.cycles_done] = paused_median_ns;
        self.cycles_done += 1;
    }

    /// Resume les cycles accumules en une mesure.
    fn conclude(&self, tick: u64) -> BaselineMeasurement {
        let count = self.cycles_done;
        let delta = median_signed(&self.deltas[..count]);
        BaselineMeasurement {
            active_median_ns: median(&self.active_medians[..count]),
            paused_median_ns: median(&self.paused_medians[..count]),
            // Bornee ici, et ici seulement.
            overhead_ns: if delta > 0 { delta as u64 } else { 0 },
            cycles: count as u32,
            tick,
        }
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

    /// Cycles deja accumules pour la mesure en cours.
    #[must_use]
    pub fn cycles_done(&self) -> u32 {
        self.cycles_done as u32
    }

    /// Cycles necessaires a une mesure.
    #[must_use]
    pub fn cycles_per_measurement(&self) -> u32 {
        self.cycles_per_measurement
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

/// Mediane d'un echantillon signe, sans allocation.
///
/// Meme forme que [`median`], sur des differences qui peuvent etre negatives : c'est
/// tout l'interet de la moyenne des cycles, le bruit devant pouvoir jouer dans les deux
/// sens.
fn median_signed(values: &[i64]) -> i64 {
    if values.is_empty() {
        return 0;
    }
    let mut buffer = [0i64; MAX_CYCLES];
    let len = values.len().min(MAX_CYCLES);
    buffer[..len].copy_from_slice(&values[..len]);
    let sample = &mut buffer[..len];
    sample.sort_unstable();
    if len % 2 == 1 {
        sample[len / 2]
    } else {
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
        let mut sampler = BaselineSampler::with_cadence(50, 20, 1);
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
        let mut sampler = BaselineSampler::with_cadence(50, 20, 1);
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
        let mut sampler = BaselineSampler::with_cadence(50, 20, 1);
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
        let mut sampler = BaselineSampler::with_cadence(5, 20, 1);
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
        let mut sampler = BaselineSampler::with_cadence(50, 20, 1);
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
        let mut sampler = BaselineSampler::with_cadence(50, 20, 1);
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
        let mut sampler = BaselineSampler::with_cadence(50, 20, 1);
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

    /// Le defaut que la campagne du 2026-09-05 a revele, et ce qui le corrige.
    ///
    /// Le cout reel vaut 500 us. La fenetre en pause dure trois millisecondes de plus
    /// ou de moins que la normale, un cycle sur deux : c'est la charge du jeu qui bouge
    /// entre deux fenetres, exactement ce qui produisait des mesures a 0,0 %.
    #[test]
    fn t140_le_bruit_s_annule_sur_trente_cycles() {
        let mut sampler = BaselineSampler::with_cadence(50, 20, 30);
        let mut cycle = 0usize;
        let mut tick = 0u64;
        let mut result = None;

        while result.is_none() {
            tick += 1;
            assert!(tick < 1_000_000, "la mesure n'aboutit jamais");
            let noise: i64 = if cycle.is_multiple_of(2) {
                -3_000_000
            } else {
                3_000_000
            };
            let value = if sampler.is_paused() {
                (10_000_000i64 + noise) as u64
            } else {
                10_500_000
            };
            if let BaselineEvent::PauseEnded(m) = sampler.record_tick(tick, cost(value)) {
                cycle += 1;
                result = m;
            }
        }

        let measurement = result.expect("mesure aboutie");
        assert_eq!(measurement.cycles, 30);
        assert_eq!(
            measurement.overhead_ns, 500_000,
            "le bruit s'annule dans la mediane des differences, le cout reste"
        );
    }

    /// La forme refutee : un seul cycle, et une difference bornee a zero. Ce test
    /// existe pour que le defaut reste visible, et qu'on ne l'y ramene pas par
    /// inadvertance.
    #[test]
    fn t140_un_seul_cycle_rend_zero_des_que_le_bruit_domine() {
        let mut sampler = BaselineSampler::with_cadence(50, 20, 1);
        let measurement =
            run_one_cycle(&mut sampler, 10_500_000, 13_000_000).expect("mesure aboutie");

        assert_eq!(measurement.cycles, 1);
        assert_eq!(
            measurement.overhead_ns, 0,
            "un cycle isole ne distingue pas 500 us d'un ecart de charge"
        );
    }

    #[test]
    fn t140_aucune_mesure_avant_le_nombre_de_cycles_voulu() {
        let mut sampler = BaselineSampler::with_cadence(50, 20, 5);
        let mut ended = 0;
        let mut tick = 0u64;

        while ended < 4 {
            tick += 1;
            let value = if sampler.is_paused() {
                10_000_000
            } else {
                12_000_000
            };
            if let BaselineEvent::PauseEnded(m) = sampler.record_tick(tick, cost(value)) {
                ended += 1;
                assert!(m.is_none(), "aucune mesure avant le cinquieme cycle");
            }
        }
        assert_eq!(sampler.measurements(), 0);
        assert_eq!(sampler.cycles_done(), 4);
        assert_eq!(sampler.cycles_per_measurement(), 5);
    }

    #[test]
    fn la_mediane_signee_traite_les_valeurs_negatives() {
        assert_eq!(median_signed(&[-3, -1, 1, 5]), 0);
        assert_eq!(median_signed(&[-5, -4, -3]), -4);
        assert_eq!(median_signed(&[]), 0);
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
        let sampler = BaselineSampler::with_cadence(0, 0, 0);
        assert_eq!(sampler.ticks_until_pause(), BASELINE_PERIOD_TICKS);
        assert!(!sampler.is_paused());
    }
}
