//! Execution d'un graphe sur le pool (C-17) : R-740, propagation des echecs, drain.
//!
//! Comme pour T-252, l'executant de test refuse toute tache dont une entree n'est pas
//! prete : un ordre de dependances viole se voit, il ne produit pas seulement une valeur
//! differente.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rfx_model::WorkId;
use rfx_scheduler::{
    GraphExecutor, JobOptions, RejectReason, Scheduler, SchedulerConfig, TaskContext, TaskGroup,
    TaskOutcome,
};
use rfx_taskgraph::{
    CancelToken, Deadline, Priority, TaskGraph, TaskId, TaskKind, TaskNode, TaskPayload, TaskSpec,
};

const GUARD: Duration = Duration::from_secs(120);

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn range(&mut self, low: u64, high: u64) -> u64 {
        low + self.next() % (high - low)
    }
}

struct Job {
    work: u64,
    batch: Option<u32>,
    inputs: Vec<usize>,
    estimated_ns: u64,
}

fn program(seed: u64) -> Vec<Job> {
    let mut random = XorShift(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
    (0..80usize)
        .map(|index| {
            let batch = (random.range(0, 10) < 3).then(|| random.range(20, 120) as u32);
            let mut inputs = Vec::new();
            if index > 0 && random.range(0, 10) < 7 {
                inputs.push(index - 1);
            }
            for _ in 0..random.range(0, 3) {
                if index > 0 {
                    let input = random.range(0, index as u64) as usize;
                    if !inputs.contains(&input) {
                        inputs.push(input);
                    }
                }
            }
            let estimated_ns = match batch {
                Some(_) => random.range(50_000, 200_000),
                None => random.range(1_000, 10_000),
            };
            Job {
                work: index as u64 / 5,
                batch,
                inputs,
                estimated_ns,
            }
        })
        .collect()
}

fn build(jobs: &[Job]) -> TaskGraph {
    let tokens: Vec<CancelToken> = (0..jobs.len()).map(|_| CancelToken::new()).collect();
    let mut graph = TaskGraph::new();
    let ids: Vec<TaskId> = jobs
        .iter()
        .enumerate()
        .map(|(handle, job)| {
            graph.add_task(TaskSpec {
                work: WorkId(job.work),
                kind: TaskKind::Analysis,
                priority: Priority::Normal,
                deadline: Deadline::None,
                estimated_ns: job.estimated_ns,
                payload: match job.batch {
                    None => TaskPayload::Unit(handle as u64),
                    Some(len) => TaskPayload::Batch {
                        handle: handle as u64,
                        start: 0,
                        len,
                    },
                },
                // Un jeton par travail : c'est ce qui autorise la fusion (ADR-043).
                cancel: tokens[job.work as usize].clone(),
            })
        })
        .collect();
    for (index, job) in jobs.iter().enumerate() {
        for &input in &job.inputs {
            graph.add_dependency(ids[index], ids[input]).unwrap();
        }
    }
    graph
}

/// Etat partage par les workers ; chaque case n'est ecrite qu'une fois.
struct World {
    jobs: Vec<Job>,
    units: Vec<AtomicU64>,
    unit_done: Vec<AtomicBool>,
    elements: Vec<Vec<AtomicU64>>,
    elements_done: Vec<AtomicU32>,
    violations: AtomicU64,
}

impl World {
    fn new(jobs: Vec<Job>) -> Self {
        let count = jobs.len();
        Self {
            units: (0..count).map(|_| AtomicU64::new(0)).collect(),
            unit_done: (0..count).map(|_| AtomicBool::new(false)).collect(),
            elements: jobs
                .iter()
                .map(|job| {
                    (0..job.batch.unwrap_or(0))
                        .map(|_| AtomicU64::new(0))
                        .collect()
                })
                .collect(),
            elements_done: (0..count).map(|_| AtomicU32::new(0)).collect(),
            violations: AtomicU64::new(0),
            jobs,
        }
    }

    fn ready(&self, handle: usize) -> bool {
        match self.jobs[handle].batch {
            None => self.unit_done[handle].load(Ordering::Acquire),
            Some(len) => self.elements_done[handle].load(Ordering::Acquire) == len,
        }
    }

    fn inputs(&self, handle: usize) -> Option<u64> {
        let mut sum = 0u64;
        for &input in &self.jobs[handle].inputs {
            if !self.ready(input) {
                self.violations.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            sum = sum.wrapping_add(match self.jobs[input].batch {
                None => self.units[input].load(Ordering::Acquire),
                Some(_) => self.elements[input].iter().fold(0, |acc: u64, value| {
                    acc.wrapping_add(value.load(Ordering::Acquire))
                }),
            });
        }
        Some(sum)
    }

    fn run(&self, payload: &TaskPayload) -> TaskOutcome {
        match payload {
            TaskPayload::Unit(handle) => {
                let handle = *handle as usize;
                let Some(inputs) = self.inputs(handle) else {
                    return TaskOutcome::Invalid;
                };
                self.units[handle].store(mix(handle as u64, inputs), Ordering::Release);
                self.unit_done[handle].store(true, Ordering::Release);
            }
            TaskPayload::Batch { handle, start, len } => {
                let handle = *handle as usize;
                let Some(inputs) = self.inputs(handle) else {
                    return TaskOutcome::Invalid;
                };
                for element in *start..start + len {
                    let value = mix(handle as u64 * 1_000 + u64::from(element), inputs);
                    self.elements[handle][element as usize].store(value, Ordering::Release);
                    self.elements_done[handle].fetch_add(1, Ordering::AcqRel);
                }
            }
            TaskPayload::Sequence(parts) => {
                for part in parts {
                    let outcome = self.run(part);
                    if outcome != TaskOutcome::Success {
                        return outcome;
                    }
                }
            }
        }
        TaskOutcome::Success
    }

    fn values(&self) -> Vec<u64> {
        let units = self.units.iter().map(|value| value.load(Ordering::Acquire));
        let elements = self
            .elements
            .iter()
            .flatten()
            .map(|value| value.load(Ordering::Acquire));
        units.chain(elements).collect()
    }
}

fn mix(seed: u64, input: u64) -> u64 {
    (seed ^ 0x9E37_79B9_7F4A_7C15)
        .wrapping_mul(0xBF58_476D_1CE4_E5B9)
        .wrapping_add(input.rotate_left(17))
}

fn scheduler(workers: usize, force_serial: bool) -> Scheduler {
    let mut config = SchedulerConfig::for_machine(16);
    config.workers = workers;
    config.force_serial = force_serial;
    Scheduler::start(config).unwrap()
}

fn execute(scheduler: &Scheduler, graph: &TaskGraph, jobs: Vec<Job>) -> Vec<u64> {
    let world = Arc::new(World::new(jobs));
    let interpreter = Arc::clone(&world);
    let executor: GraphExecutor =
        Arc::new(move |node: &TaskNode, _: &mut TaskContext<'_>| interpreter.run(&node.payload));
    let run = scheduler.run_graph(graph, executor, GUARD);
    assert!(run.completed(), "le graphe n'a pas termine");
    assert_eq!(
        world.violations.load(Ordering::Relaxed),
        0,
        "entree lue avant d'etre prete"
    );
    for handle in 0..world.jobs.len() {
        assert!(world.ready(handle), "travail {handle} incomplet");
    }
    world.values()
}

/// R-740 : le resultat ne depend ni du nombre de workers, ni de la fusion et du
/// decoupage, ni du mode `FORCE_SERIAL`.
#[test]
fn r740_results_do_not_depend_on_the_worker_count() {
    let pools: Vec<(String, Scheduler)> = vec![
        ("FORCE_SERIAL".into(), scheduler(4, true)),
        ("1 worker".into(), scheduler(1, false)),
        ("2 workers".into(), scheduler(2, false)),
        ("4 workers".into(), scheduler(4, false)),
        ("16 workers".into(), scheduler(16, false)),
    ];
    for seed in 1..=10u64 {
        let reference = execute(&pools[0].1, &build(&program(seed)), program(seed));
        for (label, pool) in &pools[1..] {
            let values = execute(pool, &build(&program(seed)), program(seed));
            assert_eq!(values, reference, "graine {seed}, {label}");
        }
        let mut transformed = build(&program(seed));
        transformed.fuse(30_000);
        transformed.split(40_000, 4, 16);
        let values = execute(&pools[3].1, &transformed, program(seed));
        assert_eq!(values, reference, "graine {seed}, fusionne et decoupe");
    }
}

fn unit(graph: &mut TaskGraph, handle: u64, deadline: Deadline) -> TaskId {
    graph.add_task(TaskSpec {
        work: WorkId(handle),
        kind: TaskKind::Compute,
        priority: Priority::High,
        deadline,
        estimated_ns: 1_000,
        payload: TaskPayload::Unit(handle),
        cancel: CancelToken::new(),
    })
}

/// Un dependant d'une tache qui n'a pas reussi n'est jamais execute, et ses propres
/// dependants non plus.
#[test]
fn a_failed_or_panicking_task_blocks_its_dependents() {
    for (failure, expected) in [(1u64, TaskOutcome::Invalid), (2, TaskOutcome::Panicked)] {
        let mut graph = TaskGraph::new();
        let failing = unit(&mut graph, failure, Deadline::None);
        let child = unit(&mut graph, 10, Deadline::None);
        let grandchild = unit(&mut graph, 11, Deadline::None);
        let independent = unit(&mut graph, 12, Deadline::None);
        graph.add_dependency(child, failing).unwrap();
        graph.add_dependency(grandchild, child).unwrap();

        let executed = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&executed);
        let executor: GraphExecutor = Arc::new(move |node: &TaskNode, _: &mut TaskContext<'_>| {
            counter.fetch_add(1, Ordering::Relaxed);
            match node.payload {
                TaskPayload::Unit(1) => TaskOutcome::Invalid,
                TaskPayload::Unit(2) => panic!("panic volontaire : propagation dans un graphe"),
                _ => TaskOutcome::Success,
            }
        });
        let pool = scheduler(2, false);
        let run = pool.run_graph(&graph, executor, GUARD);

        assert!(run.completed());
        assert_eq!(run.outcome(failing), Some(expected));
        let blocked = Some(TaskOutcome::Rejected(RejectReason::Cancelled));
        assert_eq!(run.outcome(child), blocked);
        assert_eq!(run.outcome(grandchild), blocked);
        assert_eq!(run.outcome(independent), Some(TaskOutcome::Success));
        assert_eq!(
            executed.load(Ordering::Relaxed),
            2,
            "ni l'enfant ni le petit-enfant"
        );
    }
}

/// Annule un jeton a sa destruction, y compris pendant le deroulement d'une assertion
/// qui echoue.
struct CancelOnDrop(CancelToken);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Regle de drain (PARTIE 5.16) et INV-08 : a l'echeance, la main est rendue sans
/// attendre ; la tache `THIS_TICK` en retard et son dependant sont annules, et leur
/// travail retombe sur Java.
#[test]
fn at_the_deadline_late_this_tick_work_is_cancelled_and_falls_back() {
    let mut graph = TaskGraph::new();
    let late = unit(&mut graph, 1, Deadline::ThisTick);
    let after_late = unit(&mut graph, 2, Deadline::Ticks(1));
    // Sans echeance : sur une machine chargee, elle pourrait ne pas finir en 50 ms, et
    // n'a rien a faire dans ce que le drain annule.
    let quick = unit(&mut graph, 3, Deadline::None);
    graph.add_dependency(after_late, late).unwrap();

    let executor: GraphExecutor = Arc::new(|node: &TaskNode, cx: &mut TaskContext<'_>| {
        if node.payload == TaskPayload::Unit(1) {
            // Travail trop long, qui ne s'arrete que sur annulation : s'il s'arretait de
            // lui-meme a l'echeance, il pourrait finir juste avant le drain, et le test
            // dependrait de l'ordonnanceur du systeme.
            while !cx.cancel.is_cancelled() {
                std::thread::yield_now();
            }
            return TaskOutcome::Rejected(RejectReason::Cancelled);
        }
        TaskOutcome::Success
    });
    let pool = scheduler(2, false);
    // Declare apres le pool, donc detruit avant lui : si une assertion echoue, la tache
    // en retard est annulee et l'arret du pool ne l'attend pas indefiniment.
    let _release = CancelOnDrop(graph.task(late).unwrap().cancel.clone());
    let started = Instant::now();
    let run = pool.run_graph(&graph, executor, Duration::from_millis(50));

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "la main n'a pas ete rendue"
    );
    assert!(!run.completed());
    assert_eq!(run.drain().cancelled, vec![late, after_late]);
    assert_eq!(run.drain().fallback, vec![WorkId(1), WorkId(2)]);
    assert!(graph.task(late).unwrap().cancel.is_cancelled());

    assert!(run.wait_remaining(GUARD));
    assert_eq!(run.outcome(quick), Some(TaskOutcome::Success));
    let cancelled = Some(TaskOutcome::Rejected(RejectReason::Cancelled));
    assert_eq!(run.outcome(late), cancelled);
    assert_eq!(run.outcome(after_late), cancelled);
}

