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

use jni::objects::{JByteArray, JByteBuffer, JClass, JObject};
use jni::sys::{jbyteArray, jint, jlong};
use jni::JNIEnv;

use rfx_core::error::{ErrorCode, OK};

/// Convertit un `i32` de l'ABI en `jint`.
fn to_jint(v: i32) -> jint {
    v
}

/// `RfxNative.abiVersion()` : version d'ABI du binaire charge (R-702).
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_abiVersion(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    to_jint(crate::rfx_abi_version())
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
    let Ok(bytes) = env.convert_byte_array(&config_cbor) else {
        return jlong::from(ErrorCode::InvalidArgument.ffi_code());
    };
    let mut handle: u64 = 0;
    // SAFETY : `bytes` est un `Vec<u8>` vivant, de longueur exacte, et `handle` une
    // variable locale ; les deux restent valides pendant tout l'appel.
    let result = unsafe { crate::rfx_init(bytes.as_ptr(), bytes.len(), &mut handle) };
    if result != OK {
        return jlong::from(result);
    }
    // Le motif de handle garantit une valeur positive lue comme `jlong`.
    jlong::try_from(handle).unwrap_or_else(|_| jlong::from(ErrorCode::InvalidArgument.ffi_code()))
}

/// `RfxNative.shutdown(long)`.
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_shutdown(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    to_jint(crate::rfx_shutdown(handle as u64))
}

/// `RfxNative.noop(long)` : cible d'appel pour la calibration du cout FFI (C-45).
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_noop(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    to_jint(crate::rfx_noop(handle as u64))
}

/// `RfxNative.hwProbe(long)` : declenche ou rejoue la sonde materielle (R-661).
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_hwProbe(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    to_jint(crate::rfx_hw_probe(handle as u64))
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
    let call_cost = u32::try_from(jni_call_ns).unwrap_or(0);
    let batch_cost = u32::try_from(ffi_batch_ns_per_kb).unwrap_or(0);
    to_jint(crate::rfx_hw_set_ffi_costs(
        handle as u64,
        call_cost,
        batch_cost,
    ))
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
    buffer: JByteBuffer,
    length: jint,
) -> jlong {
    let invalid = jlong::from(ErrorCode::InvalidArgument.ffi_code());

    let Ok(address) = env.get_direct_buffer_address(&buffer) else {
        return invalid;
    };
    let Ok(capacity) = env.get_direct_buffer_capacity(&buffer) else {
        return invalid;
    };
    let Ok(length) = usize::try_from(length) else {
        return invalid;
    };
    if address.is_null() || length > capacity {
        return invalid;
    }

    let mut checksum: u64 = 0;
    // SAFETY : `address` et `capacity` proviennent de la JVM pour ce tampon direct,
    // `length` a ete bornee par `capacity` ci-dessus, et le tampon reste reference
    // par l'appelant Java pendant tout l'appel.
    let result =
        unsafe { crate::rfx_transfer_probe(handle as u64, address, length, &mut checksum) };
    if result != OK {
        return jlong::from(result);
    }
    // Le bit de poids fort est efface pour que le temoin reste toujours positif :
    // l'appelant distingue un succes d'un code d'erreur par le signe, et une somme
    // depassant `i64::MAX` serait autrement lue comme une erreur. Le temoin n'a
    // d'autre role que d'empecher l'elimination de la lecture par l'optimiseur, sa
    // valeur exacte n'est jamais interpretee.
    (checksum & 0x7fff_ffff_ffff_ffff) as jlong
}

/// Nombre de tentatives pour lire un blob dont la taille peut changer.
///
/// Le protocole en deux temps — demander la taille, puis copier — n'est pas atomique :
/// entre les deux appels, un fil de mod peut deverser ses enregistrements de sonde et
/// faire changer ce qu'il y a a lire. Le second appel refuse alors un tampon devenu
/// trop petit, et l'appelant Java croit le runtime muet alors qu'il repond.
///
/// Trois tentatives suffisent largement : chacune part de la taille que le refus vient
/// de rapporter, et la taille ne varie que de quelques dizaines d'octets d'un tick a
/// l'autre. Boucler sans borne serait pire — une commande de diagnostic ne doit jamais
/// retenir le fil serveur.
const READ_ATTEMPTS: usize = 3;

