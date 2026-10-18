//! C-07 et C-08 cote runtime : construire hors tick les graphes d'appel des unites
//! chaudes, et en tirer leurs descripteurs.
//!
//! Cahier des charges : PARTIE 5.7, PARTIE 5.8, R-340, R-350, R-351, R-533, FM-16.
//! Decisions : ADR-051, ADR-054. Maturite : `STABLE`.
//!
//! # Pilotage par Java
//!
//! Java fournit la hierarchie de l'installation ([`Analysis::declare`]), puis appelle
//! [`Analysis::step`] depuis un fil d'arriere-plan. Chaque pas construit le graphe de
//! l'unite en cours avec ce que l'index contient ; s'il manque du bytecode, le pas rend
//! [`StepState::NeedsClasses`] et la liste des classes voulues ([`Analysis::requests`]),
//! que Java fournit ([`Analysis::provide`]) avant le pas suivant.
//!
//! # Descripteurs (C-08)
//!
//! Un graphe termine donne un [`Descriptor`] : dependances (DM-08), drapeaux (DM-11) et
//! confiance (DM-12), calcules par les fonctions pures de `rfx_analyzer::classify`.
//! Les lectures, ecritures et effets de bord (DM-06, DM-07) viennent de C-11, composes
//! le long du graphe (ADR-055) ; C-10, C-12 et C-13 n'existant pas encore, leurs faits
//! sont absents et tout descripteur reste `UNKNOWN` (ADR-054). Les unites a analyser
//! sont prises parmi les plus chaudes, dans
//! l'ordre de `chaleur x (1 - confiance) / cout estime` ; un descripteur est recalcule
//! si le bytecode de sa methode racine change (R-350).
//!
//! # Hors du verrou du runtime
//!
//! L'analyse a son propre verrou. Le verrou que partage le tick n'est pris que pour
//! recuperer les unites a analyser et pour publier le statut : jamais pendant la
//! construction d'un graphe.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use rfx_analyzer::dataflow::{workload_effects, DataflowLimits, DomainTable};
use rfx_analyzer::hierarchy::Skeleton;
use rfx_analyzer::{
    classify, confidence, CallGraph, ClassIndex, EdgeKind, Facts, Limits, MethodRef,
};
use rfx_bytecode::ClassFile;
use rfx_model::{
    AccessSet, AnalysisStatus, ClassificationFlags, Confidence, DependencySet, Domain, Domains,
    Heat, HierarchyEntry, Precision, RuntimeConfig, Side, SideEffectSet, WorkId,
};

/// Tours de construction au plus par graphe ; au-dela, ce qui manque reste non resolu
/// et le descripteur est tronque (FM-16).
pub const MAX_ROUNDS: u32 = 16;

/// Classes dont le bytecode est garde au plus ; au-dela, il est retire entre deux
/// graphes, et la hierarchie reste.
pub const MAX_BYTECODE_CLASSES: usize = 4_096;

/// Unites chaudes ajoutees a la file a chaque reapprovisionnement.
pub const ROOTS_PER_REFILL: usize = 32;

/// Ecart de confiance combinee juge significatif pour C-15 (ADR-054).
pub const SIGNIFICANT_CONFIDENCE_DELTA: f32 = 0.1;

/// Octets estimes par classe declaree : nom, super-classe, entrees d'index.
const BYTES_PER_DECLARED_CLASS: u64 = 256;

/// Octets estimes par methode retenue comme racine possible.
const BYTES_PER_METHOD: u64 = 192;

/// Octets estimes par descripteur, hors noms des appels.
const BYTES_PER_DESCRIPTOR: u64 = 256;

/// Rapport estime entre la memoire d'une classe lue et ses octets.
const BYTECODE_EXPANSION: u64 = 3;

/// Issue d'un pas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum StepState {
    /// Rien a analyser : hierarchie absente, analyse desactivee, ou file vide.
    Idle = 0,
    /// Du bytecode manque : le fournir, puis rappeler.
    NeedsClasses = 1,
    /// Un graphe vient d'etre termine.
    Finished = 2,
}

/// Methode sondee en attente d'etre confiee a l'analyse : unite, methode, cote.
pub type StagedRoot = (WorkId, MethodRef, Side);

/// Unite chaude proposee a l'analyse, telle que C-05 la mesure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HotWorkload {
    /// Unite de travail.
    pub work_id: WorkId,
    /// Chaleur, d'apres son cout par tick.
    pub heat: Heat,
    /// Qualite de son observation, 0 a 1.
    pub observation_quality: f32,
}

