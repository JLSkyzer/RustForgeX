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
//! # Le profiler se mesure lui-meme
//!
//! Un profiler qui ignore son propre cout attribue aux autres un temps qu'il consomme.
//! Chaque tick, le runtime lui remet le temps qu'il a passe a profiler ; au-dela du
//! budget, la profondeur descend d'un cran, jusqu'a l'arret complet. Voir
//! [`overhead`].
//!
//! # Sens des dependances
//!
//! `rfx-profiler` depend de `rfx-model` et `rfx-memory`, et de rien d'autre (INV-13).
//! Il ne connait pas `rfx-core` : c'est `rfx-core` qui l'appelle, journalise `E-1201`
//! quand la profondeur descend, et transmet a Java la table des niveaux.

#![doc(html_root_url = "https://example.invalid/rustforgex")]

pub mod level;
pub mod overhead;
pub mod store;

use rfx_memory::probe_buffer::{ProbeRecord, RecordKind};
use rfx_model::{Heat, ProfilerStatus};

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
}

impl Default for ProfilerConfig {
    fn default() -> Self {
        Self {
            max_workloads: DEFAULT_MAX_WORKLOADS,
            cpu_budget_pct: DEFAULT_CPU_BUDGET_PCT,
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
}

/// C-05 : agregation des mesures et adaptation de la profondeur.
#[derive(Debug)]
pub struct Profiler {
    config: ProfilerConfig,
    store: WorkloadStore,
    level: ProfilerLevel,
    overhead: OverheadMeter,
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
        let wanted = ProbeLevel::for_heat(Heat::Cold).min(self.level.max_probe_level());
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

        self.consolidate(tick);

        let verdict = self.overhead.record_tick(cost);
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

    /// Aligne la profondeur de chaque sonde sur la chaleur, sous le plafond global.
    fn adapt_probe_levels(&mut self) {
        let cap = self.level.max_probe_level();
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

    /// Auto-mesure du cout du profilage.
    #[must_use]
    pub fn overhead(&self) -> &OverheadMeter {
        &self.overhead
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
        let mut profiler = started();
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
}
