//! C-12 sur des classes compilees par `javac` (ADR-057).
//!
//! L'index contient les classes de `rfx-bytecode/tests/fixtures`, pas le JDK : un appel
//! vers le JDK n'est pas resolu, et rend la partie correspondante du graphe inconnue.

use std::path::PathBuf;

use rfx_analyzer::dataflow::{analyze_graph, workload_effects_from, DataflowLimits, DomainTable};
use rfx_analyzer::thread::{thread_facts, ThreadSafetyTable};
use rfx_analyzer::{CallGraph, ClassIndex, Limits, MethodRef};
use rfx_bytecode::ClassFile;
use rfx_model::{DeterminismFacts, Domain, ThreadFacts, Tri};

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

fn facts(name: &str, descriptor: &str, determinism: Option<&DeterminismFacts>) -> ThreadFacts {
    let index = index();
    let root = MethodRef::new("fixtures/Threads", name, descriptor);
    let graph = CallGraph::build(&index, &root, Limits::default());
    let analysed = analyze_graph(&graph, &index, &DataflowLimits::DEFAULT);
    let domains = DomainTable::new();
    let effects = workload_effects_from(&graph, &index, &domains, &analysed);
    thread_facts(
        &graph,
        &index,
        &analysed,
        &effects,
        &domains,
        &ThreadSafetyTable::new(),
        determinism,
    )
}

fn deterministic() -> DeterminismFacts {
    DeterminismFacts {
        deterministic: Tri::Yes,
        ..DeterminismFacts::default()
    }
}

#[test]
fn t211_derived_confinement_follows_the_mutable_fields() {
    let index = index();
    let table = ThreadSafetyTable::new();
    let unsafe_ = |name: &str| table.is_unsafe(&index, &format!("fixtures/{name}"));
    assert!(unsafe_("Counter"), "champ public mutable");
    assert!(!unsafe_("Frozen"), "champ final");
    assert!(
        unsafe_("Mutating"),
        "champ prive ecrit hors du constructeur"
    );
    assert!(
        !unsafe_("Guarded"),
        "ecrit seulement dans une methode synchronisee"
    );
    assert!(
        !unsafe_("CtorOnly"),
        "ecrit seulement par le constructeur, hors nid"
    );
    assert!(unsafe_("Threads$InnerPrivate"), "prive, mais dans un nid");
    assert!(unsafe_("CounterChild"), "herite d'une classe non sure");
    assert!(unsafe_("Missing"), "classe inconnue : non sure");
    assert!(table.is_unsafe(&index, "[I"), "tableau");
    // Memorise : meme reponse au second appel.
    assert!(!unsafe_("Frozen"));
}

#[test]
fn t210_thread_locals_are_detected() {
    let local = facts("readLocal", "()I", None);
    assert!(local.uses_thread_locals);
    // ThreadLocal.get vient du JDK, absent de l'index : partie inconnue.
    assert!(local.touches_non_threadsafe_api);
    assert_eq!(local.main_thread_required, Tri::Yes);
}

#[test]
fn writing_a_shared_unsafe_object_requires_the_main_thread() {
    let shared = facts(
        "touchShared",
        "(Lfixtures/Counter;)V",
        Some(&deterministic()),
    );
    assert!(shared.touches_non_threadsafe_api);
    assert_eq!(shared.main_thread_required, Tri::Yes);
    assert_eq!(shared.observed_roles, None, "aucune observation");
}

#[test]
fn reading_a_safe_object_is_free_only_when_deterministic() {
    let unknown = facts("readFrozen", "(Lfixtures/Frozen;)I", None);
    assert!(!unknown.touches_non_threadsafe_api);
    assert_eq!(
        unknown.main_thread_required,
        Tri::Unknown,
        "sans C-13, la purete n'est pas prouvee"
    );
    assert_eq!(unknown.reentrant, Tri::No, "graphe complet sans cycle");

    let free = facts("readFrozen", "(Lfixtures/Frozen;)I", Some(&deterministic()));
    assert_eq!(free.main_thread_required, Tri::No);
    assert!(!free.uses_synchronized && !free.uses_locks && !free.uses_thread_locals);
}

#[test]
fn synchronized_methods_and_waits_are_reported() {
    let sync = facts("syncMethod", "()V", None);
    assert!(sync.uses_synchronized);

    let waits = facts("waits", "(Ljava/lang/Object;)V", None);
    assert!(waits.uses_synchronized, "bloc synchronized");
    assert!(waits.uses_locks, "Object.wait");
}

#[test]
fn writing_game_state_requires_the_main_thread() {
    // Le graphe de `readFrozen` ne touche aucune classe non sure ; on lui prete des
    // ecritures dans l'etat des entites, que les classes de test ne peuvent pas faire.
    let index = index();
    let root = MethodRef::new("fixtures/Threads", "readFrozen", "(Lfixtures/Frozen;)I");
    let graph = CallGraph::build(&index, &root, Limits::default());
    let analysed = analyze_graph(&graph, &index, &DataflowLimits::DEFAULT);
    let domains = DomainTable::new();
    let mut effects = workload_effects_from(&graph, &index, &domains, &analysed);
    effects.writes.domains = effects.writes.domains.with(Domain::EntityState);
    let found = thread_facts(
        &graph,
        &index,
        &analysed,
        &effects,
        &domains,
        &ThreadSafetyTable::new(),
        Some(&deterministic()),
    );
    assert!(!found.touches_non_threadsafe_api);
    assert_eq!(found.main_thread_required, Tri::Yes);
}