/// Ce qu'on retient du graphe d'une unite de travail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphSummary {
    /// Noeuds.
    pub nodes: u32,
    /// Sites d'appel resolus.
    pub resolved_calls: u32,
    /// Sites d'appel non resolus.
    pub unresolved_calls: u32,
    /// Une borne a arrete l'exploration (R-340).
    pub truncated: bool,
    /// Un appel reflexif a ete rencontre.
    pub reflection: bool,
    /// La racine appartient a un cycle d'appels.
    pub recursive_root: bool,
    /// Duree de la derniere construction, en nanosecondes.
    pub build_ns: u64,
    /// Tours de construction.
    pub rounds: u32,
}

/// Descripteur d'une unite de travail (C-08) : ce que l'analyse en sait.
#[derive(Debug, Clone, PartialEq)]
pub struct Descriptor {
    /// Resume du graphe d'appel (C-07).
    pub summary: GraphSummary,
    /// Dependances (DM-08) ; celles de C-10 sont absentes.
    pub deps: DependencySet,
    /// Lectures (DM-06, C-11).
    pub reads: AccessSet,
    /// Ecritures (DM-06, C-11).
    pub writes: AccessSet,
    /// Effets de bord (DM-07, C-11).
    pub side_effects: SideEffectSet,
    /// Drapeaux (DM-11).
    pub flags: ClassificationFlags,
    /// Confiance (DM-12).
    pub confidence: Confidence,
    /// Hachage du code de la methode racine, s'il etait lisible (R-350).
    pub bytecode_hash: Option<u64>,
    /// Le bytecode de la racine a change depuis : a recalculer (R-350).
    pub stale: bool,
}

/// Racine possible d'un graphe : une methode sondee.
struct Root {
    method: MethodRef,
    side: Side,
}

struct Job {
    work_id: WorkId,
    root: MethodRef,
    rounds: u32,
}

/// Analyse statique des unites de travail.
pub struct Analysis {
    enabled: bool,
    limits: Limits,
    max_methods: usize,
    index: ClassIndex,
    domains: DomainTable,
    methods: HashMap<WorkId, Root>,
    roots_by_owner: HashMap<String, Vec<WorkId>>,
    observed: HashMap<WorkId, (Heat, f32)>,
    queue: VecDeque<WorkId>,
    queued: HashSet<WorkId>,
    descriptors: HashMap<WorkId, Descriptor>,
    descriptor_bytes: u64,
    job: Option<Job>,
    requests: Vec<String>,
    bytecode_bytes: u64,
    status: AnalysisStatus,
}

impl core::fmt::Debug for Analysis {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Analysis")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl Analysis {
    /// Analyse vide, d'apres la configuration.
    #[must_use]
    pub fn new(config: &RuntimeConfig) -> Self {
        Self {
            enabled: config.analysis_enabled,
            limits: Limits {
                max_call_depth: config.analysis_max_call_depth,
                max_nodes: config.analysis_max_nodes as usize,
            },
            max_methods: config.profiler_max_workloads as usize,
            index: ClassIndex::new(),
            domains: DomainTable::new(),
            methods: HashMap::new(),
            roots_by_owner: HashMap::new(),
            observed: HashMap::new(),
            queue: VecDeque::new(),
            queued: HashSet::new(),
            descriptors: HashMap::new(),
            descriptor_bytes: 0,
            job: None,
            requests: Vec::new(),
            bytecode_bytes: 0,
            status: AnalysisStatus {
                enabled: config.analysis_enabled,
                ..AnalysisStatus::default()
            },
        }
    }

    /// `true` si l'analyse est active.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Retient la methode d'une unite de travail sondee : racine possible d'un graphe.
    pub fn note(&mut self, work_id: WorkId, method: MethodRef, side: Side) {
        if !self.enabled
            || (self.methods.len() >= self.max_methods && !self.methods.contains_key(&work_id))
        {
            return;
        }
        let owners = self.roots_by_owner.entry(method.owner.clone()).or_default();
        if !owners.contains(&work_id) {
            owners.push(work_id);
        }
        self.methods.insert(work_id, Root { method, side });
    }

    /// Declare des classes de la hierarchie de l'installation.
    pub fn declare(&mut self, entries: Vec<HierarchyEntry>) {
        if !self.enabled {
            return;
        }
        // La hierarchie change : les domaines deja calcules peuvent ne plus valoir.
        self.domains = DomainTable::new();
        for HierarchyEntry(name, super_name, access) in entries {
            self.index.declare(
                &name,
                Skeleton {
                    super_name: (!super_name.is_empty()).then_some(super_name),
                    access: u16::try_from(access).unwrap_or(u16::MAX),
                },
            );
        }
        self.status.hierarchy_classes = self.index.declared_len() as u64;
    }

