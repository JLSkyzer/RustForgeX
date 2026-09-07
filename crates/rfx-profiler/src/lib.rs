//! C-05 : profiler — agregation des mesures et adaptation de la profondeur.
//!
//! Cahier des charges : PARTIE 5.5. Exigences : R-320 (aucune allocation dans le
//! chemin chaud), R-321 (eviction LRU au-dela du plafond), R-322 (fonctionne sans
//! aucune sonde). Invariant : INV-10 (l'overhead est mesure et borne).
//! Tests : T-140 a T-144. Maturite : `STABLE`.
//!
//! Ce crate ne mesure rien lui-meme : il recoit les enregistrements que Java a ecrits
//! dans les tampons de C-31, les agrege par unite de travail, et decide de la
//! profondeur de sondage a appliquer au tick suivant. Il ne connait ni la JVM, ni
//! Forge, ni le moindre nom de mod (INV-12).
//!
//! # Le profiler se mesure lui-meme, de deux facons
//!
//! Un profiler qui ignore son propre cout attribue aux autres un temps qu'il consomme.
//! Chaque tick, le runtime lui remet le temps qu'il a passe a profiler ; au-dela du
//! budget, la profondeur descend d'un cran, jusqu'a l'arret complet. Voir
//! [`overhead`].
//!
//! Ces compteurs ne voient que le code natif. Le cout des appels injectes dans le
//! bytecode Java leur echappe, et c'est probablement la depense dominante. Une fois
//! toutes les [`baseline::BASELINE_PERIOD_TICKS`], le profilage s'eteint donc pendant
//! vingt ticks : la difference entre les durees de tick avec et sans mesure est le
//! cout de RUSTFORGE-X, observe de l'exterieur. Voir [`baseline`] et la PARTIE 12.4.
//!
//! # Sens des dependances
//!
//! `rfx-profiler` depend de `rfx-model` et `rfx-memory`, et de rien d'autre (INV-13).
//! Il ne connait pas `rfx-core` : c'est `rfx-core` qui l'appelle, journalise `E-1201`
//! quand la profondeur descend, et transmet a Java la table des niveaux.

#![doc(html_root_url = "https://example.invalid/rustforgex")]

pub mod baseline;
pub mod level;
pub mod overhead;
pub mod store;

use rfx_memory::probe_buffer::{ProbeRecord, RecordKind};
use rfx_model::{Heat, ProfilerStatus};

pub use baseline::{
    BaselineEvent, BaselineMeasurement, BaselineSampler, BASELINE_PAUSE_TICKS,
    BASELINE_PERIOD_TICKS,
};
pub use level::{wants_call_context, ProbeLevel, ProfilerLevel};
pub use overhead::{OverheadMeter, OverheadVerdict, TickCost, DEFAULT_CPU_BUDGET_PCT};
pub use store::{WorkloadEntry, WorkloadStore};

/// Nombre d'unites de travail suivies par defaut (PARTIE 5.5).
pub const DEFAULT_MAX_WORKLOADS: usize = 20_000;

/// Ticks consecutifs sous la moitie du budget avant de remonter d'un cran.
///
/// Cinq secondes de jeu. Remonter plus vite ferait osciller la profondeur au rythme
/// des variations normales de charge, et chaque oscillation coute une retransmission
/// de la table des niveaux a Java.
const COMFORTABLE_TICKS_BEFORE_RAISE: u32 = 100;

/// Reglages du profiler effectivement lus par le code implemente.
///
/// Les budgets d'echantillonnage et de profondeur de contexte de la PARTIE 5.5 ne
/// figurent pas ici : aucun code ne les lit encore, et une option qui ne pilote rien
/// trompe l'utilisateur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfilerConfig {
    /// Plafond d'unites de travail suivies simultanement (R-321).
    pub max_workloads: usize,
    /// Part d'un cœur accordee au profilage, en pourcentage.
    pub cpu_budget_pct: f32,
    /// Ticks entre deux mises en pause de mesure (PARTIE 12.4).
    pub baseline_period_ticks: u64,
    /// Duree d'une pause de mesure, en ticks, qui fixe aussi la taille des fenetres.
    ///
    /// C'est le reglage qui decide si la ligne de base mesure quelque chose. A vingt
    /// ticks, l'erreur type de la difference vaut environ 1,6 ms pour un signal de
    /// 250 us : la mediane resume alors du bruit, et quatorze cycles sur trente rendent
    /// une valeur negative, physiquement impossible. Le prix d'une fenetre large est
    /// que le profilage est eteint d'autant plus longtemps.
    pub baseline_pause_ticks: u32,
}

impl Default for ProfilerConfig {
    fn default() -> Self {
        Self {
            max_workloads: DEFAULT_MAX_WORKLOADS,
            cpu_budget_pct: DEFAULT_CPU_BUDGET_PCT,
            baseline_period_ticks: baseline::BASELINE_PERIOD_TICKS,
            baseline_pause_ticks: baseline::BASELINE_PAUSE_TICKS,
        }
    }
}

/// Ce que la cloture d'un tick a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickReport {
    /// Etat du profiler apres cloture.
    pub level: ProfilerLevel,
    /// `true` si l'etat du profiler a change pendant cette cloture.
    pub level_changed: bool,
    /// `true` si le budget d'overhead a ete depasse (`E-1201`).
    pub over_budget: bool,
    /// `true` si la table des niveaux doit etre retransmise a Java.
    pub levels_dirty: bool,
    /// `true` si le profilage est eteint pour mesurer une ligne de base (PARTIE 12.4).
    pub baseline_paused: bool,
    /// Mesure de ligne de base achevee pendant ce tick, si une a abouti.
    pub baseline: Option<BaselineMeasurement>,
}

