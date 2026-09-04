//! DM-14 : `HardwareClass`.
//!
//! Cahier des charges : PARTIE 4.14. Producteur : C-45 (Hardware Probe).
//! Consommateur prevu : C-15 (Decision Engine), via le modele de cout (R-660).

use serde::{Deserialize, Serialize};

/// Topologie des coeurs de la machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoreTopology {
    /// Tous les coeurs sont de meme nature.
    Homogeneous {
        /// Nombre de coeurs physiques.
        cores: u16,
    },
    /// Architecture hybride : coeurs performance et coeurs efficience.
    Hybrid {
        /// Nombre de coeurs « performance ».
        performance: u16,
        /// Nombre de coeurs « efficience ».
        efficiency: u16,
    },
    /// La topologie n'a pas pu etre determinee sur cette plateforme.
    Unknown,
}

/// Jeux d'instructions vectorielles disponibles sur la machine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimdCaps {
    /// SSE2 (x86-64 : toujours present).
    pub sse2: bool,
    /// AVX2.
    pub avx2: bool,
    /// AVX-512 Foundation.
    pub avx512: bool,
    /// NEON (aarch64 : toujours present).
    pub neon: bool,
}

/// DM-14 : classe materielle mesuree au demarrage par C-45.
///
/// Convention pour les champs non mesurables sur une plateforme donnee : la valeur
/// `0` signifie **non sonde**, jamais « absent » ni « nul ». La liste des champs
/// effectivement mesures est publiee separement dans
/// [`ProbeCoverage`](crate::status::ProbeCoverage), afin qu'aucun consommateur ne
/// puisse confondre une mesure et une valeur par defaut (R-660, contrat agent 6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareClass {
    /// Nombre de coeurs physiques.
    pub physical_cores: u16,
    /// Nombre de coeurs logiques (threads materiels).
    pub logical_cores: u16,
    /// Nature des coeurs.
    pub core_kinds: CoreTopology,
    /// Taille du cache L3 en octets. `0` = non sonde.
    pub l3_bytes: u64,
    /// Nombre de noeuds NUMA. `0` = non sonde.
    pub numa_nodes: u8,
    /// Capacites SIMD.
    pub simd: SimdCaps,
    /// Memoire physique totale en octets. `0` = non sonde.
    pub mem_total_bytes: u64,
    /// Cout mesure d'un aller-retour FFI minimal, en nanosecondes. `0` = non mesure.
    pub jni_call_ns: u32,
    /// Cout mesure du transfert Java vers natif, en nanosecondes par kibioctet.
    /// `0` = non mesure.
    pub ffi_batch_ns_per_kb: u32,
}

impl Default for HardwareClass {
    fn default() -> Self {
        Self {
            physical_cores: 0,
            logical_cores: 0,
            core_kinds: CoreTopology::Unknown,
            l3_bytes: 0,
            numa_nodes: 0,
            simd: SimdCaps::default(),
            mem_total_bytes: 0,
            jni_call_ns: 0,
            ffi_batch_ns_per_kb: 0,
        }
    }
}

impl HardwareClass {
    /// Hache une **classe** de machine, pas une identite machine.
    ///
    /// Conformement a DM-14, seuls des paliers entrent dans le hachage : palier de
    /// coeurs physiques, presence d'AVX2 et d'AVX-512, palier de memoire. Deux
    /// machines de meme classe produisent le meme hachage, ce qui rend les caches
    /// partageables sans jamais identifier une machine ni son utilisateur
    /// (voir aussi PARTIE 19.5, vie privee).
    ///
    /// Les couts mesures (`jni_call_ns`, `ffi_batch_ns_per_kb`) sont volontairement
    /// **exclus** : ils varient d'une execution a l'autre et rendraient le hachage
    /// instable.
    #[must_use]
    pub fn class_hash(&self) -> u64 {
        let mut h = Fnv::new();
        h.write_u64(u64::from(core_bucket(self.physical_cores)));
        h.write_u64(u64::from(core_bucket(self.logical_cores)));
        h.write_u64(u64::from(self.simd.avx2));
        h.write_u64(u64::from(self.simd.avx512));
        h.write_u64(u64::from(self.simd.neon));
        h.write_u64(mem_bucket_gib(self.mem_total_bytes));
        h.finish()
    }
}