    /// Fournit le bytecode d'une classe, ou declare qu'il ne le sera pas (`None`). Un
    /// bytecode illisible, ou qui ne porte pas le nom attendu, vaut une absence.
    ///
    /// Si la classe porte la methode racine d'un descripteur et que le code de cette
    /// methode a change, le descripteur est a recalculer (R-350).
    pub fn provide(&mut self, name: &str, bytes: Option<&[u8]>) {
        let class = bytes.and_then(|bytes| {
            ClassFile::parse(bytes)
                .ok()
                .filter(|class| class.name == name)
                .map(|class| (class, bytes.len() as u64))
        });
        match class {
            Some((class, len)) => {
                self.bytecode_bytes = self.bytecode_bytes.saturating_add(len);
                self.index.insert(class);
                self.status.classes_provided = self.status.classes_provided.saturating_add(1);
                self.mark_changed_roots(name);
            }
            None => {
                self.index.declare_absent(name);
                self.status.classes_absent = self.status.classes_absent.saturating_add(1);
            }
        }
    }

    /// R-350 : les descripteurs dont la racine est dans `class` et dont le code a change.
    fn mark_changed_roots(&mut self, class: &str) {
        let Some(ids) = self.roots_by_owner.get(class) else {
            return;
        };
        for id in ids {
            let (Some(root), Some(descriptor)) =
                (self.methods.get(id), self.descriptors.get_mut(id))
            else {
                continue;
            };
            let current = method_hash(&self.index, &root.method);
            if descriptor.bytecode_hash.is_some() && current != descriptor.bytecode_hash {
                descriptor.stale = true;
            }
        }
    }

    /// `true` s'il faut des unites chaudes pour continuer : aucun graphe en cours, file
    /// vide. Le runtime ne les calcule qu'alors.
    #[must_use]
    pub fn needs_roots(&self) -> bool {
        self.enabled && self.job.is_none() && self.queue.is_empty()
    }

    /// Un pas : poursuit le graphe en cours, ou en commence un parmi `hot`.
    pub fn step(&mut self, hot: &[HotWorkload]) -> StepState {
        let started = Instant::now();
        let state = self.advance(hot);
        let spent = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.status.step_ns = self.status.step_ns.saturating_add(spent);
        state
    }

    fn advance(&mut self, hot: &[HotWorkload]) -> StepState {
        if !self.enabled || self.index.declared_len() == 0 {
            self.requests.clear();
            return StepState::Idle;
        }
        let mut job = match self.job.take() {
            Some(job) => job,
            None => {
                self.enqueue(hot);
                let Some((work_id, root)) = self.next_root() else {
                    self.requests.clear();
                    self.status.queue = 0;
                    return StepState::Idle;
                };
                if self.index.len() > MAX_BYTECODE_CLASSES {
                    self.index.clear_bytecode();
                    self.bytecode_bytes = 0;
                }
                Job {
                    work_id,
                    root,
                    rounds: 0,
                }
            }
        };
        self.status.queue = self.queue.len() as u64;

        let graph = CallGraph::build(&self.index, &job.root, self.limits);
        job.rounds += 1;
        self.status.rounds = self.status.rounds.saturating_add(1);
        let requests = self.index.take_requests();
        if !requests.is_empty() && job.rounds < MAX_ROUNDS {
            self.requests = requests;
            self.job = Some(job);
            return StepState::NeedsClasses;
        }
        // Tours epuises avec des classes encore voulues : budget epuise, le resultat est
        // tronque, jamais optimiste (FM-16).
        let exhausted = !requests.is_empty();
        self.requests.clear();
        self.finish(&job, &graph, exhausted);
        StepState::Finished
    }

    /// Ajoute a la file les unites chaudes sans descripteur ou a recalculer, les plus
    /// rentables a analyser d'abord : `chaleur x (1 - confiance) / cout estime`.
    fn enqueue(&mut self, hot: &[HotWorkload]) {
        for workload in hot {
            self.observed.insert(
                workload.work_id,
                (workload.heat, workload.observation_quality),
            );
        }
        let mut candidates: Vec<(f32, WorkId)> = hot
            .iter()
            .filter(|w| {
                self.methods.contains_key(&w.work_id)
                    && !self.queued.contains(&w.work_id)
                    && self
                        .descriptors
                        .get(&w.work_id)
                        .is_none_or(|descriptor| descriptor.stale)
            })
            .map(|w| (self.score(w), w.work_id))
            .collect();
        candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        for (_, id) in candidates {
            self.queued.insert(id);
            self.queue.push_back(id);
        }
    }

