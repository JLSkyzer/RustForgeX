//! C-17 : executer un graphe de taches (C-16) sur le pool.
//!
//! Cahier des charges : PARTIE 5.17 (« Exécuter le task graph en respectant dépendances,
//! priorités et deadlines »), PARTIE 5.16 (regle de drain). Exigences : R-740 (le resultat
//! ne depend pas du nombre de workers), INV-08 (le fil autoritatif n'attend jamais au-dela
//! de l'echeance de drain). Maturite : `STABLE`.
//!
//! # Par compteurs de dependances, et non par vagues
//!
//! Executer vague par vague ferait attendre toute une vague pour sa tache la plus lente.
//! Ici, chaque tache tient le compte de ses dependances non terminees ; la derniere qui
//! se termine la met en file. Le crochet de fin de tache libere les dependants **avant**
//! que la tache ne soit comptee terminee dans son groupe : le groupe ne peut donc pas
//! paraitre vide tant qu'une tache reste a lancer.
//!
//! # Une dependance qui echoue
//!
//! Un dependant n'est jamais execute si l'une de ses dependances n'a pas reussi, quelle
//! qu'en soit la raison (annulation, echeance, resultat invalide, panic) : il n'aurait
//! pas d'entree correcte. Il se termine `Rejected(Cancelled)`, et ses propres dependants
//! avec lui (UNKNOWN = CONSERVATIVE).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rfx_taskgraph::{Deadline, DrainReport, TaskGraph, TaskId, TaskNode};

use crate::pool::{Job, OnDone, Scheduler, Shared};
use crate::task::{RejectReason, TaskContext, TaskGroup, TaskOutcome};

/// Interprete la charge utile d'une tache du graphe.
pub type GraphExecutor = Arc<dyn Fn(&TaskNode, &mut TaskContext<'_>) -> TaskOutcome + Send + Sync>;

fn encode(outcome: TaskOutcome) -> u8 {
    match outcome {
        TaskOutcome::Success => 1,
        TaskOutcome::Rejected(RejectReason::Cancelled) => 2,
        TaskOutcome::Rejected(RejectReason::BudgetExceeded) => 3,
        TaskOutcome::Timeout => 4,
        TaskOutcome::Invalid => 5,
        TaskOutcome::Panicked => 6,
    }
}

fn decode(code: u8) -> Option<TaskOutcome> {
    Some(match code {
        1 => TaskOutcome::Success,
        2 => TaskOutcome::Rejected(RejectReason::Cancelled),
        3 => TaskOutcome::Rejected(RejectReason::BudgetExceeded),
        4 => TaskOutcome::Timeout,
        5 => TaskOutcome::Invalid,
        6 => TaskOutcome::Panicked,
        _ => return None,
    })
}

struct GraphState {
    /// Taches vivantes, en rang dense.
    nodes: Vec<TaskNode>,
    /// Rang dense de chaque identifiant, `None` pour une tache absorbee par une fusion.
    dense: Vec<Option<usize>>,
    dependents: Vec<Vec<usize>>,
    waiting: Vec<AtomicUsize>,
    blocked: Vec<AtomicBool>,
    outcomes: Vec<AtomicU8>,
    executor: GraphExecutor,
    group: TaskGroup,
    drain_at: Instant,
}

impl GraphState {
    fn new(graph: &TaskGraph, executor: GraphExecutor, drain_at: Instant) -> Self {
        let nodes: Vec<TaskNode> = graph.tasks().cloned().collect();
        let size = nodes
            .iter()
            .map(|node| node.id.index() + 1)
            .max()
            .unwrap_or(0);
        let mut dense = vec![None; size];
        for (rank, node) in nodes.iter().enumerate() {
            dense[node.id.index()] = Some(rank);
        }
        let rank_of = |id: TaskId| dense[id.index()].expect("dependance vivante");
        let dependents = nodes
            .iter()
            .map(|node| {
                graph
                    .dependents(node.id)
                    .iter()
                    .map(|&id| rank_of(id))
                    .collect()
            })
            .collect();
        let waiting = nodes
            .iter()
            .map(|node| AtomicUsize::new(node.deps.len()))
            .collect();
        Self {
            blocked: nodes.iter().map(|_| AtomicBool::new(false)).collect(),
            outcomes: nodes.iter().map(|_| AtomicU8::new(0)).collect(),
            nodes,
            dense,
            dependents,
            waiting,
            executor,
            group: TaskGroup::new(),
            drain_at,
        }
    }

    fn roots(&self) -> Vec<usize> {
        (0..self.nodes.len())
            .filter(|&rank| self.nodes[rank].deps.is_empty())
            .collect()
    }

    fn outcome(&self, id: TaskId) -> Option<TaskOutcome> {
        let rank = (*self.dense.get(id.index())?)?;
        decode(self.outcomes[rank].load(Ordering::Acquire))
    }

    /// Enregistre l'issue et renvoie les dependants qui viennent d'etre liberes.
    fn complete(&self, rank: usize, outcome: TaskOutcome) -> Vec<usize> {
        self.outcomes[rank].store(encode(outcome), Ordering::Release);
        let mut ready = Vec::new();
        for &dependent in &self.dependents[rank] {
            if outcome != TaskOutcome::Success {
                self.blocked[dependent].store(true, Ordering::Release);
            }
            if self.waiting[dependent].fetch_sub(1, Ordering::AcqRel) == 1 {
                ready.push(dependent);
            }
        }
        ready
    }

    fn job(state: &Arc<Self>, rank: usize, on_done: Option<OnDone>) -> Job {
        let node = &state.nodes[rank];
        let running = Arc::clone(state);
        Job {
            task: Box::new(move |cx: &mut TaskContext<'_>| {
                if running.blocked[rank].load(Ordering::Acquire) {
                    return TaskOutcome::Rejected(RejectReason::Cancelled);
                }
                (running.executor)(&running.nodes[rank], cx)
            }),
            group: Arc::clone(&state.group.0),
            cancel: node.cancel.clone(),
            deadline: (node.deadline == Deadline::ThisTick).then_some(state.drain_at),
            priority: node.priority,
            on_done,
        }
    }

    /// Met en file une tache dont la fin liberera ses dependants sur le pool.
    fn dispatch(state: &Arc<Self>, shared: &Arc<Shared>, rank: usize) {
        let (state_for_hook, shared_for_hook) = (Arc::clone(state), Arc::clone(shared));
        let hook: OnDone = Box::new(move |outcome| {
            for dependent in state_for_hook.complete(rank, outcome) {
                Self::dispatch(&state_for_hook, &shared_for_hook, dependent);
            }
        });
        shared.dispatch(Self::job(state, rank, Some(hook)));
    }

    /// `FORCE_SERIAL` : tout sur le fil appelant, sans jamais depasser l'echeance.
    fn run_serial(state: &Arc<Self>, shared: &Shared) {
        let mut ready: VecDeque<usize> = state.roots().into();
        while let Some(rank) = ready.pop_front() {
            if Instant::now() >= state.drain_at {
                return;
            }
            state.group.0.enter();
            let outcome = shared.run(Self::job(state, rank, None));
            ready.extend(state.complete(rank, outcome));
        }
    }
}

