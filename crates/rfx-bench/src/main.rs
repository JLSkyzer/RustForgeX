//! C-36 : collecte des micro-benchmarks vers le format de résultat normatif.
//!
//! Cahier des charges : PARTIE 21. Exigences : R-580 et R-860 (aucun chiffre publié
//! qui ne vienne d'un fichier généré), R-581 (aucun benchmark sans intervalle de
//! confiance ni nombre de répétitions), R-861 (résultat JSON versionné dans
//! `benchmarks/results/`). Maturité : `STABLE`.
//!
//! Criterion écrit ses mesures brutes sous `target/criterion/`. Ce programme les relit,
//! en tire médiane, écart interquartile et intervalle de confiance, et écrit un
//! fichier au schéma de la PARTIE 21.3.
//!
//! # Pourquoi recalculer médiane et IQR plutôt que lire celles de criterion
//!
//! Le fichier `estimates.json` de criterion donne une médiane et un écart absolu
//! médian, pas l'écart interquartile qu'exige le schéma. Les recalculer depuis les
//! échantillons bruts évite de faire passer une statistique pour une autre — et rend
//! le calcul lisible ici même, plutôt que de dépendre de la définition qu'en retient
//! une version donnée de criterion.
//!
//! # Ce que ce programme n'est pas
//!
//! Ce n'est pas le harnais macro. Il ne mesure rien en jeu, ne connaît ni MSPT ni TPS,
//! et ne peut donc pas répondre à la question « quel est l'overhead de RUSTFORGE-X ».
//! Cette réponse viendra du niveau B, décrit en PARTIE 21.2.

mod digest_diff;
mod macro_bench;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Version du schéma de résultat (PARTIE 21.3).
const RESULT_SCHEMA: u32 = 1;

/// Niveau de confiance des intervalles rapportés par criterion.
const CONFIDENCE_LEVEL: f64 = 0.95;

/// Une mesure publiée, au format du schéma normatif.
#[derive(Debug, Serialize)]
struct Metric {
    /// Médiane des temps par itération.
    median: f64,
    /// Écart interquartile : la dispersion, jamais la moyenne seule (PARTIE 21.3).
    iqr: f64,
    /// Borne basse de l'intervalle de confiance de la médiane.
    ci_lower: f64,
    /// Borne haute de l'intervalle de confiance de la médiane.
    ci_upper: f64,
    /// Niveau de confiance de cet intervalle.
    confidence_level: f64,
    /// Nombre d'échantillons — R-581 l'exige explicitement.
    samples: usize,
    /// Unité, toujours la nanoseconde pour les micro-benchmarks.
    unit: &'static str,
}

/// Matériel de la machine de mesure, tel que C-45 le rapporte.
#[derive(Debug, Serialize)]
struct Hardware {
    /// Cœurs physiques mesurés.
    physical_cores: u16,
    /// Cœurs logiques mesurés.
    logical_cores: u16,
    /// Mémoire physique totale, en gibioctets.
    ram_gb: u64,
    /// `false` si la sonde matérielle n'a pas pu mesurer les cœurs : les valeurs
    /// ci-dessus ne sont alors pas des mesures, et le résultat le dit.
    probed: bool,
}

/// Fichier de résultat, au schéma de la PARTIE 21.3.
#[derive(Debug, Serialize)]
struct BenchmarkResult {
    /// Version du schéma.
    schema: u32,
    /// Version du paquet mesuré.
    rfx_version: String,
    /// Commit mesuré, ou `"unknown"` si le dépôt n'a pas pu être interrogé.
    commit: String,
    /// `true` si l'arbre de travail portait des modifications non commitées.
    dirty: bool,
    /// Niveau du harnais : `"micro"` ici, `"macro"` pour le niveau B.
    level: &'static str,
    /// Horodatage de la collecte, en secondes depuis l'époque Unix.
    collected_at: u64,
    /// Matériel de mesure.
    hardware: Hardware,
    /// Mesures, indexées par identifiant de benchmark.
    metrics: BTreeMap<String, Metric>,
    /// Remarques : tout ce qui limite la lecture du résultat.
    notes: Vec<String>,
}

/// Échantillons bruts écrits par criterion.
#[derive(Debug, Deserialize)]
struct Sample {
    /// Nombre d'itérations de chaque échantillon.
    iters: Vec<f64>,
    /// Temps total de chaque échantillon, en nanosecondes.
    times: Vec<f64>,
}

/// Estimation d'une statistique par criterion.
#[derive(Debug, Deserialize)]
struct Estimate {
    confidence_interval: ConfidenceInterval,
}

