//! IF-01 : points d'entree ABI du runtime natif.
//!
//! Composant : C-27 (frontiere). Cahier des charges : PARTIE 6.1 et 6.2.
//! Decision : ADR-004 (JNI + `DirectByteBuffer`), ADR-015 (points d'entree du
//! jalon M0). Maturite : `STABLE`.
//!
//! # Contrat commun a toutes les fonctions exportees
//!
//! - toute fonction renvoie un `i32` : `0` en cas de succes, la valeur **negative**
//!   d'un code d'erreur de l'annexe A.2 sinon (R-701) ;
//! - aucune ne renvoie de pointeur brut (R-701) ;
//! - aucune structure `repr(Rust)` ne traverse la frontiere : seuls des entiers et
//!   des tampons CBOR a schema versionne circulent (R-704) ;
//! - toute chaine est UTF-8 avec longueur explicite, jamais terminee par zero (R-705) ;
//! - [`rfx_abi_version`] DOIT etre appelee avant toute autre fonction (R-702) ;
//! - **aucune panic ne traverse la frontiere** : chaque point d'entree est enveloppe
//!   dans [`std::panic::catch_unwind`] (R-522, R-523, contrat agent 3.8).
//!
//! Les donnees venant de Java sont traitees comme non fiables (PARTIE 19.2) : tout
//! pointeur nul, toute longueur aberrante et tout handle inconnu sont rejetes avant
//! usage.

pub mod jni_bridge;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;

use rfx_core::error::{ErrorCode, OK};
use rfx_core::tick::TickPhase;
use rfx_core::{hw, runtime, ABI_VERSION};
use rfx_model::{RuntimeConfig, RuntimeMode, Side};

/// Taille maximale acceptee pour le blob de configuration, en octets.
///
/// La configuration complete de la PARTIE 28 pese quelques kibioctets. Ce plafond
/// protege contre une longueur aberrante transmise par erreur (PARTIE 19.2).
const MAX_CONFIG_SIZE: usize = 1 << 20;

/// Taille maximale acceptee pour un tampon de mesure de transfert, en octets.
const MAX_TRANSFER_SIZE: usize = 64 << 20;

/// Enveloppe un point d'entree sans handle : capture toute panic (R-522).
fn guard(f: impl FnOnce() -> i32) -> i32 {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(code) => code,
        Err(_) => ErrorCode::PanicCaught.ffi_code(),
    }
}

/// Enveloppe un point d'entree disposant d'un handle : capture toute panic et la
/// comptabilise dans le runtime concerne (R-523).
///
/// Le comptage est lui-meme protege : si l'enregistrement echoue (handle deja
/// invalide, runtime detruit), la panic reste capturee et le code `E-3001` est
/// renvoye. Aucune panic ne peut donc atteindre la JVM.
fn guard_with_handle(handle: u64, f: impl FnOnce() -> i32) -> i32 {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(code) => code,
        Err(_) => {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                let _ = runtime::with(handle, |rt| rt.record_panic(Instant::now()));
            }));
            ErrorCode::PanicCaught.ffi_code()
        }
    }
}

/// Version de l'ABI implementee par ce binaire (IF-01, R-702).
///
/// Renvoie la version, toujours positive. Cette fonction ne peut pas echouer et ne
/// touche aucun etat : elle est appelable avant [`rfx_init`].
#[no_mangle]
pub extern "C" fn rfx_abi_version() -> i32 {
    guard(|| i32::try_from(ABI_VERSION).unwrap_or(i32::MAX))
}

