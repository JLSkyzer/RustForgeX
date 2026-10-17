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

/// Releve de la charge externe pour l'ordonnanceur (C-17, R-421, ADR-046).
///
/// `runnable_threads` est le nombre de fils tiers qui ont consomme au moins 10 % d'un
/// coeur depuis le releve precedent, compte par Java une fois par seconde sur son propre
/// fil : aucun appel dans le tick (R-700).
#[no_mangle]
pub extern "C" fn rfx_sched_report_external_load(handle: u64, runnable_threads: u32) -> i32 {
    guard_with_handle(handle, || {
        match runtime::with(handle, |rt| rt.report_external_load(runnable_threads)) {
            Ok(()) => OK,
            Err(e) => e.ffi_code(),
        }
    })
}

/// Ouvre la base des unites de travail (C-09, ADR-049).
///
/// `open` est un blob CBOR decrivant un `DbOpen` : le repertoire de la base et ce que
/// Java connait de l'environnement. Le fichier est relu en arriere-plan, sur le fil
/// `RFX-IO` : l'appel rend la main aussitot. Sans effet si `cache.enabled` vaut `false`.
///
/// # Safety
///
/// `open` doit pointer sur au moins `len` octets lisibles.
#[no_mangle]
pub unsafe extern "C" fn rfx_db_open(handle: u64, open: *const u8, len: usize) -> i32 {
    guard_with_handle(handle, || {
        if open.is_null() || len == 0 || len > MAX_CONFIG_SIZE {
            return ErrorCode::InvalidArgument.ffi_code();
        }
        // SAFETY : precondition de la fonction, non nul et longueur verifies ci-dessus.
        let bytes = unsafe { std::slice::from_raw_parts(open, len) };
        let Ok(open) = rfx_model::from_cbor::<rfx_model::DbOpen>(bytes) else {
            return ErrorCode::InvalidArgument.ffi_code();
        };
        match runtime::with(handle, |rt| rt.open_db(&open)) {
            Ok(Ok(())) => OK,
            Ok(Err(e)) | Err(e) => e.ffi_code(),
        }
    })
}

/// Avance d'une tranche la synchronisation de la base (C-09, ADR-049).
///
/// Ecrit dans `out_done` `1` quand le cycle est termine et son lot confie au fil
/// d'ecriture, `0` s'il faut rappeler. Le verrou du runtime est rendu entre deux
/// appels : le tick n'attend jamais plus d'une tranche. A appeler hors du tick
/// (R-361), sur un fil a part.
///
/// # Safety
///
/// `out_done` doit pointer sur un `u32` inscriptible.
#[no_mangle]
pub unsafe extern "C" fn rfx_db_sync(handle: u64, out_done: *mut u32) -> i32 {
    guard_with_handle(handle, || {
        if out_done.is_null() {
            return ErrorCode::InvalidArgument.ffi_code();
        }
        match runtime::with(handle, |rt| rt.db_sync_step()) {
            Ok(done) => {
                // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
                unsafe { out_done.write(u32::from(done)) };
                OK
            }
            Err(e) => e.ffi_code(),
        }
    })
}

/// Execute `f` sur l'analyse, sous son propre verrou, sans tenir celui du runtime : la
/// construction d'un graphe ne doit jamais faire attendre le tick (ADR-051).
fn with_analysis<R>(
    handle: u64,
    f: impl FnOnce(&mut rfx_core::analysis::Analysis) -> R,
) -> Result<R, ErrorCode> {
    let (shared, staged) = runtime::with(handle, |rt| rt.analysis_handoff())?;
    let mut analysis = shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (work_id, method, side) in staged {
        analysis.note(work_id, method, side);
    }
    Ok(f(&mut analysis))
}

