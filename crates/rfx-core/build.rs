//! Tampon du code d'analyse (ADR-064).
//!
//! Un descripteur persiste dans `workload.db` n'est relu que s'il a ete produit par le
//! meme code d'analyse. La version du paquet ne change pas d'une modification a l'autre :
//! le tampon est donc un hachage FNV-1a du source de tout ce qui produit les faits d'un
//! descripteur, fichiers lus dans un ordre fixe, fins de ligne normalisees. Toute
//! modification, meme d'un commentaire, change le tampon et fait recalculer : en cas de
//! doute, on recalcule (UNKNOWN = CONSERVATIVE).

use std::fs;
use std::path::{Path, PathBuf};

/// Repertoires et fichiers dont le source entre dans le tampon, relatifs a ce crate.
const SOURCES: [&str; 5] = [
    "../rfx-analyzer/src",
    "../rfx-bytecode/src",
    "src/analysis.rs",
    "../rfx-model/src/classification.rs",
    "../rfx-model/src/analysis.rs",
];

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Fichiers `.rs` sous `path`, recursivement, tries.
fn rust_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        panic!("source du tampon d'analyse illisible : {}", path.display());
    };
    let mut children: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    children.sort();
    for child in children {
        if child.is_dir() {
            rust_files(&child, out);
        } else if child.extension().is_some_and(|ext| ext == "rs") {
            out.push(child);
        }
    }
}

fn main() {
    let mut files = Vec::new();
    for source in SOURCES {
        println!("cargo:rerun-if-changed={source}");
        rust_files(Path::new(source), &mut files);
    }
    let mut hash = FNV_OFFSET;
    for file in &files {
        let Ok(text) = fs::read(file) else {
            panic!("source du tampon d'analyse illisible : {}", file.display());
        };
        // Le chemin relatif entre dans le tampon : deplacer du code change l'analyse
        // aussi surement que le modifier. Separateurs unifies d'un systeme a l'autre.
        let name: String = file
            .to_string_lossy()
            .chars()
            .map(|c| if std::path::is_separator(c) { '/' } else { c })
            .collect();
        hash = fnv1a(hash, name.as_bytes());
        let normalized: Vec<u8> = text.into_iter().filter(|&b| b != b'\r').collect();
        hash = fnv1a(hash, &normalized);
    }
    println!("cargo:rustc-env=RFX_ANALYSIS_STAMP={hash}");
}
