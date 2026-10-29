//! C-27 : etat global du runtime natif et confinement des pannes.
//!
//! Cahier des charges : PARTIE 5.27. Exigences : R-520 (instance unique), R-521
//! (handle opaque valide), R-523 (comptage des panics), halt d'urgence.
//! Maturite : `STABLE`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use rfx_memory::probe_buffer::DEFAULT_BUFFER_BYTES;
use rfx_memory::{MemoryBudget, Pool, ProbeBufferPool};
use rfx_model::{
    AnalysisStatus, ComponentStatus, DbOpen, HardwareClass, Maturity, ProbeCoverage, ProbeStatus,
    RuntimeConfig, RuntimeStatus, SchedulerStatus, Side, TickStatus, WorkDescriptor,
};
use rfx_profiler::{Profiler, ProfilerConfig, TickCost};
use rfx_scheduler::{Scheduler, SchedulerConfig};

use crate::analysis::{Analysis, Descriptor, HotWorkload, StagedRoot, ROOTS_PER_REFILL};
use crate::background::SchedulerExecutor;
use crate::db::DbLink;
use crate::error::ErrorCode;
use crate::state::RuntimeState;
use crate::tick::{TickPhase, TickWindow};

/// Version de l'ABI implementee par ce binaire (IF-01, `RFX_ABI_VERSION`).
pub const ABI_VERSION: u32 = 1;

/// Fenetre glissante d'observation des panics pour le halt d'urgence.
const PANIC_WINDOW: Duration = Duration::from_secs(60);

/// Nombre de panics dans [`PANIC_WINDOW`] au-dela duquel le runtime s'arrete.
const PANICS_BEFORE_HALT: usize = 10;

/// Plafond de threads dotes d'un tampon de profilage.
///
/// Un modpack peut creer beaucoup de threads ; suivre chacun d'eux ferait croitre la
/// memoire native sans borne, ce qu'interdit R-533. Au-dela, les threads
/// supplementaires ne sont pas sondes et le refus est compte.
const MAX_PROBED_THREADS: usize = 64;

/// Etat global du runtime natif.
///
/// La base des unites de travail (`store` en PARTIE 5.27) est le champ `db`. Les champs
/// `scheduler`, `decision`, `telemetry` et `caches` apparaitront au jalon qui implemente
/// les composants correspondants.
/// Ils ne sont pas declares ici tant qu'ils n'existent pas : un champ vide serait une
/// fiction au sens du contrat agent 3.1.
#[derive(Debug)]
pub struct Runtime {
    config: RuntimeConfig,
    hardware: HardwareClass,
    coverage: ProbeCoverage,
    state: RuntimeState,
    panics: u64,
    recent_panics: Vec<Instant>,
    tick_window: TickWindow,
    budget: MemoryBudget,
    probe_buffers: ProbeBufferPool,
    probe_records_consumed: u64,
    profiler: Profiler,
    /// Instant d'ouverture du tick precedent, pour mesurer la periode reelle.
    ///
    /// La periode n'est pas la duree du tick : entre deux ticks, le serveur dort. Le
    /// budget « part d'un cœur » du profiler se calcule sur la periode, celui de MSPT
    /// sur la duree utile (C-05).
    last_tick_begin: Option<Instant>,
    /// Duree ecoulee entre les deux dernieres ouvertures de tick, en nanosecondes.
    last_period_ns: u64,
    /// Temps passe dans le profilage pendant le tick courant, en nanosecondes.
    profiling_ns_this_tick: u64,
    /// Depassements du budget de profilage constates (`E-1201`).
    profiler_over_budget: u64,
    /// Table des niveaux prete a partir vers Java, en attente d'etre reclamee.
    ///
    /// Le protocole FFI se fait en deux temps — demander la taille, puis le contenu —
    /// et le profiler ne signale un changement qu'une fois. Sans cette file d'attente,
    /// la premiere etape consommerait le changement et la seconde ne rendrait rien.
    pending_levels: Option<Vec<u8>>,
    /// Charge externe relevee cote Java (R-421, ADR-046).
    scheduler: SchedulerStatus,
    /// Ordonnanceur des travaux internes (C-17, ADR-061) ; absent s'il n'a pas pu
    /// demarrer : la base tourne alors sur un fil dedie.
    pool: Option<Arc<Scheduler>>,
    /// Base des unites de travail (C-09, ADR-049).
    db: DbLink,
    /// Analyse statique (C-07, ADR-051), sous son propre verrou : un graphe se
    /// construit sans tenir celui que partage le tick.
    analysis: Arc<Mutex<Analysis>>,
    /// Methodes sondees depuis le dernier pas d'analyse, a lui confier.
    analysis_staging: Vec<StagedRoot>,
    /// Dernier etat publie par l'analyse.
    analysis_status: AnalysisStatus,
    /// Octets de l'analyse comptes dans le budget natif (`Pool::Runtime`).
    analysis_reserved: u64,
}

impl Runtime {
    /// Construit le runtime a partir d'une configuration deja validee.
    #[must_use]
    pub fn new(config: RuntimeConfig) -> Self {
        let max_native_mb = config.max_native_mb;
        // Le profiler etait construit sur ses defauts, ce qui rendait muettes deux
        // options que l'utilisateur croyait pouvoir regler. Elles sont desormais lues.
        let profiler_config = ProfilerConfig {
            max_workloads: config.profiler_max_workloads as usize,
            cpu_budget_pct: config.profiler_cpu_budget_pct as f32,
            baseline_period_ticks: u64::from(config.profiler_baseline_period_ticks),
            baseline_pause_ticks: config.profiler_baseline_pause_ticks,
            baseline_cycles: config.profiler_baseline_cycles,
        };
        let mut db = DbLink::new(&config);
        let mut budget = MemoryBudget::new(max_native_mb);
        let logical_cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        // ADR-062 : les arenes des workers sont taillees pour le budget, et leur pire cas
        // y est reserve avant le demarrage ; refusee, la base garde son fil de secours.
        let (pool_config, arena_bytes) = pool_config(budget.limit(), logical_cores);
        let pool = budget
            .reserve(Pool::Runtime, arena_bytes)
            .ok()
            .and_then(|()| Scheduler::start(pool_config).ok())
            .map(Arc::new);
        if pool.is_none() {
            budget.release(Pool::Runtime, arena_bytes);
        }
        if let Some(pool) = &pool {
            db.use_executor(Arc::new(SchedulerExecutor::new(pool)));
        }
        let analysis = Arc::new(Mutex::new(Analysis::new(&config)));
        let analysis_status = AnalysisStatus {
            enabled: config.analysis_enabled,
            ..AnalysisStatus::default()
        };
        Self {
            config,
            hardware: HardwareClass::default(),
            coverage: ProbeCoverage::default(),
            state: RuntimeState::Running,
            panics: 0,
            recent_panics: Vec::new(),
            tick_window: TickWindow::default(),
            budget,
            probe_buffers: ProbeBufferPool::new(DEFAULT_BUFFER_BYTES, MAX_PROBED_THREADS),
            probe_records_consumed: 0,
            profiler: Profiler::new(profiler_config),
            last_tick_begin: None,
            last_period_ns: 0,
            profiling_ns_this_tick: 0,
            profiler_over_budget: 0,
            pending_levels: None,
            scheduler: SchedulerStatus::default(),
            pool,
            db,
            analysis,
            analysis_staging: Vec::new(),
            analysis_status,
            analysis_reserved: 0,
        }
    }

