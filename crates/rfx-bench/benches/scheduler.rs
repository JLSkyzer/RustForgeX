//! Ordonnancement (C-16, C-17) : cout d'une soumission et construction d'un graphe.
//!
//! Cahier des charges : T-267 (« overhead de soumission < 200 ns ») et acceptance de
//! C-16 (« construction du graphe en moins de 100 us pour 500 noeuds »).
//!
//! La soumission est mesuree seule : `iter_custom` chronometre la boucle de
//! soumissions, puis attend que les workers aient tout execute, hors du temps mesure.
//! Sans cela, la file grossirait d'une iteration a l'autre et la mesure dependrait de sa
//! taille.
// `criterion_group!` engendre une fonction publique que la macro ne documente pas, et
// qu'on ne peut pas documenter depuis ici. La derogation s'arrete a ce fichier : la
// cible de benchmark n'expose aucune API, elle n'a rien a documenter pour autrui.
#![allow(missing_docs)]

use std::time::{Duration, Instant};

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rfx_model::WorkId;
use rfx_scheduler::{JobOptions, Scheduler, SchedulerConfig, TaskContext, TaskGroup, TaskOutcome};
use rfx_taskgraph::{CancelToken, Deadline, Priority, TaskGraph, TaskKind, TaskPayload, TaskSpec};

fn noop(_: &mut TaskContext<'_>) -> TaskOutcome {
    TaskOutcome::Success
}

fn bench_submit(c: &mut Criterion) {
    let logical = std::thread::available_parallelism().map_or(4, |n| n.get());
    let scheduler = Scheduler::start(SchedulerConfig::for_machine(logical)).unwrap();
    let mut group = c.benchmark_group("scheduler");
    group.bench_function("submit_noop", |b| {
        b.iter_custom(|iters| {
            let tasks = TaskGroup::new();
            let started = Instant::now();
            for _ in 0..iters {
                // HIGH : jamais refusee par contre-pression, la mesure ne compte pas de
                // refus.
                let _ = scheduler.submit(&tasks, JobOptions::new(Priority::High), noop);
            }
            let elapsed = started.elapsed();
            assert!(tasks.wait(Duration::from_secs(60)));
            elapsed
        });
    });
    // Meme mesure, jeton d'annulation partage : separe le cout du pool de celui de
    // l'allocation d'un jeton neuf par `JobOptions::new`.
    let shared_options = JobOptions::new(Priority::High);
    group.bench_function("submit_noop_shared_token", |b| {
        b.iter_custom(|iters| {
            let tasks = TaskGroup::new();
            let started = Instant::now();
            for _ in 0..iters {
                let _ = scheduler.submit(&tasks, shared_options.clone(), noop);
            }
            let elapsed = started.elapsed();
            assert!(tasks.wait(Duration::from_secs(60)));
            elapsed
        });
    });
    group.finish();
}

fn spec(index: u64, token: &CancelToken) -> TaskSpec {
    TaskSpec {
        work: WorkId(index / 8),
        kind: TaskKind::Analysis,
        priority: Priority::Normal,
        deadline: Deadline::None,
        estimated_ns: 5_000,
        payload: TaskPayload::Unit(index),
        cancel: token.clone(),
    }
}

/// 500 noeuds, deux dependances chacun vers des noeuds anterieurs : la construction
/// naturelle d'un tick (ADR-043, decision 3).
fn bench_graph(c: &mut Criterion) {
    let token = CancelToken::new();
    let mut group = c.benchmark_group("taskgraph");
    group.bench_function("build_500", |b| {
        b.iter(|| {
            let mut graph = TaskGraph::with_capacity(500);
            let mut ids = Vec::with_capacity(500);
            for index in 0..500u64 {
                let id = graph.add_task(spec(index, &token));
                if index > 0 {
                    let _ = graph.add_dependency(id, ids[index as usize - 1]);
                    let _ = graph.add_dependency(id, ids[index as usize / 2]);
                }
                ids.push(id);
            }
            black_box(graph)
        });
    });
    group.finish();
}

criterion_group!(benches, bench_submit, bench_graph);
criterion_main!(benches);
