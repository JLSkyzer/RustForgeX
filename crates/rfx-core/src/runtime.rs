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
const FENETRE_PANIC: Duration = Duration::from_secs(60);

/// Nombre de panics dans [`FENETRE_PANIC`] au-dela duquel le runtime s'arrete.
const PANICS_AVANT_HALT: usize = 10;

/// Etat global du runtime natif.
///
/// Les champs `store`, `scheduler`, `decision`, `telemetry` et `caches` decrits en
/// PARTIE 5.27 apparaitront au jalon qui implemente les composants correspondants.
/// Ils ne sont pas declares ici tant qu'ils n'existent pas : un champ vide serait une
/// fiction au sens du contrat agent 3.1.
#[derive(Debug)]
pub struct Runtime {
    config: RuntimeConfig,
    hw: HardwareClass,
    couverture: ProbeCoverage,
    etat: RuntimeState,
    panics: u64,
    panics_recentes: Vec<Instant>,
}

impl Runtime {
    /// Construit le runtime a partir d'une configuration deja validee.
    #[must_use]
    pub fn nouveau(config: RuntimeConfig) -> Self {
        Self {
            config,
            hw: HardwareClass::default(),
            couverture: ProbeCoverage::default(),
            etat: RuntimeState::Running,
            panics: 0,
            panics_recentes: Vec::new(),
        }
    }

    /// Configuration effective de cette execution.
    #[must_use]
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Etat courant.
    #[must_use]
    pub fn etat(&self) -> RuntimeState {
        self.etat
    }

    /// Classe materielle mesuree par C-45, et les champs reellement mesures.
    #[must_use]
    pub fn materiel(&self) -> (HardwareClass, ProbeCoverage) {
        (self.hw, self.couverture)
    }

    /// Enregistre le resultat de la sonde materielle (C-45).
    pub fn definir_materiel(&mut self, hw: HardwareClass, couverture: ProbeCoverage) {
        self.hw = hw;
        self.couverture = couverture;
    }

    /// Enregistre les couts de franchissement de frontiere mesures **depuis Java**.
    ///
    /// Ces deux couts ne peuvent pas etre mesures depuis le natif : ils incluent le
    /// trajet aller-retour complet depuis la JVM. C'est donc l'appelant Java qui les
    /// mesure (C-45) et les publie ici, ou ils alimenteront le modele de cout de
    /// C-15 (R-660).
    pub fn definir_couts_ffi(&mut self, jni_call_ns: u32, ffi_batch_ns_per_kb: u32) {
        self.hw.jni_call_ns = jni_call_ns;
        self.hw.ffi_batch_ns_per_kb = ffi_batch_ns_per_kb;
        self.couverture.ffi_call = jni_call_ns > 0;
        self.couverture.ffi_transfer = ffi_batch_ns_per_kb > 0;
    }

    /// Fait passer le runtime en mode degrade. Sans effet si l'etat est terminal.
    pub fn degrader(&mut self) {
        if !self.etat.est_terminal() {
            self.etat = RuntimeState::Degraded;
        }
    }

    /// Declenche le halt d'urgence : le jeu continue en Java pur (PARTIE 5.27).
    pub fn arreter_d_urgence(&mut self) {
        self.etat = RuntimeState::Halted;
    }

    /// Comptabilise une panic capturee a la frontiere FFI (R-523).
    ///
    /// Renvoie l'etat du runtime apres traitement. Au-dela de
    /// [`PANICS_AVANT_HALT`] panics dans une fenetre de 60 secondes, le runtime
    /// passe en `HALTED`.
    pub fn enregistrer_panic(&mut self, maintenant: Instant) -> RuntimeState {
        self.panics = self.panics.saturating_add(1);
        self.panics_recentes
            .retain(|t| maintenant.duration_since(*t) < FENETRE_PANIC);
        self.panics_recentes.push(maintenant);

        if self.panics_recentes.len() >= PANICS_AVANT_HALT {
            self.arreter_d_urgence();
        }
        self.etat
    }

    /// Nombre total de panics capturees depuis le demarrage.
    #[must_use]
    pub fn panics(&self) -> u64 {
        self.panics
    }