    /// Acquiert le tampon de profilage d'un thread (IF-03).
    ///
    /// Le tampon est alloue au premier appel et reste la propriete du natif (R-708).
    /// Renvoie son adresse et sa capacite, ou `None` si le budget memoire ou le
    /// plafond de threads s'y oppose — auquel cas ce thread ne sera simplement pas
    /// sonde, ce qui degrade la mesure sans jamais gener le jeu.
    pub fn acquire_probe_buffer(&mut self, thread_id: i32) -> Option<(*mut u8, usize)> {
        let already_allocated = self.probe_buffers.get_mut(thread_id).is_some();
        if !already_allocated {
            if self
                .budget
                .reserve(Pool::Probes, DEFAULT_BUFFER_BYTES as u64)
                .is_err()
            {
                return None;
            }
            if self.probe_buffers.acquire(thread_id).is_none() {
                // Le plafond de threads a refuse : la reservation faite juste avant
                // n'a plus d'objet.
                self.budget
                    .release(Pool::Probes, DEFAULT_BUFFER_BYTES as u64);
                return None;
            }
        }
        let buffer = self.probe_buffers.get_mut(thread_id)?;
        Some((buffer.address(), buffer.capacity()))
    }

    /// Consomme les enregistrements ecrits par un thread (IF-03) et les agrege (C-05).
    ///
    /// Renvoie le nombre d'enregistrements lus. Le temps passe ici compte comme du
    /// temps de profilage : c'est precisement ce que l'auto-mesure de C-05 doit voir.
    pub fn flush_probe_buffer(&mut self, thread_id: i32, used: usize) -> usize {
        let started = Instant::now();
        // Emprunts disjoints de deux champs : le tampon est lu pendant que le profiler
        // agrege, sans collection intermediaire (R-709).
        let profiler = &mut self.profiler;
        let Some(buffer) = self.probe_buffers.get_mut(thread_id) else {
            return 0;
        };
        let count = buffer.flush_with(used, |record| profiler.ingest(&record));
        self.probe_records_consumed = self.probe_records_consumed.saturating_add(count as u64);
        self.profiling_ns_this_tick = self
            .profiling_ns_this_tick
            .saturating_add(duration_ns(started.elapsed()));
        count
    }

    /// Enregistre une unite de travail et rend son identifiant de sonde (C-05).
    ///
    /// Renvoie `None` si le plafond d'unites suivies est atteint sans qu'aucune unite
    /// froide ne soit evincable : la methode n'est alors pas sondee. Une unite sondee
    /// est aussi notee pour la base (C-09) : son descripteur partira au prochain lot.
    pub fn register_workload(
        &mut self,
        descriptor: &WorkDescriptor,
        work_id: rfx_model::WorkId,
    ) -> Option<u32> {
        let probe_id = self.profiler.register(work_id);
        if probe_id.is_some() {
            self.db.note(descriptor);
            if self.analysis_status.enabled
                && self.analysis_staging.len() < self.config.profiler_max_workloads as usize
            {
                self.analysis_staging.push((
                    work_id,
                    rfx_analyzer::MethodRef::new(
                        &descriptor.class_internal_name,
                        &descriptor.method_name,
                        &descriptor.method_descriptor,
                    ),
                    descriptor.side,
                ));
            }
        }
        probe_id
    }

    /// Analyse statique, a piloter hors du verrou du registre (ADR-051), et les methodes
    /// sondees depuis le dernier pas, a lui confier.
    pub fn analysis_handoff(&mut self) -> (Arc<Mutex<Analysis>>, Vec<StagedRoot>) {
        (
            Arc::clone(&self.analysis),
            std::mem::take(&mut self.analysis_staging),
        )
    }

    /// Unites les plus couteuses deja mesurees : racines des prochains graphes, avec
    /// leur chaleur et la qualite de leur observation (C-08).
    #[must_use]
    pub fn hot_workloads(&self) -> Vec<HotWorkload> {
        self.profiler
            .store()
            .hottest(ROOTS_PER_REFILL)
            .into_iter()
            .filter(|entry| entry.cost_ns_per_tick() > 0)
            .map(|entry| HotWorkload {
                work_id: entry.work_id,
                heat: rfx_model::Heat::classify(entry.cost_ns_per_tick()),
                observation_quality: entry.dynamics.observation_quality,
            })
            .collect()
    }

    /// Publie l'etat de l'analyse et compte sa memoire dans le budget natif (R-533).
    /// Rend `false` si le budget la refuse : l'analyse doit alors s'arreter.
    pub fn publish_analysis(&mut self, status: AnalysisStatus, bytes: u64) -> bool {
        self.analysis_status = status;
        self.budget.release(Pool::Runtime, self.analysis_reserved);
        self.analysis_reserved = 0;
        if bytes == 0 {
            return true;
        }
        if self.budget.reserve(Pool::Runtime, bytes).is_err() {
            return false;
        }
        self.analysis_reserved = bytes;
        true
    }

    /// Ouvre la base des unites de travail (C-09). Le fichier est relu en arriere-plan :
    /// l'appel rend la main aussitot.
    ///
    /// # Erreurs
    ///
    /// [`ErrorCode::InvalidArgument`] si le blob porte un schema inconnu, si le
    /// repertoire est vide ou si la base est deja ouverte.
    pub fn open_db(&mut self, open: &DbOpen) -> Result<(), ErrorCode> {
        if open.schema != rfx_model::MODEL_SCHEMA_VERSION {
            return Err(ErrorCode::InvalidArgument);
        }
        let environment = crate::db::environment(open, &self.config, &self.hardware);
        let max_bytes = self.cache_cap_bytes();
        self.db
            .open(open, environment, max_bytes)
            .map_err(|_| ErrorCode::InvalidArgument)
    }