    /// `score_analyse` de la PARTIE 5.8. Chaleur ordinale (froide 1 a critique 4) ; cout
    /// estime : 1, ou le nombre de noeuds du graphe precedent.
    fn score(&self, workload: &HotWorkload) -> f32 {
        let heat = match workload.heat {
            Heat::Cold => 1.0,
            Heat::Warm => 2.0,
            Heat::Hot => 3.0,
            Heat::Critical => 4.0,
        };
        let previous = self.descriptors.get(&workload.work_id);
        let combined = previous.map_or(0.0, |d| d.confidence.combined);
        let cost = previous.map_or(1.0, |d| 1.0 + d.summary.nodes as f32);
        heat * (1.0 - combined) / cost
    }

    fn next_root(&mut self) -> Option<(WorkId, MethodRef)> {
        while let Some(work_id) = self.queue.pop_front() {
            self.queued.remove(&work_id);
            if let Some(root) = self.methods.get(&work_id) {
                return Some((work_id, root.method.clone()));
            }
        }
        None
    }

    fn finish(&mut self, job: &Job, graph: &CallGraph, exhausted: bool) {
        let summary = GraphSummary {
            nodes: u32::try_from(graph.nodes().len()).unwrap_or(u32::MAX),
            resolved_calls: graph.resolved_calls(),
            unresolved_calls: graph.unresolved_calls(),
            truncated: graph.truncated(),
            reflection: graph.reflection(),
            recursive_root: graph.find(&job.root).is_some_and(|node| node.recursive),
            build_ns: graph.build_ns(),
            rounds: job.rounds,
        };
        let s = &mut self.status;
        s.graphs = s.graphs.saturating_add(1);
        s.nodes = s.nodes.saturating_add(u64::from(summary.nodes));
        s.resolved_calls = s
            .resolved_calls
            .saturating_add(u64::from(summary.resolved_calls));
        s.unresolved_calls = s
            .unresolved_calls
            .saturating_add(u64::from(summary.unresolved_calls));
        s.truncated = s.truncated.saturating_add(u64::from(summary.truncated));
        s.reflection = s.reflection.saturating_add(u64::from(summary.reflection));
        s.last_build_ns = summary.build_ns;

        let started = Instant::now();
        let effects = if exhausted {
            // Tours epuises : ce qui manque n'a pas ete vu, rien n'est affirme (FM-16).
            unknown_effects()
        } else {
            let effects =
                workload_effects(graph, &self.index, &self.domains, &DataflowLimits::DEFAULT);
            (effects.reads, effects.writes, effects.side_effects)
        };
        let elapsed = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.status.dataflow_ns = self.status.dataflow_ns.saturating_add(elapsed);

        let descriptor = self.describe(job, graph, summary, exhausted, effects);
        self.record(job.work_id, descriptor);
    }

    /// Le descripteur d'un graphe termine : faits de C-05, C-07 et C-11, le reste absent.
    /// La racine est toujours le noeud 0 du graphe.
    fn describe(
        &self,
        job: &Job,
        graph: &CallGraph,
        summary: GraphSummary,
        exhausted: bool,
        (reads, writes, side_effects): (AccessSet, AccessSet, SideEffectSet),
    ) -> Descriptor {
        let mut calls: Vec<String> = graph
            .edges()
            .iter()
            .filter(|edge| edge.from == 0)
            .filter_map(|edge| match (edge.kind, edge.to) {
                (EdgeKind::Resolved(_), Some(to)) => Some(graph.nodes()[to].method.to_string()),
                _ => None,
            })
            .collect();
        calls.sort();
        calls.dedup();
        let depth = graph
            .nodes()
            .iter()
            .map(|node| node.depth)
            .max()
            .unwrap_or(0);
        let deps = DependencySet {
            calls,
            unresolved_calls: summary.unresolved_calls,
            data_deps: None,
            order_deps: None,
            external_locks: None,
            depth: u16::try_from(depth).unwrap_or(u16::MAX),
            truncated: summary.truncated || exhausted,
        };
        let (heat, observation_quality) = self
            .observed
            .get(&job.work_id)
            .copied()
            .unwrap_or((Heat::Cold, 0.0));
        let side = self
            .methods
            .get(&job.work_id)
            .map_or(Side::Common, |root| root.side);
        let facts = Facts {
            heat,
            side,
            observation_quality,
            deps: Some(deps.clone()),
            reflection: summary.reflection,
            reads: Some(reads),
            writes: Some(writes),
            side_effects: Some(side_effects),
            ..Facts::default()
        };
        Descriptor {
            summary,
            deps,
            reads,
            writes,
            side_effects,
            flags: classify(&facts),
            confidence: confidence(&facts),
            bytecode_hash: method_hash(&self.index, &job.root),
            stale: false,
        }
    }

