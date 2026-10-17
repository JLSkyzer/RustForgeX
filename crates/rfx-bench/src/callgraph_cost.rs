//! Cout d'un graphe d'appel sur des classes reelles (C-07, ADR-052).
//!
//! Cahier des charges : PARTIE 5.7, PARTIE 21 (R-580, R-581). Maturite : `STABLE`.
//!
//! Le corpus est celui du jeu de developpement : archives du classpath de
//! ForgeGradle et classes du JDK, assemblees par `benchmarks/callgraph-corpus.py` dans
//! un fichier hors du depot. L'analyse est pilotee comme en jeu, par l'API de
//! `rfx-core` : hierarchie complete d'abord, puis, racine apres racine, des pas qui
//! demandent des classes et des classes fournies — sans Java ni frontiere FFI, dont le
//! cout n'est pas mesure ici.
//!
//! Les racines sont les methodes `tick()V` concretes des classes `net/minecraft/`, dans
//! l'ordre des noms : un choix deterministe, proche de ce qui est chaud en jeu, pas un
//! echantillon de ce qui l'est reellement.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use rfx_analyzer::MethodRef;
use rfx_bytecode::ClassFile;
use rfx_core::analysis::{Analysis, HotWorkload, StepState};
use rfx_model::{Heat, HierarchyEntry, RuntimeConfig, Side, WorkId};
use serde::Serialize;

/// Signature du fichier de corpus.
const MAGIC: &[u8; 8] = b"RFXCORP1";

/// Pas au plus par graphe : borne une boucle qui ne finirait pas.
const MAX_STEPS: usize = 64;

/// Classes du corpus, par nom interne.
pub struct Corpus {
    /// Contenu de chaque classe.
    pub classes: HashMap<String, Vec<u8>>,
}

impl Corpus {
    /// Relit un corpus ecrit par `callgraph-corpus.py`.
    ///
    /// # Erreurs
    ///
    /// Fichier illisible, signature absente ou entree tronquee.
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("{} illisible : {e}", path.display()))?;
        Self::parse(&bytes)
    }

    /// Decode le contenu d'un corpus.
    ///
    /// # Erreurs
    ///
    /// Signature absente ou entree tronquee.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if !bytes.starts_with(MAGIC) {
            return Err("signature RFXCORP1 absente".to_owned());
        }
        let truncated = || "corpus tronque".to_owned();
        let mut classes = HashMap::new();
        let mut at = MAGIC.len();
        while at < bytes.len() {
            let name_len = usize::from(u16::from_le_bytes(
                bytes
                    .get(at..at + 2)
                    .ok_or_else(truncated)?
                    .try_into()
                    .map_err(|_| truncated())?,
            ));
            at += 2;
            let name = std::str::from_utf8(bytes.get(at..at + name_len).ok_or_else(truncated)?)
                .map_err(|_| "nom de classe en UTF-8 invalide".to_owned())?
                .to_owned();
            at += name_len;
            let len = u32::from_le_bytes(
                bytes
                    .get(at..at + 4)
                    .ok_or_else(truncated)?
                    .try_into()
                    .map_err(|_| truncated())?,
            ) as usize;
            at += 4;
            let data = bytes.get(at..at + len).ok_or_else(truncated)?.to_vec();
            at += len;
            classes.entry(name).or_insert(data);
        }
        Ok(Self { classes })
    }

    /// Hierarchie de toutes les classes lisibles, telle que Java la transmet, et le
    /// nombre de classes illisibles.
    #[must_use]
    pub fn hierarchy(&self) -> (Vec<HierarchyEntry>, usize) {
        let mut entries = Vec::with_capacity(self.classes.len());
        let mut unreadable = 0;
        for bytes in self.classes.values() {
            match ClassFile::parse(bytes) {
                Ok(class) => entries.push(HierarchyEntry(
                    class.name,
                    class.super_name.unwrap_or_default(),
                    u32::from(class.access),
                )),
                Err(_) => unreadable += 1,
            }
        }
        (entries, unreadable)
    }

    /// Les `count` premieres methodes `tick()V` concretes de `net/minecraft/`, par nom
    /// de classe.
    #[must_use]
    pub fn roots(&self, count: usize) -> Vec<MethodRef> {
        let mut names: Vec<&String> = self
            .classes
            .keys()
            .filter(|name| name.starts_with("net/minecraft/"))
            .collect();
        names.sort();
        names
            .into_iter()
            .filter_map(|name| ClassFile::parse(&self.classes[name]).ok())
            .filter(|class| {
                class
                    .methods
                    .iter()
                    .any(|m| m.name == "tick" && m.descriptor == "()V" && m.code.is_some())
            })
            .map(|class| MethodRef::new(&class.name, "tick", "()V"))
            .take(count)
            .collect()
    }
}

