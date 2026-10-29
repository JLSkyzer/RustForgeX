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
//! le long du graphe (ADR-055). Les dependances de donnees entre unites (C-10, ADR-056)
//! sont recalculees a chaque descripteur range ; les faits de determinisme (C-13,
//! ADR-058) et de fil (C-12, ADR-057) sont deduits du meme graphe. Les dependances
//! d'ordre (C-06) manquant, tout descripteur reste `UNKNOWN` (ADR-054). Les unites a
//! analyser sont prises parmi les plus chaudes, dans l'ordre de
//! `chaleur x (1 - confiance) / cout estime` ; un descripteur est recalcule si le
//! bytecode de sa methode racine change (R-350).
//!
//! # Hors du verrou du runtime
//!
//! L'analyse a son propre verrou. Le verrou que partage le tick n'est pris que pour
//! recuperer les unites a analyser et pour publier le statut : jamais pendant la
//! construction d'un graphe.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use rfx_analyzer::dataflow::{analyze_graph, workload_effects_from, DataflowLimits, DomainTable};
use rfx_analyzer::dependency::{dependencies, may_be_dispatched, WholeTick, Workload};
use rfx_analyzer::determinism::determinism_facts;
use rfx_analyzer::hierarchy::Skeleton;
use rfx_analyzer::platform;
use rfx_analyzer::thread::{thread_facts, ThreadSafetyTable};
use rfx_analyzer::{
    classify, confidence, CallGraph, ClassIndex, EdgeKind, Facts, Limits, MethodRef,
};
use rfx_bytecode::ClassFile;
use rfx_model::{
    AccessSet, AnalysisStatus, ClassificationFlags, Confidence, DependencySet, DeterminismFacts,
    Domain, Domains, Heat, HierarchyEntry, NonDetSource, PersistedAnalysis, Precision,
    RuntimeConfig, Side, SideEffectSet, ThreadFacts, Tri, WorkId,
};

/// Tampon du code d'analyse (ADR-064) : hachage du source qui produit les faits d'un
/// descripteur, calcule par `build.rs`. Une analyse persistee d'un autre tampon est
/// ecartee a la relecture.
pub const ANALYSIS_STAMP: u64 = parse_stamp(env!("RFX_ANALYSIS_STAMP"));

/// Lit l'entier decimal ecrit par `build.rs`. Un caractere qui n'est pas un chiffre
/// arrete la compilation.
const fn parse_stamp(text: &str) -> u64 {
    let bytes = text.as_bytes();
    let mut value = 0_u64;
    let mut i = 0;
    while i < bytes.len() {
        assert!(bytes[i].is_ascii_digit(), "tampon d'analyse invalide");
        value = value
            .wrapping_mul(10)
            .wrapping_add((bytes[i] - b'0') as u64);
        i += 1;
    }
    value
}

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
    /// Un verrou est pris sur un objet non local (C-11).
    pub locks: bool,
    /// Dependances dans le tick, au-dela des `data_deps` (C-10).
    pub dependency: DependencyCounts,
    /// Contraintes de fil (DM-09, C-12).
    pub thread: ThreadFacts,
    /// Determinisme (DM-10, C-13).
    pub determinism: DeterminismFacts,
    /// La racine peut etre un rappel qu'un repartiteur decide d'appeler : `order_deps`
    /// reste inconnu (C-10, ADR-060).
    pub may_be_dispatched: bool,
    /// Drapeaux (DM-11).
    pub flags: ClassificationFlags,
    /// Confiance (DM-12).
    pub confidence: Confidence,
    /// Hachage du code de la methode racine, s'il etait lisible (R-350).
    pub bytecode_hash: Option<u64>,
    /// Le bytecode de la racine a change depuis : a recalculer (R-350).
    pub stale: bool,
}

/// Dependances d'une unite dans le tick que DM-08 ne porte pas (C-10, ADR-056).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DependencyCounts {
    /// Unites dont elle lit ce qu'elles ecrivent.
    pub raw: u32,
    /// Unites qui ecrivent ce qu'elle ecrit.
    pub waw: u32,
    /// Unites qui lisent ce qu'elle ecrit.
    pub war: u32,
    /// Unites qui prennent aussi des verrous d'identite inconnue.
    pub resource: u32,
    /// Motif d'une dependance a tout le tick (R-372) ; ses `data_deps` sont alors
    /// absentes.
    pub whole_tick: Option<WholeTick>,
}

