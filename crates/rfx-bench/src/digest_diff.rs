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
    /// Scenario de la PARTIE 20.3.4, par exemple `"G-03"`.
    #[serde(default)]
    pub test: String,
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
    /// Ordonnee de la premiere section, en sections (schema 2).
    #[serde(default)]
    pub min_section_y: Option<i32>,
    /// Empreinte des blocs de chaque section, par chunk (schema 2).
    #[serde(default)]
    pub block_sections: Option<BTreeMap<String, Option<Vec<String>>>>,
    /// Entites de bloc de chaque chunk qui en porte, une ligne
    /// `"x,y,z|type|champ=empreinte;..."` par entite (diagnostic, G-08).
    #[serde(default)]
    pub block_entity_details: Option<BTreeMap<String, Option<Vec<String>>>>,
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
        if file.test != first.test {
            problems.push(format!(
                "scenarios differents : « {} » pour « {} », « {} » pour « {} »",
                first.test, first.label, file.test, file.label
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

    /// `true` si rien n'impute d'ecart au candidat, au seuil donne.
    ///
    /// Composante deterministe : aucun ecart permis, aucun hasard ne pourrait
    /// l'expliquer. Composante bruitee : le candidat ne doit pas etre l'intrus plus
    /// souvent que le hasard ne le permet, au seuil `alpha` — celui que
    /// [`Verdict::alpha`] calcule pour l'ensemble des tests.
    ///
    /// La premiere regle employee ici — ne pas etre plus souvent l'intrus que la plus
    /// bruitee des references — a fait echouer un candidat a trois occurrences contre
    /// deux, sur six en tout. Une regle sans loi derriere juge le hasard des petits
    /// effectifs comme un effet.
    #[must_use]
    pub fn passed_at(&self, alpha: f64) -> bool {
        if self.deterministic() {
            self.candidate_odd == 0
        } else {
            self.p_value() >= alpha
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
    /// Une entree par composante jugee : celles du fichier, et, quand les empreintes
    /// par section sont presentes, une par hauteur de section a la place des blocs du
    /// chunk entier.
    pub components: Vec<ComponentVerdict>,
}

impl Verdict {
    /// Composantes bruitees, donc soumises au test binomial.
    #[must_use]
    pub fn noisy_tests(&self) -> usize {
        self.components
            .iter()
            .filter(|c| !c.deterministic())
            .count()
    }

    /// Seuil applique a chaque composante bruitee : [`ALPHA`] divise par leur nombre.
    ///
    /// Juger les blocs section par section enchaine une vingtaine de tests. A un pourcent
    /// chacun, une campagne sans aucun effet donnerait une fausse alarme environ une fois
    /// sur cinq. La correction de Bonferroni ramene a un pourcent la probabilite d'en
    /// produire une, sur l'ensemble des tests.
    #[must_use]
    pub fn alpha(&self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let tests = self.noisy_tests().max(1) as f64;
        ALPHA / tests
    }

    /// `true` si la comparaison est valide et que chaque composante passe.
    #[must_use]
    pub fn passed(&self) -> bool {
        let alpha = self.alpha();
        self.invalid.is_empty() && self.components.iter().all(|c| c.passed_at(alpha))
    }
}

/// Classe un triplet d'empreintes : qui est l'intrus, s'il y en a un.
fn tally(slot: &mut ComponentVerdict, x: &str, y: &str, z: &str) {
    match (x == y, x == z, y == z) {
        (true, true, _) => slot.all_equal += 1,
        (true, false, _) => slot.candidate_odd += 1,
        (false, true, _) => slot.second_odd += 1,
        (false, false, true) => slot.reference_odd += 1,
        (false, false, false) => slot.all_different += 1,
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
            if let (Some(x), Some(y), Some(z)) = (a.get(i), b.get(i), c.get(i)) {
                tally(slot, x, y, z);
            }
        }
    }

    if let Some(strata) = section_strata(reference, second, candidate, &mut verdict.invalid) {
        // Les sections remplacent les blocs du chunk entier : la meme information, a une
        // granularite qui isole le bruit au lieu de le laisser contaminer tout le chunk.
        verdict.components.retain(|c| c.name != "blocks");
        verdict.components.extend(strata);
    }
    verdict
}

/// Une composante par hauteur de section, si les trois fichiers portent les sections.
///
/// Sur le modpack de reference, le bruit de generation se concentre entre Y = -16 et
/// Y = 79 ; au-dessus de 160, aucune section ne differe jamais entre deux references.
/// Jugees chunk par chunk, ces sections stables etaient noyees dans le bruit des autres ;
/// jugees a part, elles exigent l'egalite stricte (ADR-032).
fn section_strata(
    reference: &DigestFile,
    second: &DigestFile,
    candidate: &DigestFile,
    invalid: &mut Vec<String>,
) -> Option<Vec<ComponentVerdict>> {
    let (Some(sa), Some(sb), Some(sc)) = (
        &reference.block_sections,
        &second.block_sections,
        &candidate.block_sections,
    ) else {
        return None;
    };
    let min_y = reference.min_section_y.unwrap_or(0);
    if second.min_section_y != reference.min_section_y
        || candidate.min_section_y != reference.min_section_y
    {
        invalid.push("hauteur de la premiere section differente d'un fichier a l'autre".to_owned());
        return None;
    }

    let mut strata: Vec<ComponentVerdict> = Vec::new();
    for (chunk, a) in sa {
        let (Some(a), Some(Some(b)), Some(Some(c))) = (a, sb.get(chunk), sc.get(chunk)) else {
            continue;
        };
        if a.len() != b.len() || a.len() != c.len() {
            invalid.push(format!("{chunk} : nombre de sections different"));
            return None;
        }
        if strata.is_empty() {
            strata = (0..a.len())
                .map(|i| ComponentVerdict {
                    name: format!(
                        "blocks@y{}",
                        (i64::from(min_y) + i64::try_from(i).unwrap_or(0)) * 16
                    ),
                    ..ComponentVerdict::default()
                })
                .collect();
        }
        for (i, slot) in strata.iter_mut().enumerate() {
            tally(slot, &a[i], &b[i], &c[i]);
        }
    }
    Some(strata)
}

/// Resultat d'un aller-retour sauvegarde puis rechargement (G-08).
///
/// Contrairement a la generation, l'aller-retour n'a aucune raison d'etre aleatoire :
/// ce que le jeu ecrit sur disque doit etre exactement ce qu'il relit. L'egalite est donc
/// exigee strictement, sans loi ni seuil — un seul ecart suffit a echouer.
#[derive(Debug, Clone, Default)]
pub struct RoundTrip {
    /// Raisons d'invalidite ; un aller-retour invalide ne passe pas.
    pub invalid: Vec<String>,
    /// Chunks presents des deux cotes.
    pub compared: usize,
    /// Chunks en ecart, par composante — sections de blocs comprises.
    pub components: Vec<(String, usize)>,
    /// Quelques chunks en ecart, pour le diagnostic : `(chunk, composante)`.
    pub examples: Vec<(String, String)>,
    /// Classes de changement et leur effectif, cle de `judge_round_trips`.
    ///
    /// Une classe localisee porte son chunk : `"structures @ -13,1"`,
    /// `"blocks@y32 @ 4,7"`. Les entites de bloc, quand leur detail est present, sont
    /// classees par type et champ, toutes positions confondues :
    /// `"block_entity lootr:lootr_chest .tileId"`. Ce n'est pas le nom d'un mod qui
    /// decide de quoi que ce soit (INV-12) : la classe n'est qu'une etiquette, et seule
    /// sa presence chez les references compte.
    pub changes: BTreeMap<String, usize>,
    /// `true` si les deux empreintes portaient le detail des entites de bloc.
    pub detailed: bool,
}

/// Entites de bloc d'un chunk, par position : `(type, champ -> empreinte)`.
type ChunkEntities = BTreeMap<String, (String, BTreeMap<String, String>)>;

fn chunk_entities(file: &DigestFile, chunk: &str) -> ChunkEntities {
    let mut out = ChunkEntities::new();
    let lines = file
        .block_entity_details
        .as_ref()
        .and_then(|d| d.get(chunk))
        .and_then(Option::as_ref);
    for line in lines.into_iter().flatten() {
        let mut parts = line.splitn(3, '|');
        let (Some(pos), Some(kind)) = (parts.next(), parts.next()) else {
            continue;
        };
        let fields = parts
            .next()
            .unwrap_or("")
            .split(';')
            .filter_map(|f| f.split_once('='))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        out.insert(pos.to_owned(), (kind.to_owned(), fields));
    }
    out
}

/// Classes de changement des entites de bloc d'un chunk entre sauvegarde et relecture.
fn entity_changes(saved: &ChunkEntities, reloaded: &ChunkEntities) -> Vec<String> {
    let mut out = Vec::new();
    let positions: BTreeSet<&String> = saved.keys().chain(reloaded.keys()).collect();
    for pos in positions {
        match (saved.get(pos), reloaded.get(pos)) {
            (Some((kind, _)), None) => out.push(format!("block_entity {kind} <disparue>")),
            (None, Some((kind, _))) => out.push(format!("block_entity {kind} <apparue>")),
            (Some((k1, _)), Some((k2, _))) if k1 != k2 => {
                out.push(format!("block_entity {k1} <devenue {k2}>"));
            }
            (Some((kind, f1)), Some((_, f2))) => {
                let names: BTreeSet<&String> = f1.keys().chain(f2.keys()).collect();
                for name in names {
                    if f1.get(name) != f2.get(name) {
                        out.push(format!("block_entity {kind} .{name}"));
                    }
                }
            }
            (None, None) => {}
        }
    }
    out
}

impl RoundTrip {
    /// `true` si l'aller-retour est valide et parfaitement fidele.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.invalid.is_empty() && self.components.iter().all(|(_, n)| *n == 0)
    }
}

/// Compare l'etat releve avant la sauvegarde a l'etat relu apres le rechargement.
#[must_use]
pub fn round_trip(saved: &DigestFile, reloaded: &DigestFile) -> RoundTrip {
    const EXAMPLES: usize = 20;
    let mut result = RoundTrip {
        invalid: validity(&[saved, reloaded]),
        ..RoundTrip::default()
    };
    let with_sections = saved.block_sections.is_some() && reloaded.block_sections.is_some();
    result.detailed =
        saved.block_entity_details.is_some() && reloaded.block_entity_details.is_some();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut note = |name: String, chunk: &str, result: &mut RoundTrip| {
        if !counts.contains_key(&name) {
            order.push(name.clone());
        }
        *counts.entry(name.clone()).or_insert(0) += 1;
        if !(result.detailed && name == "block_entities") {
            *result
                .changes
                .entry(format!("{name} @ {chunk}"))
                .or_insert(0) += 1;
        }
        if result.examples.len() < EXAMPLES {
            result.examples.push((chunk.to_owned(), name));
        }
    };

    for (chunk, a) in &saved.chunks {
        let (Some(a), Some(Some(b))) = (a, reloaded.chunks.get(chunk)) else {
            continue;
        };
        result.compared += 1;
        for (i, name) in saved.components.iter().enumerate() {
            if with_sections && name == "blocks" {
                continue;
            }
            if a.get(i) != b.get(i) {
                note(name.clone(), chunk, &mut result);
                if result.detailed && name == "block_entities" {
                    let changes = entity_changes(
                        &chunk_entities(saved, chunk),
                        &chunk_entities(reloaded, chunk),
                    );
                    // Empreinte du chunk differente sans aucun champ en cause : rien
                    // n'explique l'ecart, il reste localise et donc strict.
                    let changes = if changes.is_empty() {
                        vec![format!("block_entities @ {chunk} <sans detail>")]
                    } else {
                        changes
                    };
                    for class in changes {
                        *result.changes.entry(class).or_insert(0) += 1;
                    }
                }
            }
        }
        if with_sections {
            let sections = |f: &DigestFile| {
                f.block_sections
                    .as_ref()
                    .and_then(|s| s.get(chunk).cloned().flatten())
            };
            if let (Some(sa), Some(sb)) = (sections(saved), sections(reloaded)) {
                let min_y = i64::from(saved.min_section_y.unwrap_or(0));
                for (i, (x, y)) in sa.iter().zip(sb.iter()).enumerate() {
                    if x != y {
                        let height = (min_y + i64::try_from(i).unwrap_or(0)) * 16;
                        note(format!("blocks@y{height}"), chunk, &mut result);
                    }
                }
            }
        }
    }
    result.components = order
        .into_iter()
        .map(|name| {
            let n = counts.get(&name).copied().unwrap_or(0);
            (name, n)
        })
        .collect();
    result
}

/// Une classe de changement d'aller-retour et son effectif dans chaque execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeClass {
    /// Etiquette de la classe, voir `RoundTrip::changes`.
    pub key: String,
    /// Effectif dans l'aller-retour de la premiere reference.
    pub reference: usize,
    /// Effectif dans l'aller-retour de la seconde reference.
    pub second: usize,
    /// Effectif dans l'aller-retour du candidat.
    pub candidate: usize,
}

impl ChangeClass {
    /// `true` si le jeu produit deja ce changement sans RUSTFORGE-X.
    #[must_use]
    pub fn tolerated(&self) -> bool {
        self.reference > 0 || self.second > 0
    }

    /// `true` si seul le candidat produit ce changement : il lui est imputable.
    #[must_use]
    pub fn attributable(&self) -> bool {
        self.candidate > 0 && !self.tolerated()
    }
}

/// Verdict de G-08 sur trois allers-retours : deux references, un candidat.
#[derive(Debug, Clone, Default)]
pub struct RoundTripVerdict {
    /// Raisons d'invalidite ; un verdict invalide ne passe pas.
    pub invalid: Vec<String>,
    /// Toutes les classes de changement observees, dans l'ordre de leur etiquette.
    pub classes: Vec<ChangeClass>,
}

impl RoundTripVerdict {
    /// `true` si la comparaison est valide et qu'aucun changement n'est imputable.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.invalid.is_empty() && !self.classes.iter().any(ChangeClass::attributable)
    }
}

