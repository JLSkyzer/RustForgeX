//! C-36 : comparaison des empreintes d'etat d'un test de gameplay (PARTIE 20.3.4).
//!
//! Chaque scenario est execute sans RUSTFORGE-X (reference) puis avec, et le critere
//! est l'**egalite d'etat**. Ce module compare des fichiers ecrits par
//! `DigestRecorder`, chunk par chunk et composante par composante.
//!
//! # Pourquoi trois executions, et pourquoi un test symetrique
//!
//! Le jeu et ses mods ne sont pas parfaitement deterministes. Sur le modpack de
//! reference, deux generations SANS RUSTFORGE-X different deja sur la moitie des chunks
//! pour les blocs. Comparer une reference au candidat attribuerait ce hasard a
//! RUSTFORGE-X.
//!
//! Une premiere version tenait pour « bruit » les chunks ou les deux references
//! different, et pour « attribuable » tout ecart du candidat ailleurs. C'etait faux : le
//! hasard ne frappe pas toujours les memes chunks, et une troisieme execution sans aucun
//! effet produit mecaniquement des ecarts hors de cet ensemble. La version initiale a
//! ainsi conclu a une divergence sur des donnees ou RUSTFORGE-X n'etait pas plus ecarte
//! que les references elles-memes.
//!
//! Le test retenu est symetrique. Pour chaque chunk, on regarde laquelle des trois
//! executions est l'intruse — seule a differer des deux autres. Si RUSTFORGE-X n'a pas
//! d'effet, il n'a aucune raison d'etre l'intrus plus souvent que l'une ou l'autre
//! reference.

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

/// Raisons pour lesquelles une comparaison ne vaut rien. Vide si elle est valide.
///
/// Deux mondes de graines differentes, ou dont l'un n'a pas fini sa generation,
/// produiraient des ecarts vrais qui ne diraient rien de RUSTFORGE-X : la comparaison
/// est alors declaree invalide plutot que mesuree.
#[must_use]
pub fn validity(files: &[&DigestFile]) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(first) = files.first() else {
        return problems;
    };
    for file in files {
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
        if file.seed != first.seed {
            problems.push(format!(
                "graines differentes : {} pour « {} », {} pour « {} »",
                first.seed, first.label, file.seed, file.label
            ));
        }
        if file.components != first.components {
            problems.push(format!(
                "« {} » : composantes differentes de « {} »",
                file.label, first.label
            ));
        }
        let keys: BTreeSet<&String> = file.chunks.keys().collect();
        let first_keys: BTreeSet<&String> = first.chunks.keys().collect();
        if keys != first_keys {
            problems.push(format!(
                "« {} » ne couvre pas les memes chunks que « {} »",
                file.label, first.label
            ));
        }
    }
    problems
}

/// Repartition des chunks d'une composante entre les trois executions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComponentVerdict {
    /// Nom de la composante.
    pub name: String,
    /// Chunks identiques dans les trois executions.
    pub all_equal: usize,
    /// Chunks ou seule la premiere reference differe.
    pub reference_odd: usize,
    /// Chunks ou seule la seconde reference differe.
    pub second_odd: usize,
    /// Chunks ou seul le candidat differe.
    pub candidate_odd: usize,
    /// Chunks ou les trois executions different toutes.
    pub all_different: usize,
}

impl ComponentVerdict {
    /// `true` si les deux references concordent sur tous les chunks.
    ///
    /// Une composante deterministe ne tolere aucun ecart du candidat : il n'y a pas de
    /// hasard derriere lequel il pourrait se cacher.
    #[must_use]
    pub fn deterministic(&self) -> bool {
        self.reference_odd + self.second_odd + self.all_different == 0
    }

    /// Chunks ou une seule des trois executions differe des deux autres.
    #[must_use]
    pub fn single_odd(&self) -> usize {
        self.reference_odd + self.second_odd + self.candidate_odd
    }

