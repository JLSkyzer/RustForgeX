//! C-27 : etat global du runtime natif et confinement des pannes.
//!
//! Cahier des charges : PARTIE 5.27. Exigences : R-520 (instance unique), R-521
//! (handle opaque valide), R-523 (comptage des panics), halt d'urgence.
//! Maturite : `STABLE`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use rfx_memory::probe_buffer::DEFAULT_BUFFER_BYTES;
use rfx_memory::{MemoryBudget, Pool, ProbeBufferPool};
use rfx_model::{
    ComponentStatus, HardwareClass, Maturity, ProbeCoverage, ProbeStatus, RuntimeConfig,
    RuntimeStatus, Side, TickStatus,
};
use rfx_profiler::{Profiler, ProfilerConfig, TickCost};

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
/// Les champs `store`, `scheduler`, `decision`, `telemetry` et `caches` decrits en
/// PARTIE 5.27 apparaitront au jalon qui implemente les composants correspondants.
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
        };
        Self {
            config,
            hardware: HardwareClass::default(),
            coverage: ProbeCoverage::default(),
            state: RuntimeState::Running,
            panics: 0,
            recent_panics: Vec::new(),
            tick_window: TickWindow::default(),
            budget: MemoryBudget::new(max_native_mb),
            probe_buffers: ProbeBufferPool::new(DEFAULT_BUFFER_BYTES, MAX_PROBED_THREADS),
            probe_records_consumed: 0,
            profiler: Profiler::new(profiler_config),
            last_tick_begin: None,
            last_period_ns: 0,
            profiling_ns_this_tick: 0,
            profiler_over_budget: 0,
            pending_levels: None,
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
    /// froide ne soit evincable : la methode n'est alors pas sondee.
    pub fn register_workload(&mut self, work_id: rfx_model::WorkId) -> Option<u32> {
        self.profiler.register(work_id)
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
    pub fn record_hook(&mut self, duration: std::time::Duration) -> bool {
        self.tick_window.record_hook(duration)
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
        self.profiler.store().top(limit)
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
            ],
            probes: ProbeStatus {
                records_consumed: self.probe_records_consumed,
                records_lost: self.probe_buffers.records_lost(),
                native_bytes: self.budget.in_use(),
                native_limit_bytes: self.budget.limit(),
            },
            profiler: self.profiler.status(),
            tick: {
                let m = self.tick_window.metrics();
                TickStatus {
                    ticks: m.ticks,
                    unbalanced: m.unbalanced,
                    invalid_transitions: m.invalid_transitions,
                    hook_budget_exceeded: m.hook_budget_exceeded,
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
/// # Erreurs
///
/// Renvoie [`ErrorCode::InvalidArgument`] si le handle n'est pas celui de
/// l'instance courante.
pub fn shutdown(handle: u64) -> Result<(), ErrorCode> {
    let mut reg = lock_registry();
    if handle != (HANDLE_PATTERN | reg.generation) || reg.generation == 0 {
        return Err(ErrorCode::InvalidArgument);
    }
    reg.runtime = None;
    reg.generation = 0;
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