/// Resultat de l'execution d'un graphe.
pub struct GraphRun {
    state: Arc<GraphState>,
    completed: bool,
    drain: DrainReport,
}

impl std::fmt::Debug for GraphRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphRun")
            .field("completed", &self.completed)
            .field("drain", &self.drain)
            .finish_non_exhaustive()
    }
}

impl GraphRun {
    /// `true` si toutes les taches etaient terminees a l'echeance.
    #[must_use]
    pub fn completed(&self) -> bool {
        self.completed
    }

    /// Issue d'une tache, telle qu'elle est connue maintenant ; `None` si elle n'est pas
    /// terminee. Le resultat d'une tache annulee par le drain est a ignorer, meme s'il
    /// arrive plus tard.
    #[must_use]
    pub fn outcome(&self, id: TaskId) -> Option<TaskOutcome> {
        self.state.outcome(id)
    }

    /// Ce que le drain a annule, vide si le graphe a fini a temps.
    #[must_use]
    pub fn drain(&self) -> &DrainReport {
        &self.drain
    }

    /// Attend, au plus `timeout`, la fin des taches encore en cours apres l'echeance.
    pub fn wait_remaining(&self, timeout: Duration) -> bool {
        self.state.group.wait(timeout)
    }
}

impl Scheduler {
    /// Execute `graph` et attend sa fin, au plus `budget`.
    ///
    /// Chaque tache est confiee a `executor` des que ses dependances ont reussi. A
    /// l'echeance, la regle de drain de la PARTIE 5.16 s'applique : les taches
    /// `THIS_TICK` non terminees et leurs dependants sont annules, et leur travail
    /// retombe sur Java. La methode rend alors la main sans attendre les taches en cours
    /// (INV-08).
    pub fn run_graph(
        &self,
        graph: &TaskGraph,
        executor: GraphExecutor,
        budget: Duration,
    ) -> GraphRun {
        let drain_at = Instant::now() + budget;
        let state = Arc::new(GraphState::new(graph, executor, drain_at));
        if self.workers() == 0 {
            GraphState::run_serial(&state, self.shared());
        } else {
            for rank in state.roots() {
                GraphState::dispatch(&state, self.shared(), rank);
            }
            state
                .group
                .wait(drain_at.saturating_duration_since(Instant::now()));
        }
        let completed = state
            .outcomes
            .iter()
            .all(|code| code.load(Ordering::Acquire) != 0);
        let drain = if completed {
            DrainReport::default()
        } else {
            graph.cancel_at_drain(|id| state.outcome(id).is_some())
        };
        GraphRun {
            state,
            completed,
            drain,
        }
    }
}
