//! C-16 : le graphe de taches et son invariant d'acyclicite.
//!
//! Cahier des charges : PARTIE 5.16. Invariants : INV-01 (le graphe est acyclique, toute
//! arete creant un cycle est refusee, `E-2301`), et l'annulation a la fenetre de drain
//! des taches `THIS_TICK` non terminees, dont le travail retombe sur Java pour ce tick.
//! Tests : T-250, T-251, T-253. Maturite : `STABLE`.
//!
//! # Refuser un cycle sans parcourir le graphe a chaque arete
//!
//! L'acceptance demande de construire 500 noeuds en moins de 100 us. Chercher un cycle
//! par parcours a chaque arete coute O(V + E) par arete. Or la construction naturelle
//! ajoute les taches dans l'ordre et les fait dependre de taches deja creees : chaque
//! arete va d'un identifiant vers un identifiant plus petit. Tant que c'est vrai de
//! toutes les aretes, l'ordre des identifiants est un ordre topologique, et aucune
//! arete du meme sens ne peut fermer un cycle : le test est gratuit. Le parcours n'a
//! lieu que pour une arete dans l'autre sens, ou une fois que l'ordre a ete rompu.

use rfx_model::WorkId;

use crate::node::{Deadline, TaskId, TaskNode, TaskSpec};

/// Refus d'une operation sur le graphe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphError {
    /// `E-2301` : l'arete fermerait un cycle. Elle est refusee, le graphe est intact.
    Cycle {
        /// Tache qui devait dependre de l'autre.
        task: TaskId,
        /// Tache dont elle devait dependre.
        depends_on: TaskId,
    },
    /// Identifiant inconnu, ou tache absorbee par une fusion : erreur de l'appelant.
    UnknownTask(TaskId),
}

impl GraphError {
    /// Numero normatif de l'annexe A.2, s'il en existe un.
    #[must_use]
    pub fn code(self) -> Option<u16> {
        match self {
            GraphError::Cycle { .. } => Some(2301),
            GraphError::UnknownTask(_) => None,
        }
    }
}

/// Compteurs du graphe (`rfx.taskgraph.*`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GraphStats {
    /// Aretes refusees parce qu'elles fermaient un cycle.
    pub cycles_rejected: u64,
    /// Fusions effectuees.
    pub fusions: u64,
    /// Taches decoupees.
    pub splits: u64,
}

/// Ce que la fenetre de drain a annule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrainReport {
    /// Taches annulees, par identifiant croissant.
    pub cancelled: Vec<TaskId>,
    /// Unites de travail qui executent leur chemin Java pour ce tick, sans doublon,
    /// dans l'ordre de leur premiere tache annulee.
    pub fallback: Vec<WorkId>,
}

/// Graphe de taches d'un tick.
#[derive(Debug)]
pub struct TaskGraph {
    /// Taches par identifiant ; `None` pour une tache absorbee par une fusion.
    pub(crate) nodes: Vec<Option<TaskNode>>,
    /// Pour chaque tache, celles qui en dependent.
    pub(crate) dependents: Vec<Vec<TaskId>>,
    /// `true` tant que toute arete va d'un identifiant vers un plus petit.
    pub(crate) descending_only: bool,
    pub(crate) stats: GraphStats,
}

