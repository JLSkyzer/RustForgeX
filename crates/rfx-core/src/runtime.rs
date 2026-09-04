//! C-27 : etat global du runtime natif et confinement des pannes.
//!
//! Cahier des charges : PARTIE 5.27. Exigences : R-520 (instance unique), R-521
//! (handle opaque valide), R-523 (comptage des panics), halt d'urgence.
//! Maturite : `STABLE`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use rfx_model::{
    ComponentStatus, HardwareClass, Maturity, ProbeCoverage, RuntimeConfig, RuntimeStatus,
};

use crate::error::ErrorCode;
use crate::state::RuntimeState;

/// Version de l'ABI implementee par ce binaire (IF-01, `RFX_ABI_VERSION`).
pub const ABI_VERSION: u32 = 1;

/// Fenetre glissante d'observation des panics pour le halt d'urgence.
const PANIC_WINDOW: Duration = Duration::from_secs(60);

/// Nombre de panics dans [`PANIC_WINDOW`] au-dela duquel le runtime s'arrete.
const PANICS_BEFORE_HALT: usize = 10;

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
}

impl Runtime {
    /// Construit le runtime a partir d'une configuration deja validee.
    #[must_use]
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            config,
            hardware: HardwareClass::default(),
            coverage: ProbeCoverage::default(),
            state: RuntimeState::Running,
            panics: 0,
            recent_panics: Vec::new(),
        }
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
        }
    }
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