/// Intervalle de confiance rapporté par criterion.
#[derive(Debug, Deserialize)]
struct ConfidenceInterval {
    lower_bound: f64,
    upper_bound: f64,
}

/// Estimations écrites par criterion pour un benchmark.
#[derive(Debug, Deserialize)]
struct Estimates {
    median: Estimate,
}

fn main() {
    let level = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "micro".to_owned());
    match level.as_str() {
        "micro" => micro(),
        "macro" => macro_level(),
        "digest" => digest_level(),
        other => {
            eprintln!("niveau inconnu « {other} » — attendu : micro | macro | digest");
            std::process::exit(2);
        }
    }
}

/// Test de gameplay (PARTIE 20.3.4) : egalite d'etat entre references et candidat.
///
/// `rfx-bench digest <reference> <seconde-reference> <candidat>`. Code de sortie : `0` si
/// rien n'est attribuable au candidat, `1` s'il diverge hors du bruit, `2` si la
/// comparaison ne vaut rien (fichier illisible, graines differentes, generation
/// incomplete).
fn digest_level() {
    let paths: Vec<String> = std::env::args().skip(2).collect();
    if paths.len() != 3 {
        eprintln!("usage : rfx-bench digest <reference> <seconde-reference> <candidat>");
        std::process::exit(2);
    }
    let load = |path: &str| -> digest_diff::DigestFile {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("{path} : illisible ({e})");
            std::process::exit(2);
        });
        serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("{path} : format inattendu ({e})");
            std::process::exit(2);
        })
    };
    let (reference, second, candidate) = (load(&paths[0]), load(&paths[1]), load(&paths[2]));
    let verdict = digest_diff::judge(&reference, &second, &candidate);

    println!(
        "references « {} » et « {} », candidat « {} » (RUSTFORGE-X {}, {} methodes sondees)",
        reference.label,
        second.label,
        candidate.label,
        if candidate.rfx_active {
            "actif"
        } else {
            "inactif"
        },
        candidate.methods_probed
    );
    println!(
        "{} chunks presents dans les trois executions",
        verdict.compared
    );
    println!(
        "{:<16}{:>9}{:>11}{:>11}{:>13}{:>13}{:>9}  verdict",
        "composante", "egaux", "ref1 seule", "ref2 seule", "cand. seul", "3 differents", "p"
    );
    for c in &verdict.components {
        let state = match (c.passed(), c.deterministic()) {
            (true, true) => "egalite stricte",
            (true, false) => "passe, resolution limitee par le bruit du jeu",
            (false, true) => "ECART sur une composante deterministe",
            (false, false) => "ECART : candidat intrus plus souvent que le hasard",
        };
        let p = if c.deterministic() {
            "-".to_owned()
        } else {
            format!("{:.3}", c.p_value())
        };
        println!(
            "{:<16}{:>9}{:>11}{:>11}{:>13}{:>13}{:>9}  {state}",
            c.name, c.all_equal, c.reference_odd, c.second_odd, c.candidate_odd, c.all_different, p
        );
    }
    println!(
        "seuil : p < {} sur une composante bruitee ; egalite stricte sur une composante \
         deterministe",
        digest_diff::ALPHA
    );
    for problem in &verdict.invalid {
        println!("INVALIDE : {problem}");
    }

    if !verdict.invalid.is_empty() {
        println!("VERDICT : comparaison invalide");
        std::process::exit(2);
    }
    if verdict.passed() {
        println!("VERDICT : le candidat n'est pas plus ecarte que les references");
    } else {
        println!("VERDICT : DIVERGENCE imputable au candidat");
        std::process::exit(1);
    }
}

