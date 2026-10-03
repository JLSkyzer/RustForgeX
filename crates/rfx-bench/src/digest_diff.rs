//! C-36 : comparaison des empreintes d'etat d'un test de gameplay (PARTIE 20.3.4).
//!
//! Chaque scenario est execute sans RUSTFORGE-X (reference) puis avec, et le critere
//! est l'**egalite d'etat**. Ce module compare deux fichiers ecrits par
//! `DigestRecorder`, chunk par chunk et composante par composante.
//!
//! # Pourquoi trois fichiers plutot que deux
//!
//! Le jeu et ses mods ne sont pas parfaitement deterministes : la generation est
//! multi-thread, et certains mods tirent leur alea hors de la graine. Deux references
//! peuvent donc differer entre elles. Comparer une reference a une execution instrumentee
//! sans le savoir attribuerait a RUSTFORGE-X un ecart qui ne lui doit rien.
//!
//! Avec deux references et un candidat, le verdict separe ce que le jeu fait varier tout
//! seul — le bruit, mesure par la paire de references — de ce que le candidat change en
//! plus. Seul ce second ensemble met RUSTFORGE-X en cause.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

/// Fichier d'empreinte, tel qu'ecrit par `DigestRecorder`.
#[derive(Debug, Clone, Deserialize)]
pub struct DigestFile {
    /// Etiquette de l'execution.
    pub label: String,
    /// `true` si RUSTFORGE-X etait actif.
    pub rfx_active: bool,
    /// Methodes portant une sonde ; zero pour une reference.
    pub methods_probed: u64,
    /// Graine du monde.
    pub seed: i64,
    /// Chunks attendus.
    pub chunks_expected: u32,
    /// Chunks effectivement generes au moment de l'empreinte.
    pub chunks_ready: u32,
    /// `true` si l'empreinte a ete prise avant la fin de la generation.
    pub timed_out: bool,
    /// Noms des composantes, dans l'ordre des empreintes de chaque chunk.
    pub components: Vec<String>,
    /// Empreintes par chunk `"x,z"`, ou `None` si le chunk n'etait pas charge.
    pub chunks: BTreeMap<String, Option<Vec<String>>>,
}

/// Ecart d'un chunk entre deux executions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkDivergence {
    /// Chunk `"x,z"`.
    pub chunk: String,
    /// Composantes dont l'empreinte differe.
    pub components: Vec<String>,
}

/// Resultat de la comparaison de deux fichiers.
#[derive(Debug, Clone, Default)]
pub struct Comparison {
    /// Raisons pour lesquelles la comparaison ne vaut rien. Vide si elle est valide.
    pub invalid: Vec<String>,
    /// Chunks compares.
    pub compared: usize,
    /// Chunks dont au moins une composante differe.
    pub divergent: Vec<ChunkDivergence>,
}

impl Comparison {
    /// Paires `(chunk, composante)` en ecart.
    #[must_use]
    pub fn pairs(&self) -> BTreeSet<(String, String)> {
        self.divergent
            .iter()
            .flat_map(|d| {
                d.components
                    .iter()
                    .map(move |c| (d.chunk.clone(), c.clone()))
            })
            .collect()
    }

    /// Nombre de chunks en ecart, par composante.
    #[must_use]
    pub fn per_component(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for divergence in &self.divergent {
            for component in &divergence.components {
                *counts.entry(component.clone()).or_insert(0) += 1;
            }
        }
        counts
    }
}

/// Verifie qu'un fichier decrit une generation complete.
fn completeness(file: &DigestFile, problems: &mut Vec<String>) {
    if file.timed_out || file.chunks_ready != file.chunks_expected {
        problems.push(format!(
            "« {} » : generation incomplete, {} chunks sur {}{}",
            file.label,
            file.chunks_ready,
            file.chunks_expected,
            if file.timed_out {
                ", delai depasse"
            } else {
                ""
            }
        ));
    }
}

/// Compare deux executions, chunk par chunk.
///
/// Une comparaison entre deux mondes de graines differentes, ou dont l'un n'a pas fini
/// sa generation, est declaree invalide plutot que de produire des ecarts : ils seraient
/// vrais, et ne diraient rien de RUSTFORGE-X.
#[must_use]
pub fn compare(a: &DigestFile, b: &DigestFile) -> Comparison {
    let mut result = Comparison::default();

    if a.seed != b.seed {
        result.invalid.push(format!(
            "graines differentes : {} pour « {} », {} pour « {} »",
            a.seed, a.label, b.seed, b.label
        ));
    }
    if a.components != b.components {
        result
            .invalid
            .push("composantes differentes d'un fichier a l'autre".to_owned());
    }
    completeness(a, &mut result.invalid);
    completeness(b, &mut result.invalid);

    let keys_a: BTreeSet<&String> = a.chunks.keys().collect();
    let keys_b: BTreeSet<&String> = b.chunks.keys().collect();
    if keys_a != keys_b {
        result
            .invalid
            .push("les deux fichiers ne couvrent pas les memes chunks".to_owned());
    }

    for (chunk, digest_a) in &a.chunks {
        let Some(digest_b) = b.chunks.get(chunk) else {
            continue;
        };
        let (Some(da), Some(db)) = (digest_a, digest_b) else {
            // Un chunk absent d'un cote est deja compte dans la completude.
            continue;
        };
        result.compared += 1;
        let components: Vec<String> = da
            .iter()
            .zip(db.iter())
            .enumerate()
            .filter(|(_, (x, y))| x != y)
            .map(|(i, _)| {
                a.components
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| format!("#{i}"))
            })
            .collect();
        if !components.is_empty() {
            result.divergent.push(ChunkDivergence {
                chunk: chunk.clone(),
                components,
            });
        }
    }
    result
}