    /// Probabilite d'observer le candidat intrus au moins aussi souvent, s'il n'avait
    /// aucun effet.
    ///
    /// Sans effet, le candidat est une troisieme execution comme les autres : chaque
    /// chunk a intrus unique a une chance sur trois de designer le candidat. Le nombre
    /// de fois ou il l'est suit une loi binomiale de parametre 1/3, et ceci en est la
    /// queue superieure.
    #[must_use]
    pub fn p_value(&self) -> f64 {
        binomial_upper_tail(self.single_odd(), self.candidate_odd, 1.0 / 3.0)
    }

    /// `true` si rien n'impute d'ecart au candidat.
    ///
    /// Composante deterministe : aucun ecart permis, aucun hasard ne pourrait
    /// l'expliquer. Composante bruitee : le candidat ne doit pas etre l'intrus plus
    /// souvent que le hasard ne le permet, au seuil de [`ALPHA`].
    ///
    /// La premiere regle employee ici — ne pas etre plus souvent l'intrus que la plus
    /// bruitee des references — a fait echouer un candidat a trois occurrences contre
    /// deux, sur six en tout. Une regle sans loi derriere juge le hasard des petits
    /// effectifs comme un effet.
    #[must_use]
    pub fn passed(&self) -> bool {
        if self.deterministic() {
            self.candidate_odd == 0
        } else {
            self.p_value() >= ALPHA
        }
    }
}

/// Seuil du test sur une composante bruitee.
///
/// Un pourcent : le test doit d'abord ne pas crier au loup, puisque chaque fausse
/// alarme imputerait a RUSTFORGE-X un ecart que le jeu produit seul. Le prix est
/// assume et affiche : un effet plus petit que le bruit du jeu passe inapercu.
pub const ALPHA: f64 = 0.01;

/// `P(X >= k)` pour `X` binomiale de parametres `n` et `p`.
///
/// Calculee en logarithmes : `(2/3)^n` vaut zero en virgule flottante bien avant les
/// quelques milliers de chunks d'un test.
#[must_use]
pub fn binomial_upper_tail(n: usize, k: usize, p: f64) -> f64 {
    if k == 0 {
        return 1.0;
    }
    if k > n {
        return 0.0;
    }
    let (ln_p, ln_q) = (p.ln(), (1.0 - p).ln());
    let mut ln_choose = 0.0_f64;
    let mut terms = Vec::with_capacity(n + 1);
    for i in 0..=n {
        if i > 0 {
            #[allow(clippy::cast_precision_loss)]
            {
                ln_choose += ((n - i + 1) as f64).ln() - (i as f64).ln();
            }
        }
        if i >= k {
            #[allow(clippy::cast_precision_loss)]
            terms.push(ln_choose + i as f64 * ln_p + (n - i) as f64 * ln_q);
        }
    }
    let max = terms.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let sum: f64 = terms.iter().map(|t| (t - max).exp()).sum();
    (max + sum.ln()).exp().min(1.0)
}

/// Verdict d'une comparaison a deux references et un candidat.
#[derive(Debug, Clone, Default)]
pub struct Verdict {
    /// Raisons d'invalidite ; une comparaison invalide ne passe pas.
    pub invalid: Vec<String>,
    /// Chunks presents dans les trois executions.
    pub compared: usize,
    /// Une entree par composante, dans l'ordre du fichier.
    pub components: Vec<ComponentVerdict>,
}

impl Verdict {
    /// `true` si la comparaison est valide et que chaque composante passe.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.invalid.is_empty() && self.components.iter().all(ComponentVerdict::passed)
    }
}