/// Niveau B : agrège les exécutions du serveur en un résultat comparatif.
fn macro_level() {
    let root = repository_root();
    let runs_dir = root.join("benchmarks").join("runs");

    let (result, mut notes) = match macro_bench::aggregate(&runs_dir) {
        Ok(pair) => pair,
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(2);
        }
    };

    let (commit, dirty) = git_state(&root);
    if dirty {
        notes.push(
            "arbre de travail modifié : ce résultat ne décrit aucun commit publiable".to_owned(),
        );
    }
    for (label, configuration) in &result.configurations {
        if !configuration.methodology_compliant {
            notes.push(format!(
                "configuration « {label} » non conforme : {}",
                configuration.compliance_notes.join(" ; ")
            ));
        }
        if configuration.rejected {
            notes.push(format!(
                "configuration « {label} » rejetée : dispersion inter-exécutions au-delà                  du seuil (PARTIE 21.3, point 7)"
            ));
        }
    }

    let (hardware, coverage) = rfx_core::hw::probe();
    let collected_at = unix_time();

    #[derive(Serialize)]
    struct MacroFile<'a> {
        schema: u32,
        rfx_version: String,
        commit: String,
        dirty: bool,
        level: &'static str,
        collected_at: u64,
        hardware: Hardware,
        #[serde(flatten)]
        result: &'a macro_bench::MacroResult,
        notes: Vec<String>,
    }

    let file = MacroFile {
        schema: RESULT_SCHEMA,
        rfx_version: env!("CARGO_PKG_VERSION").to_owned(),
        commit: commit.clone(),
        dirty,
        level: "macro",
        collected_at,
        hardware: Hardware {
            physical_cores: hardware.physical_cores,
            logical_cores: hardware.logical_cores,
            ram_gb: hardware.mem_total_bytes / (1024 * 1024 * 1024),
            probed: coverage.cores,
        },
        result: &result,
        notes,
    };

    let out_dir = root.join("benchmarks").join("results");
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("impossible de créer {} : {e}", out_dir.display());
        std::process::exit(1);
    }
    let out = out_dir.join(format!("macro-{collected_at}-{}.json", short(&commit)));

    let json = match serde_json::to_string_pretty(&file) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("sérialisation impossible : {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = std::fs::write(&out, json + "\n") {
        eprintln!("écriture impossible dans {} : {e}", out.display());
        std::process::exit(1);
    }

    println!(
        "{} configuration(s) agrégée(s) dans {}",
        file.result.configurations.len(),
        out.display()
    );
    if file.result.comparisons.is_empty() {
        println!(
            "ATTENTION : aucune comparaison produite — il faut au moins une référence \
             et une variante"
        );
    }
    for comparison in &file.result.comparisons {
        if !comparison.trustworthy {
            println!(
                "ATTENTION : {} contre {} — campagne non conforme ou rejetée ; ce \
                 résultat sert à décider, pas à publier",
                comparison.candidate, comparison.baseline
            );
        }
    }
}

/// Niveau A : collecte les micro-benchmarks de criterion.
fn micro() {
    let root = repository_root();
    let criterion_dir = root.join("target").join("criterion");
    if !criterion_dir.is_dir() {
        eprintln!(
            "aucune mesure sous {} — lancer d'abord `cargo bench -p rfx-bench`",
            criterion_dir.display()
        );
        std::process::exit(2);
    }

    let mut metrics = BTreeMap::new();
    let mut notes = Vec::new();
    collect(&criterion_dir, &criterion_dir, &mut metrics, &mut notes);

    if metrics.is_empty() {
        eprintln!(
            "aucun benchmark exploitable trouvé sous {}",
            criterion_dir.display()
        );
        std::process::exit(2);
    }

    let (hardware, coverage) = rfx_core::hw::probe();
    let (commit, dirty) = git_state(&root);
    if dirty {
        notes.push(
            "arbre de travail modifié : ce résultat ne décrit aucun commit publiable".to_owned(),
        );
    }
    notes.push(
        "niveau micro : ne mesure pas l'overhead en jeu, qui relève du niveau B \
         (PARTIE 21.2)"
            .to_owned(),
    );

    let result = BenchmarkResult {
        schema: RESULT_SCHEMA,
        rfx_version: env!("CARGO_PKG_VERSION").to_owned(),
        commit,
        dirty,
        level: "micro",
        collected_at: unix_time(),
        hardware: Hardware {
            physical_cores: hardware.physical_cores,
            logical_cores: hardware.logical_cores,
            ram_gb: hardware.mem_total_bytes / (1024 * 1024 * 1024),
            probed: coverage.cores,
        },
        metrics,
        notes,
    };

    let out_dir = root.join("benchmarks").join("results");
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("impossible de créer {} : {e}", out_dir.display());
        std::process::exit(1);
    }
    let out = out_dir.join(format!(
        "micro-{}-{}.json",
        result.collected_at,
        short(&result.commit)
    ));

    let json = match serde_json::to_string_pretty(&result) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("sérialisation impossible : {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = std::fs::write(&out, json + "\n") {
        eprintln!("écriture impossible dans {} : {e}", out.display());
        std::process::exit(1);
    }

    println!(
        "{} mesures écrites dans {}",
        result.metrics.len(),
        out.display()
    );
}