    /// Avance d'une tranche la collecte des dynamiques pour la base. Renvoie `true` quand
    /// le cycle est termine et son lot confie ; `false` s'il faut rappeler, apres avoir
    /// rendu le verrou.
    pub fn db_sync_step(&mut self) -> bool {
        let max_bytes = self.cache_cap_bytes();
        self.db
            .sync_step(self.profiler.store(), self.profiler.ticks(), max_bytes)
    }

    /// Statut de l'ordonnanceur : releves de charge et compteurs du pool.
    fn scheduler_status(&self) -> SchedulerStatus {
        let mut status = self.scheduler;
        if let Some(pool) = &self.pool {
            let stats = pool.stats();
            status.running = true;
            status.workers = u32::try_from(pool.workers()).unwrap_or(u32::MAX);
            status.workers_active = u32::try_from(stats.active_workers).unwrap_or(u32::MAX);
            status.queue_depth = pool.queue_depth() as u64;
            status.executed = stats.executed;
            status.steals = stats.steals;
            status.rejected = stats.rejected_submissions;
            status.panics = stats.panics;
            status.deadline_misses = stats.deadline_misses;
            status.serial = stats.serial_fallback;
        }
        status
    }

    /// Abandonne l'ordonnanceur sans rejoindre ses workers : quand la base n'a pas pu
    /// finir d'ecrire, un worker est peut-etre bloque sur le disque, et l'arret du jeu
    /// ne doit pas l'attendre (comme le fil `RFX-IO` laisse a lui-meme, ADR-061).
    fn abandon_pool(&mut self) {
        if let Some(pool) = self.pool.take() {
            std::mem::forget(pool);
        }
    }

    /// Ferme la base : dernier lot complet, puis attente de son ecriture, au plus
    /// `timeout`. A appeler hors du verrou du registre.
    pub fn close_db(&mut self, timeout: Duration) -> bool {
        let max_bytes = self.cache_cap_bytes();
        self.db.close(
            self.profiler.store(),
            self.profiler.ticks(),
            max_bytes,
            timeout,
        )
    }

    /// Plafond d'octets de la base : `cache.max_memory_mb`, borne par ce que le budget
    /// natif laisse libre (R-533). La memoire de la base y est comptee (`Pool::Cache`).
    fn cache_cap_bytes(&mut self) -> usize {
        const MIB: u64 = 1024 * 1024;
        let used = self.db.estimated_bytes();
        self.budget.release_all(Pool::Cache);
        let headroom = self.budget.limit().saturating_sub(self.budget.in_use());
        let cap = (u64::from(self.config.cache_max_memory_mb) * MIB).min(headroom);
        // Comptage seulement : la base elle-meme se ramene sous `cap` au lot suivant.
        let _ = self.budget.reserve(Pool::Cache, used.min(cap));
        usize::try_from(cap).unwrap_or(usize::MAX)
    }

    /// Signale une collision de `WorkId` (R-202, `E-2101`).
    pub fn report_workload_collision(&mut self, work_id: rfx_model::WorkId) {
        self.profiler.report_collision(work_id);
    }

    /// Table des niveaux de sonde en attente, si elle a change (ADR-016).
    ///
    /// Appelable plusieurs fois : tant que la table n'a pas ete reclamee par
    /// [`Self::clear_pending_probe_levels`], elle reste disponible.
    pub fn pending_probe_levels(&mut self) -> Option<&[u8]> {
        if self.pending_levels.is_none() {
            self.pending_levels = self.profiler.take_levels();
        }
        // Une table vide n'a rien a transmettre, et la GARDER en attente serait fatal :
        // la table n'est reprise que lorsque l'attente est vide, et un appelant qui
        // recoit une taille nulle n'a aucune raison d'accuser reception. Le cache
        // resterait donc `Some(vide)` a jamais, et plus aucune table ne serait prise.
        //
        // C'est arrive : interroger la taille avant qu'une seule unite ne soit
        // enregistree suffisait a eteindre le sondage pour toute la partie, sans le
        // moindre signe — le mod annoncait 2 647 methodes sondees et zero sonde armee.
        if self.pending_levels.as_ref().is_some_and(Vec::is_empty) {
            self.pending_levels = None;
            return None;
        }
        self.pending_levels.as_deref()
    }

    /// Declare la table des niveaux transmise a Java.
    pub fn clear_pending_probe_levels(&mut self) {
        self.pending_levels = None;
    }

    /// Profiler (C-05), pour inspection.
    #[must_use]
    pub fn profiler(&self) -> &Profiler {
        &self.profiler
    }

    /// Demarre le profilage (`OFF -> LIGHT`).
    pub fn start_profiler(&mut self) {
        self.profiler.start();
    }

    /// Depassements du budget de profilage constates depuis le demarrage (`E-1201`).
    #[must_use]
    pub fn profiler_over_budget(&self) -> u64 {
        self.profiler_over_budget
    }

    /// Enregistrements de sonde consommes depuis le demarrage.
    #[must_use]
    pub fn probe_records_consumed(&self) -> u64 {
        self.probe_records_consumed
    }

    /// Enregistrements perdus par saturation des tampons (R-709).
    #[must_use]
    pub fn probe_records_lost(&self) -> u64 {
        self.probe_buffers.records_lost()
    }

    /// Budget memoire natif, pour les diagnostics (R-534).
    #[must_use]
    pub fn budget(&self) -> &MemoryBudget {
        &self.budget
    }

    /// Ouvre la fenetre de tick (IF-02, `rfx_tick_begin`).
    pub fn tick_begin(&mut self, tick: u64, side: Side, now: Instant) {
        // R-423 : aucune tache BACKGROUND ne demarre pendant le tick.
        if let Some(pool) = &self.pool {
            pool.set_critical_window(true);
        }
        self.tick_window.begin(tick, side, now);
        self.last_period_ns = self
            .last_tick_begin
            .map_or(0, |previous| duration_ns(now.duration_since(previous)));
        self.last_tick_begin = Some(now);
        self.profiling_ns_this_tick = 0;
    }

    /// Declare une transition de phase (IF-02, `rfx_phase`).
    ///
    /// Renvoie `false` si la transition ne suit pas SM-04 : elle est alors comptee et
    /// ignoree, jamais appliquee.
    pub fn tick_phase(&mut self, phase: TickPhase) -> bool {
        self.tick_window.phase(phase)
    }

