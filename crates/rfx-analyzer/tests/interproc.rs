//! Effets interproceduraux de C-11 sur des classes compilees par `javac` (ADR-055).
//!
//! L'index contient toutes les classes de `rfx-bytecode/tests/fixtures`, mais pas le
//! JDK : un appel vers le JDK, sauf `Object.<init>`, est un appel vers une methode sans
//! code, donc inconnu. `fixtures/Interproc` n'a pas d'ancre : un champ ecrit sur un de
//! ses objets partages est du domaine `Unknown`.

use std::path::PathBuf;

use rfx_analyzer::dataflow::{workload_effects, DataflowLimits, DomainTable, WorkloadEffects};
use rfx_analyzer::{CallGraph, ClassIndex, Limits, MethodRef};
use rfx_bytecode::ClassFile;
use rfx_model::{Domain, Domains, Precision};

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

fn effects(name: &str, descriptor: &str) -> WorkloadEffects {
    let index = index();
    let root = MethodRef::new("fixtures/Interproc", name, descriptor);
    let graph = CallGraph::build(&index, &root, Limits::default());
    workload_effects(
        &graph,
        &index,
        &DomainTable::new(),
        &DataflowLimits::DEFAULT,
    )
}

fn assert_touches_nothing(name: &str, descriptor: &str) {
    let found = effects(name, descriptor);
    assert_eq!(found.reads.domains, Domains::EMPTY, "{name} : lectures");
    assert_eq!(found.writes.domains, Domains::EMPTY, "{name} : ecritures");
    assert_eq!(found.writes.precision, Precision::Exact, "{name}");
    assert!(!found.side_effects.unknown, "{name}");
    assert!(!found.side_effects.mutates_world, "{name}");
}

#[test]
fn objects_built_and_used_locally_touch_nothing() {
    // Le constructeur ecrit dans l'objet passe ; l'objet etant local a l'appelant,
    // l'ecriture l'est aussi. `Object.<init>` est sans effet.
    assert_touches_nothing("localObject", "()I");
    // Une fabrique rend un objet frais, local a l'appelant.
    assert_touches_nothing("viaFactory", "()I");
    // L'appele ecrit dans l'objet passe, qui est local.
    assert_touches_nothing("setOnLocal", "()I");
    // L'appele rend son receveur : la valeur rendue est l'objet local.
    assert_touches_nothing("selfLocal", "()I");
}

#[test]
fn a_callee_write_on_a_shared_object_reaches_the_caller() {
    let shared = effects("setOnShared", "()V");
    assert!(
        shared.writes.domains.contains(Domain::Unknown),
        "objet partage"
    );
    assert!(
        shared.reads.domains.contains(Domain::ModStatics),
        "lecture du statique"
    );
    assert_eq!(shared.causes.unresolved_sites, 0, "tout est resolu");

    // Racine avec parametre : l'objet passe vient du jeu, il est partage.
    let root = effects("setOn", "(Lfixtures/Interproc;)V");
    assert_eq!(root.writes.domains, Domains::EMPTY.with(Domain::Unknown));
}

#[test]
fn a_callee_effect_on_a_parameter_reaches_the_caller_parameter() {
    // `forward` passe son parametre a `setOn` ; a la racine, ce parametre est partage.
    let forward = effects("forward", "(Lfixtures/Interproc;)V");
    assert_eq!(forward.writes.domains, Domains::EMPTY.with(Domain::Unknown));
}

#[test]
fn the_fixed_point_revisits_a_caller_when_a_callee_changes() {
    let both = effects("twoCallers", "()V");
    assert!(both.writes.domains.contains(Domain::ModStatics));
    assert!(
        both.writes.domains.contains(Domain::Unknown),
        "`first` publie l'objet par `second` : l'ecriture suivante est partagee"
    );
}

#[test]
fn escape_through_a_callee_makes_later_writes_shared() {
    let escaped = effects("escapesViaCallee", "()V");
    assert!(
        escaped.writes.domains.contains(Domain::ModStatics),
        "l'appele publie"
    );
    assert!(
        escaped.writes.domains.contains(Domain::Unknown),
        "l'ecriture suivante porte sur un objet publie"
    );

    let published = effects("publish", "()V");
    assert!(published.writes.domains.contains(Domain::ModStatics));
}

#[test]
fn a_parameter_stored_in_a_returned_object_escapes() {
    // `wrap` range son parametre dans l'objet qu'il rend ; l'appelant publie cet objet :
    // l'objet interieur n'est plus local.
    let leak = effects("wrapLeak", "()V");
    assert!(leak.writes.domains.contains(Domain::ModStatics));
    assert!(
        leak.writes.domains.contains(Domain::Unknown),
        "ecriture sur l'objet interieur, atteignable par le statique"
    );
}

#[test]
fn recursion_reaches_a_fixed_point() {
    let recursive = effects("recursive", "()I");
    assert_eq!(recursive.writes.domains, Domains::EMPTY);
    assert_eq!(recursive.writes.precision, Precision::Exact);
    assert!(!recursive.causes.not_converged);
    assert!(recursive.passes >= 2, "la recursion est repassee");
}

#[test]
fn unresolved_and_external_calls_are_unknown() {
    // T-202 : un appel non resolu rend l'unite inconnue (R-380).
    let lambda = effects("lambda", "()V");
    assert_eq!(lambda.writes.precision, Precision::Unknown);
    assert!(lambda.side_effects.unknown);
    assert!(
        lambda.causes.unresolved_sites >= 1,
        "Runnable.run, par une interface"
    );

    let open = effects("openInterface", "()V");
    assert!(open.side_effects.unknown);

    // javac 17 appelle String.valueOf(Object) avant la concatenation : un appel vers
    // une classe absente de l'index, que C-07 ne resout pas (FM-15).
    let object = effects("concatObject", "()Ljava/lang/String;");
    assert!(object.side_effects.unknown);
    assert!(object.causes.unresolved_sites >= 1);
}

#[test]
fn lambda_creation_and_safe_concatenation_have_no_effect() {
    assert_touches_nothing("lambdaOnly", "()Ljava/lang/Runnable;");
    // Le corps ecrit un statique, mais ne s'execute pas a la creation.
    assert_touches_nothing("lambdaWrites", "()Ljava/lang/Runnable;");
    assert_touches_nothing("concat", "()Ljava/lang/String;");
}

#[test]
fn a_truncated_graph_is_unknown() {
    let index = index();
    let root = MethodRef::new("fixtures/Interproc", "localObject", "()I");
    let shallow = Limits {
        max_call_depth: 0,
        max_nodes: 2_000,
    };
    let graph = CallGraph::build(&index, &root, shallow);
    let found = workload_effects(
        &graph,
        &index,
        &DomainTable::new(),
        &DataflowLimits::DEFAULT,
    );
    assert!(found.causes.truncated);
    assert_eq!(found.writes.precision, Precision::Unknown);
}