/// Parcourt l'arborescence de criterion et retient chaque benchmark exploitable.
fn collect(
    root: &Path,
    dir: &Path,
    metrics: &mut BTreeMap<String, Metric>,
    notes: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().is_some_and(|n| n == "new") {
            match measure(&path) {
                Ok(metric) => {
                    let id = benchmark_id(root, &path);
                    metrics.insert(id, metric);
                }
                Err(reason) => notes.push(format!("{} ignoré : {reason}", path.display())),
            }
            continue;
        }
        // `base` et `change` sont les comparaisons de criterion, pas des mesures.
        if path
            .file_name()
            .is_some_and(|n| n == "base" || n == "change")
        {
            continue;
        }
        collect(root, &path, metrics, notes);
    }
}

/// Identifiant lisible d'un benchmark : son chemin relatif, sans le suffixe `new`.
fn benchmark_id(root: &Path, new_dir: &Path) -> String {
    new_dir
        .parent()
        .and_then(|p| p.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| new_dir.to_string_lossy().into_owned())
}

/// Construit la mesure d'un benchmark depuis les fichiers de criterion.
fn measure(new_dir: &Path) -> Result<Metric, String> {
    let sample: Sample = read_json(&new_dir.join("sample.json"))?;
    let estimates: Estimates = read_json(&new_dir.join("estimates.json"))?;

    if sample.iters.len() != sample.times.len() || sample.iters.is_empty() {
        return Err("échantillons incohérents".to_owned());
    }

    // Temps par iteration : criterion mesure des lots, pas des appels isolés.
    let mut per_iter: Vec<f64> = sample
        .iters
        .iter()
        .zip(sample.times.iter())
        .filter(|(iters, _)| **iters > 0.0)
        .map(|(iters, total)| total / iters)
        .collect();
    if per_iter.is_empty() {
        return Err("aucun échantillon exploitable".to_owned());
    }
    per_iter.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median = quantile(&per_iter, 0.50);
    let iqr = quantile(&per_iter, 0.75) - quantile(&per_iter, 0.25);

    Ok(Metric {
        median,
        iqr,
        ci_lower: estimates.median.confidence_interval.lower_bound,
        ci_upper: estimates.median.confidence_interval.upper_bound,
        confidence_level: CONFIDENCE_LEVEL,
        samples: per_iter.len(),
        unit: "ns",
    })
}

/// Quantile par interpolation linéaire sur un échantillon déjà trié.
pub(crate) fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let position = q.clamp(0.0, 1.0) * ((sorted.len() - 1) as f64);
    let low = position.floor() as usize;
    let high = position.ceil() as usize;
    if low == high {
        return sorted[low];
    }
    let weight = position - (low as f64);
    sorted[low] * (1.0 - weight) + sorted[high] * weight
}

/// Lit et décode un fichier JSON.
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{} illisible : {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{} indécodable : {e}", path.display()))
}

/// Racine du dépôt : le répertoire du crate, remonté de deux niveaux.
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Commit courant et propreté de l'arbre de travail.
///
/// Un résultat qui ne cite pas son commit ne prouve rien (PARTIE 21.3, point 9), et un
/// résultat mesuré sur un arbre modifié ne décrit aucun état publiable — les deux sont
/// donc rapportés, jamais devinés.
fn git_state(root: &Path) -> (String, bool) {
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());

    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(true);

    (commit, dirty)
}

/// Sept premiers caractères d'un identifiant de commit.
fn short(commit: &str) -> &str {
    &commit[..commit.len().min(7)]
}

/// Horodatage Unix, en secondes.
fn unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_interpolate_between_samples() {
        let sorted = [10.0, 20.0, 30.0, 40.0];
        assert!((quantile(&sorted, 0.0) - 10.0).abs() < f64::EPSILON);
        assert!((quantile(&sorted, 1.0) - 40.0).abs() < f64::EPSILON);
        assert!((quantile(&sorted, 0.5) - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_interquartile_range_measures_dispersion() {
        let tight = [100.0, 101.0, 102.0, 103.0, 104.0];
        let spread = [10.0, 60.0, 100.0, 140.0, 190.0];
        let iqr = |s: &[f64]| quantile(s, 0.75) - quantile(s, 0.25);
        assert!(iqr(&tight) < iqr(&spread));
    }

    #[test]
    fn a_single_sample_has_no_dispersion() {
        assert!((quantile(&[42.0], 0.25) - 42.0).abs() < f64::EPSILON);
        assert!((quantile(&[42.0], 0.75) - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn an_empty_sample_never_panics() {
        assert!((quantile(&[], 0.5) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_short_commit_is_never_cut_mid_string() {
        assert_eq!(short("abcdef1234"), "abcdef1");
        assert_eq!(short("abc"), "abc");
        assert_eq!(short("unknown"), "unknown");
    }
}