/// C-05 : agregation des mesures et adaptation de la profondeur.
#[derive(Debug)]
pub struct Profiler {
    config: ProfilerConfig,
    store: WorkloadStore,
    level: ProfilerLevel,
    overhead: OverheadMeter,
    baseline: BaselineSampler,
    records_dropped_paused: u64,
    tick: u64,
    comfortable_streak: u32,
    records_ingested: u64,
    records_unknown: u64,
    records_ignored: u64,
    samples_ingested: u64,
    zero_duration_exits: u64,
    level_changes: u64,
    levels_dirty: bool,
}

impl Profiler {
    /// Construit un profiler a l'arret.
    ///
    /// Rien n'est mesure tant que [`Self::start`] n'a pas ete appelee : le profiler
    /// existe bien avant que le jeu ne soit en etat d'etre observe.
    #[must_use]
    pub fn new(config: ProfilerConfig) -> Self {
        Self {
            store: WorkloadStore::new(config.max_workloads),
            overhead: OverheadMeter::new(config.cpu_budget_pct),
            baseline: BaselineSampler::with_cadence(
                config.baseline_period_ticks,
                config.baseline_pause_ticks,
                baseline::BASELINE_CYCLES,
            ),
            records_dropped_paused: 0,
            config,
            level: ProfilerLevel::Off,
            tick: 0,
            comfortable_streak: 0,
            records_ingested: 0,
            records_unknown: 0,
            records_ignored: 0,
            samples_ingested: 0,
            zero_duration_exits: 0,
            level_changes: 0,
            levels_dirty: false,
        }
    }

    /// Demarre le profilage au niveau le moins cher (`OFF -> LIGHT`).
    ///
    /// On ne demarre pas a `NORMAL` : le cout du profilage n'a pas encore ete mesure
    /// sur cette machine, et rien n'autorise a supposer qu'il tient le budget. La
    /// remontee vers `NORMAL` puis `DEEP` se merite, tick apres tick.
    pub fn start(&mut self) {
        if self.level == ProfilerLevel::Off {
            self.set_level(ProfilerLevel::Light);
        }
    }

    /// Arrete le profilage et eteint toutes les sondes.
    pub fn stop(&mut self) {
        self.set_level(ProfilerLevel::Off);
    }

    /// Enregistre une unite de travail et rend son identifiant de sonde.
    ///
    /// Renvoie `None` si le plafond est atteint et qu'aucune unite froide n'est
    /// evincable : la methode n'est alors pas sondee, ce qui degrade la mesure sans
    /// affecter le jeu.
    pub fn register(&mut self, work_id: rfx_model::WorkId) -> Option<u32> {
        let probe_id = self.store.register(work_id)?;
        // Une unite dont rien n'est connu est traitee comme froide : le compteur, et
        // rien de plus, sous le plafond de l'etat courant. Attendre la fin du tick
        // pour armer la sonde perdrait les mesures du tick en cours.
        //
        // Pendant une pause de mesure, le plafond est `OFF` : armer une sonde neuve
        // ajouterait au tick pause un cout que le tick actif n'avait pas, et la ligne
        // de base mesurerait alors n'importe quoi.
        let wanted = ProbeLevel::for_heat(Heat::Cold).min(self.probe_cap());
        if let Some(entry) = self.store.get_mut(probe_id) {
            entry.level = wanted;
        }
        self.levels_dirty = true;
        Some(probe_id)
    }

    /// Signale une collision de `WorkId` (R-202, `E-2101`).
    pub fn report_collision(&mut self, work_id: rfx_model::WorkId) {
        self.store.report_collision(work_id);
        self.levels_dirty = true;
    }

    /// Agrege un enregistrement de sonde.
    ///
    /// Chemin chaud au sens de R-320 : cette fonction n'alloue pas. Elle est appelee
    /// une fois par enregistrement, donc potentiellement des milliers de fois par
    /// tick, mais toujours depuis le vidage de fin de tick — jamais depuis le code du
    /// jeu.
    pub fn ingest(&mut self, record: &ProbeRecord) {
        if self.baseline.is_paused() {
            // Une pause de mesure doit etre invisible pour l'etat du profiler. Les
            // rares enregistrements encore en vol decriraient une fenetre ou les
            // sondes s'eteignent : les agreger deformerait les moyennes glissantes au
            // moment precis ou l'on cherche a mesurer proprement.
            self.records_dropped_paused = self.records_dropped_paused.saturating_add(1);
            return;
        }
        let Some(entry) = self.store.get_mut(record.probe_id) else {
            self.records_unknown = self.records_unknown.saturating_add(1);
            return;
        };

        match record.kind() {
            RecordKind::Enter => {
                // Niveau COUNTER : le passage est enregistre sans lecture d'horloge.
                entry.calls_this_tick = entry.calls_this_tick.saturating_add(1);
            }
            RecordKind::Exit => {
                entry.calls_this_tick = entry.calls_this_tick.saturating_add(1);
                entry.dynamics.cpu_ns.record(record.value);
                entry.dynamics.wall_ns.record(record.value);
                if record.value == 0 {
                    // Java remet zero quand l'horloge a recule (FM-12). Une horloge
                    // trop grossiere produit la meme valeur : les deux cas se comptent
                    // ensemble, et l'observation est conservee telle quelle.
                    self.zero_duration_exits = self.zero_duration_exits.saturating_add(1);
                }
            }
            RecordKind::Alloc => {
                entry.alloc_bytes_this_tick =
                    entry.alloc_bytes_this_tick.saturating_add(record.value);
            }
            RecordKind::Sample => {
                entry.sampled_hits = entry.sampled_hits.saturating_add(1);
                entry.sampled_ns_this_tick =
                    entry.sampled_ns_this_tick.saturating_add(record.value);
                self.samples_ingested = self.samples_ingested.saturating_add(1);
            }
            RecordKind::Event | RecordKind::Unknown(_) => {
                // Les evenements Forge relevent de C-06, et une nature inconnue ne se
                // devine pas : l'enregistrement est compte, jamais interprete.
                self.records_ignored = self.records_ignored.saturating_add(1);
            }
        }

        self.records_ingested = self.records_ingested.saturating_add(1);
    }