/// Initialise l'instance unique du runtime (R-520) et publie son handle.
///
/// `config_cbor` pointe sur un blob CBOR de `len` octets decrivant la configuration
/// validee cote Java. Le handle produit est opaque et doit etre presente a tous les
/// appels ulterieurs (R-521).
///
/// Renvoie `E-1004` si une instance existe deja, et une erreur d'argument si le blob
/// est nul, trop grand, illisible ou porte un schema inconnu.
///
/// # Safety
///
/// `config_cbor` doit pointer sur au moins `len` octets lisibles, et `out_handle` sur
/// un `u64` inscriptible. Les deux doivent rester valides pendant l'appel.
#[no_mangle]
pub unsafe extern "C" fn rfx_init(config_cbor: *const u8, len: usize, out_handle: *mut u64) -> i32 {
    guard(|| {
        if config_cbor.is_null() || out_handle.is_null() || len == 0 || len > MAX_CONFIG_SIZE {
            return ErrorCode::InvalidArgument.ffi_code();
        }
        // SAFETY : precondition de la fonction, verifiee ci-dessus pour le cas nul.
        let bytes = unsafe { std::slice::from_raw_parts(config_cbor, len) };

        let Ok(config) = rfx_model::from_cbor::<RuntimeConfig>(bytes) else {
            return ErrorCode::InvalidArgument.ffi_code();
        };
        if config.check_schema().is_err() {
            return ErrorCode::InvalidArgument.ffi_code();
        }

        match runtime::initialize(config) {
            Ok(handle) => {
                // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
                unsafe { out_handle.write(handle) };
                OK
            }
            Err(e) => e.ffi_code(),
        }
    })
}

/// Detruit l'instance du runtime. Le handle devient definitivement invalide.
#[no_mangle]
pub extern "C" fn rfx_shutdown(handle: u64) -> i32 {
    guard_with_handle(handle, || match runtime::shutdown(handle) {
        Ok(()) => OK,
        Err(e) => e.ffi_code(),
    })
}

/// Point d'entree sans effet, servant a mesurer le cout d'un aller-retour FFI.
///
/// C-45 mesure ce cout **depuis Java**, sur 10 000 appels, car il inclut le trajet
/// complet depuis la JVM. Cette fonction fait donc le strict minimum : valider le
/// handle. Le resultat est publie via [`rfx_hw_set_ffi_costs`].
#[no_mangle]
pub extern "C" fn rfx_noop(handle: u64) -> i32 {
    guard_with_handle(handle, || match runtime::with(handle, |_| ()) {
        Ok(()) => OK,
        Err(e) => e.ffi_code(),
    })
}

/// Declenche la sonde materielle native (C-45) et memorise son resultat.
///
/// Peut etre rappelee pour re-sonder la machine (R-661).
#[no_mangle]
pub extern "C" fn rfx_hw_probe(handle: u64) -> i32 {
    guard_with_handle(handle, || {
        let (hardware, coverage) = hw::probe();
        match runtime::with(handle, |rt| rt.set_hardware(hardware, coverage)) {
            Ok(()) => OK,
            Err(e) => e.ffi_code(),
        }
    })
}

/// Publie les couts de franchissement de frontiere mesures par Java (C-45).
///
/// `jni_call_ns` est le cout moyen d'un aller-retour minimal, `ffi_batch_ns_per_kb`
/// le cout de transfert par kibioctet. Une valeur nulle signifie « non mesure » et
/// laisse la couverture de sonde a `false` : aucune constante n'est substituee
/// (R-660).
#[no_mangle]
pub extern "C" fn rfx_hw_set_ffi_costs(
    handle: u64,
    jni_call_ns: u32,
    ffi_batch_ns_per_kb: u32,
) -> i32 {
    guard_with_handle(handle, || {
        match runtime::with(handle, |rt| {
            rt.set_ffi_costs(jni_call_ns, ffi_batch_ns_per_kb);
        }) {
            Ok(()) => OK,
            Err(e) => e.ffi_code(),
        }
    })
}

/// Lit un tampon fourni par Java, pour mesurer le debit de transfert Java vers natif.
///
/// La somme de controle renvoyee force la lecture effective de chaque octet : sans
/// elle, l'optimiseur pourrait supprimer le parcours et fausser la mesure.
///
/// # Safety
///
/// `data` doit pointer sur au moins `len` octets lisibles, et `out_checksum` sur
/// un `u64` inscriptible.
#[no_mangle]
pub unsafe extern "C" fn rfx_transfer_probe(
    handle: u64,
    data: *const u8,
    len: usize,
    out_checksum: *mut u64,
) -> i32 {
    guard_with_handle(handle, || {
        if data.is_null() || out_checksum.is_null() || len > MAX_TRANSFER_SIZE {
            return ErrorCode::InvalidArgument.ffi_code();
        }
        if let Err(e) = runtime::with(handle, |_| ()) {
            return e.ffi_code();
        }
        // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        let checksum = bytes.iter().fold(0_u64, |acc, b| {
            acc.wrapping_mul(31).wrapping_add(u64::from(*b))
        });
        // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
        unsafe { out_checksum.write(checksum) };
        OK
    })
}

