//! C-45 : Hardware Probe — « mesurer la machine, pas la deviner ».
//!
//! Cahier des charges : PARTIE 5.43. Modele produit : DM-14 ([`HardwareClass`]).
//! Exigences : R-660 (aucune constante de cout codee en dur), R-661 (re-sonde
//! possible). Tests : T-480, T-481, T-482. Maturite : `STABLE`.
//!
//! Deux couts de DM-14 ne sont **pas** mesures ici : `jni_call_ns` et
//! `ffi_batch_ns_per_kb`. Ils incluent le trajet aller-retour depuis la JVM et sont
//! donc mesures cote Java, puis publies via
//! [`Runtime::definir_couts_ffi`](crate::runtime::Runtime::definir_couts_ffi).
//!
//! Tout champ qu'une plateforme ne permet pas de mesurer reste a `0` et son drapeau
//! dans [`ProbeCoverage`] reste `false` : aucune valeur n'est devinee.

use rfx_model::{CoreTopology, HardwareClass, ProbeCoverage, SimdCaps};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

/// Resultat brut d'une sonde specifique a une plateforme.
#[derive(Debug, Clone, Copy, Default)]
struct SondeSysteme {
    physical_cores: u16,
    l3_bytes: u64,
    numa_nodes: u8,
    mem_total_bytes: u64,
}

/// Sonde la machine et renvoie la classe materielle mesuree, accompagnee de la liste
/// des champs effectivement mesures.
///
/// La sonde ne dure que quelques millisecondes : elle n'interroge que des compteurs
/// systeme, sans allocation notable ni entree-sortie bloquante (T-482 : < 150 ms).
#[must_use]
pub fn sonder() -> (HardwareClass, ProbeCoverage) {
    let mut hw = HardwareClass::default();
    let mut couverture = ProbeCoverage::default();

    // Coeurs logiques : disponible sur toutes les plateformes supportees.
    if let Ok(n) = std::thread::available_parallelism() {
        hw.logical_cores = u16::try_from(n.get()).unwrap_or(u16::MAX);
    }

    let sys = sonde_systeme();
    hw.physical_cores = sys.physical_cores;
    hw.l3_bytes = sys.l3_bytes;
    hw.numa_nodes = sys.numa_nodes;
    hw.mem_total_bytes = sys.mem_total_bytes;

    couverture.cores = hw.logical_cores > 0 && hw.physical_cores > 0;
    couverture.l3 = hw.l3_bytes > 0;
    couverture.numa = hw.numa_nodes > 0;
    couverture.mem_total = hw.mem_total_bytes > 0;

    hw.simd = detecter_simd();
    couverture.simd = simd_detectable();

    hw.core_kinds = topologie(hw.physical_cores, hw.logical_cores);
    couverture.topology = !matches!(hw.core_kinds, CoreTopology::Unknown);

    (hw, couverture)
}

#[cfg(target_os = "windows")]
fn sonde_systeme() -> SondeSysteme {
    windows::sonder()
}

#[cfg(target_os = "linux")]
fn sonde_systeme() -> SondeSysteme {
    linux::sonder()
}

/// Plateformes sans sonde dediee : aucun champ systeme n'est mesure, et la couverture
/// le declare. Le runtime reste parfaitement fonctionnel (contrat agent 6.1 : ne pas
/// deviner une mesure manquante).
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn sonde_systeme() -> SondeSysteme {
    SondeSysteme::default()
}

/// Determine la nature des coeurs.
///
/// Une machine dont le nombre de coeurs logiques est un multiple exact du nombre de
/// coeurs physiques est homogene (SMT uniforme). Tout autre rapport revele des coeurs
/// de natures differentes, mais le decompte par nature n'est pas derivable de ces
/// deux seuls nombres : la topologie est alors declaree inconnue plutot que devinee.
fn topologie(physical: u16, logical: u16) -> CoreTopology {
    if physical == 0 || logical == 0 || logical < physical {
        return CoreTopology::Unknown;
    }
    if logical.is_multiple_of(physical) {
        CoreTopology::Homogeneous { cores: physical }
    } else {
        CoreTopology::Unknown
    }
}