    /// Cloture le tick : consolide les mesures, adapte la profondeur.
    ///
    /// Ce n'est pas le chemin chaud : les quantiles y sont recalcules, ce qui coute
    /// bien plus qu'un enregistrement.
    pub fn end_tick(&mut self, cost: TickCost) -> TickReport {
        self.tick = self.tick.saturating_add(1);
        let tick = self.tick;

        // Pendant une pause de mesure, le profiler ne fait rien : ni consolidation, ni
        // integration au budget. C'est le sens meme d'une ligne de base — le tick doit
        // couter ce qu'il couterait sans nous. Consolider ici ajouterait au tick pause
        // une depense que la comparaison attribuerait ensuite au jeu, et refroidirait
        // vingt ticks durant des unites de travail qui n'ont rien cesse de faire.
        let paused = self.baseline.is_paused();
        if !paused {
            self.consolidate(tick);
        }

        let event = self.baseline.record_tick(tick, cost);

        let mut verdict = if paused {
            OverheadVerdict::WithinBudget
        } else {
            self.overhead.record_tick(cost)
        };

        let mut measurement = None;
        match event {
            BaselineEvent::PauseBegan => {
                // Rien a faire ici : `adapt_probe_levels` lira le nouveau plafond.
            }
            BaselineEvent::PauseEnded(result) => {
                measurement = result;
                if let Some(measured) = result {
                    // Une mesure prime sur une estimation (R-770) : elle voit le cout
                    // des appels injectes dans le bytecode, que les compteurs natifs
                    // ne peuvent pas voir.
                    if self.overhead.record_baseline(measured) == OverheadVerdict::OverBudget {
                        verdict = OverheadVerdict::OverBudget;
                    }
                }
            }
            BaselineEvent::None => {}
        }

        let before = self.level;
        self.apply_verdict(verdict);
        let level_changed = before != self.level;

        // La profondeur des sondes se recalcule apres l'eventuel changement d'etat :
        // le plafond global vient d'etre revu, et il prime sur la chaleur.
        self.adapt_probe_levels();

        TickReport {
            level: self.level,
            level_changed,
            over_budget: verdict == OverheadVerdict::OverBudget,
            levels_dirty: self.levels_dirty,
            baseline_paused: self.baseline.is_paused(),
            baseline: measurement,
        }
    }

    /// Integre les compteurs du tick dans les mesures glissantes.
    fn consolidate(&mut self, tick: u64) {
        for entry in self.store.iter_mut() {
            let calls = entry.calls_this_tick;
            let sampled = entry.sampled_ns_this_tick;

            entry.dynamics.calls_per_tick.update(calls as f64);
            entry.dynamics.total_calls = entry.dynamics.total_calls.saturating_add(calls);
            entry
                .sampled_ns_per_tick
                .update(entry.sampled_ns_this_tick as f64);
            entry
                .dynamics
                .alloc_bytes
                .update(entry.alloc_bytes_this_tick as f64);

            if calls > 0 || sampled > 0 {
                entry.dynamics.last_seen_tick = tick;
            }

            // Une unite muette et deja froide ne changera pas de classe : recalculer
            // ses quantiles a chaque tick couterait, sur vingt mille unites, bien plus
            // que ce que le profilage lui-meme s'autorise.
            if calls > 0 || sampled > 0 || entry.dynamics.heat != Heat::Cold {
                entry.dynamics.refresh();
                entry.dynamics.heat = Heat::classify(entry.cost_ns_per_tick());
            }

            entry.calls_this_tick = 0;
            entry.sampled_ns_this_tick = 0;
            entry.alloc_bytes_this_tick = 0;
        }
    }

    /// Applique le verdict de l'auto-mesure a l'etat du profiler.
    fn apply_verdict(&mut self, verdict: OverheadVerdict) {
        match verdict {
            OverheadVerdict::OverBudget => {
                self.comfortable_streak = 0;
                if self.level != ProfilerLevel::Off {
                    self.set_level(self.level.reduce());
                }
            }
            OverheadVerdict::Comfortable => {
                self.comfortable_streak = self.comfortable_streak.saturating_add(1);
                // La permission de monter vient de la derniere ligne de base, jamais
                // des compteurs seuls : ils ne voient pas les appels injectes dans le
                // bytecode. Constate deux fois en production — le profiler atteignait
                // DEEP en vingt-cinq secondes alors que son cout reel etait de 32 %,
                // puis y revenait en quinze secondes apres chaque reduction. C'est
                // l'argument de `start`, prolonge a toute la vie du profiler.
                if !self.overhead.baseline_allows_raise() {
                    return;
                }
                if self.comfortable_streak >= COMFORTABLE_TICKS_BEFORE_RAISE {
                    self.comfortable_streak = 0;
                    let raised = self.level.raise();
                    if raised != self.level {
                        self.set_level(raised);
                    }
                }
            }
            OverheadVerdict::WithinBudget => {
                self.comfortable_streak = 0;
            }
        }
    }