    /// Construit le blob de statut publie vers Java (`/rfx status`).
    #[must_use]
    pub fn statut(&self) -> RuntimeStatus {
        RuntimeStatus {
            schema: rfx_model::MODEL_SCHEMA_VERSION,
            abi_version: ABI_VERSION,
            version_native: env!("CARGO_PKG_VERSION").to_owned(),
            etat: self.etat.libelle().to_owned(),
            panics: self.panics,
            materiel: self.hw,
            couverture_sonde: self.couverture,
            composants: vec![
                ComponentStatus {
                    id: "C-27".to_owned(),
                    nom: "Rust Runtime Core".to_owned(),
                    maturite: Maturity::Stable,
                    actif: self.etat.accepte_du_travail(),
                },
                ComponentStatus {
                    id: "C-45".to_owned(),
                    nom: "Hardware Probe".to_owned(),
                    maturite: Maturity::Stable,
                    actif: self.couverture.cores,
                },
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Instance unique du processus (R-520) et handles opaques (R-521)
// ---------------------------------------------------------------------------

/// Etat partage : l'instance unique et la generation du handle courant.
struct Registre {
    runtime: Option<Runtime>,
    generation: u64,
}

fn registre() -> &'static Mutex<Registre> {
    static REGISTRE: OnceLock<Mutex<Registre>> = OnceLock::new();
    REGISTRE.get_or_init(|| {
        Mutex::new(Registre {
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
fn verrouiller() -> MutexGuard<'static, Registre> {
    registre().lock().unwrap_or_else(|e| e.into_inner())
}

/// Compteur de generations, garantissant qu'un handle libere n'est jamais revalide.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Motif de poids fort d'un handle, pour qu'un entier arbitraire (0, 1, -1) fourni par
/// erreur ne puisse pas passer pour un handle valide.
const MOTIF_HANDLE: u64 = 0x5246_5800_0000_0000;

/// Initialise l'instance unique du runtime et renvoie son handle opaque.
///
/// # Erreurs
///
/// Renvoie [`ErrorCode::DoubleInit`] (`E-1004`) si une instance existe deja (R-520).
pub fn initialiser(config: RuntimeConfig) -> Result<u64, ErrorCode> {
    let mut reg = verrouiller();
    if reg.runtime.is_some() {
        return Err(ErrorCode::DoubleInit);
    }
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    reg.runtime = Some(Runtime::nouveau(config));
    reg.generation = generation;
    Ok(MOTIF_HANDLE | generation)
}

/// Execute `f` sur l'instance, apres validation du handle (R-521).
///
/// # Erreurs
///
/// Renvoie [`ErrorCode::ArgumentInvalide`] si le handle ne correspond pas a
/// l'instance courante : handle d'une generation liberee, entier arbitraire, ou
/// runtime non initialise.
pub fn avec<R>(handle: u64, f: impl FnOnce(&mut Runtime) -> R) -> Result<R, ErrorCode> {
    let mut reg = verrouiller();
    if handle != (MOTIF_HANDLE | reg.generation) || reg.generation == 0 {
        return Err(ErrorCode::ArgumentInvalide);
    }
    let rt = reg.runtime.as_mut().ok_or(ErrorCode::ArgumentInvalide)?;
    Ok(f(rt))
}

/// Detruit l'instance. Le handle devient definitivement invalide.
///
/// # Erreurs
///
/// Renvoie [`ErrorCode::ArgumentInvalide`] si le handle n'est pas celui de
/// l'instance courante.
pub fn arreter(handle: u64) -> Result<(), ErrorCode> {
    let mut reg = verrouiller();
    if handle != (MOTIF_HANDLE | reg.generation) || reg.generation == 0 {
        return Err(ErrorCode::ArgumentInvalide);
    }
    reg.runtime = None;
    reg.generation = 0;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les tests partagent l'instance unique du processus : ils sont serialises.
    fn verrou_de_test() -> MutexGuard<'static, ()> {
        static V: Mutex<()> = Mutex::new(());
        V.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// T-363 : double initialisation refusee (R-520, `E-1004`).
    #[test]
    fn double_initialisation_refusee() {
        let _v = verrou_de_test();
        let h = initialiser(RuntimeConfig::default()).expect("premiere init");
        assert_eq!(
            initialiser(RuntimeConfig::default()).unwrap_err(),
            ErrorCode::DoubleInit
        );
        arreter(h).expect("arret");
    }

    /// T-364 : handle invalide rejete (R-521).
    #[test]
    fn handle_invalide_rejete() {
        let _v = verrou_de_test();
        let h = initialiser(RuntimeConfig::default()).expect("init");

        for faux in [0_u64, 1, u64::MAX, h ^ 1, h + 1] {
            assert_eq!(
                avec(faux, |_| ()).unwrap_err(),
                ErrorCode::ArgumentInvalide,
                "le handle {faux:#x} n'aurait pas du etre accepte"
            );
        }
        assert!(avec(h, |rt| rt.etat()).is_ok());
        arreter(h).expect("arret");
    }

    #[test]
    fn un_handle_libere_n_est_jamais_revalide() {
        let _v = verrou_de_test();
        let premier = initialiser(RuntimeConfig::default()).expect("init");
        arreter(premier).expect("arret");
        assert_eq!(
            avec(premier, |_| ()).unwrap_err(),
            ErrorCode::ArgumentInvalide
        );

        let second = initialiser(RuntimeConfig::default()).expect("re-init");
        assert_ne!(premier, second, "la generation doit avoir change");
        assert_eq!(
            avec(premier, |_| ()).unwrap_err(),
            ErrorCode::ArgumentInvalide
        );
        arreter(second).expect("arret");
    }

    #[test]
    fn arreter_avec_un_mauvais_handle_ne_detruit_rien() {
        let _v = verrou_de_test();
        let h = initialiser(RuntimeConfig::default()).expect("init");
        assert_eq!(arreter(h ^ 0xff).unwrap_err(), ErrorCode::ArgumentInvalide);
        assert!(avec(h, |rt| rt.etat()).is_ok(), "l'instance doit survivre");
        arreter(h).expect("arret");
    }

    /// Halt d'urgence : plus de 10 panics en 60 s (PARTIE 5.27).
    #[test]
    fn le_halt_d_urgence_se_declenche_au_dela_du_seuil() {
        let mut rt = Runtime::nouveau(RuntimeConfig::default());
        let t0 = Instant::now();
        for i in 1..PANICS_AVANT_HALT {
            let etat = rt.enregistrer_panic(t0);
            assert_eq!(etat, RuntimeState::Running, "panic {i} ne doit pas arreter");
        }
        assert_eq!(rt.enregistrer_panic(t0), RuntimeState::Halted);
        assert_eq!(rt.panics(), PANICS_AVANT_HALT as u64);
    }

    #[test]
    fn des_panics_hors_fenetre_ne_declenchent_pas_le_halt() {
        let mut rt = Runtime::nouveau(RuntimeConfig::default());
        let t0 = Instant::now();
        for i in 0..(PANICS_AVANT_HALT * 3) {
            // Une panic toutes les 61 secondes : la fenetre glissante ne retient
            // jamais plus d'un evenement.
            let t = t0 + FENETRE_PANIC * (i as u32 + 1) + Duration::from_secs(i as u64);
            assert_eq!(rt.enregistrer_panic(t), RuntimeState::Running);
        }
        assert_eq!(rt.panics(), (PANICS_AVANT_HALT * 3) as u64);
    }

    #[test]
    fn le_halt_est_terminal() {
        let mut rt = Runtime::nouveau(RuntimeConfig::default());
        rt.arreter_d_urgence();
        rt.degrader();
        assert_eq!(rt.etat(), RuntimeState::Halted);
    }

    #[test]
    fn le_statut_reflete_l_etat_et_les_panics() {
        let mut rt = Runtime::nouveau(RuntimeConfig::default());
        rt.enregistrer_panic(Instant::now());
        let s = rt.statut();
        assert_eq!(s.abi_version, ABI_VERSION);
        assert_eq!(s.etat, "RUNNING");
        assert_eq!(s.panics, 1);
        assert!(s.composants.iter().any(|c| c.id == "C-27"));
    }
}