/// Lit un blob CBOR par le protocole en deux temps, en reessayant si la taille change.
///
/// `read` recoit `(tampon, capacite, &mut taille)` et suit la convention des fonctions
/// exportees : capacite nulle pour interroger la taille, sinon copie. Il ecrit la
/// taille requise dans tous les cas, y compris quand il refuse — c'est ce qui permet a
/// la tentative suivante de partir de la bonne valeur.
fn read_sized_blob(mut read: impl FnMut(*mut u8, usize, &mut usize) -> i32) -> Option<Vec<u8>> {
    let mut size: usize = 0;
    if read(std::ptr::null_mut(), 0, &mut size) != OK || size == 0 {
        return None;
    }

    for _ in 0..READ_ATTEMPTS {
        let mut buffer = vec![0_u8; size];
        let mut written: usize = 0;
        let capacity = buffer.len();
        if read(buffer.as_mut_ptr(), capacity, &mut written) == OK {
            buffer.truncate(written);
            return Some(buffer);
        }
        if written == 0 || written == size {
            // La taille n'a pas change : le refus vient d'autre chose qu'une course,
            // et reessayer a l'identique ne ferait que perdre du temps.
            return None;
        }
        size = written;
    }
    None
}

/// `RfxNative.status(long)` : blob CBOR de statut, ou `null` en cas d'erreur.
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_status(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jbyteArray {
    let null_array: jbyteArray = std::ptr::null_mut();

    // SAFETY : `out` est soit nul avec une capacite nulle — forme explicitement admise
    // par `rfx_status` pour interroger la taille — soit un tampon local possedant
    // exactement `cap` octets inscriptibles et vivant au-dela de l'appel. `len` est
    // une locale de `read_sized_blob`.
    let Some(buffer) =
        read_sized_blob(|out, cap, len| unsafe { crate::rfx_status(handle as u64, out, cap, len) })
    else {
        return null_array;
    };

    match env.byte_array_from_slice(&buffer) {
        Ok(array) => array.into_raw(),
        Err(_) => null_array,
    }
}

/// `RfxNative.profilerRequestDepth(long, int, int)` : ouvre une session de diagnostic.
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_profilerRequestDepth(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    level_code: jint,
    ticks: jint,
) -> jint {
    let level = u8::try_from(level_code).unwrap_or(u8::MAX);
    let ticks = u64::try_from(ticks).unwrap_or(0);
    to_jint(crate::rfx_profiler_request_depth(handle as u64, level, ticks))
}

/// `RfxNative.profilerTop(long, int)` : classement CBOR, ou `null` en cas d'erreur.
///
/// Meme forme que `status` : une interrogation de taille, puis une copie. Le
/// classement n'est demande que par une commande d'operateur, jamais dans un tick.
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_profilerTop(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    limit: jint,
) -> jbyteArray {
    let null_array: jbyteArray = std::ptr::null_mut();
    // Une limite negative n'a pas de sens ; la ramener a zero rend un classement vide
    // mais renseigne quand meme les totaux, ce qui reste une reponse honnete.
    let limit = u32::try_from(limit).unwrap_or(0);

    // SAFETY : memes preconditions que pour `status` ci-dessus ; `read_sized_blob`
    // n'appelle jamais avec un pointeur nul et une capacite non nulle.
    let Some(buffer) = read_sized_blob(|out, cap, len| unsafe {
        crate::rfx_profiler_top(handle as u64, limit, out, cap, len)
    }) else {
        return null_array;
    };

    match env.byte_array_from_slice(&buffer) {
        Ok(array) => array.into_raw(),
        Err(_) => null_array,
    }
}

/// `RfxNative.workloadRegister(long, byte[])` : enregistre une unite de travail.
///
/// Renvoie l'identifiant de sonde, positif ou nul ; `-1` si l'unite ne sera pas
/// sondee (plafond atteint) ; un code d'erreur de l'annexe A.2, donc inferieur ou
/// egal a `-1000`, en cas d'echec. Les trois domaines sont disjoints : un identifiant
/// de sonde est borne par `profiler.max_workloads`, tres en deca de mille.
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_workloadRegister(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    descriptor_cbor: JByteArray,
) -> jint {
    let Ok(bytes) = env.convert_byte_array(&descriptor_cbor) else {
        return to_jint(ErrorCode::InvalidArgument.ffi_code());
    };
    let mut probe_id: u32 = crate::RFX_PROBE_ID_NONE;
    // SAFETY : `bytes` est un `Vec<u8>` vivant de longueur exacte, et `probe_id` une
    // variable locale ; les deux restent valides pendant tout l'appel.
    let result = unsafe {
        crate::rfx_workload_register(handle as u64, bytes.as_ptr(), bytes.len(), &mut probe_id)
    };
    if result != OK {
        return to_jint(result);
    }
    if probe_id == crate::RFX_PROBE_ID_NONE {
        return -1;
    }
    jint::try_from(probe_id).unwrap_or(-1)
}

/// `RfxNative.probeLevels(long)` : table des niveaux, ou `null` si elle n'a pas change.
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_probeLevels(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jbyteArray {
    let null_array: jbyteArray = std::ptr::null_mut();

    let mut size: usize = 0;
    // SAFETY : tampon nul avec capacite nulle, forme explicitement admise par
    // `rfx_probe_levels` pour interroger la taille requise ; `size` est une locale.
    let result =
        unsafe { crate::rfx_probe_levels(handle as u64, std::ptr::null_mut(), 0, &mut size) };
    if result != OK || size == 0 {
        return null_array;
    }

    let mut buffer = vec![0_u8; size];
    let mut written: usize = 0;
    // SAFETY : `buffer` possede exactement `size` octets inscriptibles et vit au-dela
    // de l'appel.
    let result = unsafe {
        crate::rfx_probe_levels(
            handle as u64,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut written,
        )
    };
    if result != OK {
        return null_array;
    }
    buffer.truncate(written);

    match env.byte_array_from_slice(&buffer) {
        Ok(array) => array.into_raw(),
        Err(_) => null_array,
    }
}

/// `RfxNative.profilerStart(long)` : demarre le profilage (C-05).
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_profilerStart(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    to_jint(crate::rfx_profiler_start(handle as u64))
}

/// `RfxNative.tickBegin(long, long, int)` : ouvre la fenetre de tick (IF-02).
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_tickBegin(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tick: jlong,
    side: jint,
) -> jint {
    to_jint(crate::rfx_tick_begin(handle as u64, tick as u64, side))
}

/// `RfxNative.tickPhase(long, int)` : declare une transition de phase (IF-02).
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_tickPhase(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    phase: jint,
) -> jint {
    to_jint(crate::rfx_phase(handle as u64, phase))
}

/// `RfxNative.tickEnd(long)` : ferme la fenetre de tick (IF-02).
///
/// Renvoie les drapeaux du tick ecoule, positifs, ou un code d'erreur negatif.
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_tickEnd(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    let mut flags: u64 = 0;
    // SAFETY : `flags` est une variable locale valide pendant tout l'appel.
    let result = unsafe { crate::rfx_tick_end(handle as u64, &mut flags) };
    if result != OK {
        return jlong::from(result);
    }
    // Les drapeaux tiennent sur quelques bits : la conversion ne peut pas produire
    // une valeur negative qui serait lue comme un code d'erreur.
    jlong::try_from(flags & 0x7fff_ffff_ffff_ffff).unwrap_or(0)
}

/// `RfxNative.probeBufferAcquire(long, int)` : tampon de profilage d'un thread (IF-03).
///
/// Renvoie un `DirectByteBuffer` pointant sur la memoire du natif, ou `null` si ce
/// thread ne peut pas etre sonde. Java ne libere jamais ce tampon (R-708).
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_probeBufferAcquire<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    thread_id: jint,
) -> JObject<'local> {
    let mut address: *mut u8 = std::ptr::null_mut();
    let mut capacity: usize = 0;
    // SAFETY : `address` et `capacity` sont des variables locales valides pendant
    // tout l'appel.
    let result = unsafe {
        crate::rfx_probe_buffer_acquire(handle as u64, thread_id, &mut address, &mut capacity)
    };
    if result != OK || address.is_null() || capacity == 0 {
        return JObject::null();
    }
    // SAFETY : l'adresse et la capacite viennent d'etre obtenues du natif, qui
    // possede ce tampon et le maintient en vie aussi longtemps que le handle. La JVM
    // n'en prend pas la propriete : elle se contente de l'exposer (R-708).
    match unsafe { env.new_direct_byte_buffer(address, capacity) } {
        Ok(buffer) => JObject::from(buffer),
        Err(_) => JObject::null(),
    }
}

/// `RfxNative.probeBufferFlush(long, int, int)` : consomme les enregistrements (IF-03).
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_probeBufferFlush(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    thread_id: jint,
    used: jint,
) -> jint {
    let Ok(used) = usize::try_from(used) else {
        return to_jint(ErrorCode::InvalidArgument.ffi_code());
    };
    to_jint(crate::rfx_probe_buffer_flush(
        handle as u64,
        thread_id,
        used,
    ))
}

/// `RfxNative.panicTest(long)` : verifie le confinement des panics (mode `debug`).
#[no_mangle]
pub extern "system" fn Java_dev_rustforgex_bridge_RfxNative_panicTest(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    to_jint(crate::rfx_panic_test(handle as u64))
}