impl Default for TaskGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskGraph {
    /// Graphe vide.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    /// Graphe vide, avec de la place pour `tasks` taches.
    #[must_use]
    pub fn with_capacity(tasks: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(tasks),
            dependents: Vec::with_capacity(tasks),
            descending_only: true,
            stats: GraphStats::default(),
        }
    }

    /// Ajoute une tache sans dependance.
    ///
    /// # Panics
    ///
    /// Au-dela de `u32::MAX` taches, ce qu'aucun tick ne produit.
    pub fn add_task(&mut self, spec: TaskSpec) -> TaskId {
        let id = TaskId(u32::try_from(self.nodes.len()).expect("plus de 2^32 taches"));
        self.nodes.push(Some(TaskNode::from_spec(id, spec)));
        self.dependents.push(Vec::new());
        id
    }

    /// Fait dependre `task` de `depends_on`.
    ///
    /// Idempotent. Une arete qui fermerait un cycle est refusee et comptee ; le graphe
    /// reste celui d'avant l'appel (INV-01).
    ///
    /// # Errors
    ///
    /// [`GraphError::Cycle`] si l'arete fermerait un cycle, [`GraphError::UnknownTask`]
    /// si l'une des deux taches n'existe pas.
    pub fn add_dependency(&mut self, task: TaskId, depends_on: TaskId) -> Result<(), GraphError> {
        self.live(task)?;
        self.live(depends_on)?;
        if task == depends_on {
            return Err(self.reject(task, depends_on));
        }
        if self.node(task).deps.contains(&depends_on) {
            return Ok(());
        }
        let ordered = self.descending_only && task > depends_on;
        if !ordered && self.depends_transitively(depends_on, task) {
            return Err(self.reject(task, depends_on));
        }
        if task < depends_on {
            self.descending_only = false;
        }
        self.node_mut(task).deps.push(depends_on);
        self.dependents[depends_on.index()].push(task);
        Ok(())
    }

    fn reject(&mut self, task: TaskId, depends_on: TaskId) -> GraphError {
        self.stats.cycles_rejected += 1;
        GraphError::Cycle { task, depends_on }
    }

    /// `true` si `from` depend, directement ou non, de `target`.
    pub(crate) fn depends_transitively(&self, from: TaskId, target: TaskId) -> bool {
        let mut seen = vec![false; self.nodes.len()];
        let mut stack = vec![from];
        while let Some(id) = stack.pop() {
            if id == target {
                return true;
            }
            if std::mem::replace(&mut seen[id.index()], true) {
                continue;
            }
            if let Some(node) = &self.nodes[id.index()] {
                stack.extend(node.deps.iter().copied());
            }
        }
        false
    }

    fn live(&self, id: TaskId) -> Result<(), GraphError> {
        match self.nodes.get(id.index()) {
            Some(Some(_)) => Ok(()),
            _ => Err(GraphError::UnknownTask(id)),
        }
    }

    /// Tache vivante ; a n'appeler qu'apres [`Self::live`].
    pub(crate) fn node(&self, id: TaskId) -> &TaskNode {
        self.nodes[id.index()].as_ref().expect("tache vivante")
    }

    pub(crate) fn node_mut(&mut self, id: TaskId) -> &mut TaskNode {
        self.nodes[id.index()].as_mut().expect("tache vivante")
    }

    /// La tache, si elle existe et n'a pas ete absorbee par une fusion.
    #[must_use]
    pub fn task(&self, id: TaskId) -> Option<&TaskNode> {
        self.nodes.get(id.index()).and_then(Option::as_ref)
    }

    /// Taches vivantes, par identifiant croissant.
    pub fn tasks(&self) -> impl Iterator<Item = &TaskNode> {
        self.nodes.iter().filter_map(Option::as_ref)
    }

    /// Nombre de taches vivantes (`rfx.taskgraph.nodes`).
    #[must_use]
    pub fn len(&self) -> usize {
        self.tasks().count()
    }

    /// `true` si le graphe n'a aucune tache vivante.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Taches qui dependent directement de `id`.
    #[must_use]
    pub fn dependents(&self, id: TaskId) -> &[TaskId] {
        self.dependents.get(id.index()).map_or(&[], Vec::as_slice)
    }

    /// Compteurs du graphe.
    #[must_use]
    pub fn stats(&self) -> GraphStats {
        self.stats
    }

    /// Vagues d'execution : chaque tache est dans la premiere vague qui suit toutes
    /// ses dependances (tri topologique par niveaux). Dans une vague, les taches vont
    /// de la plus urgente a la moins urgente, puis par identifiant.
    #[must_use]
    pub fn waves(&self) -> Vec<Vec<TaskId>> {
        let mut waiting: Vec<usize> = self
            .nodes
            .iter()
            .map(|slot| slot.as_ref().map_or(0, |node| node.deps.len()))
            .collect();
        let mut current: Vec<TaskId> = self
            .tasks()
            .filter(|node| node.deps.is_empty())
            .map(|node| node.id)
            .collect();
        let mut waves = Vec::new();
        while !current.is_empty() {
            current.sort_by(|a, b| {
                self.node(*b)
                    .priority
                    .cmp(&self.node(*a).priority)
                    .then(a.cmp(b))
            });
            let mut next = Vec::new();
            for &id in &current {
                for &dependent in &self.dependents[id.index()] {
                    let count = &mut waiting[dependent.index()];
                    *count -= 1;
                    if *count == 0 {
                        next.push(dependent);
                    }
                }
            }
            waves.push(std::mem::replace(&mut current, next));
        }
        waves
    }

    /// Nombre de vagues (`rfx.taskgraph.depth`).
    #[must_use]
    pub fn depth(&self) -> usize {
        self.waves().len()
    }

    /// Fenetre de drain : annule ce qui ne peut plus etre termine a temps.
    ///
    /// Sont annulees les taches `THIS_TICK` non terminees, puis, transitivement, les
    /// taches non terminees qui en dependent : sans leur entree, elles ne peuvent pas
    /// produire un resultat correct, quelle que soit leur echeance (UNKNOWN =
    /// CONSERVATIVE). Leur jeton est annule et leur unite de travail retombe sur son
    /// chemin Java pour ce tick.
    pub fn cancel_at_drain(&self, finished: impl Fn(TaskId) -> bool) -> DrainReport {
        let mut cancelled = vec![false; self.nodes.len()];
        let mut stack: Vec<TaskId> = self
            .tasks()
            .filter(|node| node.deadline == Deadline::ThisTick && !finished(node.id))
            .map(|node| node.id)
            .collect();
        while let Some(id) = stack.pop() {
            if std::mem::replace(&mut cancelled[id.index()], true) {
                continue;
            }
            stack.extend(
                self.dependents[id.index()]
                    .iter()
                    .copied()
                    .filter(|dependent| !finished(*dependent)),
            );
        }
        let mut report = DrainReport::default();
        for node in self.tasks().filter(|node| cancelled[node.id.index()]) {
            node.cancel.cancel();
            report.cancelled.push(node.id);
            if !report.fallback.contains(&node.work) {
                report.fallback.push(node.work);
            }
        }
        report
    }
}