    /// Ferme la fenetre de tick (IF-02, `rfx_tick_end`).
    ///
    /// Renvoie `false` si aucun tick n'etait ouvert.
    pub fn tick_end(&mut self, now: Instant) -> bool {
        if let Some(pool) = &self.pool {
            pool.set_critical_window(false);
        }
        if !self.tick_window.end(now) {
            // Aucun tick n'etait ouvert : il n'y a pas de cout de tick a attribuer, et
            // cloturer une fenetre de profilage inexistante fausserait la mesure.
            return false;
        }
        let report = self.profiler.end_tick(TickCost {
            profiling_ns: self.profiling_ns_this_tick,
            tick_ns: self.tick_window.metrics().last_window_ns,
            period_ns: self.last_period_ns,
        });
        if report.over_budget {
            self.profiler_over_budget = self.profiler_over_budget.saturating_add(1);
        }
        true
    }

    /// Comptabilise la duree d'un appel `rfx_tick_*` (R-707).
    ///
    /// Renvoie `false` si la deadline est depassee. Le depassement est compte, jamais
    /// transforme en erreur : c'est le signal qui fera reduire l'activite du runtime.
    pub fn record_hook(&mut self, kind: crate::HookKind, duration: std::time::Duration) -> bool {
        self.tick_window.record_hook(kind, duration)
    }

    /// Fenetre de tick courante, pour inspection.
    #[must_use]
    pub fn tick_window(&self) -> &TickWindow {
        &self.tick_window
    }

    /// Configuration effective de cette execution.
    #[must_use]
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Etat courant.
    #[must_use]
    pub fn state(&self) -> RuntimeState {
        self.state
    }

    /// Classe materielle mesuree par C-45, et les champs reellement mesures.
    #[must_use]
    pub fn hardware(&self) -> (HardwareClass, ProbeCoverage) {
        (self.hardware, self.coverage)
    }

    /// Enregistre le resultat de la sonde materielle (C-45).
    pub fn set_hardware(&mut self, hardware: HardwareClass, coverage: ProbeCoverage) {
        self.hardware = hardware;
        self.coverage = coverage;
    }

    /// Enregistre les couts de franchissement de frontiere mesures **depuis Java**.
    ///
    /// Ces deux couts ne peuvent pas etre mesures depuis le natif : ils incluent le
    /// trajet aller-retour complet depuis la JVM. C'est donc l'appelant Java qui les
    /// mesure (C-45) et les publie ici, ou ils alimenteront le modele de cout de
    /// C-15 (R-660).
    pub fn set_ffi_costs(&mut self, jni_call_ns: u32, ffi_batch_ns_per_kb: u32) {
        self.hardware.jni_call_ns = jni_call_ns;
        self.hardware.ffi_batch_ns_per_kb = ffi_batch_ns_per_kb;
        self.coverage.ffi_call = jni_call_ns > 0;
        self.coverage.ffi_transfer = ffi_batch_ns_per_kb > 0;
    }

    /// Releve de la charge externe : fils tiers qui ont consomme au moins 10 % d'un coeur
    /// depuis le releve precedent, comptes cote Java (R-421, ADR-046).
    ///
    /// Conserve, publie dans le statut, et transmis a l'ordonnanceur
    /// (`Scheduler::report_external_load`, ADR-045, ADR-061).
    pub fn report_external_load(&mut self, runnable_threads: u32) {
        if let Some(pool) = &self.pool {
            pool.report_external_load(runnable_threads as usize);
        }
        self.scheduler.external_runnable = runnable_threads;
        self.scheduler.external_runnable_max =
            self.scheduler.external_runnable_max.max(runnable_threads);
        self.scheduler.external_load_reports += 1;
    }

    /// Fait passer le runtime en mode degrade. Sans effet si l'etat est terminal.
    pub fn degrade(&mut self) {
        if !self.state.is_terminal() {
            self.state = RuntimeState::Degraded;
        }
    }

    /// Declenche le halt d'urgence : le jeu continue en Java pur (PARTIE 5.27).
    pub fn emergency_halt(&mut self) {
        self.state = RuntimeState::Halted;
    }

    /// Comptabilise une panic capturee a la frontiere FFI (R-523).
    ///
    /// Renvoie l'etat du runtime apres traitement. Au-dela de
    /// [`PANICS_BEFORE_HALT`] panics dans une fenetre de 60 secondes, le runtime
    /// passe en `HALTED`.
    pub fn record_panic(&mut self, now: Instant) -> RuntimeState {
        self.panics = self.panics.saturating_add(1);
        self.recent_panics
            .retain(|t| now.duration_since(*t) < PANIC_WINDOW);
        self.recent_panics.push(now);

        if self.recent_panics.len() >= PANICS_BEFORE_HALT {
            self.emergency_halt();
        }
        self.state
    }

    /// Nombre total de panics capturees depuis le demarrage.
    #[must_use]
    pub fn panics(&self) -> u64 {
        self.panics
    }

    /// Ouvre une session de diagnostic a la demande de l'operateur (C-35, C-38).
    ///
    /// @param level_code code de profondeur, tel que `ProbeLevel::code`
    /// @param ticks duree de la session, en ticks
    /// @return `true` si le code de profondeur est reconnu
    pub fn request_probe_depth(&mut self, level_code: u8, ticks: u64) -> bool {
        match rfx_profiler::level::ProbeLevel::from_code(level_code) {
            Some(level) => {
                self.profiler.request_depth(level, ticks);
                true
            }
            None => false,
        }
    }

    /// Classement des unites de travail les plus couteuses (C-35, `/rfx top`).
    ///
    /// Lecture pure : ne modifie rien, ne remet aucun compteur a zero. Deux appels
    /// consecutifs rendent la meme chose si aucun tick ne s'est ecoule entre eux,
    /// faute de quoi une commande de diagnostic changerait ce qu'elle observe.
    ///
    /// `limit` est borne par l'appelant ; zero rend un classement vide mais renseigne
    /// tout de meme `tracked` et `measured`, ce qui suffit a savoir s'il y avait
    /// quelque chose a voir.
    #[must_use]
    pub fn top_workloads(&self, limit: usize) -> rfx_model::TopWorkloads {
        self.profiler.top(limit)
    }

    /// Fiche d'une unite (`/rfx why`, `/rfx workload`, ADR-063) : sa mesure et ce que
    /// l'analyse en sait. L'analyse n'est jamais attendue : si elle construit un graphe,
    /// la fiche le dit, et la commande, qui tourne sur le fil du serveur, ne bloque pas.
    #[must_use]
    pub fn explain(&self, work_id: rfx_model::WorkId) -> rfx_model::WorkloadCard {
        let (analysis, analysis_busy) = match self.analysis.try_lock() {
            Ok(analysis) => (analysis.descriptor(work_id).map(Descriptor::card), false),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => (
                poisoned
                    .into_inner()
                    .descriptor(work_id)
                    .map(Descriptor::card),
                false,
            ),
            Err(std::sync::TryLockError::WouldBlock) => (None, true),
        };
        rfx_model::WorkloadCard {
            schema: rfx_model::WorkloadCard::SCHEMA,
            work_id_hex: format!("{:016x}", work_id.0),
            tick_ns: self.profiler.last_tick_ns(),
            measured: self.profiler.store().entry_of(work_id),
            analysis,
            analysis_busy,
        }
    }

