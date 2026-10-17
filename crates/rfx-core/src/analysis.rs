//! C-07 cote runtime : construire hors tick les graphes d'appel des unites chaudes.
//!
//! Cahier des charges : PARTIE 5.7, R-340, R-533. Decision : ADR-051. Maturite :
//! `STABLE`.
//!
//! # Pilotage par Java
//!
//! Java fournit la hierarchie de l'installation ([`Analysis::declare`]), puis appelle
//! [`Analysis::step`] depuis un fil d'arriere-plan. Chaque pas construit le graphe de
//! l'unite en cours avec ce que l'index contient ; s'il manque du bytecode, le pas rend
//! [`StepState::NeedsClasses`] et la liste des classes voulues ([`Analysis::requests`]),
//! que Java fournit ([`Analysis::provide`]) avant le pas suivant.
//!
//! # Hors du verrou du runtime
//!
//! L'analyse a son propre verrou. Le verrou que partage le tick n'est pris que pour
//! recuperer les unites a analyser et pour publier le statut : jamais pendant la
//! construction d'un graphe.

use std::collections::{HashMap, HashSet, VecDeque};

use rfx_analyzer::hierarchy::Skeleton;
use rfx_analyzer::{CallGraph, ClassIndex, Limits, MethodRef};
use rfx_bytecode::ClassFile;
use rfx_model::{AnalysisStatus, HierarchyEntry, RuntimeConfig, WorkId};

/// Tours de construction au plus par graphe ; au-dela, ce qui manque reste non resolu.
pub const MAX_ROUNDS: u32 = 16;

/// Classes dont le bytecode est garde au plus ; au-dela, il est retire entre deux
/// graphes, et la hierarchie reste.
pub const MAX_BYTECODE_CLASSES: usize = 4_096;

/// Unites chaudes ajoutees a la file a chaque reapprovisionnement.
pub const ROOTS_PER_REFILL: usize = 32;

/// Octets estimes par classe declaree : nom, super-classe, entrees d'index.
const BYTES_PER_DECLARED_CLASS: u64 = 256;

/// Octets estimes par methode retenue comme racine possible.
const BYTES_PER_METHOD: u64 = 192;

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
    methods: HashMap<WorkId, MethodRef>,
    queue: VecDeque<WorkId>,
    queued: HashSet<WorkId>,
    done: HashMap<WorkId, GraphSummary>,
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
            methods: HashMap::new(),
            queue: VecDeque::new(),
            queued: HashSet::new(),
            done: HashMap::new(),
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
    pub fn note(&mut self, work_id: WorkId, method: MethodRef) {
        if self.enabled
            && (self.methods.len() < self.max_methods || self.methods.contains_key(&work_id))
        {
            self.methods.insert(work_id, method);
        }
    }

    /// Declare des classes de la hierarchie de l'installation.
    pub fn declare(&mut self, entries: Vec<HierarchyEntry>) {
        if !self.enabled {
            return;
        }
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
            }
            None => {
                self.index.declare_absent(name);
                self.status.classes_absent = self.status.classes_absent.saturating_add(1);
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
    pub fn step(&mut self, hot: &[WorkId]) -> StepState {
        if !self.enabled || self.index.declared_len() == 0 {
            self.requests.clear();
            return StepState::Idle;
        }
        let mut job = match self.job.take() {
            Some(job) => job,
            None => {
                for id in hot {
                    if !self.done.contains_key(id)
                        && self.methods.contains_key(id)
                        && self.queued.insert(*id)
                    {
                        self.queue.push_back(*id);
                    }
                }
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
        // Ce qui manque encore au dernier tour reste non resolu : prudent.
        self.requests.clear();
        self.finish(&job, &graph);
        StepState::Finished
    }

    fn next_root(&mut self) -> Option<(WorkId, MethodRef)> {
        while let Some(work_id) = self.queue.pop_front() {
            self.queued.remove(&work_id);
            if let Some(root) = self.methods.get(&work_id) {
                return Some((work_id, root.clone()));
            }
        }
        None
    }

    fn finish(&mut self, job: &Job, graph: &CallGraph) {
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
        self.done.insert(job.work_id, summary);
    }

    /// Classes voulues par le dernier pas qui a rendu [`StepState::NeedsClasses`].
    #[must_use]
    pub fn requests(&self) -> &[String] {
        &self.requests
    }

    /// Resume du graphe d'une unite de travail, s'il a ete construit.
    #[must_use]
    pub fn summary(&self, work_id: WorkId) -> Option<&GraphSummary> {
        self.done.get(&work_id)
    }

    /// Etat publie (`rfx.callgraph.*`).
    #[must_use]
    pub fn status(&self) -> AnalysisStatus {
        self.status
    }

    /// Memoire estimee, en octets : hierarchie, bytecode lu, racines retenues.
    #[must_use]
    pub fn estimated_bytes(&self) -> u64 {
        (self.index.declared_len() as u64).saturating_mul(BYTES_PER_DECLARED_CLASS)
            + self.bytecode_bytes.saturating_mul(BYTECODE_EXPANSION)
            + (self.methods.len() as u64).saturating_mul(BYTES_PER_METHOD)
    }

    /// Arrete l'analyse et rend sa memoire : le budget natif l'a refusee (R-533).
    pub fn disable(&mut self) {
        self.enabled = false;
        self.index = ClassIndex::new();
        self.methods.clear();
        self.queue.clear();
        self.queued.clear();
        self.job = None;
        self.requests.clear();
        self.bytecode_bytes = 0;
        self.status.enabled = false;
        self.status.budget_refusals = self.status.budget_refusals.saturating_add(1);
    }
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

    /// Mene les pas jusqu'au bout, en fournissant ce qui est demande.
    pub(crate) fn drive(
        analysis: &mut Analysis,
        hot: &[WorkId],
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
        analysis.note(id, root);
        analysis.declare(hierarchy(&classes));
        assert!(analysis.needs_roots());

        assert_eq!(drive(&mut analysis, &[id], &classes), StepState::Finished);
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

        // Deja analysee : rien a refaire.
        assert_eq!(analysis.step(&[id]), StepState::Idle);
    }

    #[test]
    fn nothing_happens_without_hierarchy_or_when_disabled() {
        let (id, root) = vehicle();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        analysis.note(id, root.clone());
        assert_eq!(analysis.step(&[id]), StepState::Idle, "hierarchie absente");

        let mut disabled = Analysis::new(&RuntimeConfig {
            analysis_enabled: false,
            ..RuntimeConfig::default()
        });
        disabled.note(id, root);
        disabled.declare(hierarchy(&fixtures()));
        assert_eq!(disabled.step(&[id]), StepState::Idle);
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
        analysis.note(id, root);
        assert_eq!(drive(&mut analysis, &[id], &classes), StepState::Finished);
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
        analysis.note(id, root);
        assert_eq!(analysis.step(&[id]), StepState::Idle);
    }
}
