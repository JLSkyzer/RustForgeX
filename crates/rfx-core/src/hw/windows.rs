//! C-45 : sonde materielle, implementation Windows.
//!
//! Deux appels systeme seulement : `GetLogicalProcessorInformation` (coeurs
//! physiques, cache L3, noeuds NUMA) et `GlobalMemoryStatusEx` (memoire totale).
//!
//! # Justification du code `unsafe` (PARTIE 19.1)
//!
//! Ces informations ne sont accessibles que par l'API Win32. Le code `unsafe` se
//! limite a deux appels de fonction externe, tous deux encadres ainsi :
//!
//! - aucun pointeur ne survit a l'appel ; les tampons sont possedes par du code Rust
//!   et vivent plus longtemps que l'appel ;
//! - les structures sont `#[repr(C)]`, de disposition identique a celle attendue par
//!   Win32, et composees uniquement de types entiers ;
//! - la longueur du tampon est celle que le systeme a lui-meme reclamee, et le nombre
//!   d'elements relus est borne par la longueur que le systeme rapporte en retour ;
//! - un echec renvoie une sonde vide plutot qu'une valeur devinee.

#![allow(unsafe_code)]

use super::SystemProbe;

/// `SYSTEM_LOGICAL_PROCESSOR_INFORMATION.Relationship` : coeur physique.
const RELATION_PROCESSOR_CORE: u32 = 0;
/// `SYSTEM_LOGICAL_PROCESSOR_INFORMATION.Relationship` : noeud NUMA.
const RELATION_NUMA_NODE: u32 = 1;
/// `SYSTEM_LOGICAL_PROCESSOR_INFORMATION.Relationship` : niveau de cache.
const RELATION_CACHE: u32 = 2;

/// `CACHE_DESCRIPTOR` (winnt.h).
#[repr(C)]
#[derive(Clone, Copy)]
// Champs non lus : ils materialisent la disposition memoire exacte attendue par
// Win32, indispensable aux decalages des champs suivants.
#[allow(dead_code)]
struct CacheDescriptor {
    level: u8,
    associativity: u8,
    line_size: u16,
    size: u32,
    cache_type: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
// Champs non lus : ils materialisent la disposition memoire exacte attendue par
// Win32, indispensable aux decalages des champs suivants.
#[allow(dead_code)]
struct ProcessorCore {
    flags: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
// Champs non lus : ils materialisent la disposition memoire exacte attendue par
// Win32, indispensable aux decalages des champs suivants.
#[allow(dead_code)]
struct NumaNode {
    node_number: u32,
}

/// Union anonyme de `SYSTEM_LOGICAL_PROCESSOR_INFORMATION` (winnt.h).
#[repr(C)]
#[derive(Clone, Copy)]
union InfoUnion {
    processor_core: ProcessorCore,
    numa_node: NumaNode,
    cache: CacheDescriptor,
    reserved: [u64; 2],
}

/// `SYSTEM_LOGICAL_PROCESSOR_INFORMATION` (winnt.h).
#[repr(C)]
#[derive(Clone, Copy)]
// Champs non lus : ils materialisent la disposition memoire exacte attendue par
// Win32, indispensable aux decalages des champs suivants.
#[allow(dead_code)]
struct LogicalProcessorInformation {
    processor_mask: usize,
    relationship: u32,
    info: InfoUnion,
}

/// `MEMORYSTATUSEX` (sysinfoapi.h).
#[repr(C)]
#[derive(Clone, Copy, Default)]
// Champs non lus : ils materialisent la disposition memoire exacte attendue par
// Win32, indispensable aux decalages des champs suivants.
#[allow(dead_code)]
struct MemoryStatusEx {
    length: u32,
    memory_load: u32,
    total_phys: u64,
    avail_phys: u64,
    total_page_file: u64,
    avail_page_file: u64,
    total_virtual: u64,
    avail_virtual: u64,
    avail_extended_virtual: u64,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLogicalProcessorInformation(
        buffer: *mut LogicalProcessorInformation,
        returned_length: *mut u32,
    ) -> i32;

    fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
}

/// Sonde la machine. Un echec systeme laisse les champs concernes a zero.
pub(super) fn probe() -> SystemProbe {
    let mut s = SystemProbe::default();
    if let Some(entries) = processor_info() {
        let mut l3_max = 0_u64;
        for info in &entries {
            match info.relationship {
                RELATION_PROCESSOR_CORE => {
                    s.physical_cores = s.physical_cores.saturating_add(1);
                }
                RELATION_NUMA_NODE => {
                    s.numa_nodes = s.numa_nodes.saturating_add(1);
                }
                RELATION_CACHE => {
                    // SAFETY : le systeme garantit que le membre `cache` de l'union
                    // est celui renseigne lorsque `relationship` vaut
                    // `RELATION_CACHE`. Tous les membres sont des entiers `Copy`,
                    // donc aucune lecture n'est indefinie quel que soit le contenu.
                    let cache = unsafe { info.info.cache };
                    if cache.level == 3 {
                        l3_max = l3_max.max(u64::from(cache.size));
                    }
                }
                _ => {}
            }
        }
        s.l3_bytes = l3_max;
    }
    s.mem_total_bytes = total_memory();
    s
}

/// Recupere la table des informations processeur, ou `None` si le systeme refuse.
fn processor_info() -> Option<Vec<LogicalProcessorInformation>> {
    let mut length: u32 = 0;

    // Premier appel : le systeme renseigne la taille necessaire et echoue
    // volontairement. Le pointeur nul est explicitement admis par l'API dans ce cas.
    // SAFETY : `longueur` est une variable locale valide pendant tout l'appel.
    unsafe { GetLogicalProcessorInformation(core::ptr::null_mut(), &mut length) };

    let entry_size = core::mem::size_of::<LogicalProcessorInformation>();
    let count = length as usize / entry_size;
    if count == 0 {
        return None;
    }

    let zero = LogicalProcessorInformation {
        processor_mask: 0,
        relationship: 0,
        info: InfoUnion { reserved: [0; 2] },
    };
    let mut buffer = vec![zero; count];

    // SAFETY : `tampon` possede exactement `longueur` octets contigus et vit
    // au-dela de l'appel ; `longueur` est la taille reclamee par le systeme.
    let ok = unsafe { GetLogicalProcessorInformation(buffer.as_mut_ptr(), &mut length) };
    if ok == 0 {
        return None;
    }

    // Le systeme peut avoir ecrit moins d'elements que la capacite allouee.
    let written = (length as usize / entry_size).min(buffer.len());
    buffer.truncate(written);
    Some(buffer)
}

/// Memoire physique totale en octets, ou `0` si le systeme refuse.
fn total_memory() -> u64 {
    let mut status = MemoryStatusEx {
        length: u32::try_from(core::mem::size_of::<MemoryStatusEx>()).unwrap_or(0),
        ..MemoryStatusEx::default()
    };
    // SAFETY : `statut` est une structure locale `#[repr(C)]` correctement
    // dimensionnee via son champ `length`, comme l'exige l'API.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        0
    } else {
        status.total_phys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_layout_matches_win32() {
        // Dispositions attendues sur x86-64 : toute divergence signalerait une
        // erreur de transcription des en-tetes Win32.
        assert_eq!(core::mem::size_of::<LogicalProcessorInformation>(), 32);
        assert_eq!(core::mem::align_of::<LogicalProcessorInformation>(), 8);
        assert_eq!(core::mem::size_of::<InfoUnion>(), 16);
        assert_eq!(core::mem::size_of::<CacheDescriptor>(), 12);
        assert_eq!(core::mem::size_of::<MemoryStatusEx>(), 64);
    }

    #[test]
    fn windows_probe_measures_the_machine() {
        let s = probe();
        assert!(s.physical_cores > 0, "coeurs physiques non detectes");
        assert!(s.mem_total_bytes > 0, "memoire totale non detectee");
        assert!(s.numa_nodes > 0, "aucun noeud NUMA detecte");
    }
}