    /// Plafond de profondeur applicable a l'instant present.
    ///
    /// Pendant une pause de mesure, il vaut `OFF` quel que soit l'etat du profiler :
    /// la pause n'est pas un changement d'etat, c'est une extinction temporaire des
    /// sondes. L'etat, lui, ne bouge pas, et la profondeur revient d'elle-meme a la
    /// fin de la pause puisqu'elle se deduit de la chaleur.
    fn probe_cap(&self) -> ProbeLevel {
        if self.baseline.is_paused() {
            ProbeLevel::Off
        } else {
            self.level.max_probe_level()
        }
    }

    /// Aligne la profondeur de chaque sonde sur la chaleur, sous le plafond global.
    fn adapt_probe_levels(&mut self) {
        let cap = self.probe_cap();
        let mut dirty = false;
        for entry in self.store.iter_mut() {
            let wanted = ProbeLevel::for_heat(entry.dynamics.heat).min(cap);
            if entry.level != wanted {
                entry.level = wanted;
                dirty = true;
            }
        }
        self.levels_dirty |= dirty;
    }

    /// Change l'etat du profiler et marque la table des niveaux a retransmettre.
    fn set_level(&mut self, level: ProfilerLevel) {
        if self.level == level {
            return;
        }
        self.level = level;
        self.level_changes = self.level_changes.saturating_add(1);
        self.levels_dirty = true;
    }

    /// Rend la table des niveaux si elle a change depuis le dernier appel.
    ///
    /// Renvoyer `None` evite de retraverser la frontiere pour retransmettre une table
    /// identique a la precedente : le cas le plus frequent, de loin.
    pub fn take_levels(&mut self) -> Option<Vec<u8>> {
        if !self.levels_dirty {
            return None;
        }
        self.levels_dirty = false;
        Some(self.store.levels())
    }

    /// Etat courant du profiler.
    #[must_use]
    pub fn level(&self) -> ProfilerLevel {
        self.level
    }

    /// Table des unites de travail.
    #[must_use]
    pub fn store(&self) -> &WorkloadStore {
        &self.store
    }

    /// Reglages en vigueur.
    #[must_use]
    pub fn config(&self) -> ProfilerConfig {
        self.config
    }

    /// Auto-mesure du cout du profilage, par compteurs.
    #[must_use]
    pub fn overhead(&self) -> &OverheadMeter {
        &self.overhead
    }

    /// Cycle de mesure par mise en pause (PARTIE 12.4).
    #[must_use]
    pub fn baseline(&self) -> &BaselineSampler {
        &self.baseline
    }

    /// `true` si le profilage est temporairement eteint pour mesurer une ligne de base.
    #[must_use]
    pub fn baseline_paused(&self) -> bool {
        self.baseline.is_paused()
    }

    /// Ticks clotures depuis le demarrage.
    #[must_use]
    pub fn ticks(&self) -> u64 {
        self.tick
    }

    /// Etat publie (`rfx.profiler.*`).
    ///
    /// Les pourcentages sont convertis en centiemes : le blob de statut ne transporte
    /// que des entiers, par choix assume du lecteur CBOR de Java.
    #[must_use]
    pub fn status(&self) -> ProfilerStatus {
        // `baseline_measurements` a zero signifie « pas encore mesure ». Les champs
        // qui en dependent valent alors zero, et l'affichage doit le dire ainsi plutot
        // que d'annoncer un cout nul, qui serait faux (R-770).
        let baseline = self.overhead.last_baseline();
        ProfilerStatus {
            level: self.level.label().to_owned(),
            workloads_tracked: self.store.live_count() as u64,
            probes_allocated: self.store.allocated_count() as u64,
            overhead_pct_x100: pct_x100(self.overhead.overhead_pct()),
            mspt_pct_x100: pct_x100(self.overhead.mspt_pct()),
            records_ingested: self.records_ingested,
            records_unknown: self.records_unknown,
            records_ignored: self.records_ignored,
            samples_ingested: self.samples_ingested,
            evictions: self.store.evictions(),
            collisions: self.store.collisions(),
            zero_duration_exits: self.zero_duration_exits,
            level_changes: self.level_changes,
            baseline_measurements: self.baseline.measurements(),
            baseline_overhead_ns: baseline.map_or(0, |m| m.overhead_ns),
            baseline_overhead_pct_x100: baseline.map_or(0, |m| pct_x100(m.overhead_pct())),
            baseline_cycles: baseline.map_or(0, |m| u64::from(m.cycles)),
            baseline_delta_min_ns: baseline.map_or(0, |m| m.delta_min_ns),
            baseline_delta_max_ns: baseline.map_or(0, |m| m.delta_max_ns),
            baseline_positive_cycles: baseline.map_or(0, |m| u64::from(m.positive_cycles)),
            baseline_tick: baseline.map_or(0, |m| m.tick),
            baseline_ticks_until_pause: self.baseline.ticks_until_pause(),
            records_dropped_paused: self.records_dropped_paused,
        }
    }
}