/// Cout d'un graphe, phase par phase.
#[derive(Debug, Clone, Serialize)]
pub struct GraphCost {
    /// Racine, `classe.methode(descripteur)`.
    pub root: String,
    /// Duree totale : pas et classes fournies.
    pub total_ns: u64,
    /// Duree des pas : construction du graphe, tour apres tour.
    pub step_ns: u64,
    /// Duree des classes fournies : lecture du bytecode et insertion dans l'index.
    pub provide_ns: u64,
    /// Pas effectues.
    pub steps: u32,
    /// Tours de construction rapportes par l'analyse.
    pub rounds: u32,
    /// Classes demandees et trouvees dans le corpus.
    pub provided: u32,
    /// Classes demandees et absentes du corpus.
    pub absent: u32,
    /// Methodes du graphe.
    pub nodes: u32,
    /// Sites d'appel resolus.
    pub resolved_calls: u32,
    /// Sites d'appel non resolus.
    pub unresolved_calls: u32,
    /// Graphe arrete par une borne.
    pub truncated: bool,
}

/// Une repetition : une analyse neuve, toute la hierarchie, puis chaque racine.
#[derive(Debug, Serialize)]
pub struct Repetition {
    /// Duree de la declaration de la hierarchie, lecture des en-tetes exclue.
    pub declare_ns: u64,
    /// Cout de chaque graphe, dans l'ordre des racines.
    pub graphs: Vec<GraphCost>,
}

/// Une repetition complete.
#[must_use]
pub fn run(corpus: &Corpus, hierarchy: &[HierarchyEntry], roots: &[MethodRef]) -> Repetition {
    let mut analysis = Analysis::new(&RuntimeConfig::default());
    let started = Instant::now();
    analysis.declare(hierarchy.to_vec());
    let declare_ns = elapsed_ns(started);
    let graphs = roots
        .iter()
        .enumerate()
        .map(|(index, root)| {
            let id = WorkId(index as u64 + 1);
            analysis.note(id, root.clone(), Side::Common);
            drive(&mut analysis, corpus, id, root)
        })
        .collect();
    Repetition { declare_ns, graphs }
}