#[test]
fn an_empty_graph_completes_at_once() {
    let run = scheduler(1, false).run_graph(
        &TaskGraph::new(),
        Arc::new(|_: &TaskNode, _: &mut TaskContext<'_>| TaskOutcome::Success),
        GUARD,
    );
    assert!(run.completed());
    assert!(run.drain().cancelled.is_empty());
}

/// Rouvre une porte a sa destruction, y compris pendant une assertion qui echoue.
struct OpenOnDrop(Arc<AtomicBool>);

impl Drop for OpenOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn spec(handle: u64, priority: Priority, deadline: Deadline) -> TaskSpec {
    TaskSpec {
        work: WorkId(handle),
        kind: TaskKind::Compute,
        priority,
        deadline,
        estimated_ns: 1_000,
        payload: TaskPayload::Unit(handle),
        cancel: CancelToken::new(),
    }
}

/// FM-24 : une tache `CRITICAL` qu'aucun worker ne prend est executee par le fil qui
/// attend le graphe ; une tache d'une autre priorite, jamais.
#[test]
fn fm24_a_starving_critical_task_runs_on_the_waiting_thread() {
    let pool = scheduler(1, false);
    let release = Arc::new(AtomicBool::new(false));
    let _open = OpenOnDrop(Arc::clone(&release));
    let started = Arc::new(AtomicBool::new(false));
    let (flag, gate) = (Arc::clone(&started), Arc::clone(&release));
    pool.submit(
        &TaskGroup::new(),
        JobOptions::new(Priority::Critical),
        move |_: &mut TaskContext<'_>| {
            flag.store(true, Ordering::Release);
            while !gate.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            TaskOutcome::Success
        },
    )
    .unwrap();
    while !started.load(Ordering::Acquire) {
        std::thread::yield_now();
    }

    let mut graph = TaskGraph::new();
    let critical = graph.add_task(spec(1, Priority::Critical, Deadline::ThisTick));
    let normal = graph.add_task(spec(2, Priority::Normal, Deadline::None));
    let ran_on = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&ran_on);
    let executor: GraphExecutor = Arc::new(move |node: &TaskNode, _: &mut TaskContext<'_>| {
        record
            .lock()
            .unwrap()
            .push((node.payload.clone(), std::thread::current().id()));
        TaskOutcome::Success
    });
    let caller = std::thread::current().id();

    let run = pool.run_graph(&graph, executor, Duration::from_millis(300));

    assert_eq!(run.outcome(critical), Some(TaskOutcome::Success));
    assert_eq!(
        run.outcome(normal),
        None,
        "le fil qui attend ne prend que le CRITICAL"
    );
    assert!(!run.completed());
    assert_eq!(
        *ran_on.lock().unwrap(),
        vec![(TaskPayload::Unit(1), caller)]
    );
    assert_eq!(pool.stats().famine_escalations, 1);

    release.store(true, Ordering::Release);
    assert!(run.wait_remaining(GUARD));
    assert_eq!(
        run.outcome(normal),
        Some(TaskOutcome::Success),
        "le worker libere la fait"
    );
}