/// Declare des classes de la hierarchie de l'installation (C-07, ADR-051).
///
/// `entries` est un blob CBOR : tableau de `[nom, super-classe, drapeaux]`. Appelable
/// plusieurs fois, par lots.
///
/// # Safety
///
/// `entries` doit pointer sur au moins `len` octets lisibles.
#[no_mangle]
pub unsafe extern "C" fn rfx_analysis_hierarchy(
    handle: u64,
    entries: *const u8,
    len: usize,
) -> i32 {
    guard_with_handle(handle, || {
        if entries.is_null() || len == 0 || len > MAX_TRANSFER_SIZE {
            return ErrorCode::InvalidArgument.ffi_code();
        }
        // SAFETY : precondition de la fonction, non nul et longueur verifies ci-dessus.
        let bytes = unsafe { std::slice::from_raw_parts(entries, len) };
        let Ok(entries) = rfx_model::from_cbor::<Vec<rfx_model::HierarchyEntry>>(bytes) else {
            return ErrorCode::InvalidArgument.ffi_code();
        };
        match with_analysis(handle, |analysis| analysis.declare(entries)) {
            Ok(()) => OK,
            Err(e) => e.ffi_code(),
        }
    })
}

/// Fournit le bytecode d'une classe demandee (C-07, ADR-051). `bytes_len == 0` declare
/// la classe absente : son bytecode final est inconnu (cible de mixin) ou illisible.
///
/// # Safety
///
/// `name` doit pointer sur `name_len` octets lisibles en UTF-8 ; `bytes` sur
/// `bytes_len` octets lisibles, ou etre nul si `bytes_len` vaut zero.
#[no_mangle]
pub unsafe extern "C" fn rfx_analysis_class(
    handle: u64,
    name: *const u8,
    name_len: usize,
    bytes: *const u8,
    bytes_len: usize,
) -> i32 {
    guard_with_handle(handle, || {
        if name.is_null()
            || name_len == 0
            || name_len > 4_096
            || (bytes.is_null() && bytes_len > 0)
            || bytes_len > MAX_TRANSFER_SIZE
        {
            return ErrorCode::InvalidArgument.ffi_code();
        }
        // SAFETY : precondition de la fonction, non nul et longueur verifies ci-dessus.
        let name = unsafe { std::slice::from_raw_parts(name, name_len) };
        let Ok(name) = std::str::from_utf8(name) else {
            return ErrorCode::InvalidArgument.ffi_code();
        };
        let class = (bytes_len > 0).then(|| {
            // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
            unsafe { std::slice::from_raw_parts(bytes, bytes_len) }
        });
        match with_analysis(handle, |analysis| analysis.provide(name, class)) {
            Ok(()) => OK,
            Err(e) => e.ffi_code(),
        }
    })
}

/// Avance l'analyse d'un pas (C-07, ADR-051), hors du tick. Ecrit dans `out_state` :
/// 0 rien a faire, 1 du bytecode manque (lire `rfx_analysis_requests`, le fournir, puis
/// rappeler), 2 un graphe vient d'etre termine.
///
/// # Safety
///
/// `out_state` doit pointer sur un `u32` inscriptible.
#[no_mangle]
pub unsafe extern "C" fn rfx_analysis_step(handle: u64, out_state: *mut u32) -> i32 {
    guard_with_handle(handle, || {
        if out_state.is_null() {
            return ErrorCode::InvalidArgument.ffi_code();
        }
        let needs_roots = match with_analysis(handle, |analysis| analysis.needs_roots()) {
            Ok(needs) => needs,
            Err(e) => return e.ffi_code(),
        };
        let hot = if needs_roots {
            match runtime::with(handle, |rt| rt.hot_workloads()) {
                Ok(hot) => hot,
                Err(e) => return e.ffi_code(),
            }
        } else {
            Vec::new()
        };
        let stepped = with_analysis(handle, |analysis| {
            let state = analysis.step(&hot);
            (state, analysis.status(), analysis.estimated_bytes())
        });
        let (state, status, bytes) = match stepped {
            Ok(stepped) => stepped,
            Err(e) => return e.ffi_code(),
        };
        let accepted = match runtime::with(handle, |rt| rt.publish_analysis(status, bytes)) {
            Ok(accepted) => accepted,
            Err(e) => return e.ffi_code(),
        };
        if !accepted {
            // R-533 : le budget refuse la memoire de l'analyse, qui s'arrete et la rend.
            let stopped = with_analysis(handle, |analysis| {
                analysis.disable();
                analysis.status()
            });
            if let Ok(status) = stopped {
                let _ = runtime::with(handle, |rt| rt.publish_analysis(status, 0));
            }
        }
        // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
        unsafe { out_state.write(state as u32) };
        OK
    })
}

