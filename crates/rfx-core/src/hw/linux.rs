//! C-45 : sonde materielle, implementation Linux.
//!
//! Toutes les mesures proviennent de `/proc` et de `/sys`, en lecture seule. Aucun
//! appel systeme direct n'est necessaire : ce module ne contient aucun code `unsafe`.
//!
//! Un fichier absent ou illisible laisse le champ correspondant a zero, et sa
//! couverture a `false` : aucune valeur n'est devinee (contrat agent 6.1).

use std::collections::BTreeSet;
use std::fs;

use super::SondeSysteme;

/// Sonde la machine a partir des systemes de fichiers virtuels du noyau.
pub(super) fn sonder() -> SondeSysteme {
    SondeSysteme {
        physical_cores: coeurs_physiques(),
        l3_bytes: cache_l3(),
        numa_nodes: noeuds_numa(),
        mem_total_bytes: memoire_totale(),
    }
}

/// Compte les coeurs physiques distincts declares dans `/proc/cpuinfo`.
///
/// Un coeur physique est identifie par le couple (`physical id`, `core id`) : c'est
/// ce qui distingue deux coeurs distincts de deux fils d'un meme coeur (SMT). Les
/// architectures qui ne publient pas ces champs renvoient `0`.
fn coeurs_physiques() -> u16 {
    let Ok(contenu) = fs::read_to_string("/proc/cpuinfo") else {
        return 0;
    };

    let mut coeurs: BTreeSet<(u32, u32)> = BTreeSet::new();
    let mut paquet: Option<u32> = None;
    let mut coeur: Option<u32> = None;

    for ligne in contenu.lines() {
        let Some((cle, valeur)) = ligne.split_once(':') else {
            // Ligne vide : fin du bloc decrivant un processeur logique.
            if ligne.trim().is_empty() {
                if let (Some(p), Some(c)) = (paquet.take(), coeur.take()) {
                    coeurs.insert((p, c));
                }
            }
            continue;
        };
        let valeur = valeur.trim();
        match cle.trim() {
            "physical id" => paquet = valeur.parse().ok(),
            "core id" => coeur = valeur.parse().ok(),
            _ => {}
        }
    }
    // Dernier bloc, si le fichier ne se termine pas par une ligne vide.
    if let (Some(p), Some(c)) = (paquet, coeur) {
        coeurs.insert((p, c));
    }

    u16::try_from(coeurs.len()).unwrap_or(u16::MAX)
}

/// Taille du plus grand cache de niveau 3 declare pour le processeur 0.
fn cache_l3() -> u64 {
    let Ok(entrees) = fs::read_dir("/sys/devices/system/cpu/cpu0/cache") else {
        return 0;
    };
    let mut max = 0_u64;
    for entree in entrees.flatten() {
        let chemin = entree.path();
        let niveau = fs::read_to_string(chemin.join("level"))
            .ok()
            .and_then(|s| s.trim().parse::<u8>().ok());
        if niveau != Some(3) {
            continue;
        }
        if let Ok(taille) = fs::read_to_string(chemin.join("size")) {
            max = max.max(analyser_taille(taille.trim()));
        }
    }
    max
}

/// Convertit une taille du noyau (`"32768K"`, `"16M"`, `"1024"`) en octets.
fn analyser_taille(brut: &str) -> u64 {
    let (nombre, facteur) = match brut.as_bytes().last() {
        Some(b'K' | b'k') => (&brut[..brut.len() - 1], 1024),
        Some(b'M' | b'm') => (&brut[..brut.len() - 1], 1024 * 1024),
        Some(b'G' | b'g') => (&brut[..brut.len() - 1], 1024 * 1024 * 1024),
        _ => (brut, 1),
    };
    nombre.trim().parse::<u64>().unwrap_or(0) * facteur
}

/// Compte les noeuds NUMA exposes par le noyau.
fn noeuds_numa() -> u8 {
    let Ok(entrees) = fs::read_dir("/sys/devices/system/node") else {
        return 0;
    };
    let compte = entrees
        .flatten()
        .filter(|e| {
            e.file_name().to_str().is_some_and(|n| {
                n.strip_prefix("node")
                    .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit()))
            })
        })
        .count();
    u8::try_from(compte).unwrap_or(u8::MAX)
}

/// Memoire physique totale, lue depuis `MemTotal` de `/proc/meminfo`.
fn memoire_totale() -> u64 {
    let Ok(contenu) = fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    for ligne in contenu.lines() {
        if let Some(reste) = ligne.strip_prefix("MemTotal:") {
            // Format : "MemTotal:       16342184 kB"
            let mut morceaux = reste.split_whitespace();
            let valeur = morceaux.next().and_then(|v| v.parse::<u64>().ok());
            let unite = morceaux.next().unwrap_or("kB");
            if let Some(v) = valeur {
                return match unite {
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
    fn les_tailles_du_noyau_sont_converties() {
        assert_eq!(analyser_taille("32768K"), 32768 * 1024);
        assert_eq!(analyser_taille("16M"), 16 * 1024 * 1024);
        assert_eq!(analyser_taille("1G"), 1024 * 1024 * 1024);
        assert_eq!(analyser_taille("1024"), 1024);
    }

    #[test]
    fn une_taille_illisible_vaut_zero_et_non_une_valeur_devinee() {
        assert_eq!(analyser_taille(""), 0);
        assert_eq!(analyser_taille("inconnu"), 0);
        assert_eq!(analyser_taille("K"), 0);
    }

    #[test]
    fn la_sonde_linux_mesure_la_machine() {
        let s = sonder();
        assert!(s.mem_total_bytes > 0, "memoire totale non detectee");
        assert!(s.physical_cores > 0, "coeurs physiques non detectes");
    }
}
