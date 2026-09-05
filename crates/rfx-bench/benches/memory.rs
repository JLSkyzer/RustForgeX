//! C-31 : budget mémoire et vidage des tampons de sonde.
//!
//! Cahier des charges : PARTIE 5.31, PARTIE 6.4. Exigences mesurées : R-709 — un flush
//! n'alloue ni ne bloque. Le vidage a lieu une fois par tick et par thread ; son coût
//! est donc multiplié par le nombre de threads sondés, ce qui justifie de le mesurer
//! par enregistrement plutôt que par appel.
// `criterion_group!` engendre une fonction publique que la macro ne documente pas, et
// qu'on ne peut pas documenter depuis ici. La derogation s'arrete a ce fichier : la
// cible de benchmark n'expose aucune API, elle n'a rien a documenter pour autrui.
#![allow(missing_docs)]

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use rfx_memory::probe_buffer::{ProbeBuffer, ProbeRecord, RecordKind, PROBE_RECORD_SIZE};
use rfx_memory::{MemoryBudget, Pool};

/// Tampon rempli de `records` enregistrements de sortie.
fn filled_buffer(records: usize) -> (ProbeBuffer, usize) {
    let mut buffer = ProbeBuffer::new(records * PROBE_RECORD_SIZE);
    for index in 0..records {
        let record = ProbeRecord {
            probe_id: index as u32,
            flags: 0,
            context_hash16: 0,
            timestamp_ns: 1_000,
            value: 4_096,
            kind: RecordKind::Exit.to_byte(),
        };
        buffer.write_at(index * PROBE_RECORD_SIZE, &record.encode());
    }
    (buffer, records * PROBE_RECORD_SIZE)
}

fn bench_budget(c: &mut Criterion) {
    let mut group = c.benchmark_group("budget");
    let mut budget = MemoryBudget::new(64);

    group.bench_function("reserve_release", |b| {
        b.iter(|| {
            let _ = budget.reserve(black_box(Pool::Probes), black_box(64 * 1024));
            budget.release(Pool::Probes, 64 * 1024);
        })
    });

    // Le chemin de refus doit rester aussi bon marché que celui d'acceptation : c'est
    // celui qu'on emprunte quand le budget est déjà saturé, donc le plus fréquent au
    // pire moment.
    let mut saturated = MemoryBudget::new(1);
    let _ = saturated.reserve(Pool::Runtime, 1024 * 1024);
    group.bench_function("reserve_refused", |b| {
        b.iter(|| black_box(saturated.reserve(Pool::Probes, 1024 * 1024)).is_err())
    });
    group.finish();
}

fn bench_flush(c: &mut Criterion) {
    let mut group = c.benchmark_group("probe_buffer");
    for records in [64_usize, 2_048] {
        group.throughput(Throughput::Elements(records as u64));
        group.bench_function(format!("flush_{records}_records"), |b| {
            b.iter_batched_ref(
                || filled_buffer(records),
                |(buffer, used)| {
                    let mut consumed = 0_u64;
                    buffer.flush_with(*used, |record| {
                        consumed = consumed.wrapping_add(record.value);
                    });
                    black_box(consumed)
                },
                BatchSize::LargeInput,
            )
        });
    }
    group.finish();
}

fn bench_record_codec(c: &mut Criterion) {
    let record = ProbeRecord {
        probe_id: 42,
        flags: 0,
        context_hash16: 7,
        timestamp_ns: 1_234_567,
        value: 4_096,
        kind: RecordKind::Exit.to_byte(),
    };
    let bytes = record.encode();

    let mut group = c.benchmark_group("probe_record");
    group.bench_function("encode", |b| b.iter(|| black_box(&record).encode()));
    group.bench_function("decode", |b| {
        b.iter(|| ProbeRecord::decode(black_box(&bytes)))
    });
    group.finish();
}

criterion_group!(memory, bench_budget, bench_flush, bench_record_codec);
criterion_main!(memory);
