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
use rfx_core::{hw, runtime, ABI_VERSION};
use rfx_model::{RuntimeConfig, RuntimeMode};

/// Taille maximale acceptee pour le blob de configuration, en octets.
///
/// La configuration complete de la PARTIE 28 pese quelques kibioctets. Ce plafond
/// protege contre une longueur aberrante transmise par erreur (PARTIE 19.2).
const TAILLE_CONFIG_MAX: usize = 1 << 20;

/// Taille maximale acceptee pour un tampon de mesure de transfert, en octets.
const TAILLE_TRANSFERT_MAX: usize = 64 << 20;

/// Enveloppe un point d'entree sans handle : capture toute panic (R-522).
fn garde(f: impl FnOnce() -> i32) -> i32 {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(code) => code,
        Err(_) => ErrorCode::PanicCapturee.code_ffi(),
    }
}

/// Enveloppe un point d'entree disposant d'un handle : capture toute panic et la
/// comptabilise dans le runtime concerne (R-523).
///
/// Le comptage est lui-meme protege : si l'enregistrement echoue (handle deja
/// invalide, runtime detruit), la panic reste capturee et le code `E-3001` est
/// renvoye. Aucune panic ne peut donc atteindre la JVM.
fn garde_avec_handle(handle: u64, f: impl FnOnce() -> i32) -> i32 {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(code) => code,
        Err(_) => {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                let _ = runtime::avec(handle, |rt| rt.enregistrer_panic(Instant::now()));
            }));
            ErrorCode::PanicCapturee.code_ffi()
        }
    }
}

/// Version de l'ABI implementee par ce binaire (IF-01, R-702).
///
/// Renvoie la version, toujours positive. Cette fonction ne peut pas echouer et ne
/// touche aucun etat : elle est appelable avant [`rfx_init`].
#[no_mangle]
pub extern "C" fn rfx_abi_version() -> i32 {
    garde(|| i32::try_from(ABI_VERSION).unwrap_or(i32::MAX))
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
    garde(|| {
        if config_cbor.is_null() || out_handle.is_null() || len == 0 || len > TAILLE_CONFIG_MAX {
            return ErrorCode::ArgumentInvalide.code_ffi();
        }
        // SAFETY : precondition de la fonction, verifiee ci-dessus pour le cas nul.
        let octets = unsafe { std::slice::from_raw_parts(config_cbor, len) };

        let Ok(config) = rfx_model::from_cbor::<RuntimeConfig>(octets) else {
            return ErrorCode::ArgumentInvalide.code_ffi();
        };
        if config.verifier_schema().is_err() {
            return ErrorCode::ArgumentInvalide.code_ffi();
        }

        match runtime::initialiser(config) {
            Ok(handle) => {
                // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
                unsafe { out_handle.write(handle) };
                OK
            }
            Err(e) => e.code_ffi(),
        }
    })
}

/// Detruit l'instance du runtime. Le handle devient definitivement invalide.
#[no_mangle]
pub extern "C" fn rfx_shutdown(handle: u64) -> i32 {
    garde_avec_handle(handle, || match runtime::arreter(handle) {
        Ok(()) => OK,
        Err(e) => e.code_ffi(),
    })
}

/// Point d'entree sans effet, servant a mesurer le cout d'un aller-retour FFI.
///
/// C-45 mesure ce cout **depuis Java**, sur 10 000 appels, car il inclut le trajet
/// complet depuis la JVM. Cette fonction fait donc le strict minimum : valider le
/// handle. Le resultat est publie via [`rfx_hw_set_ffi_costs`].
#[no_mangle]
pub extern "C" fn rfx_noop(handle: u64) -> i32 {
    garde_avec_handle(handle, || match runtime::avec(handle, |_| ()) {
        Ok(()) => OK,
        Err(e) => e.code_ffi(),
    })
}