/// Ecrit le statut du runtime dans `out`, sous forme de blob CBOR.
///
/// Protocole en deux temps : appeler d'abord avec `cap == 0` pour obtenir la taille
/// requise dans `out_len`, puis rappeler avec un tampon de cette taille. Un tampon
/// non nul mais trop petit est refuse, `out_len` recevant la taille requise.
///
/// # Safety
///
/// `out` doit pointer sur au moins `cap` octets inscriptibles (il peut etre nul si
/// `cap` vaut zero), et `out_len` sur un `usize` inscriptible.
#[no_mangle]
pub unsafe extern "C" fn rfx_status(
    handle: u64,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> i32 {
    guard_with_handle(handle, || {
        if out_len.is_null() || (out.is_null() && cap > 0) {
            return ErrorCode::InvalidArgument.ffi_code();
        }

        let status = match runtime::with(handle, |rt| rt.status()) {
            Ok(s) => s,
            Err(e) => return e.ffi_code(),
        };
        let Ok(blob) = rfx_model::to_cbor(&status) else {
            return ErrorCode::InvalidArgument.ffi_code();
        };

        // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
        unsafe { out_len.write(blob.len()) };
        if cap == 0 {
            return OK;
        }
        if cap < blob.len() {
            return ErrorCode::InvalidArgument.ffi_code();
        }
        // SAFETY : `out` possede au moins `cap` octets inscriptibles et `cap` est
        // superieur ou egal a la longueur copiee ; les zones ne se recouvrent pas,
        // `blob` etant une allocation locale.
        unsafe { std::ptr::copy_nonoverlapping(blob.as_ptr(), out, blob.len()) };
        OK
    })
}

// ---------------------------------------------------------------------------
// IF-02 : cycle de tick
// ---------------------------------------------------------------------------

/// Execute une operation du cycle de tick en mesurant sa propre duree (R-707).
///
/// `rfx_tick_*` ne doit jamais retenir le thread autoritatif au-dela de
/// `tick.max_hook_ns`. La mesure est prise ici, au plus pres de l'appel, et le
/// depassement est compte dans la fenetre de tick.
fn timed_tick_op(handle: u64, op: impl FnOnce(&mut rfx_core::Runtime) -> i32) -> i32 {
    let started = Instant::now();
    let outcome = runtime::with(handle, |rt| {
        let code = op(rt);
        rt.record_hook(started.elapsed());
        code
    });
    match outcome {
        Ok(code) => code,
        Err(e) => e.ffi_code(),
    }
}

/// Convertit le code de cote transmis par Java.
fn side_from_code(code: i32) -> Option<Side> {
    match code {
        0 => Some(Side::Client),
        1 => Some(Side::Server),
        2 => Some(Side::Common),
        _ => None,
    }
}

/// Ouvre la fenetre de tick (IF-02).
///
/// Toujours appele depuis le thread autoritatif. Si la fenetre precedente est restee
/// ouverte — un autre mod ayant interrompu le tick avant sa fin —, elle est fermee
/// implicitement et comptee dans `rfx.tick.unbalanced` (R-706).
#[no_mangle]
pub extern "C" fn rfx_tick_begin(handle: u64, tick: u64, side: i32) -> i32 {
    guard_with_handle(handle, || {
        let Some(side) = side_from_code(side) else {
            return ErrorCode::InvalidArgument.ffi_code();
        };
        timed_tick_op(handle, |rt| {
            rt.tick_begin(tick, side, Instant::now());
            OK
        })
    })
}

/// Declare une transition de phase (IF-02).
///
/// Une transition qui ne suit pas SM-04 est comptee et refusee, sans erreur remontee :
/// le jeu ne doit jamais s'arreter parce qu'un tick s'est deroule autrement que prevu.
/// Le refus est visible dans `rfx.tick.invalid_transitions`.
#[no_mangle]
pub extern "C" fn rfx_phase(handle: u64, phase: i32) -> i32 {
    guard_with_handle(handle, || {
        let Some(phase) = TickPhase::from_code(phase) else {
            return ErrorCode::InvalidArgument.ffi_code();
        };
        timed_tick_op(handle, |rt| {
            rt.tick_phase(phase);
            OK
        })
    })
}

/// Ferme la fenetre de tick (IF-02).
///
/// `out_flags` recoit un jeu de drapeaux decrivant le tick ecoule ; a ce jalon, seul
/// le bit 0 est defini : il vaut 1 si un hook a depasse sa deadline (R-707).
///
/// # Safety
///
/// `out_flags` doit pointer sur un `u64` inscriptible, ou etre nul si l'appelant ne
/// veut pas les drapeaux.
#[no_mangle]
pub unsafe extern "C" fn rfx_tick_end(handle: u64, out_flags: *mut u64) -> i32 {
    guard_with_handle(handle, || {
        let mut flags = 0_u64;
        let code = timed_tick_op(handle, |rt| {
            rt.tick_end(Instant::now());
            flags = u64::from(rt.tick_window().metrics().hook_budget_exceeded > 0);
            OK
        });
        if code != OK {
            return code;
        }
        if !out_flags.is_null() {
            // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
            unsafe { out_flags.write(flags) };
        }
        OK
    })
}

// ---------------------------------------------------------------------------
// IF-03 : flux de profilage
// ---------------------------------------------------------------------------

/// Acquiert le tampon de profilage d'un thread (IF-03).
///
/// Le tampon est alloue au premier appel puis reutilise. Il appartient au natif :
/// Java y ecrit et ne le libere jamais (R-708). `out_addr` recoit son adresse et
/// `out_cap` sa capacite en octets.
///
/// Un refus — budget memoire atteint, ou trop de threads deja suivis — renvoie une
/// capacite nulle et une adresse nulle plutot qu'une erreur : ce thread ne sera pas
/// sonde, ce qui degrade la mesure sans jamais gener le jeu.
///
/// # Safety
///
/// `out_addr` doit pointer sur un pointeur inscriptible et `out_cap` sur un `usize`
/// inscriptible ; les deux doivent rester valides pendant l'appel.
#[no_mangle]
pub unsafe extern "C" fn rfx_probe_buffer_acquire(
    handle: u64,
    thread_id: i32,
    out_addr: *mut *mut u8,
    out_cap: *mut usize,
) -> i32 {
    guard_with_handle(handle, || {
        if out_addr.is_null() || out_cap.is_null() {
            return ErrorCode::InvalidArgument.ffi_code();
        }
        let acquired = match runtime::with(handle, |rt| rt.acquire_probe_buffer(thread_id)) {
            Ok(a) => a,
            Err(e) => return e.ffi_code(),
        };
        let (address, capacity) = acquired.unwrap_or((std::ptr::null_mut(), 0));
        // SAFETY : preconditions de la fonction, non nuls verifies ci-dessus.
        unsafe {
            out_addr.write(address);
            out_cap.write(capacity);
        }
        OK
    })
}

/// Consomme les enregistrements ecrits par un thread (IF-03).
///
/// `used` est le nombre d'octets que Java a ecrits depuis le debut du tampon. Un
/// `used` superieur a la capacite signale une saturation : les enregistrements qui
/// n'ont pas tenu sont comptes perdus (R-709). Cet appel n'alloue pas et ne bloque
/// pas.
#[no_mangle]
pub extern "C" fn rfx_probe_buffer_flush(handle: u64, thread_id: i32, used: usize) -> i32 {
    guard_with_handle(handle, || {
        match runtime::with(handle, |rt| rt.flush_probe_buffer(thread_id, used)) {
            Ok(_) => OK,
            Err(e) => e.ffi_code(),
        }
    })
}

/// Verifie le confinement des panics : declenche volontairement une panic.
///
/// Sert la commande `/rfx panic-test` (C-38), disponible **uniquement** en mode
/// `debug` (PARTIE 5.36). Dans tout autre mode, la fonction refuse d'agir.
///
/// Le succes de cette fonction est un echec : elle ne doit jamais renvoyer `OK`. Un
/// appel en mode `debug` renvoie toujours `E-3001`, preuve que la panic a ete
/// capturee a la frontiere et comptabilisee.
#[no_mangle]
pub extern "C" fn rfx_panic_test(handle: u64) -> i32 {
    guard_with_handle(handle, || {
        match runtime::with(handle, |rt| rt.config().mode) {
            Ok(RuntimeMode::Debug) => panic!("panic volontaire : verification du confinement FFI"),
            Ok(_) => ErrorCode::InvalidArgument.ffi_code(),
            Err(e) => e.ffi_code(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfx_model::RuntimeStatus;
    use std::sync::{Mutex, MutexGuard};

    /// Les tests partagent l'instance unique du processus : ils sont serialises.
    fn test_lock() -> MutexGuard<'static, ()> {
        static V: Mutex<()> = Mutex::new(());
        V.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn init(config: &RuntimeConfig) -> u64 {
        let blob = rfx_model::to_cbor(config).expect("encodage");
        let mut handle = 0_u64;
        let code = unsafe { rfx_init(blob.as_ptr(), blob.len(), &mut handle) };
        assert_eq!(code, OK, "rfx_init a echoue");
        handle
    }

    fn read_status(handle: u64) -> RuntimeStatus {
        let mut size = 0_usize;
        assert_eq!(
            unsafe { rfx_status(handle, std::ptr::null_mut(), 0, &mut size) },
            OK
        );
        let mut buffer = vec![0_u8; size];
        let mut written = 0_usize;
        assert_eq!(
            unsafe { rfx_status(handle, buffer.as_mut_ptr(), buffer.len(), &mut written) },
            OK
        );
        rfx_model::from_cbor(&buffer[..written]).expect("statut decodable")
    }

    #[test]
    fn abi_is_announced_before_any_initialization() {
        assert_eq!(rfx_abi_version(), 1);
    }

    #[test]
    fn nominal_life_cycle() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());
        assert_eq!(rfx_noop(handle), OK);
        assert_eq!(rfx_hw_probe(handle), OK);
        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// R-520 / T-363 : double initialisation refusee avec `E-1004`.
    #[test]
    fn double_initialization_is_refused() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());
        let blob = rfx_model::to_cbor(&RuntimeConfig::default()).expect("encodage");
        let mut second = 0_u64;
        assert_eq!(
            unsafe { rfx_init(blob.as_ptr(), blob.len(), &mut second) },
            ErrorCode::DoubleInit.ffi_code()
        );
        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// R-521 / T-364 : tout handle inconnu est rejete, sans effet de bord.
    #[test]
    fn invalid_handle_is_rejected_by_every_entry_point() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());
        let fake = handle ^ 0xdead;
        let expected = ErrorCode::InvalidArgument.ffi_code();

        assert_eq!(rfx_noop(fake), expected);
        assert_eq!(rfx_hw_probe(fake), expected);
        assert_eq!(rfx_hw_set_ffi_costs(fake, 1, 1), expected);
        assert_eq!(rfx_shutdown(fake), expected);
        let mut size = 0_usize;
        assert_eq!(
            unsafe { rfx_status(fake, std::ptr::null_mut(), 0, &mut size) },
            expected
        );

        assert_eq!(rfx_shutdown(handle), OK, "l'instance doit avoir survecu");
    }

    /// PARTIE 19.2 : les donnees venant de Java sont traitees comme non fiables.
    #[test]
    fn aberrant_arguments_are_rejected_without_panicking() {
        let _guard = test_lock();
        let expected = ErrorCode::InvalidArgument.ffi_code();
        let mut handle = 0_u64;

        assert_eq!(
            unsafe { rfx_init(std::ptr::null(), 10, &mut handle) },
            expected
        );
        let blob = rfx_model::to_cbor(&RuntimeConfig::default()).expect("encodage");
        assert_eq!(
            unsafe { rfx_init(blob.as_ptr(), 0, &mut handle) },
            expected,
            "longueur nulle"
        );
        assert_eq!(
            unsafe { rfx_init(blob.as_ptr(), MAX_CONFIG_SIZE + 1, &mut handle) },
            expected,
            "longueur aberrante"
        );
        assert_eq!(
            unsafe { rfx_init(blob.as_ptr(), blob.len(), std::ptr::null_mut()) },
            expected,
            "handle de sortie nul"
        );
        assert_eq!(handle, 0, "aucun handle ne doit avoir ete publie");
    }

    #[test]
    fn an_unreadable_configuration_blob_is_refused() {
        let _guard = test_lock();
        let garbage = [0xff_u8; 32];
        let mut handle = 0_u64;
        assert_eq!(
            unsafe { rfx_init(garbage.as_ptr(), garbage.len(), &mut handle) },
            ErrorCode::InvalidArgument.ffi_code()
        );
    }

    #[test]
    fn an_unknown_configuration_schema_is_refused() {
        let _guard = test_lock();
        let config = RuntimeConfig {
            schema: rfx_model::MODEL_SCHEMA_VERSION + 1,
            ..RuntimeConfig::default()
        };
        let blob = rfx_model::to_cbor(&config).expect("encodage");
        let mut handle = 0_u64;
        assert_eq!(
            unsafe { rfx_init(blob.as_ptr(), blob.len(), &mut handle) },
            ErrorCode::InvalidArgument.ffi_code()
        );
    }

    #[test]
    fn status_exposes_abi_state_and_hardware() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());
        assert_eq!(rfx_hw_probe(handle), OK);
        assert_eq!(rfx_hw_set_ffi_costs(handle, 250, 40), OK);

        let status = read_status(handle);
        assert_eq!(status.abi_version, 1);
        assert_eq!(status.state, "RUNNING");
        assert_eq!(status.panics, 0);
        assert!(status.hardware.logical_cores > 0);
        assert_eq!(status.hardware.jni_call_ns, 250);
        assert_eq!(status.hardware.ffi_batch_ns_per_kb, 40);
        assert!(status.probe_coverage.ffi_call);
        assert!(status.components.iter().any(|c| c.id == "C-45"));

        assert_eq!(rfx_shutdown(handle), OK);
    }

    #[test]
    fn a_status_buffer_too_small_is_refused_and_announces_the_size() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());
        let mut required = 0_usize;
        assert_eq!(
            unsafe { rfx_status(handle, std::ptr::null_mut(), 0, &mut required) },
            OK
        );
        assert!(required > 0);

        let mut too_small = vec![0_u8; required - 1];
        let mut announced = 0_usize;
        assert_eq!(
            unsafe {
                rfx_status(
                    handle,
                    too_small.as_mut_ptr(),
                    too_small.len(),
                    &mut announced,
                )
            },
            ErrorCode::InvalidArgument.ffi_code()
        );
        assert_eq!(announced, required, "la taille requise doit etre publiee");
        assert_eq!(rfx_shutdown(handle), OK);
    }

    #[test]
    fn the_transfer_probe_reads_every_byte() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());
        let data = vec![7_u8; 4096];
        let mut checksum = 0_u64;
        assert_eq!(
            unsafe { rfx_transfer_probe(handle, data.as_ptr(), data.len(), &mut checksum) },
            OK
        );
        assert_ne!(checksum, 0, "la somme doit dependre du contenu lu");

        let mut empty_checksum = 0_u64;
        assert_eq!(
            unsafe { rfx_transfer_probe(handle, data.as_ptr(), 0, &mut empty_checksum) },
            OK
        );
        assert_eq!(empty_checksum, 0);
        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// T-360 : une panic est capturee a la frontiere FFI et ne traverse jamais
    /// vers la JVM (R-522, contrat agent 3.8).
    #[test]
    fn a_panic_is_caught_and_counted() {
        let _guard = test_lock();
        let config = RuntimeConfig {
            mode: RuntimeMode::Debug,
            ..RuntimeConfig::default()
        };
        let handle = init(&config);

        // Le hook de panic par defaut ecrirait la trace sur stderr : on le neutralise
        // le temps du test pour que la sortie reste lisible.
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let code = rfx_panic_test(handle);
        std::panic::set_hook(hook);

        assert_eq!(code, ErrorCode::PanicCaught.ffi_code());
        let status = read_status(handle);
        assert_eq!(status.panics, 1, "la panic doit etre comptabilisee");
        assert_eq!(
            status.state, "RUNNING",
            "une seule panic n'arrete pas le runtime"
        );

        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// `/rfx panic-test` n'est disponible qu'en mode `debug` (PARTIE 5.36).
    #[test]
    fn the_panic_test_is_refused_outside_debug_mode() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());
        assert_eq!(
            rfx_panic_test(handle),
            ErrorCode::InvalidArgument.ffi_code()
        );
        assert_eq!(read_status(handle).panics, 0);
        assert_eq!(rfx_shutdown(handle), OK);
    }
}