/// Detecte les jeux d'instructions vectorielles disponibles.
#[must_use]
fn detecter_simd() -> SimdCaps {
    #[cfg(target_arch = "x86_64")]
    {
        SimdCaps {
            // SSE2 fait partie de la ligne de base x86-64 : toujours present.
            sse2: true,
            avx2: std::arch::is_x86_feature_detected!("avx2"),
            avx512: std::arch::is_x86_feature_detected!("avx512f"),
            neon: false,
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        SimdCaps {
            sse2: false,
            avx2: false,
            avx512: false,
            // NEON fait partie de la ligne de base aarch64 : toujours present.
            neon: true,
        }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        SimdCaps::default()
    }
}

/// Indique si l'architecture courante permet de detecter les capacites SIMD.
fn simd_detectable() -> bool {
    cfg!(any(target_arch = "x86_64", target_arch = "aarch64"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// T-481 : detection correcte des coeurs.
    #[test]
    fn les_coeurs_sont_detectes_et_coherents() {
        let (hw, couverture) = sonder();
        assert!(hw.logical_cores > 0, "au moins un coeur logique");
        assert!(couverture.cores, "les coeurs doivent etre mesures");
        assert!(hw.physical_cores > 0, "au moins un coeur physique");
        assert!(
            hw.logical_cores >= hw.physical_cores,
            "{} logiques < {} physiques",
            hw.logical_cores,
            hw.physical_cores
        );
    }

    /// T-482 : duree de sonde inferieure a 150 ms.
    #[test]
    fn la_sonde_tient_dans_son_budget() {
        let debut = Instant::now();
        let _ = sonder();
        let duree = debut.elapsed();
        assert!(
            duree.as_millis() < 150,
            "sonde trop lente : {duree:?} (budget 150 ms)"
        );
    }

    /// T-480 : mesures stables entre executions (ecart < 20 %).
    ///
    /// Les champs sondes ici decrivent la machine : ils doivent etre **identiques**
    /// d'une execution a l'autre, ce qui satisfait largement le seuil de 20 %. Les
    /// couts mesures, eux, sont evalues cote Java.
    #[test]
    fn les_mesures_sont_stables_entre_executions() {
        let (a, ca) = sonder();
        let (b, cb) = sonder();
        assert_eq!(
            a, b,
            "la sonde doit etre deterministe sur une machine donnee"
        );
        assert_eq!(ca, cb);
    }

    #[test]
    fn aucun_champ_non_mesure_n_est_declare_couvert() {
        let (hw, c) = sonder();
        assert_eq!(c.l3, hw.l3_bytes > 0);
        assert_eq!(c.numa, hw.numa_nodes > 0);
        assert_eq!(c.mem_total, hw.mem_total_bytes > 0);
        // Les couts FFI ne sont jamais mesures par la sonde native.
        assert!(!c.ffi_call);
        assert!(!c.ffi_transfer);
        assert_eq!(hw.jni_call_ns, 0);
        assert_eq!(hw.ffi_batch_ns_per_kb, 0);
    }

    #[test]
    fn la_topologie_n_est_jamais_devinee() {
        assert_eq!(topologie(0, 8), CoreTopology::Unknown);
        assert_eq!(topologie(8, 0), CoreTopology::Unknown);
        // Plus de coeurs physiques que de logiques : incoherent, donc inconnu.
        assert_eq!(topologie(16, 8), CoreTopology::Unknown);
        // Rapport non entier : coeurs de natures differentes, decompte indeterminable.
        assert_eq!(topologie(10, 12), CoreTopology::Unknown);
        assert_eq!(topologie(8, 16), CoreTopology::Homogeneous { cores: 8 });
        assert_eq!(topologie(8, 8), CoreTopology::Homogeneous { cores: 8 });
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn sse2_est_la_ligne_de_base_sur_x86_64() {
        let (hw, c) = sonder();
        assert!(hw.simd.sse2);
        assert!(!hw.simd.neon);
        assert!(c.simd);
    }
}