/// Classes voulues par le dernier pas (CBOR : tableau de noms internes). Meme protocole
/// en deux temps que [`rfx_status`].
///
/// # Safety
///
/// `out` doit pointer sur au moins `cap` octets inscriptibles (il peut etre nul si
/// `cap` vaut zero), et `out_len` sur un `usize` inscriptible.
#[no_mangle]
pub unsafe extern "C" fn rfx_analysis_requests(
    handle: u64,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> i32 {
    guard_with_handle(handle, || {
        if out_len.is_null() || (out.is_null() && cap > 0) {
            return ErrorCode::InvalidArgument.ffi_code();
        }
        let blob = match with_analysis(handle, |analysis| rfx_model::to_cbor(&analysis.requests()))
        {
            Ok(Ok(blob)) => blob,
            Ok(Err(_)) => return ErrorCode::InvalidArgument.ffi_code(),
            Err(e) => return e.ffi_code(),
        };
        // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
        unsafe { out_len.write(blob.len()) };
        if cap == 0 {
            return OK;
        }
        if cap < blob.len() {
            return ErrorCode::InvalidArgument.ffi_code();
        }
        // SAFETY : `out` possede au moins `cap` octets inscriptibles, `cap` couvre la
        // longueur copiee, et `blob` est une allocation locale distincte.
        unsafe { std::ptr::copy_nonoverlapping(blob.as_ptr(), out, blob.len()) };
        OK
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

/// Ouvre une session de diagnostic a la demande de l'operateur (C-35).
///
/// Le plafond automatique vaut `COUNTER` tant que le profileur est a `LIGHT`, et il n'en
/// sort qu'apres une ligne de base — soixante-quatre minutes a la cadence par defaut.
/// Aucune sonde n'est donc chronometree pendant la premiere heure. Cette porte permet a
/// un operateur d'obtenir des durees tout de suite, pour la duree qu'il choisit.
///
/// Elle ne desarme rien : le gouverneur reste maitre a la baisse et annule la session
/// des que le budget est depasse.
///
/// # Safety
///
/// Aucun pointeur n'est deref
#[no_mangle]
pub extern "C" fn rfx_profiler_request_depth(handle: u64, level_code: u8, ticks: u64) -> i32 {
    guard_with_handle(handle, || {
        match runtime::with(handle, |rt| rt.request_probe_depth(level_code, ticks)) {
            Ok(true) => OK,
            Ok(false) => ErrorCode::InvalidArgument.ffi_code(),
            Err(e) => e.ffi_code(),
        }
    })
}

/// Ecrit le classement des unites les plus couteuses dans `out`, en CBOR (C-35).
///
/// Meme protocole en deux temps que [`rfx_status`] : appeler d'abord avec `cap == 0`
/// pour obtenir la taille requise dans `out_len`, puis rappeler avec un tampon de
/// cette taille.
///
/// `limit` borne le nombre d'entrees rendues. Le classement ne transporte aucun nom de
/// classe ni de methode : le natif ne les retient pas, et c'est Java qui les a
/// declares. Il rend des identifiants de sonde, que Java sait resoudre.
///
/// R-700 : un seul passage rend tout le classement. Interroger les unites une par une
/// couterait une traversee par unite, pour une commande qui en veut vingt.
///
/// # Safety
///
/// `out` doit pointer sur au moins `cap` octets inscriptibles (il peut etre nul si
/// `cap` vaut zero), et `out_len` sur un `usize` inscriptible.
#[no_mangle]
pub unsafe extern "C" fn rfx_profiler_top(
    handle: u64,
    limit: u32,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> i32 {
    guard_with_handle(handle, || {
        if out_len.is_null() || (out.is_null() && cap > 0) {
            return ErrorCode::InvalidArgument.ffi_code();
        }

        let top = match runtime::with(handle, |rt| rt.top_workloads(limit as usize)) {
            Ok(t) => t,
            Err(e) => return e.ffi_code(),
        };
        let Ok(blob) = rfx_model::to_cbor(&top) else {
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

/// Valeur ecrite dans `out_probe_id` quand l'unite de travail ne sera pas sondee.
///
/// Ce n'est pas une erreur : le plafond d'unites suivies est atteint et aucune unite
/// froide n'est evincable. La methode s'execute normalement, sans mesure. La valeur
/// est hors du domaine des identifiants, bornes par `profiler.max_workloads`.
pub const RFX_PROBE_ID_NONE: u32 = u32::MAX;

/// Enregistre une unite de travail et rend son identifiant de sonde (C-05, DM-01).
///
/// `descriptor` est un blob CBOR decrivant un `WorkDescriptor`. Le `WorkId` est
/// calcule ici et nulle part ailleurs : deux implementations d'un meme hachage
/// finiraient par diverger sur un detail d'encodage (DM-01).
///
/// # Safety
///
/// `descriptor` doit pointer sur au moins `len` octets lisibles, et `out_probe_id`
/// sur un `u32` inscriptible.
#[no_mangle]
pub unsafe extern "C" fn rfx_workload_register(
    handle: u64,
    descriptor: *const u8,
    len: usize,
    out_probe_id: *mut u32,
) -> i32 {
    guard_with_handle(handle, || {
        if descriptor.is_null() || out_probe_id.is_null() || len == 0 || len > MAX_TRANSFER_SIZE {
            return ErrorCode::InvalidArgument.ffi_code();
        }
        // SAFETY : precondition de la fonction, non nul et longueur verifies ci-dessus.
        let bytes = unsafe { std::slice::from_raw_parts(descriptor, len) };
        let Ok(descriptor) = rfx_model::from_cbor::<rfx_model::WorkDescriptor>(bytes) else {
            return ErrorCode::InvalidArgument.ffi_code();
        };
        let work_id = rfx_model::WorkId::compute(&descriptor);

        match runtime::with(handle, |rt| rt.register_workload(&descriptor, work_id)) {
            Ok(probe_id) => {
                // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
                unsafe { out_probe_id.write(probe_id.unwrap_or(RFX_PROBE_ID_NONE)) };
                OK
            }
            Err(e) => e.ffi_code(),
        }
    })
}

/// Ecrit la table des niveaux de sonde dans `out`, si elle a change (ADR-016).
///
/// Meme protocole en deux temps que [`rfx_status`] : `cap == 0` rend la taille
/// requise dans `out_len`, puis un second appel avec un tampon de cette taille copie
/// la table et la declare transmise. Une table inchangee rend `out_len == 0` : il n'y
/// a alors rien a appliquer, et rien ne traverse la frontiere.
///
/// # Safety
///
/// `out` doit pointer sur au moins `cap` octets inscriptibles (il peut etre nul si
/// `cap` vaut zero), et `out_len` sur un `usize` inscriptible.
#[no_mangle]
pub unsafe extern "C" fn rfx_probe_levels(
    handle: u64,
    out: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> i32 {
    guard_with_handle(handle, || {
        if out_len.is_null() || (out.is_null() && cap > 0) {
            return ErrorCode::InvalidArgument.ffi_code();
        }
        let result = runtime::with(handle, |rt| {
            let Some(levels) = rt.pending_probe_levels() else {
                return (0_usize, false);
            };
            let required = levels.len();
            if cap == 0 || cap < required {
                // Rien n'est copie : la table reste en attente pour le prochain appel.
                return (required, false);
            }
            // SAFETY : `out` possede au moins `cap` octets inscriptibles, `cap` est
            // superieur ou egal a `required`, et les zones ne se recouvrent pas — la
            // table appartient au natif, le tampon a Java.
            unsafe { std::ptr::copy_nonoverlapping(levels.as_ptr(), out, required) };
            (required, true)
        });

        match result {
            Ok((required, delivered)) => {
                if delivered {
                    let _ = runtime::with(handle, rfx_core::Runtime::clear_pending_probe_levels);
                } else if cap > 0 && required > cap {
                    // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
                    unsafe { out_len.write(required) };
                    return ErrorCode::InvalidArgument.ffi_code();
                }
                // SAFETY : precondition de la fonction, non nul verifie ci-dessus.
                unsafe { out_len.write(required) };
                OK
            }
            Err(e) => e.ffi_code(),
        }
    })
}

/// Demarre le profilage (C-05, `OFF -> LIGHT`).
///
/// Appelee par Java quand le jeu est en etat d'etre observe. Tant qu'elle ne l'a pas
/// ete, les sondes posees dans le bytecode restent eteintes.
#[no_mangle]
pub extern "C" fn rfx_profiler_start(handle: u64) -> i32 {
    guard_with_handle(handle, || {
        match runtime::with(handle, rfx_core::Runtime::start_profiler) {
            Ok(()) => OK,
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

    /// Blob d'ouverture de la base dans `dir`.
    fn db_open(dir: &std::path::Path) -> rfx_model::DbOpen {
        rfx_model::DbOpen {
            schema: rfx_model::MODEL_SCHEMA_VERSION,
            dir: dir.to_string_lossy().into_owned(),
            minecraft: "1.20.1".to_owned(),
            forge: "47.4.0".to_owned(),
            java: "test".to_owned(),
            mods: vec!["rustforgex@0.1.0@0".to_owned()],
        }
    }

    fn open_db(handle: u64, dir: &std::path::Path) -> i32 {
        let blob = rfx_model::to_cbor(&db_open(dir)).expect("encodage");
        unsafe { rfx_db_open(handle, blob.as_ptr(), blob.len()) }
    }

    /// Synchronise jusqu'a la fin du cycle ; rend le nombre d'appels.
    fn sync_until_done(handle: u64) -> u32 {
        let mut calls = 0;
        loop {
            let mut done = 0_u32;
            assert_eq!(unsafe { rfx_db_sync(handle, &mut done) }, OK);
            calls += 1;
            if done == 1 {
                return calls;
            }
        }
    }

    /// Repertoire temporaire, supprime a la fin du test.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .expect("horloge")
                .as_nanos();
            Self(
                std::env::temp_dir()
                    .join(format!("rfx-ffi-{name}-{}-{unique}", std::process::id())),
            )
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// C-09 (ADR-049) : une unite enregistree avant l'ouverture part au premier lot ;
    /// l'arret ecrit la base, et le lancement suivant la relit.
    #[test]
    fn the_workload_database_survives_a_restart() {
        let _guard = test_lock();
        let dir = TempDir::new("db");
        let handle = init(&RuntimeConfig::default());
        assert!(register(handle, "tick").is_some());
        assert_eq!(open_db(handle, &dir.0), OK);
        assert_eq!(
            open_db(handle, &dir.0),
            ErrorCode::InvalidArgument.ffi_code(),
            "deja ouverte"
        );
        sync_until_done(handle);
        let db = read_status(handle).db;
        assert!(db.enabled && db.open);
        assert_eq!((db.syncs, db.pending), (1, 0));
        // Enregistree apres le dernier lot : seule la fermeture a l'arret peut la porter
        // jusqu'au fichier.
        assert!(register(handle, "late").is_some());
        assert_eq!(rfx_shutdown(handle), OK);
        assert!(dir.0.join("workload.db").exists());

        let handle = init(&RuntimeConfig::default());
        assert_eq!(open_db(handle, &dir.0), OK);
        let deadline = Instant::now() + std::time::Duration::from_secs(30);
        let db = loop {
            let db = read_status(handle).db;
            if db.load != 0 || Instant::now() > deadline {
                break db;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(db.load, 4, "relue");
        assert_eq!(db.loaded_entries, 2, "la derniere ecrite a l'arret");
        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// La collecte avance par tranches : le verrou est rendu entre deux appels.
    #[test]
    fn the_sync_walks_the_profiler_by_slices() {
        let _guard = test_lock();
        let dir = TempDir::new("slices");
        let handle = init(&RuntimeConfig::default());
        for n in 0..1_200 {
            assert!(register(handle, &format!("m{n}")).is_some());
        }
        assert_eq!(open_db(handle, &dir.0), OK);
        assert_eq!(sync_until_done(handle), 3, "1 200 entrees, tranches de 512");
        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// `cache.enabled = false` : rien n'est ouvert, rien n'est ecrit.
    #[test]
    fn a_disabled_database_does_nothing() {
        let _guard = test_lock();
        let dir = TempDir::new("disabled");
        let handle = init(&RuntimeConfig {
            cache_enabled: false,
            ..RuntimeConfig::default()
        });
        assert!(register(handle, "tick").is_some());
        assert_eq!(open_db(handle, &dir.0), OK);
        assert_eq!(sync_until_done(handle), 1);
        let db = read_status(handle).db;
        assert!(!db.enabled && !db.open);
        assert_eq!(db.pending, 0, "rien n'est garde en attente");
        assert_eq!(rfx_shutdown(handle), OK);
        assert!(!dir.0.exists());
    }

    /// Hierarchie et bytecode des classes de test de `rfx-bytecode`.
    fn analysis_fixtures() -> Vec<(rfx_model::HierarchyEntry, Vec<u8>)> {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../rfx-bytecode/tests/fixtures/classes/fixtures");
        std::fs::read_dir(&dir)
            .expect("classes de test")
            .map(|entry| entry.expect("entree").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "class"))
            .map(|path| {
                let bytes = std::fs::read(&path).expect("lecture");
                (class_header(&bytes), bytes)
            })
            .collect()
    }

    /// Nom, super-classe et drapeaux d'un `.class`, lus sans dependre de `rfx-bytecode`
    /// (le crate FFI n'en depend pas) : en-tete JVMS §4.1, reservoir de constantes saute.
    fn class_header(bytes: &[u8]) -> rfx_model::HierarchyEntry {
        let u16_at = |at: usize| u16::from_be_bytes([bytes[at], bytes[at + 1]]);
        let count = u16_at(8);
        let mut at = 10;
        let mut utf8 = std::collections::HashMap::new();
        let mut classes = std::collections::HashMap::new();
        let mut index = 1;
        while index < count {
            match bytes[at] {
                1 => {
                    let len = usize::from(u16_at(at + 1));
                    let text = String::from_utf8_lossy(&bytes[at + 3..at + 3 + len]);
                    utf8.insert(index, text.into_owned());
                    at += 3 + len;
                }
                7 => {
                    classes.insert(index, u16_at(at + 1));
                    at += 3;
                }
                8 | 16 | 19 | 20 => at += 3,
                15 => at += 4,
                3 | 4 | 9 | 10 | 11 | 12 | 17 | 18 => at += 5,
                5 | 6 => {
                    at += 9;
                    index += 1;
                }
                other => panic!("etiquette {other}"),
            }
            index += 1;
        }
        let access = u32::from(u16_at(at));
        let name = utf8[&classes[&u16_at(at + 2)]].clone();
        let super_index = u16_at(at + 4);
        let super_name = if super_index == 0 {
            String::new()
        } else {
            utf8[&classes[&super_index]].clone()
        };
        rfx_model::HierarchyEntry(name, super_name, access)
    }

    fn step(handle: u64) -> u32 {
        let mut state = u32::MAX;
        assert_eq!(unsafe { rfx_analysis_step(handle, &mut state) }, OK);
        state
    }

    /// C-07 (ADR-051) : la hierarchie et les classes passent la frontiere, les pas
    /// avancent, et le statut publie ce qui a ete recu.
    #[test]
    fn the_analysis_entry_points_cross_the_boundary() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());
        let fixtures = analysis_fixtures();
        let entries: Vec<_> = fixtures.iter().map(|(entry, _)| entry.clone()).collect();
        let blob = rfx_model::to_cbor(&entries).expect("encodage");
        assert_eq!(
            unsafe { rfx_analysis_hierarchy(handle, blob.as_ptr(), blob.len()) },
            OK
        );
        let (entry, bytes) = &fixtures[0];
        assert_eq!(
            unsafe {
                rfx_analysis_class(
                    handle,
                    entry.0.as_ptr(),
                    entry.0.len(),
                    bytes.as_ptr(),
                    bytes.len(),
                )
            },
            OK
        );
        assert_eq!(
            unsafe {
                rfx_analysis_class(handle, b"fixtures/Car".as_ptr(), 12, std::ptr::null(), 0)
            },
            OK,
            "classe declaree absente"
        );

        assert_eq!(step(handle), 0, "aucune unite chaude : rien a analyser");
        let analysis = read_status(handle).analysis;
        assert!(analysis.enabled);
        assert_eq!(analysis.hierarchy_classes, entries.len() as u64);
        assert_eq!((analysis.classes_provided, analysis.classes_absent), (1, 1));

        let mut size = 0_usize;
        assert_eq!(
            unsafe { rfx_analysis_requests(handle, std::ptr::null_mut(), 0, &mut size) },
            OK
        );
        let mut buffer = vec![0_u8; size];
        let mut written = 0_usize;
        assert_eq!(
            unsafe { rfx_analysis_requests(handle, buffer.as_mut_ptr(), size, &mut written) },
            OK
        );
        let requests: Vec<String> = rfx_model::from_cbor(&buffer[..written]).expect("CBOR");
        assert!(requests.is_empty());

        // Arguments aberrants : refuses, jamais une panic.
        let refused = ErrorCode::InvalidArgument.ffi_code();
        assert_eq!(
            unsafe { rfx_analysis_hierarchy(handle, [0xff_u8].as_ptr(), 1) },
            refused
        );
        assert_eq!(
            unsafe { rfx_analysis_hierarchy(handle, std::ptr::null(), 4) },
            refused
        );
        assert_eq!(
            unsafe { rfx_analysis_class(handle, [0xff_u8].as_ptr(), 1, std::ptr::null(), 0) },
            refused,
            "nom en UTF-8 invalide"
        );
        assert_eq!(
            unsafe { rfx_analysis_class(handle, std::ptr::null(), 3, std::ptr::null(), 0) },
            refused
        );
        assert_eq!(
            unsafe { rfx_analysis_class(handle, b"a/B".as_ptr(), 3, std::ptr::null(), 8) },
            refused,
            "longueur sans tampon"
        );
        assert_eq!(
            unsafe { rfx_analysis_step(handle, std::ptr::null_mut()) },
            refused
        );
        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// `analysis.enabled = false` : la hierarchie est ignoree, rien n'est analyse.
    #[test]
    fn a_disabled_analysis_does_nothing() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig {
            analysis_enabled: false,
            ..RuntimeConfig::default()
        });
        let entries: Vec<_> = analysis_fixtures()
            .into_iter()
            .map(|(entry, _)| entry)
            .collect();
        let blob = rfx_model::to_cbor(&entries).expect("encodage");
        assert_eq!(
            unsafe { rfx_analysis_hierarchy(handle, blob.as_ptr(), blob.len()) },
            OK
        );
        assert_eq!(step(handle), 0);
        let analysis = read_status(handle).analysis;
        assert!(!analysis.enabled);
        assert_eq!(analysis.hierarchy_classes, 0);
        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// Enregistre une unite de travail et rend son identifiant de sonde.
    fn register(handle: u64, method_name: &str) -> Option<u32> {
        let descriptor = rfx_model::WorkDescriptor {
            owner_id: "minecraft".to_owned(),
            class_internal_name: "net/minecraft/world/entity/Mob".to_owned(),
            method_name: method_name.to_owned(),
            method_descriptor: "()V".to_owned(),
            call_context_hash: 0,
            side: rfx_model::Side::Server,
        };
        let blob = rfx_model::to_cbor(&descriptor).expect("encodage");
        let mut probe_id = RFX_PROBE_ID_NONE;
        assert_eq!(
            unsafe { rfx_workload_register(handle, blob.as_ptr(), blob.len(), &mut probe_id) },
            OK
        );
        (probe_id != RFX_PROBE_ID_NONE).then_some(probe_id)
    }

    /// Lit la table des niveaux, ou `None` si elle n'a pas change.
    fn read_levels(handle: u64) -> Option<Vec<u8>> {
        let mut size = 0_usize;
        assert_eq!(
            unsafe { rfx_probe_levels(handle, std::ptr::null_mut(), 0, &mut size) },
            OK
        );
        if size == 0 {
            return None;
        }
        let mut buffer = vec![0_u8; size];
        let mut written = 0_usize;
        assert_eq!(
            unsafe { rfx_probe_levels(handle, buffer.as_mut_ptr(), buffer.len(), &mut written) },
            OK
        );
        buffer.truncate(written);
        Some(buffer)
    }

    /// Le meme descripteur rend toujours le meme identifiant de sonde (R-200).
    #[test]
    fn registering_the_same_workload_twice_yields_the_same_probe() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());

        let first = register(handle, "tick").expect("sonde");
        let again = register(handle, "tick").expect("sonde");
        let other = register(handle, "aiStep").expect("sonde");

        assert_eq!(first, again);
        assert_ne!(first, other);
        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// R-421, ADR-046 : le releve de charge externe arrive dans le statut ; un handle
    /// inconnu est refuse.
    #[test]
    fn the_external_load_report_reaches_the_status() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());
        assert_eq!(read_status(handle).scheduler.external_load_reports, 0);

        assert_eq!(rfx_sched_report_external_load(handle, 30), OK);
        assert_eq!(rfx_sched_report_external_load(handle, 12), OK);

        let scheduler = read_status(handle).scheduler;
        assert_eq!(scheduler.external_runnable, 12);
        assert_eq!(scheduler.external_runnable_max, 30);
        assert_eq!(scheduler.external_load_reports, 2);
        assert_ne!(
            rfx_sched_report_external_load(handle ^ 1, 5),
            OK,
            "handle inconnu"
        );
        assert_eq!(rfx_shutdown(handle), OK);
    }

    #[test]
    fn an_undecodable_descriptor_is_refused_without_panicking() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());

        let garbage = [0xff_u8; 8];
        let mut probe_id = 0_u32;
        assert_eq!(
            unsafe {
                rfx_workload_register(handle, garbage.as_ptr(), garbage.len(), &mut probe_id)
            },
            ErrorCode::InvalidArgument.ffi_code()
        );
        assert_eq!(read_status(handle).panics, 0, "aucune panic n'a traverse");
        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// ADR-016 : la table des niveaux ne traverse la frontiere que si elle a change.
    #[test]
    fn the_level_table_crosses_the_boundary_only_when_it_changed() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());
        assert_eq!(rfx_profiler_start(handle), OK);

        register(handle, "tick").expect("sonde");
        register(handle, "aiStep").expect("sonde");

        let levels = read_levels(handle).expect("table changee");
        assert_eq!(levels.len(), 2);
        assert!(read_levels(handle).is_none(), "table inchangee");

        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// Le protocole en deux temps ne consomme pas la table a l'etape de la taille.
    #[test]
    fn asking_for_the_level_table_size_does_not_consume_it() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());
        register(handle, "tick").expect("sonde");

        let mut size = 0_usize;
        assert_eq!(
            unsafe { rfx_probe_levels(handle, std::ptr::null_mut(), 0, &mut size) },
            OK
        );
        assert_eq!(size, 1);
        assert_eq!(
            read_levels(handle)
                .expect("table toujours disponible")
                .len(),
            1
        );
        assert_eq!(rfx_shutdown(handle), OK);
    }

    #[test]
    fn a_level_buffer_too_small_is_refused_and_the_table_stays_pending() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());
        register(handle, "tick").expect("sonde");
        register(handle, "aiStep").expect("sonde");

        let mut tiny = [0_u8; 1];
        let mut required = 0_usize;
        assert_eq!(
            unsafe { rfx_probe_levels(handle, tiny.as_mut_ptr(), tiny.len(), &mut required) },
            ErrorCode::InvalidArgument.ffi_code()
        );
        assert_eq!(required, 2, "la taille requise est rendue malgre le refus");
        assert_eq!(read_levels(handle).expect("toujours en attente").len(), 2);
        assert_eq!(rfx_shutdown(handle), OK);
    }

    /// Le statut publie les compteurs du profiler (C-05).
    #[test]
    fn the_status_reports_the_profiler() {
        let _guard = test_lock();
        let handle = init(&RuntimeConfig::default());
        assert_eq!(rfx_profiler_start(handle), OK);
        register(handle, "tick").expect("sonde");

        let status = read_status(handle);
        assert_eq!(status.profiler.level, "LIGHT");
        assert_eq!(status.profiler.workloads_tracked, 1);
        assert_eq!(status.profiler.probes_allocated, 1);
        assert_eq!(rfx_shutdown(handle), OK);
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
        let mut done = 0_u32;
        assert_eq!(unsafe { rfx_db_sync(fake, &mut done) }, expected);
        assert_eq!(unsafe { rfx_analysis_step(fake, &mut done) }, expected);
        let hierarchy = rfx_model::to_cbor(&vec![rfx_model::HierarchyEntry(
            "a/B".to_owned(),
            String::new(),
            0,
        )])
        .expect("encodage");
        assert_eq!(
            unsafe { rfx_analysis_hierarchy(fake, hierarchy.as_ptr(), hierarchy.len()) },
            expected
        );
        assert_eq!(
            unsafe { rfx_analysis_class(fake, b"a/B".as_ptr(), 3, std::ptr::null(), 0) },
            expected
        );
        let mut requested = 0_usize;
        assert_eq!(
            unsafe { rfx_analysis_requests(fake, std::ptr::null_mut(), 0, &mut requested) },
            expected
        );
        let open = rfx_model::to_cbor(&db_open(&std::env::temp_dir())).expect("encodage");
        assert_eq!(
            unsafe { rfx_db_open(fake, open.as_ptr(), open.len()) },
            expected
        );
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
