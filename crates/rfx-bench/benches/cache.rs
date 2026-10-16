//! Base des unites de travail (C-09, C-48) : chargement et encodage de `workload.db`.
//!
//! Cahier des charges : acceptance de C-09 (« chargement du cache en moins de
//! 200 ms » pour 20 000 unites). La memoire (« moins de 64 Mio ») est mesuree a part,
//! par allocateur instrumente : `crates/rfx-cache/tests/acceptance_memory.rs`.
//!
//! Le fichier est ecrit une fois sur le vrai disque, dans le repertoire temporaire, avec
//! 20 000 unites au pire (noms longs, historique plein). `load_20k` mesure ce que fait
//! le demarrage : lecture, verification de l'en-tete et de la somme, decodage, index.
//! `encode_20k` mesure ce que fait le fil `RFX-IO` avant d'ecrire, a titre indicatif.
// `criterion_group!` engendre une fonction publique que la macro ne documente pas, et
// qu'on ne peut pas documenter depuis ici. La derogation s'arrete a ce fichier : la
// cible de benchmark n'expose aucune API, elle n'a rien a documenter pour autrui.
#![allow(missing_docs)]

use std::path::PathBuf;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rfx_cache::{
    file, load, FileSystem, HistoryEvent, HistoryKind, StdFileSystem, WorkloadRecord,
    HISTORY_CAPACITY,
};
use rfx_model::{Fingerprint, Heat, WorkDescriptor};

const WORKLOADS: u64 = 20_000;

/// Meme cas que le test d'acceptance de la memoire : noms longs, historique plein.
fn worst_case(n: u64) -> WorkloadRecord {
    let descriptor = WorkDescriptor {
        owner_id: format!("an_unusually_long_mod_identifier_{:03}", n % 300),
        class_internal_name: format!(
            "com/example/somemod/content/machines/processing/advanced/AdvancedMachineBlockEntity{n}"
        ),
        method_name: format!("lambda$serverTickProcessingRecipes${n}"),
        method_descriptor: "(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)V".to_owned(),
        call_context_hash: n,
        ..WorkDescriptor::default()
    };
    let fingerprint = Fingerprint {
        bytecode_hash: n,
        ..environment()
    };
    let mut record = WorkloadRecord::new(descriptor, fingerprint);
    record.dynamics.heat = Heat::Warm;
    record.dynamics.last_seen_tick = n;
    for tick in 0..HISTORY_CAPACITY as u64 {
        record.history.push(HistoryEvent {
            tick: 1_000_000 + tick,
            kind: HistoryKind::HeatChanged {
                from: Heat::Cold,
                to: Heat::Warm,
            },
        });
    }
    record
}

fn environment() -> Fingerprint {
    Fingerprint {
        schema_version: 1,
        mc_version: 0x1201,
        mod_set_hash: 99,
        ..Fingerprint::default()
    }
}

/// Fichier temporaire, supprime a la fin du banc.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn bench_cache(c: &mut Criterion) {
    let records: Vec<WorkloadRecord> = (0..WORKLOADS).map(worst_case).collect();
    let bytes = file::encode(&environment(), &records).unwrap();
    let path = TempFile(
        std::env::temp_dir().join(format!("rfx-bench-workload-{}.db", std::process::id())),
    );
    StdFileSystem.write_atomic(&path.0, &bytes).unwrap();
    println!(
        "workload.db : {WORKLOADS} unites, {} octets ({:.1} Mio)",
        bytes.len(),
        bytes.len() as f64 / f64::from(1 << 20)
    );

    let mut group = c.benchmark_group("cache");
    group.sample_size(20);
    group.bench_function("load_20k", |b| {
        b.iter(|| {
            let outcome = load(&StdFileSystem, &path.0, &environment(), |_| None).unwrap();
            let db = outcome.into_db();
            assert_eq!(db.len(), WORKLOADS as usize);
            black_box(db)
        });
    });
    group.bench_function("encode_20k", |b| {
        b.iter(|| black_box(file::encode(&environment(), black_box(&records)).unwrap()));
    });
    group.finish();
}

criterion_group!(benches, bench_cache);
criterion_main!(benches);