/// Mene un graphe jusqu'au bout, comme le fil d'analyse en jeu.
fn drive(analysis: &mut Analysis, corpus: &Corpus, id: WorkId, root: &MethodRef) -> GraphCost {
    let mut cost = GraphCost {
        root: root.to_string(),
        total_ns: 0,
        step_ns: 0,
        provide_ns: 0,
        steps: 0,
        rounds: 0,
        provided: 0,
        absent: 0,
        nodes: 0,
        resolved_calls: 0,
        unresolved_calls: 0,
        truncated: false,
    };
    for _ in 0..MAX_STEPS {
        let started = Instant::now();
        // La chaleur ne change rien au graphe : seulement a l'ordre et aux drapeaux.
        let state = analysis.step(&[HotWorkload {
            work_id: id,
            heat: Heat::Hot,
            observation_quality: 1.0,
        }]);
        cost.step_ns += elapsed_ns(started);
        cost.steps += 1;
        if state != StepState::NeedsClasses {
            break;
        }
        for name in analysis.requests().to_vec() {
            let bytes = corpus.classes.get(&name).map(Vec::as_slice);
            if bytes.is_some() {
                cost.provided += 1;
            } else {
                cost.absent += 1;
            }
            let started = Instant::now();
            analysis.provide(&name, bytes);
            cost.provide_ns += elapsed_ns(started);
        }
    }
    if let Some(summary) = analysis.summary(id) {
        cost.rounds = summary.rounds;
        cost.nodes = summary.nodes;
        cost.resolved_calls = summary.resolved_calls;
        cost.unresolved_calls = summary.unresolved_calls;
        cost.truncated = summary.truncated;
    }
    cost.total_ns = cost.step_ns + cost.provide_ns;
    cost
}

fn elapsed_ns(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// Distribution d'une grandeur : mediane, dispersion et intervalle de confiance de la
/// mediane (R-581).
#[derive(Debug, Serialize)]
pub struct Distribution {
    /// Mediane.
    pub median: f64,
    /// Ecart interquartile.
    pub iqr: f64,
    /// 90e centile.
    pub p90: f64,
    /// Maximum.
    pub max: f64,
    /// Borne basse de l'intervalle de confiance a 95 % de la mediane.
    pub ci_lower: f64,
    /// Borne haute de cet intervalle.
    pub ci_upper: f64,
    /// Nombre d'echantillons.
    pub samples: usize,
}

impl Distribution {
    /// Distribution d'un echantillon. L'intervalle de confiance de la mediane vient des
    /// statistiques d'ordre (approximation normale de la loi binomiale), sans
    /// hypothese sur la forme de la distribution.
    #[must_use]
    pub fn of(values: &[f64]) -> Self {
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        let n = sorted.len();
        let rank = |offset: f64| -> f64 {
            if n == 0 {
                return 0.0;
            }
            let position = (n as f64 / 2.0 + offset).clamp(0.0, (n - 1) as f64);
            sorted[position as usize]
        };
        let spread = 1.96 * (n as f64).sqrt() / 2.0;
        Self {
            median: crate::quantile(&sorted, 0.5),
            iqr: crate::quantile(&sorted, 0.75) - crate::quantile(&sorted, 0.25),
            p90: crate::quantile(&sorted, 0.9),
            max: sorted.last().copied().unwrap_or(0.0),
            ci_lower: rank(-spread - 1.0),
            ci_upper: rank(spread),
            samples: n,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn a_corpus_keeps_the_first_of_two_homonyms() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend(entry("a/B", b"first"));
        bytes.extend(entry("a/C", b"other"));
        bytes.extend(entry("a/B", b"second"));
        let corpus = Corpus::parse(&bytes).expect("lisible");
        assert_eq!(corpus.classes.len(), 2);
        assert_eq!(corpus.classes["a/B"], b"first");
    }

    #[test]
    fn a_truncated_or_unsigned_corpus_is_refused() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend(entry("a/B", b"contenu"));
        for cut in [9, 12, bytes.len() - 1] {
            assert!(Corpus::parse(&bytes[..cut]).is_err(), "tronque a {cut}");
        }
        assert!(Corpus::parse(b"RFXCORP0").is_err());
    }

    #[test]
    fn the_median_interval_brackets_the_median() {
        let values: Vec<f64> = (1..=101).map(f64::from).collect();
        let d = Distribution::of(&values);
        assert!((d.median - 51.0).abs() < f64::EPSILON);
        assert!(d.ci_lower < d.median && d.median < d.ci_upper);
        assert!((d.max - 101.0).abs() < f64::EPSILON);
        assert_eq!(d.samples, 101);
        assert_eq!(Distribution::of(&[]).samples, 0);
    }
}