/// Juge un candidat contre deux references, composante par composante.
#[must_use]
pub fn judge(reference: &DigestFile, second: &DigestFile, candidate: &DigestFile) -> Verdict {
    let mut verdict = Verdict {
        invalid: validity(&[reference, second, candidate]),
        components: reference
            .components
            .iter()
            .map(|name| ComponentVerdict {
                name: name.clone(),
                ..ComponentVerdict::default()
            })
            .collect(),
        ..Verdict::default()
    };

    for (chunk, a) in &reference.chunks {
        let (Some(a), Some(Some(b)), Some(Some(c))) =
            (a, second.chunks.get(chunk), candidate.chunks.get(chunk))
        else {
            // Absent d'un cote : deja compte comme generation incomplete.
            continue;
        };
        verdict.compared += 1;
        for (i, slot) in verdict.components.iter_mut().enumerate() {
            let (Some(x), Some(y), Some(z)) = (a.get(i), b.get(i), c.get(i)) else {
                continue;
            };
            match (x == y, x == z, y == z) {
                (true, true, _) => slot.all_equal += 1,
                (true, false, _) => slot.candidate_odd += 1,
                (false, true, _) => slot.second_odd += 1,
                (false, false, true) => slot.reference_odd += 1,
                (false, false, false) => slot.all_different += 1,
            }
        }
    }
    verdict
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

    fn blocks(verdict: &Verdict) -> &ComponentVerdict {
        &verdict.components[0]
    }

    #[test]
    fn identical_runs_pass_and_every_component_is_deterministic() {
        let r1 = file("r1", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]);
        let r2 = file("r2", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]);
        let c = file("c", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]);

        let verdict = judge(&r1, &r2, &c);

        assert!(verdict.invalid.is_empty(), "{:?}", verdict.invalid);
        assert_eq!(verdict.compared, 2);
        assert!(verdict
            .components
            .iter()
            .all(ComponentVerdict::deterministic));
        assert!(verdict.passed());
    }

    /// Une composante sur laquelle les references concordent toujours ne tolere aucun
    /// ecart du candidat : il n'y a pas de hasard derriere lequel se cacher.
    #[test]
    fn a_single_candidate_divergence_fails_a_deterministic_component() {
        let r1 = file("r1", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]);
        let r2 = file("r2", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]);
        let c = file(
            "c",
            &[("0,0", Some(SAME)), ("0,1", Some(["a", "X", "c", "d"]))],
        );

        let verdict = judge(&r1, &r2, &c);

        assert_eq!(verdict.components[1].candidate_odd, 1);
        assert!(verdict.components[1].deterministic());
        assert!(!verdict.passed());
    }

    /// Le defaut de la premiere version : le hasard ne frappe pas toujours les memes
    /// chunks. Un candidat qui n'est pas plus souvent l'intrus que les references n'est
    /// pas en cause, meme s'il differe la ou elles concordaient.
    #[test]
    fn a_candidate_no_more_odd_than_the_references_passes_a_noisy_component() {
        let r1 = file(
            "r1",
            &[
                ("0,0", Some(["X", "b", "c", "d"])),
                ("0,1", Some(SAME)),
                ("0,2", Some(SAME)),
            ],
        );
        let r2 = file(
            "r2",
            &[
                ("0,0", Some(SAME)),
                ("0,1", Some(["Y", "b", "c", "d"])),
                ("0,2", Some(SAME)),
            ],
        );
        let c = file(
            "c",
            &[
                ("0,0", Some(SAME)),
                ("0,1", Some(SAME)),
                ("0,2", Some(["Z", "b", "c", "d"])),
            ],
        );

        let verdict = judge(&r1, &r2, &c);
        let b = blocks(&verdict);

        assert_eq!((b.reference_odd, b.second_odd, b.candidate_odd), (1, 1, 1));
        assert!(!b.deterministic());
        assert!(verdict.passed());
    }

    /// Le candidat est intrus bien plus souvent que le hasard ne le permet.
    #[test]
    fn a_candidate_far_odder_than_chance_fails_a_noisy_component() {
        let component = ComponentVerdict {
            name: "blocks".to_owned(),
            all_equal: 1_000,
            reference_odd: 10,
            second_odd: 12,
            candidate_odd: 60,
            all_different: 50,
        };

        assert!(component.p_value() < ALPHA, "p = {}", component.p_value());
        assert!(!component.passed());
    }

    /// Le cas qui a fait echouer la premiere regle sur de vraies donnees : trois contre
    /// deux et un, sur six. C'est le hasard ordinaire des petits effectifs.
    #[test]
    fn three_against_two_and_one_is_chance_not_an_effect() {
        let component = ComponentVerdict {
            name: "block_entities".to_owned(),
            all_equal: 1_716,
            reference_odd: 1,
            second_odd: 2,
            candidate_odd: 3,
            all_different: 303,
        };

        assert!(component.passed(), "p = {}", component.p_value());
    }

    /// Valeurs exactes de la queue binomiale, et tenue numerique sur un grand effectif.
    #[test]
    fn the_binomial_tail_has_the_expected_values() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-4;

        // P(X >= 3), X ~ B(6, 1/3) = 1 - (64 + 192 + 240) / 729.
        assert!(close(binomial_upper_tail(6, 3, 1.0 / 3.0), 233.0 / 729.0));
        assert!(close(binomial_upper_tail(10, 0, 1.0 / 3.0), 1.0));
        assert!(close(binomial_upper_tail(3, 3, 0.5), 0.125));
        assert!(close(binomial_upper_tail(5, 6, 0.5), 0.0));

        // Un effectif de plusieurs milliers ne doit ni deborder ni tomber a zero.
        let tail = binomial_upper_tail(3_000, 1_000, 1.0 / 3.0);
        assert!(
            tail > 0.4 && tail < 0.6,
            "queue autour de la moyenne : {tail}"
        );
    }

    /// Les chunks ou les trois executions different toutes ne chargent personne : chacun
    /// y est aussi ecarte que les autres.
    #[test]
    fn chunks_where_all_three_differ_blame_nobody() {
        let r1 = file("r1", &[("0,0", Some(["X", "b", "c", "d"]))]);
        let r2 = file("r2", &[("0,0", Some(["Y", "b", "c", "d"]))]);
        let c = file("c", &[("0,0", Some(["Z", "b", "c", "d"]))]);

        let verdict = judge(&r1, &r2, &c);

        assert_eq!(blocks(&verdict).all_different, 1);
        assert_eq!(blocks(&verdict).candidate_odd, 0);
        assert!(verdict.passed());
    }

    /// Deux graines differentes produisent des mondes differents : les ecarts seraient
    /// vrais, et ne diraient rien de RUSTFORGE-X.
    #[test]
    fn different_seeds_make_the_comparison_invalid() {
        let r1 = file("r1", &[("0,0", Some(SAME))]);
        let r2 = file("r2", &[("0,0", Some(SAME))]);
        let mut c = file("c", &[("0,0", Some(SAME))]);
        c.seed = 7;

        let verdict = judge(&r1, &r2, &c);

        assert!(!verdict.invalid.is_empty());
        assert!(!verdict.passed());
    }

    /// Un monde a moitie genere ne se compare pas : un chunk absent n'est pas une
    /// divergence, c'est une mesure qui n'a pas eu lieu (R-660).
    #[test]
    fn an_incomplete_generation_makes_the_comparison_invalid() {
        let r1 = file("r1", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]);
        let r2 = file("r2", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]);
        let c = file("c", &[("0,0", Some(SAME)), ("0,1", None)]);

        let verdict = judge(&r1, &r2, &c);

        assert!(!verdict.invalid.is_empty());
        assert_eq!(verdict.compared, 1, "le chunk absent n'est pas compare");
        assert!(!verdict.passed());
    }

    #[test]
    fn a_timed_out_candidate_does_not_pass() {
        let r1 = file("r1", &[("0,0", Some(SAME))]);
        let r2 = file("r2", &[("0,0", Some(SAME))]);
        let mut c = file("c", &[("0,0", Some(SAME))]);
        c.timed_out = true;

        assert!(!judge(&r1, &r2, &c).passed());
    }
}