/// Convertit un pourcentage en centiemes de pourcent, borne a zero.
fn pct_x100(pct: f32) -> u64 {
    if !pct.is_finite() || pct <= 0.0 {
        return 0;
    }
    (f64::from(pct) * 100.0).round() as u64
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new(ProfilerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfx_model::WorkId;

    fn record(probe_id: u32, kind: RecordKind, value: u64) -> ProbeRecord {
        ProbeRecord {
            probe_id,
            flags: 0,
            context_hash16: 0,
            timestamp_ns: 0,
            value,
            kind: kind.to_byte(),
        }
    }

    /// Un tick bon marche : le profilage y tient largement son budget.
    fn cheap_tick() -> TickCost {
        TickCost {
            profiling_ns: 10_000,
            tick_ns: 10_000_000,
            period_ns: 50_000_000,
        }
    }

    fn started() -> Profiler {
        let mut profiler = Profiler::default();
        profiler.start();
        profiler
    }

    #[test]
    fn a_new_profiler_measures_nothing_until_it_is_started() {
        let profiler = Profiler::default();
        assert_eq!(profiler.level(), ProfilerLevel::Off);
        assert_eq!(profiler.level().max_probe_level(), ProbeLevel::Off);
    }

    #[test]
    fn starting_begins_at_the_cheapest_level_that_still_measures() {
        let profiler = started();
        assert_eq!(profiler.level(), ProfilerLevel::Light);
        assert_eq!(profiler.level().max_probe_level(), ProbeLevel::Counter);
    }

    #[test]
    fn durations_feed_the_histogram_and_calls_are_counted() {
        let mut profiler = started();
        let probe = profiler.register(WorkId(1)).expect("sonde");

        for _ in 0..10 {
            profiler.ingest(&record(probe, RecordKind::Exit, 1_000));
        }
        profiler.end_tick(cheap_tick());

        let entry = profiler.store().get(probe).expect("unite");
        assert_eq!(entry.dynamics.cpu_ns.count(), 10);
        assert_eq!(entry.dynamics.total_calls, 10);
        assert!(entry.dynamics.calls_per_tick.value() > 0.0);
    }

    #[test]
    fn counter_records_are_counted_without_a_duration() {
        let mut profiler = started();
        let probe = profiler.register(WorkId(1)).expect("sonde");

        for _ in 0..5 {
            profiler.ingest(&record(probe, RecordKind::Enter, 0));
        }
        profiler.end_tick(cheap_tick());

        let entry = profiler.store().get(probe).expect("unite");
        assert_eq!(entry.dynamics.total_calls, 5);
        assert_eq!(
            entry.dynamics.cpu_ns.count(),
            0,
            "le mode compteur ne lit pas l'horloge"
        );
    }

    #[test]
    fn a_record_for_an_unknown_probe_is_counted_not_guessed() {
        let mut profiler = started();
        profiler.ingest(&record(4_242, RecordKind::Exit, 1_000));

        let status = profiler.status();
        assert_eq!(status.records_unknown, 1);
        assert_eq!(status.records_ingested, 0);
    }

    /// FM-12 : une duree nulle est conservee, et signalee.
    #[test]
    fn a_zero_duration_exit_is_recorded_and_flagged() {
        let mut profiler = started();
        let probe = profiler.register(WorkId(1)).expect("sonde");
        profiler.ingest(&record(probe, RecordKind::Exit, 0));

        assert_eq!(profiler.status().zero_duration_exits, 1);
        let entry = profiler.store().get(probe).expect("unite");
        assert_eq!(entry.dynamics.cpu_ns.count(), 1);
    }

    /// T-141 : sous une charge couteuse, la profondeur suit la chaleur.
    #[test]
    fn depth_follows_heat_once_the_profiler_is_at_normal() {
        let mut profiler = with_short_baseline();
        // La profondeur ne remonte pas tant qu'une ligne de base n'a pas confirme
        // qu'il y a de la place (PARTIE 12.4) : on en fait aboutir une, sous budget.
        run_until_paused(&mut profiler, 10_000_000);
        run_until_measured(&mut profiler, 9_990_000);
        assert_eq!(profiler.level(), ProfilerLevel::Light, "aucune reduction");

        let cold = profiler.register(WorkId(1)).expect("froide");
        let hot = profiler.register(WorkId(2)).expect("chaude");

        // Assez de ticks bon marche pour que le profiler remonte a NORMAL.
        for _ in 0..(COMFORTABLE_TICKS_BEFORE_RAISE + 50) {
            profiler.ingest(&record(cold, RecordKind::Exit, 100));
            for _ in 0..10 {
                // 10 appels de 200 us : 2 ms par tick, donc CRITICAL.
                profiler.ingest(&record(hot, RecordKind::Exit, 200_000));
            }
            profiler.end_tick(cheap_tick());
        }

        assert_eq!(profiler.level(), ProfilerLevel::Normal);
        assert_eq!(
            profiler.store().get(hot).expect("chaude").dynamics.heat,
            Heat::Critical
        );
        assert_eq!(
            profiler.store().get(hot).expect("chaude").level,
            ProbeLevel::Timed,
            "la profondeur DEEP reste plafonnee par l'etat NORMAL"
        );
        assert_eq!(
            profiler.store().get(cold).expect("froide").level,
            ProbeLevel::Counter
        );
    }

    /// T-141 : un profilage trop cher fait descendre la profondeur, cran par cran.
    #[test]
    fn an_expensive_profiler_steps_itself_down_to_off() {
        let mut profiler = started();
        profiler.register(WorkId(1)).expect("sonde");

        let expensive = TickCost {
            profiling_ns: 20_000_000,
            tick_ns: 30_000_000,
            period_ns: 50_000_000,
        };

        let mut reached_off = false;
        for _ in 0..200 {
            let report = profiler.end_tick(expensive);
            if report.level == ProfilerLevel::Off {
                reached_off = true;
                break;
            }
        }

        assert!(reached_off, "un profilage a 40 % d'un cœur doit s'arreter");
        assert_eq!(
            profiler.status().level_changes,
            3,
            "OFF -> LIGHT au demarrage, puis LIGHT -> THROTTLED -> OFF"
        );
    }

    #[test]
    fn a_stopped_profiler_turns_every_probe_off() {
        let mut profiler = started();
        let probe = profiler.register(WorkId(1)).expect("sonde");
        profiler.end_tick(cheap_tick());

        profiler.stop();
        profiler.end_tick(cheap_tick());

        assert_eq!(
            profiler.store().get(probe).expect("unite").level,
            ProbeLevel::Off
        );
    }

    /// T-144 : sans aucune sonde, l'echantillonnage suffit a classer une unite.
    #[test]
    fn sampling_alone_is_enough_to_rank_a_workload() {
        let mut profiler = started();
        let probe = profiler.register(WorkId(1)).expect("sonde");
        // Toutes sondes eteintes : c'est l'etat THROTTLED.
        for _ in 0..40 {
            // 2 ms attribuees par tick par l'echantillonnage.
            profiler.ingest(&record(probe, RecordKind::Sample, 2_000_000));
            profiler.end_tick(cheap_tick());
        }

        let entry = profiler.store().get(probe).expect("unite");
        assert_eq!(entry.sampled_hits, 40);
        assert!(
            entry.cost_ns_per_tick() > 1_000_000,
            "obtenu {} ns",
            entry.cost_ns_per_tick()
        );
        assert_eq!(entry.dynamics.heat, Heat::Critical);
        assert!(profiler.status().samples_ingested == 40);
    }

    /// T-144 : un tick sans le moindre enregistrement reste coherent.
    #[test]
    fn a_tick_without_any_record_is_handled() {
        let mut profiler = started();
        for _ in 0..50 {
            profiler.end_tick(cheap_tick());
        }
        let status = profiler.status();
        assert_eq!(status.records_ingested, 0);
        assert_eq!(status.workloads_tracked, 0);
        assert_eq!(profiler.ticks(), 50);
    }

    #[test]
    fn the_level_table_is_only_handed_over_when_it_changed() {
        let mut profiler = started();
        profiler.register(WorkId(1)).expect("sonde");
        profiler.end_tick(cheap_tick());

        assert!(profiler.take_levels().is_some(), "la table a change");
        assert!(
            profiler.take_levels().is_none(),
            "retransmettre une table identique traverserait la frontiere pour rien"
        );
    }

    /// Une sonde ne s'arme jamais au-dela de ce que l'etat du profiler autorise.
    #[test]
    fn a_probe_registered_while_the_profiler_is_off_stays_off() {
        let mut profiler = Profiler::default();
        let probe = profiler.register(WorkId(1)).expect("sonde");

        assert_eq!(profiler.level(), ProfilerLevel::Off);
        assert_eq!(
            profiler.store().get(probe).expect("unite").level,
            ProbeLevel::Off,
            "armer une sonde alors que le profilage est arrete n'aurait aucun sens"
        );
        assert_eq!(profiler.take_levels().expect("table"), vec![0]);
    }

    #[test]
    fn a_probe_registered_while_started_counts_immediately() {
        let mut profiler = started();
        let probe = profiler.register(WorkId(1)).expect("sonde");
        assert_eq!(
            profiler.store().get(probe).expect("unite").level,
            ProbeLevel::Counter,
            "attendre la fin du tick perdrait les mesures du tick en cours"
        );
    }

    #[test]
    fn the_level_table_carries_one_entry_per_allocated_probe() {
        let mut profiler = started();
        profiler.register(WorkId(1)).expect("sonde");
        profiler.register(WorkId(2)).expect("sonde");
        profiler.end_tick(cheap_tick());

        let levels = profiler.take_levels().expect("table");
        assert_eq!(levels.len(), 2);
        assert!(levels.iter().all(|&l| ProbeLevel::from_code(l).is_some()));
    }

    /// R-202 : une collision exclut l'unite de toute mesure.
    #[test]
    fn a_collision_excludes_the_workload() {
        let mut profiler = started();
        let probe = profiler.register(WorkId(1)).expect("sonde");
        profiler.report_collision(WorkId(1));

        profiler.ingest(&record(probe, RecordKind::Exit, 1_000));
        assert_eq!(profiler.status().records_unknown, 1);
        assert_eq!(profiler.status().collisions, 1);
    }

    #[test]
    fn a_forge_event_record_is_ignored_rather_than_interpreted() {
        let mut profiler = started();
        let probe = profiler.register(WorkId(1)).expect("sonde");
        profiler.ingest(&record(probe, RecordKind::Event, 999));
        profiler.end_tick(cheap_tick());

        let entry = profiler.store().get(probe).expect("unite");
        assert_eq!(entry.dynamics.total_calls, 0);
        assert_eq!(entry.dynamics.cpu_ns.count(), 0);
    }

    #[test]
    fn allocations_are_aggregated_per_tick() {
        let mut profiler = started();
        let probe = profiler.register(WorkId(1)).expect("sonde");
        profiler.ingest(&record(probe, RecordKind::Alloc, 4_096));
        profiler.ingest(&record(probe, RecordKind::Alloc, 4_096));
        profiler.end_tick(cheap_tick());

        let entry = profiler.store().get(probe).expect("unite");
        assert!((entry.dynamics.alloc_bytes.value() - 8_192.0).abs() < f64::EPSILON);
        assert_eq!(entry.alloc_bytes_this_tick, 0, "remis a zero a la cloture");
    }

    /// R-321 : le plafond est tenu, quoi qu'il arrive.
    #[test]
    fn the_workload_count_never_exceeds_the_configured_ceiling() {
        let mut profiler = Profiler::new(ProfilerConfig {
            max_workloads: 8,
            ..ProfilerConfig::default()
        });
        profiler.start();

        for n in 1..=100_u64 {
            profiler.register(WorkId(n));
            profiler.end_tick(cheap_tick());
        }

        assert!(profiler.store().live_count() <= 8);
        assert!(profiler.status().evictions > 0);
    }

    // ----------------------------------------------------------------------
    // T-140 : auto-mesure par mise en pause (PARTIE 12.4)
    // ----------------------------------------------------------------------

    /// Un tick de duree choisie, dont le cout de profilage tient le budget sans le
    /// laisser confortable — ce qui evite qu'une remontee de profondeur vienne se
    /// meler aux assertions.
    fn tick_of(tick_ns: u64) -> TickCost {
        TickCost {
            profiling_ns: 100_000,
            tick_ns,
            period_ns: 50_000_000,
        }
    }

    /// Profiler demarre, cycle de mesure raccourci pour tenir dans un test.
    ///
    /// La periode normative est de six mille ticks : la reproduire ici ferait tourner
    /// le test cinq minutes de jeu simule sans rien prouver de plus. Elle reste assez
    /// longue pour que les vingt ticks de la fenetre active soient tous mesures.
    fn with_short_baseline() -> Profiler {
        let mut profiler = started();
        profiler.baseline = BaselineSampler::with_cadence(200, 20, 1);
        profiler
    }

    /// Fait tourner le profiler jusqu'a l'entree en pause.
    fn run_until_paused(profiler: &mut Profiler, tick_ns: u64) {
        for _ in 0..10_000 {
            if profiler.end_tick(tick_of(tick_ns)).baseline_paused {
                return;
            }
        }
        panic!("la pause n'est jamais arrivee");
    }

    /// Comme [`run_until_paused`], en alimentant une sonde a chaque tick.
    ///
    /// Rend la profondeur de cette sonde telle qu'elle etait au dernier tick actif :
    /// c'est a elle que la profondeur d'apres la pause doit etre comparee.
    fn run_until_paused_feeding(profiler: &mut Profiler, probe: u32, tick_ns: u64) -> ProbeLevel {
        for _ in 0..10_000 {
            profiler.ingest(&record(probe, RecordKind::Exit, 5_000_000));
            let level = profiler.store().get(probe).expect("unite").level;
            if profiler.end_tick(tick_of(tick_ns)).baseline_paused {
                return level;
            }
        }
        panic!("la pause n'est jamais arrivee");
    }

    /// Fait tourner le profiler jusqu'a la fin de la pause et rend la mesure.
    fn run_until_measured(profiler: &mut Profiler, tick_ns: u64) -> Option<BaselineMeasurement> {
        for _ in 0..10_000 {
            let report = profiler.end_tick(tick_of(tick_ns));
            if !report.baseline_paused {
                return report.baseline;
            }
        }
        panic!("la pause ne s'acheve jamais");
    }

    #[test]
    fn t140_pendant_la_pause_toutes_les_sondes_sont_eteintes() {
        let mut profiler = with_short_baseline();
        let probe = profiler.register(WorkId(1)).expect("sonde");
        let before = run_until_paused_feeding(&mut profiler, probe, 12_000_000);

        assert_ne!(before, ProbeLevel::Off, "la sonde etait bien armee");
        assert!(profiler.baseline_paused());
        assert_eq!(
            profiler.store().get(probe).expect("unite").level,
            ProbeLevel::Off,
            "une pause de mesure eteint toutes les sondes"
        );
        assert!(
            profiler.take_levels().is_some(),
            "Java doit recevoir la table eteinte, sans quoi la pause ne mesure rien"
        );
    }

    #[test]
    fn t140_apres_la_pause_les_sondes_reprennent_leur_profondeur() {
        let mut profiler = with_short_baseline();
        let probe = profiler.register(WorkId(1)).expect("sonde");
        let before = run_until_paused_feeding(&mut profiler, probe, 12_000_000);
        // Une ligne de base sous le budget : la profondeur ne doit changer que du fait
        // de la pause, jamais d'une reduction decidee par la mesure.
        run_until_measured(&mut profiler, 11_990_000);

        assert!(!profiler.baseline_paused());
        assert_eq!(
            profiler.store().get(probe).expect("unite").level,
            before,
            "la profondeur se deduit de la chaleur : elle revient d'elle-meme"
        );
    }

    #[test]
    fn t140_les_enregistrements_arrives_pendant_la_pause_sont_ecartes() {
        let mut profiler = with_short_baseline();
        let probe = profiler.register(WorkId(1)).expect("sonde");
        run_until_paused(&mut profiler, 12_000_000);

        let ingested_before = profiler.status().records_ingested;
        profiler.ingest(&record(probe, RecordKind::Exit, 9_000_000));

        let status = profiler.status();
        assert_eq!(
            status.records_ingested, ingested_before,
            "rien ne s'agrege pendant une pause de mesure"
        );
        assert_eq!(status.records_dropped_paused, 1);
    }

    /// La pause doit etre invisible pour l'etat du profiler, pas seulement pour le
    /// joueur : vingt ticks de silence artificiel refroidiraient des unites de travail
    /// qui n'ont rien cesse de faire.
    #[test]
    fn t140_la_pause_ne_refroidit_pas_les_unites_de_travail() {
        let mut profiler = with_short_baseline();
        let probe = profiler.register(WorkId(1)).expect("sonde");
        run_until_paused_feeding(&mut profiler, probe, 12_000_000);

        let entry = profiler.store().get(probe).expect("unite");
        let heat_before = entry.dynamics.heat;
        let calls_before = entry.dynamics.calls_per_tick.value();
        let seen_before = entry.dynamics.last_seen_tick;

        run_until_measured(&mut profiler, 11_990_000);

        let entry = profiler.store().get(probe).expect("unite");
        assert_eq!(entry.dynamics.heat, heat_before);
        assert_eq!(entry.dynamics.last_seen_tick, seen_before);
        assert!(
            (entry.dynamics.calls_per_tick.value() - calls_before).abs() < f64::EPSILON,
            "les moyennes glissantes n'avancent pas pendant la pause"
        );
    }

    #[test]
    fn t140_une_ligne_de_base_au_dessus_du_budget_fait_descendre_la_profondeur() {
        let mut profiler = with_short_baseline();
        run_until_paused(&mut profiler, 12_000_000);
        let level_before = profiler.level();

        // Dix millisecondes sans profilage contre douze avec : vingt pour cent, bien
        // au-dela des 1,5 % de budget de MSPT.
        let measurement = run_until_measured(&mut profiler, 10_000_000).expect("mesure aboutie");
        assert_eq!(measurement.overhead_ns, 2_000_000);

        assert_eq!(
            profiler.level(),
            level_before.reduce(),
            "une mesure au-dessus du budget fait descendre d'un cran"
        );
        assert_eq!(profiler.overhead().baseline_over_budget(), 1);
    }

    #[test]
    fn t140_une_ligne_de_base_dans_le_budget_ne_change_rien() {
        let mut profiler = with_short_baseline();
        run_until_paused(&mut profiler, 10_050_000);
        let level_before = profiler.level();

        // Cinquante microsecondes sur dix millisecondes : 0,5 %, sous le budget.
        let measurement = run_until_measured(&mut profiler, 10_000_000).expect("mesure aboutie");
        assert_eq!(measurement.overhead_ns, 50_000);

        assert_eq!(profiler.level(), level_before);
        assert_eq!(profiler.overhead().baseline_over_budget(), 0);
    }

    /// Les compteurs ne voient pas les appels injectes dans le bytecode. Monter en
    /// profondeur sur leur seule foi, c'est monter sur une preuve qu'on sait
    /// incomplete — en production, le profiler atteignait `DEEP` en vingt-cinq secondes
    /// alors que son cout reel etait de 32 %.
    #[test]
    fn t140_la_profondeur_ne_remonte_pas_avant_la_premiere_ligne_de_base() {
        let mut profiler = started();
        assert_eq!(profiler.level(), ProfilerLevel::Light);

        // Mille ticks bon marche : dix fois de quoi declencher une remontee.
        for _ in 0..(COMFORTABLE_TICKS_BEFORE_RAISE * 10) {
            profiler.end_tick(cheap_tick());
        }

        assert_eq!(
            profiler.level(),
            ProfilerLevel::Light,
            "les compteurs seuls n'autorisent pas a monter"
        );
        assert_eq!(profiler.status().baseline_measurements, 0);
    }

    #[test]
    fn t140_la_profondeur_remonte_une_fois_la_ligne_de_base_mesuree() {
        let mut profiler = with_short_baseline();
        run_until_paused(&mut profiler, 10_000_000);
        run_until_measured(&mut profiler, 9_990_000);
        assert_eq!(profiler.level(), ProfilerLevel::Light);

        for _ in 0..(COMFORTABLE_TICKS_BEFORE_RAISE + 5) {
            profiler.end_tick(cheap_tick());
        }

        assert_eq!(
            profiler.level(),
            ProfilerLevel::Normal,
            "une mesure sous le budget autorise a monter"
        );
    }

    /// Le defaut que la seconde campagne a rendu visible : une ligne de base au-dessus
    /// du budget faisait descendre la profondeur, puis les compteurs la faisaient
    /// remonter d'un cran toutes les cinq secondes. En quinze secondes, la mesure etait
    /// defaite. La permission de monter doit venir de la DERNIERE mesure.
    #[test]
    fn t140_une_ligne_de_base_au_dessus_du_budget_interdit_de_remonter() {
        let mut profiler = with_short_baseline();
        run_until_paused(&mut profiler, 12_000_000);
        run_until_measured(&mut profiler, 10_000_000).expect("mesure aboutie");
        let after_reduction = profiler.level();

        // De quoi declencher une remontee, sans atteindre la pause suivante : c'est
        // bien le refus qu'on mesure, pas l'effet d'une seconde mesure.
        for _ in 0..(COMFORTABLE_TICKS_BEFORE_RAISE + 50) {
            profiler.end_tick(tick_of(10_000_000));
        }

        assert!(
            !profiler.baseline_paused(),
            "la pause suivante n'a pas commence"
        );
        assert_eq!(
            profiler.level(),
            after_reduction,
            "les compteurs ne defont pas ce qu'une mesure vient de decider"
        );
        assert!(!profiler.overhead().baseline_allows_raise());
    }

    /// R-770 : tant qu'aucune pause n'a abouti, le statut ne pretend pas connaitre le
    /// cout. Zero mesure signifie « pas encore mesure », jamais « ne coute rien ».
    #[test]
    fn t140_le_statut_distingue_pas_encore_mesure_de_cout_nul() {
        let mut profiler = started();
        for _ in 0..50 {
            profiler.end_tick(tick_of(10_000_000));
        }
        let status = profiler.status();
        assert_eq!(status.baseline_measurements, 0);
        assert_eq!(status.baseline_overhead_ns, 0);
        assert!(
            status.baseline_ticks_until_pause > 0,
            "la prochaine mesure est annoncee, ce qui distingue l'attente de l'absence"
        );

        let mut measured = with_short_baseline();
        run_until_paused(&mut measured, 12_000_000);
        run_until_measured(&mut measured, 10_000_000);
        let status = measured.status();
        assert_eq!(status.baseline_measurements, 1);
        assert_eq!(status.baseline_overhead_ns, 2_000_000);
        assert_eq!(status.baseline_overhead_pct_x100, 2_000);
        assert!(status.baseline_tick > 0);
    }
}