    /// Range un descripteur, compte les recalculs et les changements significatifs.
    fn record(&mut self, work_id: WorkId, descriptor: Descriptor) {
        let s = &mut self.status;
        s.completed = s.completed.saturating_add(1);
        s.descriptors_truncated = s
            .descriptors_truncated
            .saturating_add(u64::from(descriptor.deps.truncated));
        let bytes = descriptor_bytes(&descriptor);
        if let Some(previous) = self.descriptors.get(&work_id) {
            if previous.stale {
                s.recomputes = s.recomputes.saturating_add(1);
            }
            let moved = (previous.confidence.combined - descriptor.confidence.combined).abs();
            if previous.flags != descriptor.flags || moved >= SIGNIFICANT_CONFIDENCE_DELTA {
                s.descriptor_changes = s.descriptor_changes.saturating_add(1);
            }
            self.descriptor_bytes = self
                .descriptor_bytes
                .saturating_sub(descriptor_bytes(previous));
        }
        self.descriptor_bytes = self.descriptor_bytes.saturating_add(bytes);
        self.descriptors.insert(work_id, descriptor);
        let unknown = self
            .descriptors
            .values()
            .filter(|d| d.flags.contains(ClassificationFlags::UNKNOWN))
            .count();
        let exact = self
            .descriptors
            .values()
            .filter(|d| d.writes.precision == Precision::Exact)
            .count();
        self.status.descriptors = self.descriptors.len() as u64;
        self.status.descriptors_unknown = unknown as u64;
        self.status.dataflow_exact = exact as u64;
    }

    /// Classes voulues par le dernier pas qui a rendu [`StepState::NeedsClasses`].
    #[must_use]
    pub fn requests(&self) -> &[String] {
        &self.requests
    }

    /// Resume du graphe d'une unite de travail, s'il a ete construit.
    #[must_use]
    pub fn summary(&self, work_id: WorkId) -> Option<&GraphSummary> {
        self.descriptors.get(&work_id).map(|d| &d.summary)
    }

    /// Descripteur d'une unite de travail, s'il a ete calcule (C-08).
    #[must_use]
    pub fn descriptor(&self, work_id: WorkId) -> Option<&Descriptor> {
        self.descriptors.get(&work_id)
    }

    /// Etat publie (`rfx.callgraph.*`, `rfx.analysis.*`).
    #[must_use]
    pub fn status(&self) -> AnalysisStatus {
        self.status
    }

    /// Memoire estimee, en octets : hierarchie, bytecode lu, racines retenues,
    /// descripteurs.
    #[must_use]
    pub fn estimated_bytes(&self) -> u64 {
        (self.index.declared_len() as u64).saturating_mul(BYTES_PER_DECLARED_CLASS)
            + self.bytecode_bytes.saturating_mul(BYTECODE_EXPANSION)
            + (self.methods.len() as u64).saturating_mul(BYTES_PER_METHOD)
            + self.descriptor_bytes
    }

    /// Arrete l'analyse et rend sa memoire : le budget natif l'a refusee (R-533).
    pub fn disable(&mut self) {
        self.enabled = false;
        self.index = ClassIndex::new();
        self.domains = DomainTable::new();
        self.methods.clear();
        self.roots_by_owner.clear();
        self.observed.clear();
        self.queue.clear();
        self.queued.clear();
        self.descriptors.clear();
        self.descriptor_bytes = 0;
        self.job = None;
        self.requests.clear();
        self.bytecode_bytes = 0;
        self.status.enabled = false;
        self.status.descriptors = 0;
        self.status.descriptors_unknown = 0;
        self.status.dataflow_exact = 0;
        self.status.budget_refusals = self.status.budget_refusals.saturating_add(1);
    }
}

/// Effets d'une unite dont l'analyse n'a pas abouti : tout est possible (R-206, R-380).
fn unknown_effects() -> (AccessSet, AccessSet, SideEffectSet) {
    let unknown = AccessSet {
        domains: Domains::EMPTY.with(Domain::Unknown),
        precision: Precision::Unknown,
    };
    let side_effects = SideEffectSet {
        mutates_world: true,
        throws_observable: true,
        unknown: true,
        ..SideEffectSet::default()
    };
    (unknown, unknown, side_effects)
}

