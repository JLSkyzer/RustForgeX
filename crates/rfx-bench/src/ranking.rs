//! T-140 : stabilite du classement des unites les plus couteuses entre deux executions.
//!
//! Cahier des charges : PARTIE 5.5, acceptance de C-05 — « le classement des 20
//! workloads les plus couteux est stable entre deux executions identiques (correlation
//! de rang > 0,9) ».
//!
//! Deux classements tronques ne contiennent pas forcement les memes unites. La
//! correlation est donc calculee sur l'**union** des deux top-k :
//!
//! - une unite est rangee a sa position dans chaque liste **complete** (jusqu'a 25
//!   lignes dans le fichier d'execution), pas seulement dans le top-k ;
//! - une unite absente d'une liste y prend le rang moyen des absentes (ex aequo
//!   apres la derniere ligne), ce qui est le traitement standard des egalites ;
//! - la correlation est celle de Pearson sur ces rangs, c'est-a-dire le rho de
//!   Spearman avec correction des ex aequo.
//!
//! Une unite qui sort du classement d'une execution a l'autre penalise donc la
//! correlation : c'est voulu, une instabilite de composition est une instabilite.

use serde::Serialize;

/// Seuil de l'acceptance de C-05.
pub const THRESHOLD: f64 = 0.9;

/// Taille du classement compare.
pub const TOP_K: usize = 20;

/// Une unite de l'union, avec son rang dans chaque execution.
#[derive(Debug, Serialize)]
pub struct Row {
    /// Identite de l'unite : proprietaire et methode.
    pub unit: String,
    /// Position dans la premiere execution, si elle y figure.
    pub first: Option<usize>,
    /// Position dans la seconde execution, si elle y figure.
    pub second: Option<usize>,
}

/// Verdict de stabilite.
#[derive(Debug, Serialize)]
pub struct Verdict {
    /// Taille du classement compare.
    pub k: usize,
    /// Unites communes aux deux top-k.
    pub overlap: usize,
    /// Unites de l'union.
    pub union: usize,
    /// Correlation de rang ; `None` si elle n'est pas calculable (moins de deux unites,
    /// ou rangs constants).
    pub rho: Option<f64>,
    /// Seuil de l'acceptance.
    pub threshold: f64,
    /// Correlation calculable et superieure au seuil.
    pub stable: bool,
    /// Detail, dans l'ordre de la premiere execution puis des absentes.
    pub rows: Vec<Row>,
}

/// Compare deux classements, de l'unite la plus couteuse a la moins couteuse.
#[must_use]
pub fn compare(first: &[String], second: &[String], k: usize) -> Verdict {
    let mut union: Vec<&String> = Vec::new();
    for unit in first.iter().take(k).chain(second.iter().take(k)) {
        if !union.contains(&unit) {
            union.push(unit);
        }
    }
    let position = |list: &[String], unit: &String| list.iter().position(|u| u == unit);

    let ranks_first = ranks(first, &union, position);
    let ranks_second = ranks(second, &union, position);
    let rho = pearson(&ranks_first, &ranks_second);

    let overlap = first
        .iter()
        .take(k)
        .filter(|u| second.iter().take(k).any(|v| v == *u))
        .count();
    let rows = union
        .iter()
        .map(|unit| Row {
            unit: (*unit).clone(),
            first: position(first, unit).map(|p| p + 1),
            second: position(second, unit).map(|p| p + 1),
        })
        .collect();

    Verdict {
        k,
        overlap,
        union: union.len(),
        rho,
        threshold: THRESHOLD,
        stable: rho.is_some_and(|r| r > THRESHOLD),
        rows,
    }
}

/// Rang de chaque unite de l'union dans `list` ; les absentes partagent le rang moyen
/// des places qui suivent la derniere ligne.
fn ranks(
    list: &[String],
    union: &[&String],
    position: impl Fn(&[String], &String) -> Option<usize>,
) -> Vec<f64> {
    let missing = union.iter().filter(|u| position(list, u).is_none()).count();
    // Les absentes occupent les places len+1 .. len+missing : leur rang moyen.
    let missing_rank = list.len() as f64 + (missing as f64 + 1.0) / 2.0;
    union
        .iter()
        .map(|u| position(list, u).map_or(missing_rank, |p| (p + 1) as f64))
        .collect()
}

/// Correlation de Pearson ; `None` si une serie est constante ou trop courte.
fn pearson(a: &[f64], b: &[f64]) -> Option<f64> {
    let n = a.len();
    if n < 2 || n != b.len() {
        return None;
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / n as f64;
    let (ma, mb) = (mean(a), mean(b));
    let mut cov = 0.0;
    let mut va = 0.0;
    let mut vb = 0.0;
    for (x, y) in a.iter().zip(b) {
        cov += (x - ma) * (y - mb);
        va += (x - ma).powi(2);
        vb += (y - mb).powi(2);
    }
    if va == 0.0 || vb == 0.0 {
        return None;
    }
    Some(cov / (va.sqrt() * vb.sqrt()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn identical_rankings_correlate_perfectly() {
        let a = list(&["a", "b", "c", "d"]);
        let verdict = compare(&a, &a, 4);
        assert!((verdict.rho.unwrap() - 1.0).abs() < 1e-12);
        assert_eq!(verdict.overlap, 4);
        assert!(verdict.stable);
    }

    #[test]
    fn reversed_rankings_anticorrelate() {
        let a = list(&["a", "b", "c", "d"]);
        let b = list(&["d", "c", "b", "a"]);
        let verdict = compare(&a, &b, 4);
        assert!((verdict.rho.unwrap() + 1.0).abs() < 1e-12);
        assert!(!verdict.stable);
    }

    #[test]
    fn a_unit_leaving_the_ranking_lowers_the_correlation() {
        let a = list(&["a", "b", "c", "d", "e"]);
        let swapped = list(&["a", "b", "d", "c", "e"]);
        let dropped = list(&["a", "b", "c", "d", "x"]);
        let swap = compare(&a, &swapped, 5).rho.unwrap();
        let drop = compare(&a, &dropped, 5).rho.unwrap();
        assert!(swap < 1.0 && drop < 1.0);
        assert_eq!(
            compare(&a, &dropped, 5).union,
            6,
            "e et x sont tous deux comptes"
        );
    }

    #[test]
    fn a_unit_just_below_the_cut_keeps_its_true_rank() {
        // `e` est 5e dans la premiere liste et 6e dans la seconde : hors du top-5 de la
        // seconde, mais pas absente. Son vrai rang compte, pas un rang d'absente.
        let a = list(&["a", "b", "c", "d", "e", "f"]);
        let b = list(&["a", "b", "c", "d", "f", "e"]);
        let verdict = compare(&a, &b, 5);
        assert_eq!(
            verdict.rows.iter().find(|r| r.unit == "e").unwrap().second,
            Some(6)
        );
        assert!(verdict.rho.unwrap() > 0.8);
    }

    #[test]
    fn too_short_a_ranking_is_not_judged() {
        let verdict = compare(&list(&["a"]), &list(&["a"]), 20);
        assert!(verdict.rho.is_none());
        assert!(
            !verdict.stable,
            "une correlation incalculable n'est pas une stabilite"
        );
    }
}
