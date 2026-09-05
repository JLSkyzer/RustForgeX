//! Chemin chaud de C-05 : agrégation d'un enregistrement et clôture d'un tick.
//!
//! Cahier des charges : PARTIE 5.5, PARTIE 21.5. Exigence mesurée ici : R-320 — le
//! profiler n'alloue pas dans le chemin chaud. Le test T-142 prouve qu'il n'alloue
//! pas ; ces mesures disent ce qu'il coûte.
//!
//! `ingest` est appelé une fois par enregistrement produit, `end_tick` vingt fois par
//! seconde. Les deux ont donc des budgets de nature différente, et sont mesurés
//! séparément.
// `criterion_group!` engendre une fonction publique que la macro ne documente pas, et
// qu'on ne peut pas documenter depuis ici. La derogation s'arrete a ce fichier : la
// cible de benchmark n'expose aucune API, elle n'a rien a documenter pour autrui.
#![allow(missing_docs)]

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use rfx_memory::probe_buffer::{ProbeRecord, RecordKind};
use rfx_model::{Histogram, WorkId};
use rfx_profiler::{Profiler, ProfilerConfig, TickCost};

/// Tick représentatif : 10 ms de travail utile dans une période de 50 ms.
fn tick_cost() -> TickCost {
    TickCost {
        profiling_ns: 10_000,
        tick_ns: 10_000_000,
        period_ns: 50_000_000,
    }
}

fn record(probe_id: u32, kind: RecordKind, value: u64) -> ProbeRecord {
    ProbeRecord {
        probe_id,
        flags: 0,
        context_hash16: 0,
        timestamp_ns: 0,
        value,
        kind: kind.to_byte(),
    }
}

/// Profiler démarré, portant `workloads` unités déjà enregistrées.
fn profiler_with(workloads: u64) -> (Profiler, Vec<u32>) {
    let mut profiler = Profiler::new(ProfilerConfig::default());
    profiler.start();
    let probes = (1..=workloads)
        .filter_map(|n| profiler.register(WorkId(n)))
        .collect();
    (profiler, probes)
}

fn bench_ingest(c: &mut Criterion) {
    let (mut profiler, probes) = profiler_with(1_000);
    let exit = record(probes[0], RecordKind::Exit, 4_096);
    let enter = record(probes[0], RecordKind::Enter, 0);
    let sample = record(probes[0], RecordKind::Sample, 1_000);
    let unknown = record(999_999, RecordKind::Exit, 4_096);

    let mut group = c.benchmark_group("ingest");
    group.bench_function("exit", |b| b.iter(|| profiler.ingest(black_box(&exit))));
    group.bench_function("enter", |b| b.iter(|| profiler.ingest(black_box(&enter))));
    group.bench_function("sample", |b| b.iter(|| profiler.ingest(black_box(&sample))));
    // Un identifiant inconnu est le chemin de refus : il doit rester bon marché, sans
    // quoi un flux d'enregistrements orphelins coûterait plus cher qu'un flux utile.
    group.bench_function("unknown", |b| {
        b.iter(|| profiler.ingest(black_box(&unknown)))
    });
    group.finish();
}

fn bench_end_tick(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_tick");
    for workloads in [100_u64, 1_000, 10_000] {
        group.bench_function(format!("{workloads}_workloads"), |b| {
            b.iter_batched_ref(
                || {
                    let (mut profiler, probes) = profiler_with(workloads);
                    // Un tick où chaque unité a été appelée : le cas le plus cher,
                    // puisque chacune voit ses quantiles recalculés.
                    for &probe in &probes {
                        profiler.ingest(&record(probe, RecordKind::Exit, 4_096));
                    }
                    profiler
                },
                |profiler| profiler.end_tick(black_box(tick_cost())),
                BatchSize::LargeInput,
            )
        });
    }
    group.finish();
}

fn bench_histogram(c: &mut Criterion) {
    let mut group = c.benchmark_group("histogram");

    group.bench_function("record", |b| {
        let mut histogram = Histogram::new();
        b.iter(|| histogram.record(black_box(4_096)))
    });

    // Le quantile parcourt les soixante-quatre compartiments : c'est le coût qui
    // justifie de ne pas rafraîchir une unité muette et déjà froide à chaque tick.
    let mut filled = Histogram::new();
    for value in 1..=10_000_u64 {
        filled.record(value);
    }
    group.bench_function("p95", |b| b.iter(|| black_box(&filled).p95()));
    group.finish();
}

criterion_group!(profiler, bench_ingest, bench_end_tick, bench_histogram);
criterion_main!(profiler);
