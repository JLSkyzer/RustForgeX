//! C-07 : T-160 a T-164 sur des classes compilees par `javac` (ADR-050).
//!
//! Les classes sont celles de `rfx-bytecode/tests/fixtures`. L'index n'en contient
//! aucune du JDK : tout appel vers le JDK est donc un appel vers une classe absente.

use std::path::PathBuf;

use rfx_analyzer::{CallGraph, ClassIndex, EdgeKind, Limits, MethodRef, Resolution, Unresolved};
use rfx_bytecode::ClassFile;

fn index() -> ClassIndex {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rfx-bytecode/tests/fixtures/classes/fixtures");
    let classes = std::fs::read_dir(&dir)
        .expect("classes de test")
        .map(|entry| entry.expect("entree").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "class"))
        .map(|path| ClassFile::parse(&std::fs::read(&path).expect("lecture")).expect("lisible"));
    ClassIndex::from_classes(classes)
}

fn method(owner: &str, name: &str, descriptor: &str) -> MethodRef {
    MethodRef::new(&format!("fixtures/{owner}"), name, descriptor)
}

fn build(root: &MethodRef) -> CallGraph {
    CallGraph::build(&index(), root, Limits::default())
}

/// Aretes resolues d'une methode : `(cible, resolution)`, triees.
fn resolved_from(graph: &CallGraph, from: &MethodRef) -> Vec<(String, Resolution)> {
    let mut found: Vec<_> = graph
        .edges_from(from)
        .filter_map(|edge| match (edge.kind, edge.to) {
            (EdgeKind::Resolved(resolution), Some(to)) => {
                Some((graph.nodes()[to].method.to_string(), resolution))
            }
            _ => None,
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Raisons des aretes non resolues d'une methode, dans l'ordre du code.
fn unresolved_from(graph: &CallGraph, from: &MethodRef) -> Vec<Unresolved> {
    graph
        .edges_from(from)
        .filter_map(|edge| match edge.kind {
            EdgeKind::VirtualUnresolved(reason) => Some(reason),
            _ => None,
        })
        .collect()
}

/// T-160 : un appel virtuel a une seule implementation chargee est resolu vers elle ;
/// les appels lies statiquement le sont aussi, prive compris (`invokevirtual` depuis
/// Java 11).
#[test]
fn t160_monomorphic_and_static_calls_are_resolved() {
    let root = method(
        "Calls",
        "monomorphic",
        "(Lfixtures/Animal;)Ljava/lang/String;",
    );
    let graph = build(&root);
    assert_eq!(
        resolved_from(&graph, &root),
        vec![(
            "fixtures/Dog.sound()Ljava/lang/String;".to_owned(),
            Resolution::Monomorphic
        )]
    );
    assert_eq!(
        resolved_from(&graph, &method("Dog", "sound", "()Ljava/lang/String;")),
        vec![(
            "fixtures/Dog.bark()Ljava/lang/String;".to_owned(),
            Resolution::Exact
        )],
        "methode privee"
    );

    let direct = method("Calls", "direct", "()I");
    let graph = build(&direct);
    assert_eq!(
        resolved_from(&graph, &direct),
        vec![
            ("fixtures/Calls.<init>()V".to_owned(), Resolution::Special),
            ("fixtures/Calls.helper(I)I".to_owned(), Resolution::Static),
            ("fixtures/Calls.sealed()I".to_owned(), Resolution::Exact),
            ("fixtures/Calls.secret()I".to_owned(), Resolution::Exact),
        ]
    );
    let init = graph
        .find(&method("Calls", "<init>", "()V"))
        .expect("constructeur");
    assert!(
        resolved_from(&graph, &init.method)
            .iter()
            .any(|(target, _)| target == "java/lang/Object.<init>()V"),
        "appel au constructeur de Object, noeud externe"
    );
    assert!(
        graph
            .find(&MethodRef::new("java/lang/Object", "<init>", "()V"))
            .unwrap()
            .external
    );

    let derived = method("Derived", "value", "()I");
    assert_eq!(
        resolved_from(&build(&derived), &derived),
        vec![("fixtures/Base.value()I".to_owned(), Resolution::Special)],
        "appel a super"
    );
}

/// Polymorphisme borne : trois implementations, trois aretes ; non borne (cinq) :
/// non resolu, sans aucune arete vers les implementations (R-341, P-02).
#[test]
fn bounded_polymorphism_is_resolved_and_unbounded_is_not() {
    let bounded = method("Calls", "bounded", "(Lfixtures/Shape;)D");
    let graph = build(&bounded);
    assert_eq!(
        resolved_from(&graph, &bounded),
        vec![
            (
                "fixtures/Circle.area()D".to_owned(),
                Resolution::Polymorphic
            ),
            (
                "fixtures/Square.area()D".to_owned(),
                Resolution::Polymorphic
            ),
            (
                "fixtures/Triangle.area()D".to_owned(),
                Resolution::Polymorphic
            ),
        ]
    );

    let unbounded = method("Calls", "unbounded", "(Lfixtures/Handler;)V");
    let graph = build(&unbounded);
    assert!(resolved_from(&graph, &unbounded).is_empty());
    assert_eq!(
        unresolved_from(&graph, &unbounded),
        vec![Unresolved::Megamorphic]
    );
    assert_eq!(graph.nodes().len(), 1, "aucune implementation explorée");
}

/// Methode par defaut heritee par la seule implementation chargee.
#[test]
fn default_methods_are_selected_through_interfaces() {
    let greet = method("Calls", "greet", "(Lfixtures/Greeter;)Ljava/lang/String;");
    assert_eq!(
        resolved_from(&build(&greet), &greet),
        vec![(
            "fixtures/Greeter.greet()Ljava/lang/String;".to_owned(),
            Resolution::Monomorphic
        )]
    );
}

/// T-161 : la methode qui implemente une lambda, et la cible d'une reference de
/// methode statique, sont des aretes resolues ; une concatenation ne l'est pas.
#[test]
fn t161_lambdas_resolve_to_their_implementation() {
    let lambda = method("Calls", "lambda", "()I");
    let graph = build(&lambda);
    let targets = resolved_from(&graph, &lambda);
    assert!(
        targets.iter().any(
            |(target, resolution)| target.starts_with("fixtures/Calls.lambda$")
                && *resolution == Resolution::Lambda
        ),
        "{targets:?}"
    );
    assert!(targets.contains(&(
        "java/lang/Integer.parseInt(Ljava/lang/String;)I".to_owned(),
        Resolution::Lambda
    )));
    // `Function.apply` deux fois, puis le deballage `Integer.intValue` deux fois :
    // classes du JDK, absentes de l'index (FM-15). Ordre et nombre verifies par javap.
    assert_eq!(
        unresolved_from(&graph, &lambda),
        vec![Unresolved::UnknownClass; 4]
    );
    // Le corps de la lambda appelle `helper` : l'exploration le suit.
    assert!(graph.find(&method("Calls", "helper", "(I)I")).is_some());

    let concat = method("Calls", "concat", "(Ljava/lang/String;I)Ljava/lang/String;");
    assert_eq!(
        unresolved_from(&build(&concat), &concat),
        vec![Unresolved::Dynamic]
    );
}

/// Une reference de methode virtuelle passe par la resolution virtuelle : resolue si
/// la hierarchie chargee le permet, non resolue sinon.
#[test]
fn virtual_method_references_go_through_virtual_resolution() {
    let refs = method("Calls", "methodReferences", "(Lfixtures/Dog;)I");
    let graph = build(&refs);
    assert_eq!(
        resolved_from(&graph, &refs),
        vec![(
            "fixtures/Dog.sound()Ljava/lang/String;".to_owned(),
            Resolution::Monomorphic
        )]
    );
    // `String::length`, puis `Function.apply` deux fois et `Integer.intValue` : JDK.
    assert_eq!(
        unresolved_from(&graph, &refs),
        vec![Unresolved::UnknownClass; 4]
    );
}

/// Zero faux resolu : la seule implementation chargee herite sa methode d'une classe
/// du JDK, absente de l'index. Rien n'est affirme.
#[test]
fn a_method_inherited_from_an_absent_class_is_not_resolved() {
    let run = method("Calls", "runJob", "(Lfixtures/Job;)V");
    let graph = build(&run);
    assert!(resolved_from(&graph, &run).is_empty());
    assert_eq!(
        unresolved_from(&graph, &run),
        vec![Unresolved::UnknownClass]
    );
}

/// Zero faux resolu : une interface du JDK, absente de l'index, pourrait fournir une
/// methode par defaut plus specifique que celle qu'on connait.
#[test]
fn a_default_method_next_to_an_absent_interface_is_not_resolved() {
    let label = method("Calls", "label", "(Lfixtures/Named;)Ljava/lang/String;");
    let graph = build(&label);
    assert!(resolved_from(&graph, &label).is_empty());
    assert_eq!(
        unresolved_from(&graph, &label),
        vec![Unresolved::UnknownClass]
    );
}

/// T-162 : la reflexion est non resolue, et signalee.
#[test]
fn t162_reflection_is_unresolved_and_flagged() {
    let reflective = method("Calls", "reflective", "()Ljava/lang/Object;");
    let graph = build(&reflective);
    assert!(graph.reflection());
    let reasons = unresolved_from(&graph, &reflective);
    assert_eq!(
        reasons
            .iter()
            .filter(|reason| **reason == Unresolved::Reflection)
            .count(),
        2,
        "Method.invoke et MethodHandle.invokeExact : {reasons:?}"
    );
    assert!(!build(&method("Calls", "direct", "()I")).reflection());
}

/// T-163 : recursion directe et mutuelle ; une chaine n'est pas recursive.
#[test]
fn t163_recursion_is_detected() {
    let factorial = method("Calls", "factorial", "(I)J");
    let graph = build(&factorial);
    assert!(graph.find(&factorial).unwrap().recursive);

    let even = method("Calls", "isEven", "(I)Z");
    let graph = build(&even);
    assert!(graph.find(&even).unwrap().recursive);
    assert!(
        graph
            .find(&method("Calls", "isOdd", "(I)Z"))
            .unwrap()
            .recursive
    );

    let chain = build(&method("Bounds", "c0", "()I"));
    assert!(chain.nodes().iter().all(|node| !node.recursive));
}

/// T-164 : la profondeur et le nombre de noeuds sont bornes, et le depassement est
/// signale (R-340).
#[test]
fn t164_bounds_are_respected() {
    let chain = build(&method("Bounds", "c0", "()I"));
    assert!(chain.truncated(), "vingt appels, profondeur 12");
    assert_eq!(chain.nodes().len(), 13, "c0 a c12");
    assert!(chain.nodes().iter().all(|node| node.depth <= 12));

    let fan = method("Bounds", "fan", "()I");
    let whole = build(&fan);
    assert!(!whole.truncated());
    assert_eq!(
        whole.nodes().len(),
        1 + 8 + 8,
        "racine, huit appelees, huit feuilles"
    );

    let small = Limits {
        max_nodes: 5,
        ..Limits::default()
    };
    let capped = CallGraph::build(&index(), &fan, small);
    assert!(capped.truncated());
    assert_eq!(capped.nodes().len(), 5);

    let shallow = Limits {
        max_call_depth: 1,
        ..Limits::default()
    };
    let one_level = CallGraph::build(&index(), &fan, shallow);
    assert!(one_level.truncated(), "les appelees ont des appels");
    assert_eq!(one_level.nodes().len(), 9);
}

/// FM-15 : une classe absente ne fait jamais echouer la construction. Un appel
/// statique vers elle reste resolu (lie statiquement) vers un noeud externe.
#[test]
fn absent_classes_never_break_the_build() {
    let outside = method("Bounds", "outside", "()Ljava/lang/String;");
    let graph = build(&outside);
    let target = MethodRef::new(
        "java/lang/System",
        "getProperty",
        "(Ljava/lang/String;)Ljava/lang/String;",
    );
    assert!(graph.find(&target).unwrap().external);

    let missing_root = MethodRef::new("absent/Classe", "run", "()V");
    let graph = build(&missing_root);
    assert_eq!(graph.nodes().len(), 1);
    assert!(graph.nodes()[0].external);
    assert!(!graph.truncated());

    let empty = CallGraph::build(
        &ClassIndex::new(),
        &method("Calls", "direct", "()I"),
        Limits::default(),
    );
    assert!(empty.nodes()[0].external);
}

/// Les aretes observees completent le graphe, sans doublon, et entrent dans la
/// detection des cycles (PARTIE 5.7, etape 6).
#[test]
fn observed_edges_complete_the_graph() {
    let unbounded = method("Calls", "unbounded", "(Lfixtures/Handler;)V");
    let mut graph = build(&unbounded);
    let observed = method("H3", "handle", "()V");
    assert!(graph.add_observed(&unbounded, &observed, Limits::default()));
    assert!(
        !graph.add_observed(&unbounded, &observed, Limits::default()),
        "deja connue"
    );
    assert!(graph
        .edges_from(&unbounded)
        .any(|edge| edge.kind == EdgeKind::DynamicObserved));

    assert!(!graph.find(&unbounded).unwrap().recursive);
    assert!(graph.add_observed(&observed, &unbounded, Limits::default()));
    assert!(graph.find(&unbounded).unwrap().recursive, "cycle observe");
}

/// Metriques de la fiche : noeuds, part non resolue, duree.
#[test]
fn metrics_describe_the_graph() {
    let lambda = build(&method("Calls", "lambda", "()I"));
    assert!(lambda.unresolved_calls() >= 2);
    let ratio = lambda.unresolved_ratio();
    assert!(ratio > 0.0 && ratio < 1.0, "{ratio}");

    let direct = build(&method("Calls", "secret", "()I"));
    assert_eq!(
        direct.unresolved_ratio(),
        0.0,
        "aucun site : rien de non resolu"
    );
    assert!(lambda.build_ns() > 0);
}
