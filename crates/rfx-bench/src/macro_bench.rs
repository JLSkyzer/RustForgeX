//! C-36, niveau B : agrégation des exécutions de macro-benchmark.
//!
//! Cahier des charges : PARTIE 21.2 et 21.3. Exigences : R-581 (jamais de mesure sans
//! dispersion ni nombre de répétitions), R-861 (résultat JSON versionné).
//! Maturité : `STABLE`.
//!
//! Chaque exécution du serveur écrit un fichier dans `benchmarks/runs/`. Ce module les
//! relit, les groupe par configuration, et rend pour chaque métrique la médiane et
//! l'écart interquartile **entre exécutions** — jamais la moyenne seule, et jamais un
//! chiffre tiré d'une exécution unique.
//!
//! # Ce qui distingue une mesure d'une intuition
//!
//! Trois garde-fous, tous dans le fichier produit :
//!
//! - la **conformité** à la méthodologie de la PARTIE 21.3 est calculée, pas
//!   supposée. Une campagne trop courte produit un résultat marqué non conforme, qui
//!   ne pourra jamais être cité comme une mesure publiable ;
//! - une **dispersion inter-exécutions supérieure à 10 %** invalide la configuration
//!   (PARTIE 21.3, point 7) : la machine n'était pas au repos, et le chiffre ne
//!   décrit pas le logiciel ;
//! - la **comparaison** n'est produite que si exactement deux configurations sont
//!   présentes, et elle nomme laquelle sert de référence.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Exécutions minimales exigées par la PARTIE 21.3, point 5.
const REQUIRED_RUNS: usize = 5;

/// Ticks d'échauffement minimaux : trois minutes à vingt ticks par seconde.
const REQUIRED_WARMUP_TICKS: u64 = 3_600;

/// Ticks mesurés minimaux (PARTIE 21.3, point 4).
const REQUIRED_MEASURED_TICKS: u64 = 12_000;

/// Dispersion inter-exécutions au-delà de laquelle la campagne est invalidée.
const MAX_RELATIVE_STDDEV: f64 = 0.10;

/// Métrique servant de juge pour le rejet d'exécution.
const JUDGE_METRIC: &str = "mspt_p95";

/// Fichier écrit par une exécution du serveur.
#[derive(Debug, Deserialize)]
pub struct RunFile {
    /// Étiquette de configuration comparée.
    pub label: String,
    /// Numéro de l'exécution.
    pub run: u32,
    /// Profil de charge appliqué au monde (PARTIE 22).
    ///
    /// Absent des exécutions écrites avant que le harnais sache appliquer une charge :
    /// une campagne archivée reste donc lisible, et se déclare de charge inconnue.
    #[serde(default)]
    pub load: Option<String>,
    /// État de RUSTFORGE-X à l'arrêt, dont la surface sondée.
    #[serde(default)]
    pub rfx: Option<RunRfx>,
    /// Ticks d'échauffement ignorés.
    pub warmup_ticks: u64,
    /// Ticks effectivement mesurés.
    pub measured_ticks: u64,
    /// Durée réelle de la fenêtre de mesure.
    pub window_seconds: f64,
    /// Mesures de cette exécution.
    pub metrics: BTreeMap<String, f64>,
}

/// Ce que l'exécution rapporte de RUSTFORGE-X lui-même.
#[derive(Debug, Deserialize)]
pub struct RunRfx {
    /// Méthodes portant une sonde à l'arrêt du serveur.
    #[serde(default)]
    pub methods_probed: u64,
}

/// Statistique d'une métrique sur l'ensemble des exécutions d'une configuration.
#[derive(Debug, Serialize)]
pub struct Aggregate {
    /// Médiane inter-exécutions.
    pub median: f64,
    /// Écart interquartile inter-exécutions.
    pub iqr: f64,
    /// Écart-type relatif, qui décide du rejet (PARTIE 21.3, point 7).
    pub relative_stddev: f64,
    /// Plus petite valeur observée.
    pub min: f64,
    /// Plus grande valeur observée.
    pub max: f64,
    /// Nombre d'exécutions ayant contribué — R-581 l'exige.
    pub runs: usize,
}