/// Effets d'une unite (C-11) : lectures, ecritures, effets de bord, verrou.
type Effects = (AccessSet, AccessSet, SideEffectSet, bool);

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
    safety: ThreadSafetyTable,
    methods: HashMap<WorkId, Root>,
    roots_by_owner: HashMap<String, Vec<WorkId>>,
    observed: HashMap<WorkId, (Heat, f32)>,
    queue: VecDeque<WorkId>,
    queued: HashSet<WorkId>,
    descriptors: HashMap<WorkId, Descriptor>,
    descriptor_bytes: u64,
    /// Descripteurs calcules depuis la derniere remise a la base (ADR-064).
    unsaved: HashSet<WorkId>,
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
            safety: ThreadSafetyTable::new(),
            methods: HashMap::new(),
            roots_by_owner: HashMap::new(),
            observed: HashMap::new(),
            queue: VecDeque::new(),
            queued: HashSet::new(),
            descriptors: HashMap::new(),
            descriptor_bytes: 0,
            unsaved: HashSet::new(),
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
        // La hierarchie change : les domaines et le confinement deja calcules peuvent ne
        // plus valoir.
        self.domains = DomainTable::new();
        self.safety = ThreadSafetyTable::new();
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
        // Un bytecode nouveau peut changer les champs d'une classe deja jugee (C-12).
        self.safety = ThreadSafetyTable::new();
        let class = bytes.and_then(|bytes| {
            ClassFile::parse(bytes)
                .ok()
                .filter(|class| class.name == name)
                .map(|class| (class, bytes.len() as u64))
        });
        // Ce qu'un greffon de lancement de Forge reecrit sans qu'on puisse le reproduire
        // est inconnu, comme une cible de mixin (ADR-059).
        let class = class
            .and_then(|(class, len)| platform::as_loaded(class, &self.index).map(|c| (c, len)));
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
        let mut reobserved = Vec::new();
        for workload in hot {
            let previous = self.observed.insert(
                workload.work_id,
                (workload.heat, workload.observation_quality),
            );
            // Une unite decrite vue pour la premiere fois, ou dont la chaleur change :
            // ses drapeaux et sa confiance dependent de l'observation. Sans cela, un
            // descripteur relu de la base (ADR-064) garderait ceux d'une unite jamais
            // observee tant qu'aucun graphe ne se termine.
            if self.descriptors.contains_key(&workload.work_id)
                && previous.is_none_or(|(heat, _)| heat != workload.heat)
            {
                reobserved.push(workload.work_id);
            }
        }
        if !reobserved.is_empty() {
            self.reclassify(&reobserved);
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
        let (effects, thread, determinism, external_locks) = if exhausted {
            // Tours epuises : ce qui manque n'a pas ete vu, rien n'est affirme (FM-16).
            (
                unknown_effects(),
                unknown_thread(),
                unknown_determinism(),
                None,
            )
        } else {
            let analysed = analyze_graph(graph, &self.index, &DataflowLimits::DEFAULT);
            let effects = workload_effects_from(graph, &self.index, &self.domains, &analysed);
            let determinism =
                determinism_facts(graph, &self.index, &analysed, &effects, &self.domains);
            let thread = thread_facts(
                graph,
                &self.index,
                &analysed,
                &effects,
                &self.domains,
                &self.safety,
                Some(&determinism),
            );
            // DM-08 : l'identite des verrous n'est pas suivie ; seule l'absence de tout
            // verrou, sur un graphe complet, est une liste connue (ADR-058).
            let lock_free =
                !effects.causes.any() && !thread.uses_synchronized && !thread.uses_locks;
            (
                (
                    effects.reads,
                    effects.writes,
                    effects.side_effects,
                    effects.locks,
                ),
                thread,
                determinism,
                lock_free.then(Vec::new),
            )
        };
        let elapsed = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.status.dataflow_ns = self.status.dataflow_ns.saturating_add(elapsed);

        let mut descriptor = self.describe(job, graph, summary, exhausted, effects);
        descriptor.thread = thread;
        descriptor.determinism = determinism;
        descriptor.deps.external_locks = external_locks;
        // Calcule tant que le bytecode de la racine est dans l'index (ADR-060). Une unite
        // epuisee a des acces inconnus : son ordre l'est de toute facon.
        descriptor.may_be_dispatched = may_be_dispatched(&self.index, &job.root);
        let facts = facts_of(
            &descriptor,
            self.observed.get(&job.work_id),
            self.methods.get(&job.work_id).map(|root| root.side),
        );
        descriptor.flags = classify(&facts);
        descriptor.confidence = confidence(&facts);
        self.record(job.work_id, descriptor);
        self.refresh_dependencies();
    }

    /// Le descripteur d'un graphe termine : faits de C-05, C-07 et C-11, le reste absent.
    /// La racine est toujours le noeud 0 du graphe.
    fn describe(
        &self,
        job: &Job,
        graph: &CallGraph,
        summary: GraphSummary,
        exhausted: bool,
        (reads, writes, side_effects, locks): Effects,
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
        let mut descriptor = Descriptor {
            summary,
            deps,
            reads,
            writes,
            side_effects,
            locks,
            dependency: DependencyCounts::default(),
            thread: unknown_thread(),
            determinism: unknown_determinism(),
            may_be_dispatched: true,
            flags: ClassificationFlags::EMPTY,
            confidence: Confidence::new(0.0, 0.0, 0.0),
            bytecode_hash: method_hash(&self.index, &job.root),
            stale: false,
        };
        let facts = facts_of(
            &descriptor,
            self.observed.get(&job.work_id),
            self.methods.get(&job.work_id).map(|root| root.side),
        );
        descriptor.flags = classify(&facts);
        descriptor.confidence = confidence(&facts);
        descriptor
    }

    /// Recalcule les dependances de toutes les unites decrites (C-10, ADR-056), puis
    /// leurs drapeaux et leur confiance, qui en dependent.
    fn refresh_dependencies(&mut self) {
        let started = Instant::now();
        let mut ids: Vec<WorkId> = self.descriptors.keys().copied().collect();
        ids.sort_unstable();
        let workloads: Vec<Workload> = ids
            .iter()
            .map(|id| {
                let d = &self.descriptors[id];
                Workload {
                    id: *id,
                    reads: d.reads,
                    writes: d.writes,
                    locks: d.locks,
                    dispatched: d.may_be_dispatched,
                }
            })
            .collect();
        let found = dependencies(&workloads);
        let (observed, methods) = (&self.observed, &self.methods);
        let mut changes = 0_u64;
        for (id, deps) in ids.iter().zip(found.per_workload) {
            let Some(descriptor) = self.descriptors.get_mut(id) else {
                continue;
            };
            descriptor.deps.data_deps = deps.producers;
            descriptor.deps.order_deps = deps.ordered;
            descriptor.dependency = DependencyCounts {
                raw: deps.raw,
                waw: deps.waw,
                war: deps.war,
                resource: deps.resource,
                whole_tick: deps.whole_tick,
            };
            let facts = facts_of(
                descriptor,
                observed.get(id),
                methods.get(id).map(|root| root.side),
            );
            let flags = classify(&facts);
            changes += u64::from(flags != descriptor.flags);
            descriptor.flags = flags;
            descriptor.confidence = confidence(&facts);
        }
        self.descriptor_bytes = self.descriptors.values().map(descriptor_bytes).sum();
        let unknown = self
            .descriptors
            .values()
            .filter(|d| d.flags.contains(ClassificationFlags::UNKNOWN))
            .count();
        let elapsed = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        let s = &mut self.status;
        s.descriptors_unknown = unknown as u64;
        s.descriptor_changes = s.descriptor_changes.saturating_add(changes);
        s.dep_edges = found.edges;
        s.dep_conflicts = found.conflicts;
        s.dep_conservative_edges = found.conservative_edges;
        s.dep_ns = s.dep_ns.saturating_add(elapsed);
        let thread = |required: Tri| {
            self.descriptors
                .values()
                .filter(|d| d.thread.main_thread_required == required)
                .count() as u64
        };
        let (main_required, thread_unknown) = (thread(Tri::Yes), thread(Tri::Unknown));
        self.status.thread_main_required = main_required;
        self.status.thread_unknown = thread_unknown;
        let mut det_sources = [0_u64; 9];
        for descriptor in self.descriptors.values() {
            let bits = descriptor.determinism.sources_of_nondeterminism;
            for (count, source) in det_sources.iter_mut().zip(NonDetSource::ALL) {
                *count += u64::from(bits & (1 << source as u16) != 0);
            }
        }
        self.status.det_sources = det_sources;
        self.status.det_deterministic = self
            .descriptors
            .values()
            .filter(|d| d.determinism.deterministic == Tri::Yes)
            .count() as u64;
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
        self.unsaved.insert(work_id);
        self.recount();
    }

    /// Recalcule drapeaux et confiance de ces unites, avec leurs observations du moment.
    fn reclassify(&mut self, ids: &[WorkId]) {
        let (observed, methods) = (&self.observed, &self.methods);
        let mut changes = 0_u64;
        for id in ids {
            let Some(descriptor) = self.descriptors.get_mut(id) else {
                continue;
            };
            let facts = facts_of(
                descriptor,
                observed.get(id),
                methods.get(id).map(|root| root.side),
            );
            let flags = classify(&facts);
            changes += u64::from(flags != descriptor.flags);
            descriptor.flags = flags;
            descriptor.confidence = confidence(&facts);
        }
        self.status.descriptor_changes = self.status.descriptor_changes.saturating_add(changes);
        self.recount();
    }

    /// Comptes des descripteurs publies dans le statut.
    fn recount(&mut self) {
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

    /// Reprend les descripteurs relus de `workload.db` (ADR-064). Une analyse d'un
    /// autre tampon est ecartee ; une unite deja decrite garde son descripteur, plus
    /// recent. Dependances, drapeaux et confiance sont recalcules sur l'ensemble.
    /// Rend le nombre de descripteurs repris.
    pub fn restore(&mut self, analyses: Vec<(WorkId, PersistedAnalysis)>) -> usize {
        if !self.enabled {
            return 0;
        }
        let mut restored = 0;
        for (work_id, analysis) in analyses {
            if analysis.stamp != ANALYSIS_STAMP {
                self.status.restore_rejected = self.status.restore_rejected.saturating_add(1);
                continue;
            }
            if self.descriptors.contains_key(&work_id) {
                continue;
            }
            let descriptor = Descriptor::from_persisted(analysis);
            self.descriptor_bytes = self
                .descriptor_bytes
                .saturating_add(descriptor_bytes(&descriptor));
            self.descriptors.insert(work_id, descriptor);
            restored += 1;
        }
        self.status.descriptors_restored = self
            .status
            .descriptors_restored
            .saturating_add(restored as u64);
        if restored > 0 {
            self.refresh_dependencies();
            self.recount();
        }
        restored
    }

    /// Les descripteurs calcules depuis le dernier appel, a confier a la base
    /// (ADR-064). Un descripteur perime n'est pas rendu : il sera recalcule.
    pub fn take_unsaved(&mut self) -> Vec<(WorkId, PersistedAnalysis)> {
        let mut ids: Vec<WorkId> = self.unsaved.drain().collect();
        ids.sort_unstable();
        ids.into_iter()
            .filter_map(|id| {
                self.descriptors
                    .get(&id)
                    .filter(|d| !d.stale)
                    .map(|d| (id, d.persisted()))
            })
            .collect()
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

    /// Toutes les unites decrites, dans un ordre quelconque.
    pub fn described(&self) -> impl Iterator<Item = (WorkId, &Descriptor)> {
        self.descriptors.iter().map(|(id, d)| (*id, d))
    }

    /// `true` si la classe de la methode racine a ete declaree absente : un autre
    /// transformateur la vise (mixin, access transformer, coremod), Forge la reecrit, ou
    /// elle est illisible. C'est la propriete « transformateur concurrent » de C-33
    /// (R-552), au sens prudent : le bytecode final est inconnu.
    #[must_use]
    pub fn root_bytecode_unknown(&self, work_id: WorkId) -> bool {
        self.methods
            .get(&work_id)
            .is_some_and(|root| self.index.is_absent(&root.method.owner))
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
        self.safety = ThreadSafetyTable::new();
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
fn unknown_effects() -> Effects {
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
    (unknown, unknown, side_effects, true)
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
    let producers = descriptor.deps.data_deps.as_ref().map_or(0, Vec::len) as u64;
    let ordered = descriptor.deps.order_deps.as_ref().map_or(0, Vec::len) as u64;
    BYTES_PER_DESCRIPTOR
        + (producers + ordered) * 8
        + descriptor
            .deps
            .calls
            .iter()
            .map(|call| call.len() as u64 + 24)
            .sum::<u64>()
}

impl Descriptor {
    /// Les faits propres a l'unite, a persister (ADR-064).
    #[must_use]
    pub fn persisted(&self) -> PersistedAnalysis {
        let s = &self.summary;
        PersistedAnalysis {
            stamp: ANALYSIS_STAMP,
            bytecode_hash: self.bytecode_hash,
            nodes: s.nodes,
            resolved_calls: s.resolved_calls,
            unresolved_calls: s.unresolved_calls,
            graph_truncated: s.truncated,
            reflection: s.reflection,
            recursive_root: s.recursive_root,
            rounds: s.rounds,
            calls: self.deps.calls.clone(),
            depth: self.deps.depth,
            truncated: self.deps.truncated,
            external_locks: self.deps.external_locks.clone(),
            reads: self.reads,
            writes: self.writes,
            side_effects: self.side_effects,
            locks: self.locks,
            thread: self.thread,
            determinism: self.determinism,
            may_be_dispatched: self.may_be_dispatched,
        }
    }

    /// Descripteur relu : les faits persistes, sans dependances ni drapeaux, que
    /// [`Analysis::restore`] recalcule.
    #[must_use]
    pub fn from_persisted(p: PersistedAnalysis) -> Self {
        Self {
            summary: GraphSummary {
                nodes: p.nodes,
                resolved_calls: p.resolved_calls,
                unresolved_calls: p.unresolved_calls,
                truncated: p.graph_truncated,
                reflection: p.reflection,
                recursive_root: p.recursive_root,
                build_ns: 0,
                rounds: p.rounds,
            },
            deps: DependencySet {
                calls: p.calls,
                unresolved_calls: p.unresolved_calls,
                data_deps: None,
                order_deps: None,
                external_locks: p.external_locks,
                depth: p.depth,
                truncated: p.truncated,
            },
            reads: p.reads,
            writes: p.writes,
            side_effects: p.side_effects,
            locks: p.locks,
            dependency: DependencyCounts::default(),
            thread: p.thread,
            determinism: p.determinism,
            may_be_dispatched: p.may_be_dispatched,
            flags: ClassificationFlags::EMPTY,
            confidence: Confidence::new(0.0, 0.0, 0.0),
            bytecode_hash: p.bytecode_hash,
            stale: false,
        }
    }

    /// Bloc `ANALYSIS` de la fiche de l'unite (`/rfx why`, ADR-063).
    #[must_use]
    pub fn card(&self) -> rfx_model::AnalysisCard {
        let domains = |set: &AccessSet| -> Vec<String> {
            Domain::ALL
                .iter()
                .filter(|d| set.domains.contains(**d))
                .map(|d| format!("{d:?}"))
                .collect()
        };
        let e = &self.side_effects;
        let effects = [
            (e.mutates_world, "mutates_world"),
            (e.spawns_or_removes_entity, "spawns_or_removes_entity"),
            (e.sends_packets, "sends_packets"),
            (e.schedules_tasks, "schedules_tasks"),
            (e.fires_events, "fires_events"),
            (e.io, "io"),
            (e.native_call, "native_call"),
            (e.throws_observable, "throws_observable"),
            (e.cancels_event, "cancels_event"),
        ];
        let sources = self.determinism.sources_of_nondeterminism;
        rfx_model::AnalysisCard {
            graph_nodes: self.summary.nodes,
            resolved_calls: self.summary.resolved_calls,
            unresolved_calls: self.summary.unresolved_calls,
            truncated: self.deps.truncated,
            reflection: self.summary.reflection,
            reads: domains(&self.reads),
            reads_precision: format!("{:?}", self.reads.precision),
            writes: domains(&self.writes),
            writes_precision: format!("{:?}", self.writes.precision),
            side_effects: effects
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, name)| (*name).to_owned())
                .collect(),
            side_effects_unknown: e.unknown,
            data_deps: self
                .deps
                .data_deps
                .as_ref()
                .map(|deps| u32::try_from(deps.len()).unwrap_or(u32::MAX)),
            order_deps: self
                .deps
                .order_deps
                .as_ref()
                .map(|deps| u32::try_from(deps.len()).unwrap_or(u32::MAX)),
            external_locks_known: self.deps.external_locks.is_some(),
            may_be_dispatched: self.may_be_dispatched,
            main_thread_required: format!("{:?}", self.thread.main_thread_required),
            touches_non_threadsafe_api: self.thread.touches_non_threadsafe_api,
            deterministic: format!("{:?}", self.determinism.deterministic),
            nondet_sources: NonDetSource::ALL
                .iter()
                .filter(|s| sources & (1 << **s as u16) != 0)
                .map(|s| format!("{s:?}"))
                .collect(),
            order_sensitive: format!("{:?}", self.determinism.order_sensitive),
            float_sensitive: self.determinism.float_sensitive,
            flags: self.flags.names().into_iter().map(str::to_owned).collect(),
            confidence_x1000: (self.confidence.combined * 1000.0).clamp(0.0, 1000.0) as u32,
            stale: self.stale,
        }
    }
}

/// Faits d'un descripteur pour la classification (C-08), avec ce que le profileur
/// observe de l'unite (C-05).
fn facts_of(descriptor: &Descriptor, observed: Option<&(Heat, f32)>, side: Option<Side>) -> Facts {
    let (heat, observation_quality) = observed.copied().unwrap_or((Heat::Cold, 0.0));
    Facts {
        heat,
        side: side.unwrap_or(Side::Common),
        observation_quality,
        deps: Some(descriptor.deps.clone()),
        reflection: descriptor.summary.reflection,
        reads: Some(descriptor.reads),
        writes: Some(descriptor.writes),
        side_effects: Some(descriptor.side_effects),
        thread: Some(descriptor.thread),
        determinism: Some(descriptor.determinism),
    }
}

/// Determinisme d'une unite dont l'analyse n'a pas abouti : rien n'est affirme.
fn unknown_determinism() -> DeterminismFacts {
    DeterminismFacts {
        deterministic: Tri::Unknown,
        sources_of_nondeterminism: 1 << NonDetSource::Unknown as u16,
        order_sensitive: Tri::Unknown,
        float_sensitive: true,
        verified_runs: 0,
        mismatches: 0,
    }
}

/// Contraintes de fil d'une unite dont l'analyse n'a pas abouti : tout est possible.
fn unknown_thread() -> ThreadFacts {
    ThreadFacts {
        main_thread_required: Tri::Unknown,
        touches_non_threadsafe_api: true,
        uses_synchronized: true,
        uses_locks: true,
        uses_thread_locals: true,
        reentrant: Tri::Unknown,
        observed_roles: None,
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
        // C-10 (ADR-056) : seule unite decrite, aux acces connus, sans producteur.
        assert_eq!(descriptor.deps.data_deps, Some(vec![]));
        assert!(
            descriptor.deps.order_deps.is_none(),
            "C-06 absent : inconnu"
        );
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

    /// ADR-060 : une unite statique a parametres primitifs, au graphe complet, sans
    /// effet ni source de non-determinisme, sort d'UNKNOWN : premier drapeau PURE.
    #[test]
    fn a_complete_static_unit_is_the_first_known_descriptor() {
        let classes = fixtures();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        let pure = WorkId(15);
        analysis.note(
            pure,
            MethodRef::new("fixtures/Chance", "pure", "(II)I"),
            Side::Server,
        );
        analysis.declare(hierarchy(&classes));
        let hot_one = [hot(pure)];
        assert_eq!(
            drive(&mut analysis, &hot_one, &classes),
            StepState::Finished
        );
        let pure = analysis.descriptor(pure).expect("descripteur");
        assert!(!pure.may_be_dispatched);
        assert_eq!(pure.deps.order_deps, Some(vec![]));
        assert!(
            !pure.flags.contains(ClassificationFlags::UNKNOWN),
            "{:?}",
            pure.flags
        );
        assert!(
            pure.flags.contains(ClassificationFlags::PURE),
            "{:?}",
            pure.flags
        );
        assert!(pure.flags.contains(ClassificationFlags::DETERMINISTIC));
        // ADR-063 : la fiche reprend ces faits, nommes.
        let card = pure.card();
        assert!(card.flags.contains(&"PURE".to_owned()), "{:?}", card.flags);
        assert!(!card.flags.contains(&"UNKNOWN".to_owned()));
        assert_eq!(card.deterministic, "Yes");
        assert_eq!(card.writes, Vec::<String>::new());
        assert_eq!(card.writes_precision, "Exact");
        assert_eq!((card.data_deps, card.order_deps), (Some(0), Some(0)));
        assert!(card.external_locks_known && !card.may_be_dispatched);
        assert!(card.nondet_sources.is_empty());
        assert!(card.confidence_x1000 > 0);
    }

    /// ADR-064 : un descripteur rendu a la base, puis repris par une nouvelle analyse,
    /// redonne les memes faits, et les memes dependances, drapeaux et confiance une fois
    /// recalcules ; il n'est ni reanalyse ni rendu de nouveau.
    #[test]
    fn a_persisted_descriptor_is_restored_identical_and_not_reanalysed() {
        let classes = fixtures();
        let config = RuntimeConfig::default();
        let pure = WorkId(15);
        let root = MethodRef::new("fixtures/Chance", "pure", "(II)I");
        let mut first = Analysis::new(&config);
        first.note(pure, root.clone(), Side::Server);
        first.declare(hierarchy(&classes));
        assert_eq!(
            drive(&mut first, &[hot(pure)], &classes),
            StepState::Finished
        );
        let saved = first.take_unsaved();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].1.stamp, ANALYSIS_STAMP);
        assert!(first.take_unsaved().is_empty(), "rendu une seule fois");

        let mut second = Analysis::new(&config);
        second.note(pure, root, Side::Server);
        assert_eq!(second.restore(saved), 1);
        let (before, after) = (
            first.descriptor(pure).unwrap(),
            second.descriptor(pure).unwrap(),
        );
        assert_eq!(after.persisted(), before.persisted());
        assert_eq!(after.deps, before.deps);
        // Drapeaux et confiance se recalculent avec les observations de la partie en
        // cours : aucune encore, donc pas `HOT`. Ils valent ceux des memes faits sans
        // observation.
        let unobserved = facts_of(before, None, Some(Side::Server));
        assert_eq!(after.flags, classify(&unobserved));
        assert!(!after.flags.contains(ClassificationFlags::HOT));
        assert!(before.flags.contains(ClassificationFlags::HOT));
        assert!((after.confidence.combined - confidence(&unobserved).combined).abs() < 1e-6);
        assert_eq!(second.status().descriptors, 1);
        assert_eq!(second.status().descriptors_restored, 1);
        assert!(second.take_unsaved().is_empty(), "repris, pas recalcule");

        second.declare(hierarchy(&classes));
        assert_eq!(
            drive(&mut second, &[hot(pure)], &classes),
            StepState::Idle,
            "deja decrit : rien a analyser"
        );
        // Observee chaude a cette partie, l'unite reprend ses drapeaux d'observation,
        // sans qu'aucun graphe ne se termine.
        let after = second.descriptor(pure).unwrap();
        assert_eq!(after.flags, before.flags);
        assert!(after.flags.contains(ClassificationFlags::HOT));
        assert!((after.confidence.combined - before.confidence.combined).abs() < 1e-6);
    }

    /// ADR-064 : une analyse produite par un autre code d'analyse est ecartee, et une
    /// unite deja decrite garde son descripteur.
    #[test]
    fn a_foreign_stamp_is_rejected_and_a_fresh_descriptor_kept() {
        let classes = fixtures();
        let config = RuntimeConfig::default();
        let pure = WorkId(15);
        let mut analysis = Analysis::new(&config);
        analysis.note(
            pure,
            MethodRef::new("fixtures/Chance", "pure", "(II)I"),
            Side::Server,
        );
        analysis.declare(hierarchy(&classes));
        drive(&mut analysis, &[hot(pure)], &classes);
        let fresh = analysis.descriptor(pure).unwrap().persisted();

        let mut foreign = fresh.clone();
        foreign.stamp = ANALYSIS_STAMP.wrapping_add(1);
        let mut older = fresh.clone();
        older.nodes += 99;
        assert_eq!(
            analysis.restore(vec![(WorkId(99), foreign), (pure, older)]),
            0
        );
        assert_eq!(analysis.status().restore_rejected, 1);
        assert!(analysis.descriptor(WorkId(99)).is_none());
        assert_eq!(analysis.descriptor(pure).unwrap().persisted(), fresh);
    }

    /// ADR-059 : une classe qu'un greffon de lancement de Forge reecrit (ici, une
    /// descendante d'`Event` selon la hierarchie declaree) est fournie comme absente ;
    /// une classe ordinaire est fournie telle quelle.
    #[test]
    fn a_class_rewritten_by_a_forge_plugin_is_provided_as_absent() {
        let classes = fixtures();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        let mut entries = hierarchy(&classes);
        for HierarchyEntry(name, parent, _) in &mut entries {
            if name == "fixtures/Animal" {
                *parent = platform::EVENT.to_owned();
            }
        }
        analysis.declare(entries);
        analysis.provide("fixtures/Dog", Some(&classes["fixtures/Dog"]));
        analysis.provide("fixtures/Calls", Some(&classes["fixtures/Calls"]));
        let status = analysis.status();
        assert_eq!((status.classes_provided, status.classes_absent), (1, 1));
    }

    /// C-13 nourrit C-12 : une unite pure, deterministe et en lecture seule d'un objet
    /// sur, n'exige pas le fil autoritatif (ADR-057, ADR-058).
    #[test]
    fn a_pure_unit_is_free_of_the_main_thread() {
        let classes = fixtures();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        let pure = WorkId(13);
        let synced = WorkId(14);
        analysis.note(
            pure,
            MethodRef::new("fixtures/Threads", "readFrozen", "(Lfixtures/Frozen;)I"),
            Side::Server,
        );
        analysis.note(
            synced,
            MethodRef::new("fixtures/Threads", "syncMethod", "()V"),
            Side::Server,
        );
        analysis.declare(hierarchy(&classes));
        let hot_both = [hot(pure), hot(synced)];
        assert_eq!(
            drive(&mut analysis, &hot_both, &classes),
            StepState::Finished
        );
        assert_eq!(
            drive(&mut analysis, &hot_both, &classes),
            StepState::Finished
        );
        let pure = analysis.descriptor(pure).expect("descripteur");
        assert_eq!(pure.determinism.deterministic, Tri::Yes);
        assert!(pure.flags.contains(ClassificationFlags::DETERMINISTIC));
        assert_eq!(pure.thread.main_thread_required, Tri::No);
        assert_eq!(pure.deps.external_locks, Some(vec![]));

        // `synchronized`, et `Object.hashCode` ecrit tel quel par javac (hachage
        // d'identite) vers le JDK absent de l'index : verrous inconnus, source definie.
        let synced = analysis.descriptor(synced).expect("descripteur");
        assert_eq!(synced.deps.external_locks, None);
        let bit = |source: NonDetSource| 1_u16 << source as u16;
        assert_eq!(
            synced.determinism.sources_of_nondeterminism,
            bit(NonDetSource::IdentityHash) | bit(NonDetSource::Unknown)
        );
        assert_eq!(synced.determinism.deterministic, Tri::No);
        assert!(!synced.flags.contains(ClassificationFlags::DETERMINISTIC));
        let status = analysis.status();
        assert_eq!(status.det_deterministic, 1);
        let mut expected = [0_u64; 9];
        expected[NonDetSource::IdentityHash as usize] = 1;
        expected[NonDetSource::Unknown as usize] = 1;
        assert_eq!(status.det_sources, expected);
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
        // ADR-060 : racine statique sans parametre, graphe complet, acces connus :
        // l'ordre est connu, et le descripteur sort d'UNKNOWN.
        assert!(!local.may_be_dispatched);
        assert_eq!(local.deps.order_deps, Some(vec![]));
        assert!(
            !local.flags.contains(ClassificationFlags::UNKNOWN),
            "{:?}",
            local.flags
        );

        let shared = analysis.descriptor(shared).expect("descripteur");
        assert!(shared.writes.domains.contains(Domain::Unknown));
        assert!(shared.flags.contains(ClassificationFlags::WRITE));
        assert_eq!(analysis.status().dataflow_exact, 1);

        // C-10 : l'unite locale ne lit rien de partage, elle n'a pas de producteur ;
        // l'autre ecrit l'inconnu, elle depend de tout le tick (ADR-056).
        assert_eq!(local.deps.data_deps, Some(vec![]));
        assert_eq!(local.dependency.whole_tick, None);
        assert_eq!(shared.deps.data_deps, None);
        assert_eq!(shared.dependency.whole_tick, Some(WholeTick::UnknownAccess));
        assert!(shared.flags.contains(ClassificationFlags::UNKNOWN));

        // C-12 (ADR-057) : l'unite qui ecrit un objet partage d'une classe non sure exige
        // le fil autoritatif, et ecrire sans observation des fils la rend UNSAFE (R-390).
        // L'unite locale aussi exige le fil : son constructeur ecrit `this`, objet d'une
        // classe non sure, et C-12 ne suit pas la localite d'un appel a l'autre (prudent).
        // N'ecrivant rien de partage, elle n'est pas UNSAFE.
        assert_eq!(shared.thread.main_thread_required, Tri::Yes);
        assert!(shared.flags.contains(ClassificationFlags::UNSAFE));
        assert_eq!(local.thread.main_thread_required, Tri::Yes);
        assert!(!local.flags.contains(ClassificationFlags::UNSAFE));
        assert_eq!(analysis.status().thread_main_required, 2);

        // C-13 (ADR-058) : aucune source dans l'unite locale, graphe complet sans verrou ;
        // l'unite qui ecrit est sensible a l'ordre. Aucun shadow run : rien n'est verifie.
        assert_eq!(local.determinism.deterministic, Tri::Yes);
        assert_eq!(local.determinism.sources_of_nondeterminism, 0);
        assert_eq!(local.determinism.order_sensitive, Tri::No);
        assert_eq!(local.deps.external_locks, Some(vec![]));
        assert_ne!(shared.determinism.order_sensitive, Tri::No);
        assert_eq!(shared.determinism.verified_runs, 0);
        let status = analysis.status();
        assert!(status.det_deterministic >= 1, "l'unite locale");
        assert_eq!(
            status.det_sources.iter().sum::<u64>(),
            u64::from(local.determinism.sources_of_nondeterminism.count_ones())
                + u64::from(shared.determinism.sources_of_nondeterminism.count_ones())
        );
        let status = analysis.status();
        assert_eq!((status.dep_edges, status.dep_conservative_edges), (1, 1));
        assert!(status.dep_ns > 0);
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

    /// ADR-064 : un descripteur perime avant d'etre confie a la base n'est pas ecrit ;
    /// son recalcul le sera.
    #[test]
    fn a_stale_descriptor_is_not_handed_to_the_database() {
        let classes = fixtures();
        let mut analysis = Analysis::new(&RuntimeConfig::default());
        let id = WorkId(11);
        analysis.note(
            id,
            MethodRef::new("fixtures/Calls", "helper", "(I)I"),
            Side::Common,
        );
        analysis.declare(hierarchy(&classes));
        drive(&mut analysis, &[hot(id)], &classes);
        let changed = calls_with_changed_helper(&classes["fixtures/Calls"]);
        analysis.provide("fixtures/Calls", Some(&changed));
        assert!(analysis.descriptor(id).unwrap().stale);
        assert!(analysis.take_unsaved().is_empty(), "perime : rien a ecrire");

        assert_eq!(analysis.step(&[hot(id)]), StepState::Finished);
        let saved = analysis.take_unsaved();
        assert_eq!(saved.len(), 1, "le recalcul est ecrit");
        assert_eq!(
            saved[0].1.bytecode_hash,
            analysis.descriptor(id).unwrap().bytecode_hash
        );
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