/// Declenche la sonde materielle native (C-45) et memorise son resultat.
///
/// Peut etre rappelee pour re-sonder la machine (R-661).
#[no_mangle]
pub extern "C" fn rfx_hw_probe(handle: u64) -> i32 {
    garde_avec_handle(handle, || {
        let (materiel, couverture) = hw::sonder();
        match runtime::avec(handle, |rt| rt.definir_materiel(materiel, couverture)) {
            Ok(()) => OK,
            Err(e) => e.code_ffi(),
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
    garde_avec_handle(handle, || {
        match runtime::avec(handle, |rt| {
            rt.definir_couts_ffi(jni_call_ns, ffi_batch_ns_per_kb);
        }) {
            Ok(()) => OK,
            Err(e) => e.code_ffi(),
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
/// `donnees` doit pointer sur au moins `len` octets lisibles, et `out_checksum` sur
/// un `u64` inscriptible.
#[no_mangle]
pub unsafe extern "C" fn rfx_transfer_probe(
    handle: u64,
    donnees: *const u8,
    len: usize,
    out_checksum: *mut u64,
) -> i32 {
    garde_avec_handle(handle, || {
        if donnees.is_null() || out_checksum.is_null() || len > TAILLE_TRANSFERT_MAX {
            return ErrorCode::ArgumentInvalide.code_ffi();
        }
        if let Err(e) = runtime::avec(handle, |_| ()) {
            return e.code_ffi();
        }
        // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
        let octets = unsafe { std::slice::from_raw_parts(donnees, len) };
        let somme = octets.iter().fold(0_u64, |acc, o| {
            acc.wrapping_mul(31).wrapping_add(u64::from(*o))
        });
        // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
        unsafe { out_checksum.write(somme) };
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
    garde_avec_handle(handle, || {
        if out_len.is_null() || (out.is_null() && cap > 0) {
            return ErrorCode::ArgumentInvalide.code_ffi();
        }

        let statut = match runtime::avec(handle, |rt| rt.statut()) {
            Ok(s) => s,
            Err(e) => return e.code_ffi(),
        };
        let Ok(blob) = rfx_model::to_cbor(&statut) else {
            return ErrorCode::ArgumentInvalide.code_ffi();
        };

        // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
        unsafe { out_len.write(blob.len()) };
        if cap == 0 {
            return OK;
        }
        if cap < blob.len() {
            return ErrorCode::ArgumentInvalide.code_ffi();
        }
        // SAFETY : `out` possede au moins `cap` octets inscriptibles et `cap` est
        // superieur ou egal a la longueur copiee ; les zones ne se recouvrent pas,
        // `blob` etant une allocation locale.
        unsafe { std::ptr::copy_nonoverlapping(blob.as_ptr(), out, blob.len()) };
        OK
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
    garde_avec_handle(handle, || {
        match runtime::avec(handle, |rt| rt.config().mode) {
            Ok(RuntimeMode::Debug) => panic!("panic volontaire : verification du confinement FFI"),
            Ok(_) => ErrorCode::ArgumentInvalide.code_ffi(),
            Err(e) => e.code_ffi(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfx_model::RuntimeStatus;
    use std::sync::{Mutex, MutexGuard};

    /// Les tests partagent l'instance unique du processus : ils sont serialises.
    fn verrou() -> MutexGuard<'static, ()> {
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

    fn lire_statut(handle: u64) -> RuntimeStatus {
        let mut taille = 0_usize;
        assert_eq!(
            unsafe { rfx_status(handle, std::ptr::null_mut(), 0, &mut taille) },
            OK
        );
        let mut tampon = vec![0_u8; taille];
        let mut ecrit = 0_usize;
        assert_eq!(
            unsafe { rfx_status(handle, tampon.as_mut_ptr(), tampon.len(), &mut ecrit) },
            OK
        );
        rfx_model::from_cbor(&tampon[..ecrit]).expect("statut decodable")
    }

    #[test]
    fn l_abi_est_annoncee_avant_toute_initialisation() {
        assert_eq!(rfx_abi_version(), 1);
    }

    #[test]
    fn cycle_de_vie_nominal() {
        let _v = verrou();
        let handle = init(&RuntimeConfig::default());
        assert_eq!(rfx_noop(handle), OK);
        assert_eq!(rfx_hw_probe(handle), OK);
        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// R-520 / T-363 : double initialisation refusee avec `E-1004`.
    #[test]
    fn double_initialisation_refusee() {
        let _v = verrou();
        let handle = init(&RuntimeConfig::default());
        let blob = rfx_model::to_cbor(&RuntimeConfig::default()).expect("encodage");
        let mut second = 0_u64;
        assert_eq!(
            unsafe { rfx_init(blob.as_ptr(), blob.len(), &mut second) },
            ErrorCode::DoubleInit.code_ffi()
        );
        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// R-521 / T-364 : tout handle inconnu est rejete, sans effet de bord.
    #[test]
    fn handle_invalide_rejete_par_chaque_point_d_entree() {
        let _v = verrou();
        let handle = init(&RuntimeConfig::default());
        let faux = handle ^ 0xdead;
        let attendu = ErrorCode::ArgumentInvalide.code_ffi();

        assert_eq!(rfx_noop(faux), attendu);
        assert_eq!(rfx_hw_probe(faux), attendu);
        assert_eq!(rfx_hw_set_ffi_costs(faux, 1, 1), attendu);
        assert_eq!(rfx_shutdown(faux), attendu);
        let mut taille = 0_usize;
        assert_eq!(
            unsafe { rfx_status(faux, std::ptr::null_mut(), 0, &mut taille) },
            attendu
        );

        assert_eq!(rfx_shutdown(handle), OK, "l'instance doit avoir survecu");
    }

    /// PARTIE 19.2 : les donnees venant de Java sont traitees comme non fiables.
    #[test]
    fn les_arguments_aberrants_sont_rejetes_sans_panique() {
        let _v = verrou();
        let attendu = ErrorCode::ArgumentInvalide.code_ffi();
        let mut handle = 0_u64;

        assert_eq!(
            unsafe { rfx_init(std::ptr::null(), 10, &mut handle) },
            attendu
        );
        let blob = rfx_model::to_cbor(&RuntimeConfig::default()).expect("encodage");
        assert_eq!(
            unsafe { rfx_init(blob.as_ptr(), 0, &mut handle) },
            attendu,
            "longueur nulle"
        );
        assert_eq!(
            unsafe { rfx_init(blob.as_ptr(), TAILLE_CONFIG_MAX + 1, &mut handle) },
            attendu,
            "longueur aberrante"
        );
        assert_eq!(
            unsafe { rfx_init(blob.as_ptr(), blob.len(), std::ptr::null_mut()) },
            attendu,
            "handle de sortie nul"
        );
        assert_eq!(handle, 0, "aucun handle ne doit avoir ete publie");
    }

    #[test]
    fn un_blob_de_configuration_illisible_est_refuse() {
        let _v = verrou();
        let ordures = [0xff_u8; 32];
        let mut handle = 0_u64;
        assert_eq!(
            unsafe { rfx_init(ordures.as_ptr(), ordures.len(), &mut handle) },
            ErrorCode::ArgumentInvalide.code_ffi()
        );
    }

    #[test]
    fn un_schema_de_configuration_inconnu_est_refuse() {
        let _v = verrou();
        let config = RuntimeConfig {
            schema: rfx_model::MODEL_SCHEMA_VERSION + 1,
            ..RuntimeConfig::default()
        };
        let blob = rfx_model::to_cbor(&config).expect("encodage");
        let mut handle = 0_u64;
        assert_eq!(
            unsafe { rfx_init(blob.as_ptr(), blob.len(), &mut handle) },
            ErrorCode::ArgumentInvalide.code_ffi()
        );
    }

    #[test]
    fn le_statut_expose_l_abi_l_etat_et_le_materiel() {
        let _v = verrou();
        let handle = init(&RuntimeConfig::default());
        assert_eq!(rfx_hw_probe(handle), OK);
        assert_eq!(rfx_hw_set_ffi_costs(handle, 250, 40), OK);

        let statut = lire_statut(handle);
        assert_eq!(statut.abi_version, 1);
        assert_eq!(statut.etat, "RUNNING");
        assert_eq!(statut.panics, 0);
        assert!(statut.materiel.logical_cores > 0);
        assert_eq!(statut.materiel.jni_call_ns, 250);
        assert_eq!(statut.materiel.ffi_batch_ns_per_kb, 40);
        assert!(statut.couverture_sonde.ffi_call);
        assert!(statut.composants.iter().any(|c| c.id == "C-45"));

        assert_eq!(rfx_shutdown(handle), OK);
    }

    #[test]
    fn un_tampon_de_statut_trop_petit_est_refuse_et_annonce_la_taille() {
        let _v = verrou();
        let handle = init(&RuntimeConfig::default());
        let mut requis = 0_usize;
        assert_eq!(
            unsafe { rfx_status(handle, std::ptr::null_mut(), 0, &mut requis) },
            OK
        );
        assert!(requis > 0);

        let mut trop_petit = vec![0_u8; requis - 1];
        let mut annonce = 0_usize;
        assert_eq!(
            unsafe {
                rfx_status(
                    handle,
                    trop_petit.as_mut_ptr(),
                    trop_petit.len(),
                    &mut annonce,
                )
            },
            ErrorCode::ArgumentInvalide.code_ffi()
        );
        assert_eq!(annonce, requis, "la taille requise doit etre publiee");
        assert_eq!(rfx_shutdown(handle), OK);
    }

    #[test]
    fn la_mesure_de_transfert_lit_tous_les_octets() {
        let _v = verrou();
        let handle = init(&RuntimeConfig::default());
        let donnees = vec![7_u8; 4096];
        let mut somme = 0_u64;
        assert_eq!(
            unsafe { rfx_transfer_probe(handle, donnees.as_ptr(), donnees.len(), &mut somme) },
            OK
        );
        assert_ne!(somme, 0, "la somme doit dependre du contenu lu");

        let mut somme_vide = 0_u64;
        assert_eq!(
            unsafe { rfx_transfer_probe(handle, donnees.as_ptr(), 0, &mut somme_vide) },
            OK
        );
        assert_eq!(somme_vide, 0);
        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// T-360 : une panic est capturee a la frontiere FFI et ne traverse jamais
    /// vers la JVM (R-522, contrat agent 3.8).
    #[test]
    fn une_panic_est_capturee_et_comptabilisee() {
        let _v = verrou();
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

        assert_eq!(code, ErrorCode::PanicCapturee.code_ffi());
        let statut = lire_statut(handle);
        assert_eq!(statut.panics, 1, "la panic doit etre comptabilisee");
        assert_eq!(
            statut.etat, "RUNNING",
            "une seule panic n'arrete pas le runtime"
        );

        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// `/rfx panic-test` n'est disponible qu'en mode `debug` (PARTIE 5.36).
    #[test]
    fn le_test_de_panic_est_refuse_hors_du_mode_debug() {
        let _v = verrou();
        let handle = init(&RuntimeConfig::default());
        assert_eq!(
            rfx_panic_test(handle),
            ErrorCode::ArgumentInvalide.code_ffi()
        );
        assert_eq!(lire_statut(handle).panics, 0);
        assert_eq!(rfx_shutdown(handle), OK);
    }
}