/// Résultat agrégé d'une configuration.
#[derive(Debug, Serialize)]
pub struct Configuration {
    /// Exécutions retenues.
    pub runs: usize,
    /// Numéros des exécutions ayant contribué : une exécution perdue se voit ici.
    pub run_indices: Vec<u32>,
    /// Ticks d'échauffement de la campagne.
    pub warmup_ticks: u64,
    /// Ticks mesurés par exécution.
    pub measured_ticks: u64,
    /// Profil de charge, ou `inconnue` si les exécutions n'en déclarent pas.
    ///
    /// Vaut `mêlées` si les exécutions agrégées n'ont pas tourné sous la même charge :
    /// les agréger n'aurait alors aucun sens, et le dire vaut mieux que le taire.
    pub load: String,
    /// Méthodes sondées, médiane des exécutions ; `0` sans instrumentation.
    ///
    /// Décisif pour comparer deux campagnes : le surcoût dépend de la surface sondée
    /// (ADR-021), et le moment de l'armement la fait varier d'un facteur cinq
    /// (ADR-022). Deux résultats qui ne l'exposent pas ne sont pas comparables.
    pub methods_probed: u64,
    /// `true` si la campagne suit la méthodologie de la PARTIE 21.3.
    pub methodology_compliant: bool,
    /// Ce qui manque à la conformité, en clair.
    pub compliance_notes: Vec<String>,
    /// `true` si la dispersion inter-exécutions dépasse le seuil de rejet.
    pub rejected: bool,
    /// Métriques agrégées.
    pub metrics: BTreeMap<String, Aggregate>,
}

/// Comparaison de deux configurations.
#[derive(Debug, Serialize)]
pub struct Comparison {
    /// Configuration de référence, au dénominateur.
    pub baseline: String,
    /// Configuration comparée, au numérateur.
    pub candidate: String,
    /// Écart relatif par métrique, en pourcentage de la référence.
    pub delta_pct: BTreeMap<String, f64>,
    /// `true` si les deux configurations sont conformes et non rejetées.
    pub trustworthy: bool,
}

/// Résultat complet du niveau B.
#[derive(Debug, Serialize)]
pub struct MacroResult {
    /// Configurations mesurées, par étiquette.
    pub configurations: BTreeMap<String, Configuration>,
    /// Comparaison, présente seulement si exactement deux configurations existent.
    pub comparisons: Vec<Comparison>,
}

/// Lit toutes les exécutions d'un répertoire et les agrège.
///
/// # Erreurs
///
/// Renvoie un message si le répertoire est illisible ou ne contient aucune exécution
/// exploitable.
pub fn aggregate(runs_dir: &Path) -> Result<(MacroResult, Vec<String>), String> {
    let mut by_label: BTreeMap<String, Vec<RunFile>> = BTreeMap::new();
    let mut notes = Vec::new();

    let entries = std::fs::read_dir(runs_dir)
        .map_err(|e| format!("{} illisible : {e}", runs_dir.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_slice::<RunFile>(&b).map_err(|e| e.to_string()))
        {
            Ok(run) => by_label.entry(run.label.clone()).or_default().push(run),
            Err(reason) => notes.push(format!("{} ignoré : {reason}", path.display())),
        }
    }

    if by_label.is_empty() {
        return Err(format!(
            "aucune exécution exploitable dans {}",
            runs_dir.display()
        ));
    }

    let configurations: BTreeMap<String, Configuration> = by_label
        .into_iter()
        .map(|(label, runs)| (label, summarize(&runs)))
        .collect();

    let comparisons = compare(&configurations);

    Ok((
        MacroResult {
            configurations,
            comparisons,
        },
        notes,
    ))
}

