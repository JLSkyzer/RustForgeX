//! Coût de la sérialisation CBOR du statut (IF-01).
//!
//! Composant mesuré : DM-xx et la frontière. Le blob de statut traverse à chaque
//! `/rfx status`, donc au plus quelques fois par seconde et jamais dans le tick — mais
//! R-601 lui impose un budget de 5 ms, qu'il faut pouvoir vérifier.
// `criterion_group!` engendre une fonction publique que la macro ne documente pas, et
// qu'on ne peut pas documenter depuis ici. La derogation s'arrete a ce fichier : la
// cible de benchmark n'expose aucune API, elle n'a rien a documenter pour autrui.
#![allow(missing_docs)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rfx_model::{
    ComponentStatus, HardwareClass, Maturity, ProbeCoverage, ProbeStatus, ProfilerStatus,
    RuntimeStatus, TickStatus,
};

fn status() -> RuntimeStatus {
    RuntimeStatus {
        schema: rfx_model::MODEL_SCHEMA_VERSION,
        abi_version: 1,
        native_version: "0.1.0".to_owned(),
        state: "RUNNING".to_owned(),
        panics: 0,
        hardware: HardwareClass::default(),
        probe_coverage: ProbeCoverage::default(),
        components: (0..8)
            .map(|n| ComponentStatus {
                id: format!("C-{n:02}"),
                name: "Composant de mesure".to_owned(),
                maturity: Maturity::Stable,
                active: true,
            })
            .collect(),
        tick: TickStatus::default(),
        probes: ProbeStatus::default(),
        profiler: ProfilerStatus::default(),
    }
}

fn bench(c: &mut Criterion) {
    let status = status();
    let blob = rfx_model::to_cbor(&status).expect("encodage");

    c.bench_function("encode", |b| {
        b.iter(|| rfx_model::to_cbor(black_box(&status)).expect("encodage"))
    });
    c.bench_function("decode", |b| {
        b.iter(|| {
            let decoded: RuntimeStatus = rfx_model::from_cbor(black_box(&blob)).expect("decodage");
            decoded
        })
    });
}

criterion_group!(codec, bench);
criterion_main!(codec);
