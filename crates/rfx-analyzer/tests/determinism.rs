//! C-13 sur des classes compilees par `javac` (ADR-058).
//!
//! L'index contient les classes de `rfx-bytecode/tests/fixtures`, pas le JDK : un appel
//! vers le JDK n'est pas resolu, et rend la partie correspondante du graphe inconnue.
//! L'appel ecrit reste lu : il suffit a nommer une source.

use std::path::PathBuf;

use rfx_analyzer::dataflow::{analyze_graph, workload_effects_from, DataflowLimits, DomainTable};
use rfx_analyzer::determinism::determinism_facts;
use rfx_analyzer::{CallGraph, ClassIndex, Limits, MethodRef};
use rfx_bytecode::ClassFile;
use rfx_model::{DeterminismFacts, Domain, NonDetSource, Tri};

fn index() -> ClassIndex {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rfx-bytecode/tests/fixtures/classes/fixtures");
    ClassIndex::from_classes(
        std::fs::read_dir(dir)
            .expect("classes de test")
            .map(|entry| {
                let bytes = std::fs::read(entry.expect("entree").path()).expect("lecture");
                ClassFile::parse(&bytes).expect("lisible")
            }),
    )
}

fn facts_of(owner: &str, name: &str, descriptor: &str) -> DeterminismFacts {
    let index = index();
    let root = MethodRef::new(owner, name, descriptor);
    let graph = CallGraph::build(&index, &root, Limits::default());
    let analysed = analyze_graph(&graph, &index, &DataflowLimits::DEFAULT);
    let domains = DomainTable::new();
    let effects = workload_effects_from(&graph, &index, &domains, &analysed);
    determinism_facts(&graph, &index, &analysed, &effects, &domains)
}

fn facts(name: &str, descriptor: &str) -> DeterminismFacts {
    facts_of("fixtures/Chance", name, descriptor)
}

fn bit(source: NonDetSource) -> u16 {
    1 << source as u16
}

#[test]
fn t220_each_source_is_detected_and_makes_the_unit_nondeterministic() {
    let cases = [
        ("roll", "(Ljava/util/Random;)I", NonDetSource::SharedRandom),
        ("now", "()J", NonDetSource::SystemTime),
        (
            "identity",
            "(Ljava/lang/Object;)I",
            NonDetSource::IdentityHash,
        ),
        (
            "keys",
            "(Ljava/util/HashMap;)I",
            NonDetSource::HashMapIteration,
        ),
        ("exists", "(Ljava/io/File;)Z", NonDetSource::ExternalIo),
        (
            "locked",
            "(Ljava/util/concurrent/locks/Lock;)V",
            NonDetSource::ThreadScheduling,
        ),
        (
            "reflect",
            "(Ljava/lang/reflect/Method;)Ljava/lang/Object;",
            NonDetSource::Reflection,
        ),
        ("callsNative", "()I", NonDetSource::NativeCall),
    ];
    for (name, descriptor, source) in cases {
        let found = facts(name, descriptor);
        assert_ne!(
            found.sources_of_nondeterminism & bit(source),
            0,
            "{name} : {source:?} attendue, sources {:#b}",
            found.sources_of_nondeterminism
        );
        assert_eq!(found.deterministic, Tri::No, "{name}");
        assert_eq!(found.verified_runs, 0, "aucune execution en shadow");
        assert_eq!(found.mismatches, 0);
    }
}

#[test]
fn t221_a_complete_graph_without_source_is_deterministic() {
    let pure = facts("pure", "(II)I");
    assert_eq!(pure.sources_of_nondeterminism, 0);
    assert_eq!(pure.deterministic, Tri::Yes);
    assert!(!pure.float_sensitive, "arithmetique entiere");
    assert_eq!(pure.order_sensitive, Tri::No, "lecture seule");

    let sum = facts("sum", "(DD)D");
    assert_eq!(sum.deterministic, Tri::Yes);
    assert!(sum.float_sensitive, "dadd");

    let store = facts("store", "(I)V");
    assert_eq!(store.deterministic, Tri::Yes);
    assert_eq!(store.order_sensitive, Tri::Yes, "ecrit un statique");
}

#[test]
fn an_unknown_part_of_the_graph_alone_leaves_determinism_unknown() {
    // `roll` appelle le JDK : partie inconnue, mais une source definie en plus, NO
    // l'emporte.
    let roll = facts("roll", "(Ljava/util/Random;)I");
    assert_ne!(
        roll.sources_of_nondeterminism & bit(NonDetSource::Unknown),
        0
    );
    assert!(roll.float_sensitive, "graphe incomplet : prudent");

    // `Threads.readLocal` n'appelle que `ThreadLocal.get`, du JDK, absent de l'index :
    // seule la partie inconnue empeche de conclure.
    let local = facts_of("fixtures/Threads", "readLocal", "()I");
    assert_eq!(local.sources_of_nondeterminism, bit(NonDetSource::Unknown));
    assert_eq!(local.deterministic, Tri::Unknown);
    assert_eq!(local.order_sensitive, Tri::Unknown);
}

#[test]
fn domains_reached_through_the_dataflow_are_sources() {
    // Les classes de test n'atteignent aucune ancre de domaine ; on prete au graphe de
    // `pure` des acces que seul le jeu peut faire.
    let index = index();
    let root = MethodRef::new("fixtures/Chance", "pure", "(II)I");
    let graph = CallGraph::build(&index, &root, Limits::default());
    let analysed = analyze_graph(&graph, &index, &DataflowLimits::DEFAULT);
    let domains = DomainTable::new();
    let base = workload_effects_from(&graph, &index, &domains, &analysed);
    let cases = [
        (Domain::Random, false, NonDetSource::SharedRandom),
        (Domain::Time, false, NonDetSource::SystemTime),
        (Domain::FileSystem, true, NonDetSource::ExternalIo),
        (Domain::Network, false, NonDetSource::ExternalIo),
    ];
    for (domain, write, source) in cases {
        let mut effects = base.clone();
        if write {
            effects.writes.domains = effects.writes.domains.with(domain);
        } else {
            effects.reads.domains = effects.reads.domains.with(domain);
        }
        let found = determinism_facts(&graph, &index, &analysed, &effects, &domains);
        assert_eq!(found.sources_of_nondeterminism, bit(source), "{domain:?}");
        assert_eq!(found.deterministic, Tri::No, "{domain:?}");
    }
}