/// Agrège les exécutions d'une configuration.
fn summarize(runs: &[RunFile]) -> Configuration {
    let mut metrics: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for run in runs {
        for (name, value) in &run.metrics {
            metrics.entry(name.clone()).or_default().push(*value);
        }
        // La durée réelle de la fenêtre est une mesure comme une autre : deux
        // exécutions censées durer le même temps et qui divergent signalent une
        // machine occupée, avant même de regarder le MSPT.
        metrics
            .entry("window_seconds".to_owned())
            .or_default()
            .push(run.window_seconds);
    }

    let mut run_indices: Vec<u32> = runs.iter().map(|r| r.run).collect();
    run_indices.sort_unstable();

    let aggregated: BTreeMap<String, Aggregate> = metrics
        .into_iter()
        .map(|(name, values)| (name, aggregate_values(&values)))
        .collect();

    let warmup = runs.iter().map(|r| r.warmup_ticks).min().unwrap_or(0);
    let measured = runs.iter().map(|r| r.measured_ticks).min().unwrap_or(0);

    let mut loads: Vec<&str> = runs
        .iter()
        .map(|r| r.load.as_deref().unwrap_or("inconnue"))
        .collect();
    loads.sort_unstable();
    loads.dedup();
    let load = if loads.len() == 1 {
        loads[0].to_string()
    } else {
        format!("mêlées ({})", loads.join(", "))
    };

    let mut probed: Vec<u64> = runs
        .iter()
        .map(|r| r.rfx.as_ref().map_or(0, |x| x.methods_probed))
        .collect();
    probed.sort_unstable();
    let methods_probed = probed.get(probed.len() / 2).copied().unwrap_or(0);

    let mut compliance_notes = Vec::new();
    if runs.len() < REQUIRED_RUNS {
        compliance_notes.push(format!(
            "{} exécutions au lieu des {REQUIRED_RUNS} exigées (PARTIE 21.3, point 5)",
            runs.len()
        ));
    }
    if warmup < REQUIRED_WARMUP_TICKS {
        compliance_notes.push(format!(
            "{warmup} ticks d'échauffement au lieu des {REQUIRED_WARMUP_TICKS} exigés \
             (PARTIE 21.3, point 3)"
        ));
    }
    if measured < REQUIRED_MEASURED_TICKS {
        compliance_notes.push(format!(
            "{measured} ticks mesurés au lieu des {REQUIRED_MEASURED_TICKS} exigés \
             (PARTIE 21.3, point 4)"
        ));
    }

    let rejected = aggregated
        .get(JUDGE_METRIC)
        .is_some_and(|a| a.relative_stddev > MAX_RELATIVE_STDDEV);

    Configuration {
        runs: runs.len(),
        run_indices,
        warmup_ticks: warmup,
        measured_ticks: measured,
        load,
        methods_probed,
        methodology_compliant: compliance_notes.is_empty(),
        compliance_notes,
        rejected,
        metrics: aggregated,
    }
}

/// Médiane, écart interquartile et dispersion d'une série de valeurs.
pub(crate) fn aggregate_values(values: &[f64]) -> Aggregate {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median = crate::quantile(&sorted, 0.50);
    let iqr = crate::quantile(&sorted, 0.75) - crate::quantile(&sorted, 0.25);

    let mean = if sorted.is_empty() {
        0.0
    } else {
        sorted.iter().sum::<f64>() / (sorted.len() as f64)
    };
    // Écart-type d'échantillon : une seule exécution ne dit rien de la dispersion, et
    // rendre zéro laisserait croire à une mesure stable.
    let relative_stddev = if sorted.len() < 2 || mean == 0.0 {
        f64::NAN
    } else {
        let variance =
            sorted.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / ((sorted.len() - 1) as f64);
        variance.sqrt() / mean.abs()
    };

    Aggregate {
        median,
        iqr,
        relative_stddev,
        min: sorted.first().copied().unwrap_or(0.0),
        max: sorted.last().copied().unwrap_or(0.0),
        runs: sorted.len(),
    }
}

