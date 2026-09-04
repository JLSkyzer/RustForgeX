//! Adaptateur JNI : expose l'ABI C de ce crate aux methodes `native` de Java.
//!
//! Composant : C-27 (frontiere). Decision : ADR-004 (JNI classique comme mecanisme
//! principal, `DirectByteBuffer` comme mode de transfert par defaut).
//! Classe Java correspondante : `dev.rustforgex.bridge.RfxNative`.
//! Maturite : `STABLE`.
//!
//! Ce module **n'implemente rien** : il traduit les types JNI vers l'ABI C definie
//! dans [`crate`], qui reste la seule implementation. Toute la protection contre les
//! panics, la validation des handles et celle des pointeurs sont donc appliquees une
//! seule fois, dans l'ABI.
//!
//! Convention de retour, identique a l'ABI (R-701) : `0` en cas de succes, un code
//! d'erreur negatif sinon. [`Java_dev_rustforgex_bridge_RfxNative_init`] fait
//! exception et renvoie le handle, toujours strictement positif, ou un code d'erreur
//! negatif.

use jni::objects::{JByteArray, JByteBuffer, JClass};
use jni::sys::{jbyteArray, jint, jlong};
use jni::JNIEnv;

use rfx_core::error::{ErrorCode, OK};

/// Convertit un `i32` de l'ABI en `jint`.
fn code(v: i32) -> jint {
    v
}

/// `RfxNative.abiVersion()` : version d'ABI du binaire charge (R-702).
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_abiVersion(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    code(crate::rfx_abi_version())
}

/// `RfxNative.init(byte[])` : initialise le runtime et renvoie son handle.
///
/// Renvoie le handle (strictement positif) ou un code d'erreur negatif. Le tableau
/// est copie dans un tampon Rust avant usage : la JVM reste libre de deplacer ses
/// objets pendant l'appel.
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_init(
    env: JNIEnv,
    _class: JClass,
    config_cbor: JByteArray,
) -> jlong {
    let Ok(octets) = env.convert_byte_array(&config_cbor) else {
        return jlong::from(ErrorCode::ArgumentInvalide.code_ffi());
    };
    let mut handle: u64 = 0;
    // SAFETY : `octets` est un `Vec<u8>` vivant, de longueur exacte, et `handle` une
    // variable locale ; les deux restent valides pendant tout l'appel.
    let resultat = unsafe { crate::rfx_init(octets.as_ptr(), octets.len(), &mut handle) };
    if resultat != OK {
        return jlong::from(resultat);
    }
    // Le motif de handle garantit une valeur positive lue comme `jlong`.
    jlong::try_from(handle).unwrap_or_else(|_| jlong::from(ErrorCode::ArgumentInvalide.code_ffi()))
}

/// `RfxNative.shutdown(long)`.
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_shutdown(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    code(crate::rfx_shutdown(handle as u64))
}

/// `RfxNative.noop(long)` : cible d'appel pour la calibration du cout FFI (C-45).
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_noop(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    code(crate::rfx_noop(handle as u64))
}

/// `RfxNative.hwProbe(long)` : declenche ou rejoue la sonde materielle (R-661).
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_hwProbe(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    code(crate::rfx_hw_probe(handle as u64))
}

/// `RfxNative.hwSetFfiCosts(long, int, int)` : publie les couts mesures par Java.
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_hwSetFfiCosts(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    jni_call_ns: jint,
    ffi_batch_ns_per_kb: jint,
) -> jint {
    let appel = u32::try_from(jni_call_ns).unwrap_or(0);
    let lot = u32::try_from(ffi_batch_ns_per_kb).unwrap_or(0);
    code(crate::rfx_hw_set_ffi_costs(handle as u64, appel, lot))
}

/// `RfxNative.transferProbe(ByteBuffer, int)` : mesure du debit Java vers natif.
///
/// Le tampon DOIT etre direct (`ByteBuffer.allocateDirect`) : c'est le mode de
/// transfert retenu par ADR-004, sans copie cote natif. Un tampon non direct est
/// refuse.
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_transferProbe(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tampon: JByteBuffer,
    longueur: jint,
) -> jlong {
    let invalide = jlong::from(ErrorCode::ArgumentInvalide.code_ffi());

    let Ok(adresse) = env.get_direct_buffer_address(&tampon) else {
        return invalide;
    };
    let Ok(capacite) = env.get_direct_buffer_capacity(&tampon) else {
        return invalide;
    };
    let Ok(longueur) = usize::try_from(longueur) else {
        return invalide;
    };
    if adresse.is_null() || longueur > capacite {
        return invalide;
    }

    let mut somme: u64 = 0;
    // SAFETY : `adresse` et `capacite` proviennent de la JVM pour ce tampon direct,
    // `longueur` a ete bornee par `capacite` ci-dessus, et le tampon reste reference
    // par l'appelant Java pendant tout l'appel.
    let resultat =
        unsafe { crate::rfx_transfer_probe(handle as u64, adresse, longueur, &mut somme) };
    if resultat != OK {
        return jlong::from(resultat);
    }
    // La somme n'est qu'un temoin de lecture : sa reinterpretation en `jlong` est sans
    // consequence, l'appelant ne l'utilise que pour empecher l'elimination du calcul.
    somme as jlong
}

/// `RfxNative.status(long)` : blob CBOR de statut, ou `null` en cas d'erreur.
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_status(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jbyteArray {
    let nul: jbyteArray = std::ptr::null_mut();

    let mut taille: usize = 0;
    // SAFETY : tampon nul avec capacite nulle, forme explicitement admise par
    // `rfx_status` pour interroger la taille requise ; `taille` est une locale.
    let resultat =
        unsafe { crate::rfx_status(handle as u64, std::ptr::null_mut(), 0, &mut taille) };
    if resultat != OK || taille == 0 {
        return nul;
    }

    let mut tampon = vec![0_u8; taille];
    let mut ecrit: usize = 0;
    // SAFETY : `tampon` possede exactement `taille` octets inscriptibles et vit
    // au-dela de l'appel.
    let resultat =
        unsafe { crate::rfx_status(handle as u64, tampon.as_mut_ptr(), tampon.len(), &mut ecrit) };
    if resultat != OK {
        return nul;
    }
    tampon.truncate(ecrit);

    match env.byte_array_from_slice(&tampon) {
        Ok(tableau) => tableau.into_raw(),
        Err(_) => nul,
    }
}

/// `RfxNative.panicTest(long)` : verifie le confinement des panics (mode `debug`).
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_panicTest(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    code(crate::rfx_panic_test(handle as u64))
}