/// Verdict d'une comparaison a deux references et un candidat.
#[derive(Debug, Clone, Default)]
pub struct Verdict {
    /// Reference contre reference : ce que le jeu fait varier tout seul.
    pub noise: Comparison,
    /// Reference contre candidat.
    pub candidate: Comparison,
    /// Paires `(chunk, composante)` en ecart chez le candidat **et** stables entre les
    /// deux references. Ce sont les seules qui mettent RUSTFORGE-X en cause.
    pub attributable: BTreeSet<(String, String)>,
}

impl Verdict {
    /// `true` si la comparaison est valide et que rien n'est attribuable au candidat.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.noise.invalid.is_empty()
            && self.candidate.invalid.is_empty()
            && self.attributable.is_empty()
    }
}

/// Juge un candidat contre deux references.
#[must_use]
pub fn judge(reference: &DigestFile, second: &DigestFile, candidate: &DigestFile) -> Verdict {
    let noise = compare(reference, second);
    let candidate_cmp = compare(reference, candidate);
    let noisy = noise.pairs();
    let attributable = candidate_cmp
        .pairs()
        .into_iter()
        .filter(|pair| !noisy.contains(pair))
        .collect();
    Verdict {
        noise,
        candidate: candidate_cmp,
        attributable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(label: &str, chunks: &[(&str, Option<[&str; 4]>)]) -> DigestFile {
        DigestFile {
            label: label.to_owned(),
            rfx_active: false,
            methods_probed: 0,
            seed: 42,
            chunks_expected: u32::try_from(chunks.len()).expect("petit"),
            chunks_ready: u32::try_from(chunks.iter().filter(|(_, d)| d.is_some()).count())
                .expect("petit"),
            timed_out: false,
            components: ["blocks", "biomes", "block_entities", "structures"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            chunks: chunks
                .iter()
                .map(|(k, d)| {
                    (
                        (*k).to_owned(),
                        d.map(|arr| arr.iter().map(|s| (*s).to_owned()).collect()),
                    )
                })
                .collect(),
        }
    }

    const SAME: [&str; 4] = ["a", "b", "c", "d"];

    #[test]
    fn identical_runs_do_not_diverge() {
        let a = file("a", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]);
        let b = file("b", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]);

        let result = compare(&a, &b);

        assert!(result.invalid.is_empty(), "{:?}", result.invalid);
        assert_eq!(result.compared, 2);
        assert!(result.divergent.is_empty());
    }

    /// Un ecart doit dire OU il est : la composante, pas seulement le chunk.
    #[test]
    fn a_divergence_names_its_component() {
        let a = file("a", &[("3,4", Some(SAME))]);
        let b = file("b", &[("3,4", Some(["a", "b", "X", "d"]))]);

        let result = compare(&a, &b);

        assert_eq!(
            result.divergent,
            vec![ChunkDivergence {
                chunk: "3,4".to_owned(),
                components: vec!["block_entities".to_owned()],
            }]
        );
    }

    /// Deux graines differentes produisent des mondes differents : les ecarts seraient
    /// vrais, et ne diraient rien de RUSTFORGE-X.
    #[test]
    fn different_seeds_make_the_comparison_invalid() {
        let a = file("a", &[("0,0", Some(SAME))]);
        let mut b = file("b", &[("0,0", Some(SAME))]);
        b.seed = 7;

        assert!(!compare(&a, &b).invalid.is_empty());
    }

    /// Un monde a moitie genere ne se compare pas : un chunk absent d'un cote n'est pas
    /// une divergence, c'est une mesure qui n'a pas eu lieu (R-660).
    #[test]
    fn an_incomplete_generation_makes_the_comparison_invalid() {
        let a = file("a", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]);
        let b = file("b", &[("0,0", Some(SAME)), ("0,1", None)]);

        let result = compare(&a, &b);

        assert!(!result.invalid.is_empty());
        assert_eq!(result.compared, 1, "le chunk absent n'est pas compare");
        assert!(result.divergent.is_empty());
    }

    /// Le coeur du critere : un ecart que les deux references presentent deja entre
    /// elles est du bruit du jeu, pas un effet de RUSTFORGE-X.
    #[test]
    fn a_divergence_already_present_between_references_is_noise() {
        let reference = file("ref1", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]);
        let second = file(
            "ref2",
            &[("0,0", Some(["a", "b", "Z", "d"])), ("0,1", Some(SAME))],
        );
        let candidate = file(
            "rfx",
            &[("0,0", Some(["a", "b", "Y", "d"])), ("0,1", Some(SAME))],
        );

        let verdict = judge(&reference, &second, &candidate);

        assert!(verdict.attributable.is_empty());
        assert!(verdict.passed());
    }

    #[test]
    fn a_divergence_absent_between_references_is_attributable() {
        let reference = file("ref1", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]);
        let second = file("ref2", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]);
        let candidate = file(
            "rfx",
            &[("0,0", Some(SAME)), ("0,1", Some(["X", "b", "c", "d"]))],
        );

        let verdict = judge(&reference, &second, &candidate);

        assert!(!verdict.passed());
        assert_eq!(
            verdict.attributable,
            [("0,1".to_owned(), "blocks".to_owned())]
                .into_iter()
                .collect()
        );
    }

    /// Un candidat qui n'a pas fini sa generation ne passe pas, meme sans ecart visible.
    #[test]
    fn an_invalid_candidate_does_not_pass() {
        let reference = file("ref1", &[("0,0", Some(SAME))]);
        let second = file("ref2", &[("0,0", Some(SAME))]);
        let mut candidate = file("rfx", &[("0,0", Some(SAME))]);
        candidate.timed_out = true;

        assert!(!judge(&reference, &second, &candidate).passed());
    }
}