    /// Construit le blob de statut publie vers Java (`/rfx status`).
    #[must_use]
    pub fn status(&self) -> RuntimeStatus {
        RuntimeStatus {
            schema: rfx_model::MODEL_SCHEMA_VERSION,
            abi_version: ABI_VERSION,
            native_version: env!("CARGO_PKG_VERSION").to_owned(),
            state: self.state.label().to_owned(),
            panics: self.panics,
            hardware: self.hardware,
            probe_coverage: self.coverage,
            components: vec![
                ComponentStatus {
                    id: "C-27".to_owned(),
                    name: "Rust Runtime Core".to_owned(),
                    maturity: Maturity::Stable,
                    active: self.state.accepts_work(),
                },
                ComponentStatus {
                    id: "C-45".to_owned(),
                    name: "Hardware Probe".to_owned(),
                    maturity: Maturity::Stable,
                    active: self.coverage.cores,
                },
                ComponentStatus {
                    id: "C-07".to_owned(),
                    name: "Call Graph".to_owned(),
                    maturity: Maturity::Stable,
                    active: self.analysis_status.enabled
                        && self.analysis_status.hierarchy_classes > 0,
                },
                ComponentStatus {
                    id: "C-08".to_owned(),
                    name: "Workload Profiler".to_owned(),
                    maturity: Maturity::Stable,
                    active: self.analysis_status.enabled
                        && self.analysis_status.hierarchy_classes > 0,
                },
                ComponentStatus {
                    id: "C-09".to_owned(),
                    name: "Workload Database".to_owned(),
                    maturity: Maturity::Stable,
                    active: self.db.is_open(),
                },
            ],
            probes: ProbeStatus {
                records_consumed: self.probe_records_consumed,
                records_lost: self.probe_buffers.records_lost(),
                native_bytes: self.budget.in_use(),
                native_limit_bytes: self.budget.limit(),
            },
            profiler: self.profiler.status(),
            scheduler: self.scheduler_status(),
            db: self.db.status(),
            analysis: self.analysis_status,
            tick: {
                let m = self.tick_window.metrics();
                TickStatus {
                    ticks: m.ticks,
                    unbalanced: m.unbalanced,
                    invalid_transitions: m.invalid_transitions,
                    hook_budget_exceeded: m.hook_budget_exceeded,
                    hook_exceeded_begin: m.hook_exceeded_begin,
                    hook_exceeded_phase: m.hook_exceeded_phase,
                    hook_exceeded_end: m.hook_exceeded_end,
                    worst_hook_ns: m.worst_hook_ns,
                    worst_hook: m
                        .worst_hook
                        .map_or_else(String::new, |k| k.name().to_owned()),
                    last_window_ns: m.last_window_ns,
                }
            },
        }
    }
}

/// Duree en nanosecondes, saturee a `u64::MAX`.
fn duration_ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

// ---------------------------------------------------------------------------
// Instance unique du processus (R-520) et handles opaques (R-521)
// ---------------------------------------------------------------------------

/// Etat partage : l'instance unique et la generation du handle courant.
struct Registry {
    runtime: Option<Runtime>,
    generation: u64,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        Mutex::new(Registry {
            runtime: None,
            generation: 0,
        })
    })
}

/// Verrouille le registre en ignorant un eventuel empoisonnement.
///
/// Un `Mutex` empoisonne signifie qu'une panic est survenue alors que le verrou etait
/// tenu. Cette panic a deja ete capturee et comptabilisee a la frontiere FFI (R-523) ;
/// refuser le verrou ensuite rendrait le runtime definitivement inutilisable et
/// empecherait meme son arret propre. On reprend donc la main sur l'etat interieur.
fn lock_registry() -> MutexGuard<'static, Registry> {
    registry().lock().unwrap_or_else(|e| e.into_inner())
}

/// Compteur de generations, garantissant qu'un handle libere n'est jamais revalide.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Motif de poids fort d'un handle, pour qu'un entier arbitraire (0, 1, -1) fourni par
/// erreur ne puisse pas passer pour un handle valide.
const HANDLE_PATTERN: u64 = 0x5246_5800_0000_0000;

/// Initialise l'instance unique du runtime et renvoie son handle opaque.
///
/// # Erreurs
///
/// Renvoie [`ErrorCode::DoubleInit`] (`E-1004`) si une instance existe deja (R-520).
pub fn initialize(config: RuntimeConfig) -> Result<u64, ErrorCode> {
    let mut reg = lock_registry();
    if reg.runtime.is_some() {
        return Err(ErrorCode::DoubleInit);
    }
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    reg.runtime = Some(Runtime::new(config));
    reg.generation = generation;
    Ok(HANDLE_PATTERN | generation)
}

/// Part du budget natif accordee aux arenes des workers (ADR-062).
pub const ARENA_BUDGET_SHARE: u64 = 4;

/// Plafond d'une arene au plus bas, quel que soit le budget (ADR-062).
pub const MIN_ARENA_BYTES: usize = 256 << 10;

/// Reglages de l'ordonnanceur pour `logical_cores` coeurs et un budget natif de
/// `budget_bytes` octets, et le pire cas borne de ses arenes, a reserver (ADR-062) :
/// `workers x (max_nesting + 1)` arenes, chacune plafonnee pour que le tout tienne dans
/// `budget / ARENA_BUDGET_SHARE`, entre [`MIN_ARENA_BYTES`] et le defaut du cahier.
#[must_use]
pub fn pool_config(budget_bytes: u64, logical_cores: usize) -> (SchedulerConfig, u64) {
    let mut config = SchedulerConfig::for_machine(logical_cores);
    let arenas = config.effective_workers() as u64 * (u64::from(config.max_nesting) + 1);
    let share = budget_bytes / ARENA_BUDGET_SHARE / arenas.max(1);
    let max = usize::try_from(share)
        .unwrap_or(usize::MAX)
        .clamp(MIN_ARENA_BYTES, rfx_memory::DEFAULT_ARENA_MAX_BYTES);
    config.arena_max_bytes = max;
    config.arena_initial_bytes = rfx_memory::DEFAULT_ARENA_INITIAL_BYTES.min(max);
    (config, arenas * max as u64)
}