/// Juge l'aller-retour du candidat contre celui des deux references (G-08).
///
/// L'egalite stricte de `round_trip` echoue aussi pour les references : certains mods
/// reecrivent une partie de leurs entites de bloc a chaque rechargement (ADR-032). Un
/// changement n'est donc imputable au candidat que si sa **classe** n'apparait dans
/// aucun aller-retour de reference. Une classe localisee (un chunk, une section) reste
/// stricte ; une entite de bloc est classee par type et par champ, puisque les
/// conteneurs concernes ne sont pas aux memes positions d'un monde a l'autre.
///
/// Limite assumee : une classe toleree l'est entierement, quel que soit son effectif
/// chez le candidat. Un defaut qui ne toucherait que des champs deja instables chez
/// les references passerait inapercu ; les effectifs sont rapportes pour le voir.
///
/// Le detail des entites de bloc est exige : sans lui, leurs changements seraient
/// localises par chunk, et le hasard de la generation les ferait echouer a tort.
#[must_use]
pub fn judge_round_trips(
    reference: (&DigestFile, &DigestFile),
    second: (&DigestFile, &DigestFile),
    candidate: (&DigestFile, &DigestFile),
) -> RoundTripVerdict {
    let mut verdict = RoundTripVerdict {
        invalid: validity(&[reference.0, second.0, candidate.0]),
        ..RoundTripVerdict::default()
    };
    let trips = [reference, second, candidate].map(|(s, r)| round_trip(s, r));
    for (trip, (saved, _)) in trips.iter().zip([reference, second, candidate]) {
        for problem in &trip.invalid {
            verdict
                .invalid
                .push(format!("aller-retour « {} » : {problem}", saved.label));
        }
        if !trip.detailed {
            verdict.invalid.push(format!(
                "aller-retour « {} » : detail des entites de bloc absent (DETAILS=true)",
                saved.label
            ));
        }
    }
    let keys: BTreeSet<&String> = trips.iter().flat_map(|t| t.changes.keys()).collect();
    let count = |i: usize, key: &String| trips[i].changes.get(key).copied().unwrap_or(0);
    verdict.classes = keys
        .into_iter()
        .map(|key| ChangeClass {
            key: key.clone(),
            reference: count(0, key),
            second: count(1, key),
            candidate: count(2, key),
        })
        .collect();
    verdict
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L'aller-retour exige l'egalite stricte : un seul ecart, meme dans une section,
    /// suffit a echouer, et il est nomme.
    #[test]
    fn a_round_trip_names_every_divergent_section() {
        let saved = with_sections(
            file("save", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]),
            &[("0,0", ["s0", "s1", "s2"]), ("0,1", ["s0", "s1", "s2"])],
        );
        let reloaded = with_sections(
            file("load", &[("0,0", Some(SAME)), ("0,1", Some(SAME))]),
            &[("0,0", ["s0", "s1", "s2"]), ("0,1", ["s0", "X", "s2"])],
        );

        let result = round_trip(&saved, &reloaded);

        assert!(result.invalid.is_empty(), "{:?}", result.invalid);
        assert_eq!(result.compared, 2);
        assert_eq!(result.components, vec![("blocks@y-48".to_owned(), 1)]);
        assert!(!result.passed());
    }

    #[test]
    fn a_faithful_round_trip_passes() {
        let s = [("0,0", ["s0", "s1", "s2"])];
        let saved = with_sections(file("save", &[("0,0", Some(SAME))]), &s);
        let reloaded = with_sections(file("load", &[("0,0", Some(SAME))]), &s);

        assert!(round_trip(&saved, &reloaded).passed());
    }

    #[test]
    fn a_round_trip_against_another_scenario_is_invalid() {
        let saved = file("save", &[("0,0", Some(SAME))]);
        let mut reloaded = file("load", &[("0,0", Some(SAME))]);
        reloaded.test = "G-01".to_owned();

        assert!(!round_trip(&saved, &reloaded).passed());
    }

    fn file(label: &str, chunks: &[(&str, Option<[&str; 4]>)]) -> DigestFile {
        DigestFile {
            test: "G-03".to_owned(),
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
            min_section_y: None,
            block_sections: None,
            block_entity_details: None,
        }
    }

    fn with_details(mut f: DigestFile, details: &[(&str, &[&str])]) -> DigestFile {
        f.block_entity_details = Some(
            details
                .iter()
                .map(|(k, lines)| {
                    (
                        (*k).to_owned(),
                        Some(lines.iter().map(|l| (*l).to_owned()).collect()),
                    )
                })
                .collect(),
        );
        f
    }

    /// Le digest d'un chunk dont l'empreinte des entites de bloc a change.
    const BE_CHANGED: [&str; 4] = ["a", "b", "X", "d"];

    /// Un aller-retour sur deux chunks : une entite de bloc dans `chunk`, decrite par
    /// `line_before` a la sauvegarde et `line_after` a la relecture. L'empreinte du chunk
    /// ne change que si les deux lignes different.
    fn chest_trip(
        label: &str,
        chunk: &str,
        line_before: &str,
        line_after: &str,
    ) -> (DigestFile, DigestFile) {
        let chunks = |changed: bool| {
            ["0,0", "0,1"]
                .iter()
                .map(|k| {
                    let digest = if changed && *k == chunk {
                        BE_CHANGED
                    } else {
                        SAME
                    };
                    (*k, Some(digest))
                })
                .collect::<Vec<_>>()
        };
        let saved = with_details(file(label, &chunks(false)), &[(chunk, &[line_before])]);
        let reloaded = with_details(
            file(label, &chunks(line_before != line_after)),
            &[(chunk, &[line_after])],
        );
        (saved, reloaded)
    }

    fn pair(t: &(DigestFile, DigestFile)) -> (&DigestFile, &DigestFile) {
        (&t.0, &t.1)
    }

    /// Le bruit d'un mod : le meme champ du meme type change chez les references et
    /// chez le candidat, a des positions differentes. Rien n'est imputable.
    #[test]
    fn a_field_unstable_in_the_references_is_tolerated_wherever_it_changes() {
        let r1 = chest_trip(
            "r1",
            "0,0",
            "1,64,1|ex:chest|Items=1;Id=7",
            "1,64,1|ex:chest|Items=2;Id=7",
        );
        let r2 = chest_trip(
            "r2",
            "0,1",
            "3,70,20|ex:chest|Items=5;Id=7",
            "3,70,20|ex:chest|Items=6;Id=7",
        );
        let c = chest_trip(
            "c",
            "0,1",
            "9,40,30|ex:chest|Items=1;Id=7",
            "9,40,30|ex:chest|Items=9;Id=7",
        );

        let verdict = judge_round_trips(pair(&r1), pair(&r2), pair(&c));

        assert!(verdict.invalid.is_empty(), "{:?}", verdict.invalid);
        assert_eq!(
            verdict.classes,
            vec![ChangeClass {
                key: "block_entity ex:chest .Items".to_owned(),
                reference: 1,
                second: 1,
                candidate: 1,
            }]
        );
        assert!(verdict.passed());
    }

    /// Un autre champ du meme type, stable chez les references, reste strict.
    #[test]
    fn a_field_stable_in_the_references_is_strict_even_in_a_noisy_type() {
        let r1 = chest_trip(
            "r1",
            "0,0",
            "1,64,1|ex:chest|Items=1;Id=7",
            "1,64,1|ex:chest|Items=2;Id=7",
        );
        let r2 = chest_trip(
            "r2",
            "0,0",
            "1,64,1|ex:chest|Items=1;Id=7",
            "1,64,1|ex:chest|Items=1;Id=7",
        );
        let c = chest_trip(
            "c",
            "0,0",
            "1,64,1|ex:chest|Items=1;Id=7",
            "1,64,1|ex:chest|Items=1;Id=8",
        );

        let verdict = judge_round_trips(pair(&r1), pair(&r2), pair(&c));

        let attributable: Vec<&str> = verdict
            .classes
            .iter()
            .filter(|c| c.attributable())
            .map(|c| c.key.as_str())
            .collect();
        assert_eq!(attributable, vec!["block_entity ex:chest .Id"]);
        assert!(!verdict.passed());
    }

    /// Une entite que le candidat perd au rechargement lui est imputable.
    #[test]
    fn an_entity_lost_only_by_the_candidate_is_attributable() {
        let stable = "1,64,1|ex:furnace|Burn=1";
        let r1 = chest_trip("r1", "0,0", stable, stable);
        let r2 = chest_trip("r2", "0,0", stable, stable);
        let mut c = chest_trip("c", "0,0", stable, stable);
        c.1 = with_details(
            file("c", &[("0,0", Some(BE_CHANGED)), ("0,1", Some(SAME))]),
            &[],
        );

        let verdict = judge_round_trips(pair(&r1), pair(&r2), pair(&c));

        assert_eq!(verdict.classes.len(), 1);
        assert_eq!(verdict.classes[0].key, "block_entity ex:furnace <disparue>");
        assert!(verdict.classes[0].attributable());
        assert!(!verdict.passed());
    }

    /// Une classe localisee ne tolere que le meme chunk : une structure qui change
    /// partout au meme endroit est toleree, ailleurs elle est imputable.
    #[test]
    fn a_located_change_is_tolerated_only_at_the_same_place() {
        let stable = "1,64,1|ex:furnace|Burn=1";
        let structure_changed = |label: &str, chunk: &str| {
            let mut t = chest_trip(label, "0,0", stable, stable);
            if let Some(Some(d)) = t.1.chunks.get_mut(chunk) {
                d[3] = "S".to_owned();
            }
            t
        };
        let r1 = structure_changed("r1", "0,0");
        let r2 = structure_changed("r2", "0,0");

        let same_place = structure_changed("c", "0,0");
        assert!(judge_round_trips(pair(&r1), pair(&r2), pair(&same_place)).passed());

        let elsewhere = structure_changed("c", "0,1");
        let verdict = judge_round_trips(pair(&r1), pair(&r2), pair(&elsewhere));
        assert!(!verdict.passed());
        assert!(verdict
            .classes
            .iter()
            .any(|c| c.key == "structures @ 0,1" && c.attributable()));
    }

    /// Sans le detail des entites de bloc, le jugement n'est pas rendu.
    #[test]
    fn round_trips_without_entity_details_are_invalid() {
        let plain = |label: &str| {
            let f = file(label, &[("0,0", Some(SAME))]);
            (f.clone(), f)
        };
        let (r1, r2, c) = (plain("r1"), plain("r2"), plain("c"));

        let verdict = judge_round_trips(pair(&r1), pair(&r2), pair(&c));

        assert_eq!(verdict.invalid.len(), 3, "{:?}", verdict.invalid);
        assert!(!verdict.passed());
    }

    const SAME: [&str; 4] = ["a", "b", "c", "d"];

    fn blocks(verdict: &Verdict) -> &ComponentVerdict {
        &verdict.components[0]
    }

    /// Ajoute des empreintes par section : trois sections, de Y = -64 a Y = -17.
    fn with_sections(mut f: DigestFile, sections: &[(&str, [&str; 3])]) -> DigestFile {
        f.min_section_y = Some(-4);
        f.block_sections = Some(
            sections
                .iter()
                .map(|(k, s)| {
                    (
                        (*k).to_owned(),
                        Some(s.iter().map(|h| (*h).to_owned()).collect()),
                    )
                })
                .collect(),
        );
        f
    }

    fn named<'a>(verdict: &'a Verdict, name: &str) -> &'a ComponentVerdict {
        verdict
            .components
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("composante {name} absente"))
    }

    /// Les sections remplacent les blocs du chunk entier, une composante par hauteur.
    #[test]
    fn sections_replace_whole_chunk_blocks() {
        let s = [("0,0", ["s0", "s1", "s2"])];
        let r1 = with_sections(file("r1", &[("0,0", Some(SAME))]), &s);
        let r2 = with_sections(file("r2", &[("0,0", Some(SAME))]), &s);
        let c = with_sections(file("c", &[("0,0", Some(SAME))]), &s);

        let verdict = judge(&r1, &r2, &c);
        let names: Vec<&str> = verdict.components.iter().map(|c| c.name.as_str()).collect();

        assert!(!names.contains(&"blocks"), "{names:?}");
        assert!(names.contains(&"blocks@y-64"));
        assert!(names.contains(&"blocks@y-32"));
        assert!(verdict.passed());
    }

    /// Le gain de resolution : une section ou les references concordent toujours est
    /// deterministe, meme si le chunk entier est bruite. Un ecart du candidat y est
    /// impute strictement, la ou le jugement par chunk l'aurait noye dans le bruit.
    #[test]
    fn a_candidate_divergence_in_a_stable_section_fails_despite_a_noisy_chunk() {
        // Les references different sur la section basse : le chunk entier est bruite.
        let r1 = with_sections(
            file("r1", &[("0,0", Some(SAME))]),
            &[("0,0", ["bas1", "milieu", "haut"])],
        );
        let r2 = with_sections(
            file("r2", &[("0,0", Some(SAME))]),
            &[("0,0", ["bas2", "milieu", "haut"])],
        );
        // Le candidat differe sur la section haute, stable chez les references.
        let c = with_sections(
            file("c", &[("0,0", Some(SAME))]),
            &[("0,0", ["bas1", "milieu", "HAUT"])],
        );

        let verdict = judge(&r1, &r2, &c);

        let top = named(&verdict, "blocks@y-32");
        assert!(top.deterministic());
        assert_eq!(top.candidate_odd, 1);
        assert!(!verdict.passed());
    }

    /// Une vingtaine de tests a un pourcent chacun produiraient une fausse alarme une
    /// campagne sur cinq : le seuil se divise par le nombre de tests bruites.
    #[test]
    fn the_threshold_is_divided_by_the_number_of_noisy_tests() {
        let r1 = with_sections(
            file("r1", &[("0,0", Some(SAME))]),
            &[("0,0", ["x", "y", "z"])],
        );
        let r2 = with_sections(
            file("r2", &[("0,0", Some(SAME))]),
            &[("0,0", ["X", "Y", "z"])],
        );
        let c = with_sections(
            file("c", &[("0,0", Some(SAME))]),
            &[("0,0", ["x", "y", "z"])],
        );

        let verdict = judge(&r1, &r2, &c);

        assert_eq!(verdict.noisy_tests(), 2);
        assert!((verdict.alpha() - ALPHA / 2.0).abs() < 1e-12);
    }

    #[test]
    fn different_scenarios_make_the_comparison_invalid() {
        let r1 = file("r1", &[("0,0", Some(SAME))]);
        let r2 = file("r2", &[("0,0", Some(SAME))]);
        let mut c = file("c", &[("0,0", Some(SAME))]);
        c.test = "G-01".to_owned();

        assert!(!judge(&r1, &r2, &c).invalid.is_empty());
    }

    #[test]
    fn different_section_origins_make_the_comparison_invalid() {
        let s = [("0,0", ["s0", "s1", "s2"])];
        let r1 = with_sections(file("r1", &[("0,0", Some(SAME))]), &s);
        let r2 = with_sections(file("r2", &[("0,0", Some(SAME))]), &s);
        let mut c = with_sections(file("c", &[("0,0", Some(SAME))]), &s);
        c.min_section_y = Some(0);

        assert!(!judge(&r1, &r2, &c).invalid.is_empty());
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
        assert!(!component.passed_at(ALPHA));
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

        assert!(component.passed_at(ALPHA), "p = {}", component.p_value());
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