/// Compare deux configurations, si et seulement s'il y en a exactement deux.
///
/// La référence est celle dont l'étiquette vient en premier dans l'ordre
/// alphabétique, ce qui rend la comparaison reproductible sans avoir à la déclarer.
/// **Toutes** les autres configurations lui sont comparées : une campagne à trois
/// configurations — la référence et deux variantes du même système — est un cas
/// normal, et n'en comparer que deux en abandonnerait une sans le dire.
fn compare(configurations: &BTreeMap<String, Configuration>) -> Vec<Comparison> {
    let mut iter = configurations.iter();
    let Some((baseline_label, baseline)) = iter.next() else {
        return Vec::new();
    };

    iter.map(|(candidate_label, candidate)| {
        let mut delta_pct = BTreeMap::new();
        for (name, base) in &baseline.metrics {
            if let Some(cand) = candidate.metrics.get(name) {
                if base.median != 0.0 {
                    delta_pct.insert(
                        name.clone(),
                        (cand.median - base.median) / base.median * 100.0,
                    );
                }
            }
        }

        Comparison {
            baseline: baseline_label.clone(),
            candidate: candidate_label.clone(),
            delta_pct,
            trustworthy: baseline.methodology_compliant
                && candidate.methodology_compliant
                && !baseline.rejected
                && !candidate.rejected,
        }
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(label: &str, index: u32, p95: f64) -> RunFile {
        RunFile {
            label: label.to_owned(),
            run: index,
            load: Some("mobs".to_owned()),
            rfx: Some(RunRfx {
                methods_probed: 2_635,
            }),
            warmup_ticks: REQUIRED_WARMUP_TICKS,
            measured_ticks: REQUIRED_MEASURED_TICKS,
            window_seconds: 600.0,
            metrics: BTreeMap::from([("mspt_p95".to_owned(), p95)]),
        }
    }

    /// Deux campagnes qui ne disent pas sous quelle charge ni avec quelle surface
    /// sondée elles ont tourné ne sont pas comparables. C'est arrivé : le résultat
    /// agrégé taisait les deux, et deux campagnes ont failli être mises en regard sans
    /// qu'on puisse vérifier qu'elles portaient sur la même chose (ADR-022).
    #[test]
    fn an_aggregate_states_its_load_and_its_probed_surface() {
        let runs: Vec<RunFile> = (1..=5).map(|n| run("rfx-on", n, 10.0)).collect();
        let summary = summarize(&runs);

        assert_eq!(summary.load, "mobs");
        assert_eq!(summary.methods_probed, 2_635);
    }

    /// Agréger des exécutions de charges différentes n'a aucun sens. Le harnais ne
    /// l'interdit pas — une campagne interrompue puis reprise autrement arrive — mais
    /// il doit le dire, faute de quoi la moyenne passerait pour une mesure.
    #[test]
    fn mixed_loads_are_named_as_such() {
        let mut runs: Vec<RunFile> = (1..=4).map(|n| run("rfx-on", n, 10.0)).collect();
        runs[3].load = Some("none".to_owned());

        assert_eq!(summarize(&runs).load, "mêlées (mobs, none)");
    }

    /// Une campagne archivée avant que le harnais sache appliquer une charge doit
    /// rester lisible : elle se déclare de charge inconnue, sans échouer.
    #[test]
    fn an_archived_campaign_without_a_load_stays_readable() {
        let mut runs: Vec<RunFile> = (1..=5).map(|n| run("rfx-off", n, 10.0)).collect();
        for r in &mut runs {
            r.load = None;
            r.rfx = None;
        }
        let summary = summarize(&runs);

        assert_eq!(summary.load, "inconnue");
        assert_eq!(summary.methods_probed, 0);
    }

    #[test]
    fn a_compliant_campaign_is_recognised_as_such() {
        let runs: Vec<RunFile> = (1..=5)
            .map(|n| run("rfx-off", n, 10.0 + f64::from(n) * 0.01))
            .collect();
        let summary = summarize(&runs);
        assert!(
            summary.methodology_compliant,
            "{:?}",
            summary.compliance_notes
        );
        assert!(!summary.rejected);
        assert_eq!(summary.runs, 5);
    }

    #[test]
    fn a_short_campaign_is_marked_non_compliant_with_its_reasons() {
        let mut short = run("rfx-off", 1, 10.0);
        short.warmup_ticks = 100;
        short.measured_ticks = 200;
        let summary = summarize(&[short]);

        assert!(!summary.methodology_compliant);
        assert_eq!(
            summary.compliance_notes.len(),
            3,
            "exécutions, échauffement et durée manquent tous les trois"
        );
    }

    /// PARTIE 21.3, point 7 : au-delà de 10 % de dispersion, la campagne est invalidée.
    #[test]
    fn an_unstable_campaign_is_rejected() {
        let runs: Vec<RunFile> = [10.0, 11.0, 9.0, 25.0, 8.0]
            .iter()
            .enumerate()
            .map(|(n, v)| run("rfx-on", n as u32, *v))
            .collect();
        let summary = summarize(&runs);
        assert!(
            summary.rejected,
            "dispersion de la machine, pas du logiciel"
        );
    }

    #[test]
    fn a_single_run_never_claims_a_dispersion_it_cannot_know() {
        let summary = summarize(&[run("rfx-on", 1, 10.0)]);
        let judge = &summary.metrics[JUDGE_METRIC];
        assert!(judge.relative_stddev.is_nan(), "une exécution ne dit rien");
        assert!(!summary.rejected, "on ne rejette pas sur une non-mesure");
    }

    #[test]
    fn the_comparison_names_its_baseline() {
        let mut configurations = BTreeMap::new();
        configurations.insert(
            "a-off".to_owned(),
            summarize(&(1..=5).map(|n| run("a-off", n, 10.0)).collect::<Vec<_>>()),
        );
        configurations.insert(
            "b-on".to_owned(),
            summarize(&(1..=5).map(|n| run("b-on", n, 11.0)).collect::<Vec<_>>()),
        );

        let comparisons = compare(&configurations);
        assert_eq!(comparisons.len(), 1);
        assert_eq!(comparisons[0].baseline, "a-off");
        assert_eq!(comparisons[0].candidate, "b-on");
        assert!((comparisons[0].delta_pct["mspt_p95"] - 10.0).abs() < 0.001);
        assert!(comparisons[0].trustworthy);
    }

    /// Comparer les deux moments d'armement à la même référence demande trois
    /// configurations dans la même campagne, appariées exécution par exécution
    /// (ADR-022). L'ancienne comparaison n'en traitait que deux et rendait `None`
    /// au-delà : la troisième aurait été mesurée pendant des heures, puis
    /// silencieusement abandonnée.
    #[test]
    fn every_variant_is_compared_to_the_baseline() {
        let mut configurations = BTreeMap::new();
        for (label, p95) in [("a-off", 10.0), ("b-tard", 11.0), ("c-tot", 12.0)] {
            configurations.insert(
                label.to_owned(),
                summarize(&(1..=5).map(|n| run(label, n, p95)).collect::<Vec<_>>()),
            );
        }

        let comparisons = compare(&configurations);

        assert_eq!(comparisons.len(), 2, "aucune variante ne doit être perdue");
        assert!(comparisons.iter().all(|c| c.baseline == "a-off"));
        assert_eq!(comparisons[0].candidate, "b-tard");
        assert_eq!(comparisons[1].candidate, "c-tot");
        assert!((comparisons[1].delta_pct["mspt_p95"] - 20.0).abs() < 0.001);
    }

    #[test]
    fn no_comparison_is_produced_from_a_single_configuration() {
        let mut configurations = BTreeMap::new();
        configurations.insert("seule".to_owned(), summarize(&[run("seule", 1, 10.0)]));
        assert!(compare(&configurations).is_empty());
    }

    #[test]
    fn a_comparison_of_non_compliant_campaigns_is_not_trustworthy() {
        let mut short = run("a-off", 1, 10.0);
        short.measured_ticks = 10;
        let mut configurations = BTreeMap::new();
        configurations.insert("a-off".to_owned(), summarize(&[short]));
        configurations.insert(
            "b-on".to_owned(),
            summarize(&(1..=5).map(|n| run("b-on", n, 11.0)).collect::<Vec<_>>()),
        );

        let comparisons = compare(&configurations);
        assert!(
            !comparisons[0].trustworthy,
            "une campagne courte ne prouve rien"
        );
    }
}