/// Execute `f` sur l'instance, apres validation du handle (R-521).
///
/// # Erreurs
///
/// Renvoie [`ErrorCode::InvalidArgument`] si le handle ne correspond pas a
/// l'instance courante : handle d'une generation liberee, entier arbitraire, ou
/// runtime non initialise.
pub fn with<R>(handle: u64, f: impl FnOnce(&mut Runtime) -> R) -> Result<R, ErrorCode> {
    let mut reg = lock_registry();
    if handle != (HANDLE_PATTERN | reg.generation) || reg.generation == 0 {
        return Err(ErrorCode::InvalidArgument);
    }
    let rt = reg.runtime.as_mut().ok_or(ErrorCode::InvalidArgument)?;
    Ok(f(rt))
}

/// Detruit l'instance. Le handle devient definitivement invalide.
///
/// L'instance est retiree du registre, puis fermee **apres** que le verrou est rendu :
/// la fermeture attend l'ecriture finale de la base (au plus
/// [`CLOSE_TIMEOUT`](crate::db::CLOSE_TIMEOUT)), et rien ne doit attendre le disque
/// en tenant le verrou.
///
/// # Erreurs
///
/// Renvoie [`ErrorCode::InvalidArgument`] si le handle n'est pas celui de
/// l'instance courante.
pub fn shutdown(handle: u64) -> Result<(), ErrorCode> {
    let mut reg = lock_registry();
    if handle != (HANDLE_PATTERN | reg.generation) || reg.generation == 0 {
        return Err(ErrorCode::InvalidArgument);
    }
    let runtime = reg.runtime.take();
    reg.generation = 0;
    drop(reg);
    if let Some(mut runtime) = runtime {
        // La base d'abord, l'ordonnanceur ensuite : ses workers executent encore la
        // derniere ecriture (ADR-061).
        if !runtime.close_db(crate::db::CLOSE_TIMEOUT) {
            runtime.abandon_pool();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les tests partagent l'instance unique du processus : ils sont serialises.
    fn test_lock() -> MutexGuard<'static, ()> {
        static V: Mutex<()> = Mutex::new(());
        V.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// ADR-062 : les arenes tiennent dans un quart du budget, entre le plancher et le
    /// defaut du cahier ; le pire cas rendu est celui qui sera reserve.
    #[test]
    fn worker_arenas_are_sized_for_the_native_budget() {
        const MIB: u64 = 1 << 20;
        for cores in [1, 4, 14, 64] {
            let (config, reserved) = pool_config(512 * MIB, cores);
            let arenas = config.effective_workers() as u64 * (u64::from(config.max_nesting) + 1);
            assert_eq!(
                reserved,
                arenas * config.arena_max_bytes as u64,
                "{cores} coeurs"
            );
            assert!(config.arena_initial_bytes <= config.arena_max_bytes);
            assert!(config.arena_max_bytes <= rfx_memory::DEFAULT_ARENA_MAX_BYTES);
            if config.arena_max_bytes > MIN_ARENA_BYTES {
                assert!(reserved <= 512 * MIB / ARENA_BUDGET_SHARE, "{cores} coeurs");
            }
        }
        let (roomy, _) = pool_config(64 * 1024 * MIB, 4);
        assert_eq!(
            roomy.arena_max_bytes,
            rfx_memory::DEFAULT_ARENA_MAX_BYTES,
            "defaut du cahier"
        );
        assert_eq!(
            roomy.arena_initial_bytes,
            rfx_memory::DEFAULT_ARENA_INITIAL_BYTES
        );
        let (tight, _) = pool_config(MIB, 14);
        assert_eq!(tight.arena_max_bytes, MIN_ARENA_BYTES, "plancher");
        assert_eq!(tight.arena_initial_bytes, MIN_ARENA_BYTES);
    }

    /// ADR-062 : le pire cas des arenes est reserve dans le budget ; un budget trop petit
    /// pour lui laisse l'ordonnanceur eteint, et rien n'est reserve.
    #[test]
    fn the_arena_reservation_starts_or_withholds_the_pool() {
        let runtime = Runtime::new(RuntimeConfig::default());
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        let (_, reserved) = pool_config(runtime.budget.limit(), cores);
        assert!(runtime.status().scheduler.running);
        assert_eq!(runtime.budget.in_use_by(Pool::Runtime), reserved);

        let tiny = Runtime::new(RuntimeConfig {
            max_native_mb: 1,
            ..RuntimeConfig::default()
        });
        assert!(!tiny.status().scheduler.running, "reservation refusee");
        assert_eq!(tiny.budget.in_use_by(Pool::Runtime), 0);
    }

    /// Les deux options de profilage du fichier de configuration doivent atteindre le
    /// profiler. Elles etaient declarees et lues cote Java, puis perdues : le profiler
    /// se construisait sur ses defauts. Une option qui ne pilote rien trompe
    /// l'utilisateur (contrat agent 3.1).
    #[test]
    fn les_options_de_profilage_atteignent_le_profiler() {
        let config = RuntimeConfig {
            profiler_max_workloads: 4_096,
            profiler_cpu_budget_pct: 7,
            ..RuntimeConfig::default()
        };
        let runtime = Runtime::new(config);

        assert_eq!(runtime.profiler().config().max_workloads, 4_096);
        assert!(
            (runtime.profiler().overhead().cpu_budget_pct() - 7.0).abs() < f32::EPSILON,
            "le budget doit etre celui de la configuration, pas le defaut"
        );
    }

    /// T-363 : double initialisation refusee (R-520, `E-1004`).
    #[test]
    fn double_initialization_is_refused() {
        let _guard = test_lock();
        let h = initialize(RuntimeConfig::default()).expect("premiere init");
        assert_eq!(
            initialize(RuntimeConfig::default()).unwrap_err(),
            ErrorCode::DoubleInit
        );
        shutdown(h).expect("arret");
    }

    /// T-364 : handle invalide rejete (R-521).
    #[test]
    fn invalid_handle_is_rejected() {
        let _guard = test_lock();
        let h = initialize(RuntimeConfig::default()).expect("init");

        for fake in [0_u64, 1, u64::MAX, h ^ 1, h + 1] {
            assert_eq!(
                with(fake, |_| ()).unwrap_err(),
                ErrorCode::InvalidArgument,
                "le handle {fake:#x} n'aurait pas du etre accepte"
            );
        }
        assert!(with(h, |rt| rt.state()).is_ok());
        shutdown(h).expect("arret");
    }

    #[test]
    fn a_released_handle_is_never_revalidated() {
        let _guard = test_lock();
        let first = initialize(RuntimeConfig::default()).expect("init");
        shutdown(first).expect("arret");
        assert_eq!(with(first, |_| ()).unwrap_err(), ErrorCode::InvalidArgument);

        let second = initialize(RuntimeConfig::default()).expect("re-init");
        assert_ne!(first, second, "la generation doit avoir change");
        assert_eq!(with(first, |_| ()).unwrap_err(), ErrorCode::InvalidArgument);
        shutdown(second).expect("arret");
    }

    #[test]
    fn shutdown_with_a_wrong_handle_destroys_nothing() {
        let _guard = test_lock();
        let h = initialize(RuntimeConfig::default()).expect("init");
        assert_eq!(shutdown(h ^ 0xff).unwrap_err(), ErrorCode::InvalidArgument);
        assert!(with(h, |rt| rt.state()).is_ok(), "l'instance doit survivre");
        shutdown(h).expect("arret");
    }

    /// Halt d'urgence : plus de 10 panics en 60 s (PARTIE 5.27).
    #[test]
    fn emergency_halt_triggers_beyond_the_threshold() {
        let mut rt = Runtime::new(RuntimeConfig::default());
        let t0 = Instant::now();
        for i in 1..PANICS_BEFORE_HALT {
            let state = rt.record_panic(t0);
            assert_eq!(
                state,
                RuntimeState::Running,
                "panic {i} ne doit pas arreter"
            );
        }
        assert_eq!(rt.record_panic(t0), RuntimeState::Halted);
        assert_eq!(rt.panics(), PANICS_BEFORE_HALT as u64);
    }

    #[test]
    fn panics_outside_the_window_do_not_trigger_the_halt() {
        let mut rt = Runtime::new(RuntimeConfig::default());
        let t0 = Instant::now();
        for i in 0..(PANICS_BEFORE_HALT * 3) {
            // Une panic toutes les 61 secondes : la fenetre glissante ne retient
            // jamais plus d'un evenement.
            let t = t0 + PANIC_WINDOW * (i as u32 + 1) + Duration::from_secs(i as u64);
            assert_eq!(rt.record_panic(t), RuntimeState::Running);
        }
        assert_eq!(rt.panics(), (PANICS_BEFORE_HALT * 3) as u64);
    }

    #[test]
    fn halt_is_terminal() {
        let mut rt = Runtime::new(RuntimeConfig::default());
        rt.emergency_halt();
        rt.degrade();
        assert_eq!(rt.state(), RuntimeState::Halted);
    }

    /// Une table vide ne doit jamais rester en attente.
    ///
    /// Defaut reel, et silencieux : l'attente n'est rechargee que lorsqu'elle est
    /// vide, et un appelant qui recoit une taille nulle n'accuse pas reception. Une
    /// seule interrogation faite avant qu'une unite ne soit enregistree suffisait donc
    /// a bloquer la livraison des niveaux pour toute la partie. Le mod annoncait
    /// 2 647 methodes sondees, zero sonde armee, et ne mesurait plus que par
    /// echantillonnage de piles — sans le moindre message.
    #[test]
    fn an_empty_level_table_never_stays_pending() {
        let _guard = test_lock();
        let mut rt = Runtime::new(RuntimeConfig::default());
        rt.start_profiler();

        // Aucune unite enregistree : la table existe mais elle est vide.
        assert!(
            rt.pending_probe_levels().is_none(),
            "une table vide n'a rien a transmettre"
        );

        // Une unite apparait : la table suivante doit pouvoir etre prise.
        rt.register_workload(&WorkDescriptor::default(), rfx_model::WorkId(1));
        rt.tick_begin(1, Side::Server, Instant::now());
        rt.tick_end(Instant::now());

        let levels = rt
            .pending_probe_levels()
            .expect("la table doit etre livrable");
        assert_eq!(levels.len(), 1, "une unite enregistree, un niveau");
        assert_ne!(
            levels[0], 0,
            "sous le plafond LIGHT une unite froide doit etre armee en COUNTER,              faute de quoi le sondage ne mesure rien"
        );
    }

    /// Le statut doit rester lisible par Java, champ signe compris.
    ///
    /// Troisieme defaut de cette famille en une journee. Celui-ci : trois differences
    /// signees ajoutees a `ProfilerStatus` pour diagnostiquer le bruit de la ligne de
    /// base. `CborReader` n'avait aucun `case 1` — les entiers negatifs n'etaient pas
    /// supportes du tout — et le blob ENTIER devenait illisible. Le fichier de campagne
    /// perdait d'un coup le niveau du profileur et la ligne de base, sans message.
    ///
    /// Le statut porte des valeurs qui n'existent qu'a l'execution : ce test le
    /// remplit avec une mesure de ligne de base et une unite de travail, sinon il ne
    /// verifierait que la carcasse.
    /// R-533 : la base ne depasse ni `cache.max_memory_mb`, ni ce que le budget natif
    /// laisse libre.
    #[test]
    fn the_database_cap_respects_the_native_budget() {
        const MIB: usize = 1024 * 1024;
        let mut rt = Runtime::new(RuntimeConfig::default());
        assert_eq!(rt.cache_cap_bytes(), 128 * MIB, "cache.max_memory_mb");

        let mut tight = Runtime::new(RuntimeConfig {
            max_native_mb: 16,
            ..RuntimeConfig::default()
        });
        assert!(tight.acquire_probe_buffer(1).is_some());
        let cap = tight.cache_cap_bytes();
        assert!(cap < 16 * MIB, "borne par le budget natif : {cap}");
        assert_eq!(
            cap as u64,
            tight.budget.limit() - tight.budget.in_use(),
            "ce que les tampons de sonde laissent"
        );
    }

    /// C-09 (ADR-049) : les mesures du profileur atteignent le fichier. Le serveur de
    /// test du jeu s'arrete apres un tick, trop tot pour le montrer : ce test fait
    /// tourner quarante ticks avec de vrais enregistrements de sonde.
    /// C-07 (ADR-051) : une unite sondee qui tourne devient chaude, son graphe est
    /// construit hors du verrou du registre, et le statut le publie.
    #[test]
    fn a_hot_probed_workload_gets_its_call_graph() {
        use crate::analysis::tests::{drive, fixtures, hierarchy};
        use crate::analysis::StepState;
        use rfx_memory::probe_buffer::{ProbeRecord, RecordKind, PROBE_RECORD_SIZE};

        let classes = fixtures();
        let mut rt = Runtime::new(RuntimeConfig::default());
        rt.start_profiler();
        let descriptor = WorkDescriptor {
            owner_id: "fixtures".to_owned(),
            class_internal_name: "fixtures/Calls".to_owned(),
            method_name: "vehicle".to_owned(),
            method_descriptor: "(Lfixtures/Vehicle;)I".to_owned(),
            ..WorkDescriptor::default()
        };
        let work_id = rfx_model::WorkId::compute(&descriptor);
        let probe_id = rt.register_workload(&descriptor, work_id).expect("sondee");
        assert!(rt.acquire_probe_buffer(3).is_some());
        let record = ProbeRecord {
            probe_id,
            value: 50_000,
            kind: RecordKind::Exit.to_byte(),
            ..ProbeRecord::default()
        };
        for tick in 1..=40 {
            rt.tick_begin(tick, Side::Server, Instant::now());
            let buffer = rt.probe_buffers.get_mut(3).expect("tampon");
            assert!(buffer.write_at(0, &record.encode()));
            rt.flush_probe_buffer(3, PROBE_RECORD_SIZE);
            rt.tick_end(Instant::now());
        }

        let (shared, staged) = rt.analysis_handoff();
        assert_eq!(staged.len(), 1, "la methode sondee attend l'analyse");
        let hot = rt.hot_workloads();
        assert_eq!(
            hot.iter().map(|h| h.work_id).collect::<Vec<_>>(),
            vec![work_id],
            "mesuree, donc chaude"
        );
        assert!(hot[0].observation_quality > 0.0);
        let (status, bytes) = {
            let mut analysis = shared.lock().unwrap();
            for (id, method, side) in staged {
                analysis.note(id, method, side);
            }
            analysis.declare(hierarchy(&classes));
            assert_eq!(drive(&mut analysis, &hot, &classes), StepState::Finished);
            (analysis.status(), analysis.estimated_bytes())
        };
        assert!(rt.publish_analysis(status, bytes));
        let published = rt.status();
        assert_eq!(published.analysis.graphs, 1);
        assert_eq!(published.analysis.resolved_calls, 1);
        assert!(published.probes.native_bytes >= bytes, "memoire comptee");
        assert!(published
            .components
            .iter()
            .any(|component| component.id == "C-07" && component.active));
        assert!(published
            .components
            .iter()
            .any(|component| component.id == "C-08" && component.active));
        assert_eq!(published.analysis.descriptors, 1, "C-08 : un descripteur");

        // Un budget trop petit pour cette memoire est refuse.
        let mut tight = Runtime::new(RuntimeConfig {
            max_native_mb: 16,
            ..RuntimeConfig::default()
        });
        assert!(!tight.publish_analysis(status, 64 * 1024 * 1024));
    }

    #[test]
    fn profiler_measurements_reach_the_workload_database() {
        use rfx_memory::probe_buffer::{ProbeRecord, RecordKind, PROBE_RECORD_SIZE};

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .expect("horloge")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rfx-core-db-{}-{unique}", std::process::id()));
        let mut rt = Runtime::new(RuntimeConfig::default());
        rt.start_profiler();
        let descriptor = WorkDescriptor {
            owner_id: "minecraft".to_owned(),
            class_internal_name: "net/minecraft/world/entity/Mob".to_owned(),
            method_name: "tick".to_owned(),
            method_descriptor: "()V".to_owned(),
            ..WorkDescriptor::default()
        };
        let work_id = rfx_model::WorkId::compute(&descriptor);
        let probe_id = rt.register_workload(&descriptor, work_id).expect("sondee");
        let open = DbOpen {
            schema: rfx_model::MODEL_SCHEMA_VERSION,
            dir: dir.to_string_lossy().into_owned(),
            minecraft: "1.20.1".to_owned(),
            ..DbOpen::default()
        };
        rt.open_db(&open).expect("ouverte");
        assert!(rt.acquire_probe_buffer(7).is_some());

        let records = [
            ProbeRecord {
                probe_id,
                value: 3,
                kind: RecordKind::Enter.to_byte(),
                ..ProbeRecord::default()
            },
            ProbeRecord {
                probe_id,
                value: 40_000,
                kind: RecordKind::Exit.to_byte(),
                ..ProbeRecord::default()
            },
        ];
        for tick in 1..=40 {
            rt.tick_begin(tick, Side::Server, Instant::now());
            let buffer = rt.probe_buffers.get_mut(7).expect("tampon");
            for (index, record) in records.iter().enumerate() {
                assert!(buffer.write_at(index * PROBE_RECORD_SIZE, &record.encode()));
            }
            rt.flush_probe_buffer(7, records.len() * PROBE_RECORD_SIZE);
            rt.tick_end(Instant::now());
        }
        while !rt.db_sync_step() {}
        assert!(rt.close_db(Duration::from_secs(30)), "ecrite a temps");
        // ADR-061 : la relecture et l'ecriture ont tourne sur le pool de l'ordonnanceur
        // (une seule tache de vidage peut servir les deux).
        let scheduler = rt.status().scheduler;
        assert!(scheduler.running);
        assert!(scheduler.workers >= 1);
        assert!(scheduler.executed >= 1, "{scheduler:?}");

        let bytes = std::fs::read(dir.join(crate::db::FILE_NAME)).expect("fichier");
        let _ = std::fs::remove_dir_all(&dir);
        let decoded = rfx_cache::file::decode(&bytes).expect("lisible");
        let record = decoded
            .records
            .iter()
            .find(|record| record.id == work_id)
            .expect("unite presente");
        assert_eq!(record.descriptor, descriptor);
        assert_eq!(
            record.dynamics.total_calls,
            40 * 4,
            "trois passages et une sortie"
        );
        assert_eq!(record.dynamics.last_seen_tick, 40);
    }

    #[test]
    fn the_status_stays_readable_by_java() {
        let _guard = test_lock();
        let mut rt = Runtime::new(RuntimeConfig::default());
        rt.start_profiler();
        rt.register_workload(&WorkDescriptor::default(), rfx_model::WorkId(1));
        rt.record_panic(Instant::now());
        for tick in 1..=400 {
            rt.tick_begin(tick, Side::Server, Instant::now());
            rt.tick_end(Instant::now());
        }

        let blob = rfx_model::to_cbor(&rt.status()).expect("serialisation");

        if let Err(reason) = rfx_model::assert_java_readable(&blob) {
            panic!("le statut n'est pas lisible par Java : {reason}");
        }
    }

    #[test]
    fn status_reflects_state_and_panics() {
        let mut rt = Runtime::new(RuntimeConfig::default());
        rt.record_panic(Instant::now());
        let s = rt.status();
        assert_eq!(s.abi_version, ABI_VERSION);
        assert_eq!(s.state, "RUNNING");
        assert_eq!(s.panics, 1);
        assert!(s.components.iter().any(|c| c.id == "C-27"));
    }
}
