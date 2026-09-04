//! C-45 : sonde materielle, implementation Linux.
//!
//! Toutes les mesures proviennent de `/proc` et de `/sys`, en lecture seule. Aucun
//! appel systeme direct n'est necessaire : ce module ne contient aucun code `unsafe`.
//!
//! Un fichier absent ou illisible laisse le champ correspondant a zero, et sa
//! couverture a `false` : aucune valeur n'est devinee (contrat agent 6.1).

use std::collections::BTreeSet;
use std::fs;

use super::SystemProbe;

/// Sonde la machine a partir des systemes de fichiers virtuels du noyau.
pub(super) fn probe() -> SystemProbe {
    SystemProbe {
        physical_cores: physical_cores(),
        l3_bytes: l3_cache(),
        numa_nodes: numa_node_count(),
        mem_total_bytes: total_memory(),
    }
}

/// Compte les coeurs physiques distincts declares dans `/proc/cpuinfo`.
///
/// Un coeur physique est identifie par le couple (`physical id`, `core id`) : c'est
/// ce qui distingue deux coeurs distincts de deux fils d'un meme coeur (SMT). Les
/// architectures qui ne publient pas ces champs renvoient `0`.
fn physical_cores() -> u16 {
    let Ok(content) = fs::read_to_string("/proc/cpuinfo") else {
        return 0;
    };

    let mut cores: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut package: Option<u32> = None;
    let mut core: Option<u32> = None;

    for line in content.lines() {
        let Some((key, value)) = line.split_once(':') else {
            // Ligne vide : fin du bloc decrivant un processeur logique.
            if line.trim().is_empty() {
                if let (Some(p), Some(c)) = (package.take(), core.take()) {
                    cores.insert((p, c));
                }
            }
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "physical id" => package = value.parse().ok(),
            "core id" => core = value.parse().ok(),
            _ => {}
        }
    }
    // Dernier bloc, si le fichier ne se termine pas par une ligne vide.
    if let (Some(p), Some(c)) = (package, core) {
        cores.insert((p, c));
    }

    u16::try_from(cores.len()).unwrap_or(u16::MAX)
}

/// Taille du plus grand cache de niveau 3 declare pour le processeur 0.
fn l3_cache() -> u64 {
    let Ok(entries) = fs::read_dir("/sys/devices/system/cpu/cpu0/cache") else {
        return 0;
    };
    let mut max = 0_u64;
    for entry in entries.flatten() {
        let path = entry.path();
        let level = fs::read_to_string(path.join("level"))
            .ok()
            .and_then(|s| s.trim().parse::<u8>().ok());
        if level != Some(3) {
            continue;
        }
        if let Ok(size) = fs::read_to_string(path.join("size")) {
            max = max.max(parse_size(size.trim()));
        }
    }
    max
}

/// Convertit une taille du noyau (`"32768K"`, `"16M"`, `"1024"`) en octets.
fn parse_size(raw: &str) -> u64 {
    let (number, factor) = match raw.as_bytes().last() {
        Some(b'K' | b'k') => (&raw[..raw.len() - 1], 1024),
        Some(b'M' | b'm') => (&raw[..raw.len() - 1], 1024 * 1024),
        Some(b'G' | b'g') => (&raw[..raw.len() - 1], 1024 * 1024 * 1024),
        _ => (raw, 1),
    };
    number.trim().parse::<u64>().unwrap_or(0) * factor
}

/// Compte les noeuds NUMA exposes par le noyau.
fn numa_node_count() -> u8 {
    let Ok(entries) = fs::read_dir("/sys/devices/system/node") else {
        return 0;
    };
    let count = entries
        .flatten()
        .filter(|e| {
            e.file_name().to_str().is_some_and(|n| {
                n.strip_prefix("node")
                    .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit()))
            })
        })
        .count();
    u8::try_from(count).unwrap_or(u8::MAX)
}

/// Memoire physique totale, lue depuis `MemTotal` de `/proc/meminfo`.
fn total_memory() -> u64 {
    let Ok(content) = fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            // Format : "MemTotal:       16342184 kB"
            let mut parts = rest.split_whitespace();
            let value = parts.next().and_then(|v| v.parse::<u64>().ok());
            let unit = parts.next().unwrap_or("kB");
            if let Some(v) = value {
                return match unit {
                    "kB" | "KB" => v * 1024,
                    "mB" | "MB" => v * 1024 * 1024,
                    _ => v,
                };
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_sizes_are_converted() {
        assert_eq!(parse_size("32768K"), 32768 * 1024);
        assert_eq!(parse_size("16M"), 16 * 1024 * 1024);
        assert_eq!(parse_size("1G"), 1024 * 1024 * 1024);
        assert_eq!(parse_size("1024"), 1024);
    }

    #[test]
    fn an_unreadable_size_is_zero_not_a_guess() {
        assert_eq!(parse_size(""), 0);
        assert_eq!(parse_size("inconnu"), 0);
        assert_eq!(parse_size("K"), 0);
    }

    #[test]
    fn the_linux_probe_measures_the_machine() {
        let s = probe();
        assert!(s.mem_total_bytes > 0, "memoire totale non detectee");
        assert!(s.physical_cores > 0, "coeurs physiques non detectes");
    }
}