/// Ramene un nombre de coeurs a un palier stable.
fn core_bucket(cores: u16) -> u16 {
    const BUCKETS: [u16; 11] = [1, 2, 4, 6, 8, 12, 16, 24, 32, 48, 64];
    let mut selected = 0;
    for p in BUCKETS {
        if cores >= p {
            selected = p;
        }
    }
    selected
}

/// Ramene une quantite de memoire a un palier en gibioctets.
fn mem_bucket_gib(bytes: u64) -> u64 {
    const GIB: u64 = 1024 * 1024 * 1024;
    const BUCKETS: [u64; 9] = [1, 2, 4, 8, 12, 16, 32, 64, 128];
    let gib = bytes / GIB;
    let mut selected = 0;
    for p in BUCKETS {
        if gib >= p {
            selected = p;
        }
    }
    selected
}

/// FNV-1a 64 bits : hachage stable, sans dependance, sans graine aleatoire.
///
/// La stabilite entre executions et entre versions est requise : `class_hash` sert de
/// composante de cle de cache (PARTIE 14.1). `DefaultHasher` de la bibliotheque
/// standard ne garantit pas cette stabilite.
struct Fnv(u64);

impl Fnv {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn write_u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(physical: u16, logical: u16, mem_gib: u64, avx2: bool) -> HardwareClass {
        HardwareClass {
            physical_cores: physical,
            logical_cores: logical,
            core_kinds: CoreTopology::Homogeneous { cores: physical },
            mem_total_bytes: mem_gib * 1024 * 1024 * 1024,
            simd: SimdCaps {
                sse2: true,
                avx2,
                ..SimdCaps::default()
            },
            ..HardwareClass::default()
        }
    }

    #[test]
    fn hash_ignores_measured_costs() {
        let mut a = machine(8, 16, 32, true);
        let b = a;
        a.jni_call_ns = 120;
        a.ffi_batch_ns_per_kb = 45;
        assert_eq!(a.class_hash(), b.class_hash());
    }

    #[test]
    fn machines_of_same_class_share_the_hash() {
        // Meme palier de coeurs (8), meme palier memoire (32 Gio), meme SIMD.
        let a = machine(8, 16, 32, true);
        let b = machine(9, 17, 33, true);
        assert_eq!(a.class_hash(), b.class_hash());
    }

    #[test]
    fn different_classes_are_distinguished() {
        let eight_cores = machine(8, 16, 32, true);
        let sixteen_cores = machine(16, 32, 32, true);
        let without_avx2 = machine(8, 16, 32, false);
        assert_ne!(eight_cores.class_hash(), sixteen_cores.class_hash());
        assert_ne!(eight_cores.class_hash(), without_avx2.class_hash());
    }

    #[test]
    fn hash_is_stable_across_runs() {
        // Valeur figee, relevee sur l'implementation de reference : toute evolution
        // de l'algorithme invalide les caches et doit donc etre deliberee
        // (PARTIE 14.6, versionnement de schema).
        assert_eq!(machine(8, 16, 32, true).class_hash(), 0xed79_e1d3_ea5e_c25c);
    }

    #[test]
    fn buckets_group_nearby_values() {
        assert_eq!(core_bucket(0), 0);
        assert_eq!(core_bucket(7), 6);
        assert_eq!(core_bucket(8), 8);
        assert_eq!(core_bucket(200), 64);
        assert_eq!(mem_bucket_gib(0), 0);
        assert_eq!(mem_bucket_gib(31 * 1024 * 1024 * 1024), 16);
    }
}