/// Hachage du code d'une methode, si sa classe est lue et la methode a du code.
fn method_hash(index: &ClassIndex, method: &MethodRef) -> Option<u64> {
    index
        .get(&method.owner)?
        .method(&method.name, &method.descriptor)?
        .code
        .as_deref()
        .map(xxhash_rust::xxh3::xxh3_64)
}

/// Memoire estimee d'un descripteur.
fn descriptor_bytes(descriptor: &Descriptor) -> u64 {
    BYTES_PER_DESCRIPTOR
        + descriptor
            .deps
            .calls
            .iter()
            .map(|call| call.len() as u64 + 24)
            .sum::<u64>()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// Bytecode des classes de test de `rfx-bytecode`, par nom interne.
    pub(crate) fn fixtures() -> BTreeMap<String, Vec<u8>> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../rfx-bytecode/tests/fixtures/classes/fixtures");
        std::fs::read_dir(&dir)
            .expect("classes de test")
            .map(|entry| entry.expect("entree").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "class"))
            .map(|path| {
                let bytes = std::fs::read(&path).expect("lecture");
                (ClassFile::parse(&bytes).expect("lisible").name, bytes)
            })
            .collect()
    }

    /// Hierarchie des classes de test, telle que Java la transmet.
    pub(crate) fn hierarchy(classes: &BTreeMap<String, Vec<u8>>) -> Vec<HierarchyEntry> {
        classes
            .values()
            .map(|bytes| {
                let class = ClassFile::parse(bytes).expect("lisible");
                HierarchyEntry(
                    class.name,
                    class.super_name.unwrap_or_default(),
                    u32::from(class.access),
                )
            })
            .collect()
    }

    /// Unite chaude, bien observee.
    pub(crate) fn hot(work_id: WorkId) -> HotWorkload {
        HotWorkload {
            work_id,
            heat: Heat::Hot,
            observation_quality: 1.0,
        }
    }

    /// Mene les pas jusqu'au bout, en fournissant ce qui est demande.
    pub(crate) fn drive(
        analysis: &mut Analysis,
        hot: &[HotWorkload],
        classes: &BTreeMap<String, Vec<u8>>,
    ) -> StepState {
        for _ in 0..64 {
            match analysis.step(hot) {
                StepState::NeedsClasses => {
                    for name in analysis.requests().to_vec() {
                        analysis.provide(&name, classes.get(&name).map(Vec::as_slice));
                    }
                }
                done => return done,
            }
        }
        panic!("l'analyse ne converge pas");
    }

    fn vehicle() -> (WorkId, MethodRef) {
        (
            WorkId(7),
            MethodRef::new("fixtures/Calls", "vehicle", "(Lfixtures/Vehicle;)I"),
        )
    }

    #[test]
    fn a_hot_workload_gets_its_graph() {
        let classes = fixtures();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        let (id, root) = vehicle();
        analysis.note(id, root, Side::Server);
        analysis.declare(hierarchy(&classes));
        assert!(analysis.needs_roots());

        assert_eq!(
            drive(&mut analysis, &[hot(id)], &classes),
            StepState::Finished
        );
        let summary = *analysis.summary(id).expect("graphe");
        assert_eq!(summary.resolved_calls, 1, "un site, trois cibles");
        assert_eq!(summary.unresolved_calls, 0);
        assert_eq!(summary.nodes, 4, "racine et trois sous-classes");
        assert!(summary.rounds >= 2, "rien n'etait fourni au depart");

        let status = analysis.status();
        assert_eq!(status.graphs, 1);
        assert_eq!(status.hierarchy_classes, classes.len() as u64);
        assert!(status.classes_provided >= 4);
        assert!(analysis.estimated_bytes() > 0);

        // C-08 : un descripteur, UNKNOWN faute des faits de C-10 a C-13 (ADR-054).
        let descriptor = analysis.descriptor(id).expect("descripteur");
        assert!(descriptor.flags.contains(ClassificationFlags::UNKNOWN));
        assert!(descriptor.flags.contains(ClassificationFlags::HOT));
        assert!(descriptor.flags.contains(ClassificationFlags::SERVER_ONLY));
        assert!(!descriptor
            .flags
            .contains(ClassificationFlags::PARALLELIZABLE));
        assert!(descriptor.confidence.combined.abs() < f32::EPSILON);
        assert_eq!(descriptor.deps.calls.len(), 3, "les trois roues");
        assert!(descriptor.deps.data_deps.is_none(), "C-10 absent : inconnu");
        assert!(!descriptor.deps.truncated);
        assert!(descriptor.bytecode_hash.is_some());
        assert_eq!((status.descriptors, status.completed), (1, 1));
        assert_eq!(status.descriptors_unknown, 1);
        assert!(status.step_ns > 0);
        assert!(status.dataflow_ns > 0, "C-11 : effets composes");

        // Deja analysee : rien a refaire.
        assert_eq!(analysis.step(&[hot(id)]), StepState::Idle);
    }

    #[test]
    fn nothing_happens_without_hierarchy_or_when_disabled() {
        let (id, root) = vehicle();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        analysis.note(id, root.clone(), Side::Common);
        assert_eq!(
            analysis.step(&[hot(id)]),
            StepState::Idle,
            "hierarchie absente"
        );

        let mut disabled = Analysis::new(&RuntimeConfig {
            analysis_enabled: false,
            ..RuntimeConfig::default()
        });
        disabled.note(id, root, Side::Common);
        disabled.declare(hierarchy(&fixtures()));
        assert_eq!(disabled.step(&[hot(id)]), StepState::Idle);
        assert_eq!(disabled.status().hierarchy_classes, 0);
        assert!(!disabled.needs_roots());
    }

    #[test]
    fn unreadable_or_misnamed_bytecode_counts_as_absent() {
        let classes = fixtures();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        analysis.declare(hierarchy(&classes));
        analysis.provide("fixtures/Car", Some(b"pas une classe"));
        analysis.provide("fixtures/Bike", Some(&classes["fixtures/Truck"]));
        analysis.provide("fixtures/Truck", None);
        let status = analysis.status();
        assert_eq!((status.classes_provided, status.classes_absent), (0, 3));

        // Sous-classes absentes : l'appel par `Vehicle` n'est pas resolu.
        let (id, root) = vehicle();
        analysis.note(id, root, Side::Common);
        assert_eq!(
            drive(&mut analysis, &[hot(id)], &classes),
            StepState::Finished
        );
        let summary = analysis.summary(id).unwrap();
        assert_eq!((summary.resolved_calls, summary.unresolved_calls), (0, 1));
    }

    #[test]
    fn a_budget_refusal_stops_and_frees_the_analysis() {
        let classes = fixtures();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        analysis.declare(hierarchy(&classes));
        analysis.disable();
        let status = analysis.status();
        assert!(!status.enabled);
        assert_eq!(status.budget_refusals, 1);
        assert_eq!(analysis.estimated_bytes(), 0);
        let (id, root) = vehicle();
        analysis.note(id, root, Side::Common);
        assert_eq!(analysis.step(&[hot(id)]), StepState::Idle);
    }

    /// C-11 : les effets composes le long du graphe remplissent le descripteur. Un objet
    /// construit et lu sur place ne touche aucun domaine : ecritures exactes et vides.
    #[test]
    fn the_descriptor_carries_the_dataflow_effects() {
        let classes = fixtures();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        let local = WorkId(11);
        let shared = WorkId(12);
        analysis.note(
            local,
            MethodRef::new("fixtures/Interproc", "localObject", "()I"),
            Side::Server,
        );
        analysis.note(
            shared,
            MethodRef::new("fixtures/Interproc", "setOnShared", "()V"),
            Side::Server,
        );
        analysis.declare(hierarchy(&classes));
        let hot_both = [hot(local), hot(shared)];
        assert_eq!(
            drive(&mut analysis, &hot_both, &classes),
            StepState::Finished
        );
        assert_eq!(
            drive(&mut analysis, &hot_both, &classes),
            StepState::Finished
        );

        let local = analysis.descriptor(local).expect("descripteur");
        assert_eq!(local.writes.precision, Precision::Exact);
        assert_eq!(local.writes.domains, Domains::EMPTY);
        assert!(!local.side_effects.unknown);
        assert!(
            !local.flags.contains(ClassificationFlags::WRITE),
            "aucune ecriture"
        );
        assert!(
            local.flags.contains(ClassificationFlags::UNKNOWN),
            "C-12 et C-13 manquent encore"
        );

        let shared = analysis.descriptor(shared).expect("descripteur");
        assert!(shared.writes.domains.contains(Domain::Unknown));
        assert!(shared.flags.contains(ClassificationFlags::WRITE));
        assert_eq!(analysis.status().dataflow_exact, 1);
    }

    /// T-171 : budget epuise (tours a la borne, classes encore voulues) : resultat
    /// tronque, donc UNKNOWN — jamais optimiste (FM-16).
    #[test]
    fn t171_an_exhausted_budget_gives_a_conservative_descriptor() {
        let classes = fixtures();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        let (id, root) = vehicle();
        analysis.note(id, root, Side::Common);
        analysis.declare(hierarchy(&classes));
        // Rien n'est fourni : chaque tour redemande les memes classes.
        let states: Vec<StepState> = (0..MAX_ROUNDS).map(|_| analysis.step(&[hot(id)])).collect();
        assert_eq!(states.last(), Some(&StepState::Finished));
        assert!(states[..states.len() - 1]
            .iter()
            .all(|s| *s == StepState::NeedsClasses));
        let descriptor = analysis.descriptor(id).expect("descripteur");
        assert!(descriptor.deps.truncated, "budget epuise");
        assert!(descriptor.flags.contains(ClassificationFlags::UNKNOWN));
        assert_eq!(
            descriptor.writes.precision,
            Precision::Unknown,
            "C-11 : rien d'affirme"
        );
        assert!(descriptor.side_effects.unknown);
        assert!(!descriptor.flags.contains(ClassificationFlags::PURE));
        assert_eq!(analysis.status().descriptors_truncated, 1);
    }

    /// Le bytecode de `Calls`, avec `iconst_1` de `helper` change en `iconst_2`.
    fn calls_with_changed_helper(bytes: &[u8]) -> Vec<u8> {
        let helper = [0x1a, 0x04, 0x60, 0xac]; // iload_0 iconst_1 iadd ireturn
        let at: Vec<usize> = bytes
            .windows(helper.len())
            .enumerate()
            .filter(|(_, w)| *w == helper)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(at.len(), 1, "code de helper unique dans la classe");
        let mut changed = bytes.to_vec();
        changed[at[0] + 1] = 0x05;
        changed
    }

    /// T-172 : le bytecode de la methode racine change, le descripteur est recalcule ;
    /// le meme bytecode fourni a nouveau ne declenche rien (R-350).
    #[test]
    fn t172_a_changed_root_bytecode_recomputes_the_descriptor() {
        let classes = fixtures();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        let id = WorkId(11);
        let root = MethodRef::new("fixtures/Calls", "helper", "(I)I");
        analysis.note(id, root, Side::Common);
        analysis.declare(hierarchy(&classes));
        assert_eq!(
            drive(&mut analysis, &[hot(id)], &classes),
            StepState::Finished
        );
        let first = analysis.descriptor(id).expect("descripteur").bytecode_hash;
        assert!(first.is_some());

        analysis.provide("fixtures/Calls", Some(&classes["fixtures/Calls"]));
        assert!(!analysis.descriptor(id).unwrap().stale, "meme code");
        assert_eq!(analysis.step(&[hot(id)]), StepState::Idle);

        let changed = calls_with_changed_helper(&classes["fixtures/Calls"]);
        analysis.provide("fixtures/Calls", Some(&changed));
        assert!(analysis.descriptor(id).unwrap().stale, "code change");
        assert_eq!(analysis.step(&[hot(id)]), StepState::Finished);
        let second = analysis.descriptor(id).unwrap();
        assert!(!second.stale);
        assert_ne!(second.bytecode_hash, first);
        let status = analysis.status();
        assert_eq!((status.recomputes, status.completed), (1, 2));
        assert_eq!(status.descriptors, 1);
    }

    /// Les unites les plus rentables a analyser passent d'abord : chaleur, puis
    /// identifiant a egalite (PARTIE 5.8, etape 1).
    #[test]
    fn the_hottest_workloads_are_analysed_first() {
        let classes = fixtures();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        let warm = WorkId(1);
        let critical = WorkId(2);
        analysis.note(
            warm,
            MethodRef::new("fixtures/Calls", "helper", "(I)I"),
            Side::Common,
        );
        analysis.note(
            critical,
            MethodRef::new("fixtures/Calls", "secret", "()I"),
            Side::Common,
        );
        analysis.declare(hierarchy(&classes));
        let hot = [
            HotWorkload {
                work_id: warm,
                heat: Heat::Warm,
                observation_quality: 1.0,
            },
            HotWorkload {
                work_id: critical,
                heat: Heat::Critical,
                observation_quality: 1.0,
            },
        ];
        assert_eq!(drive(&mut analysis, &hot, &classes), StepState::Finished);
        assert!(analysis.descriptor(critical).is_some(), "critique d'abord");
        assert!(analysis.descriptor(warm).is_none());
        assert_eq!(drive(&mut analysis, &hot, &classes), StepState::Finished);
        assert!(analysis.descriptor(warm).is_some());
    }
}
