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

/// Polymorphisme borne par une classe : trois sous-classes, trois aretes ; non borne
/// (cinq) : non resolu, sans aucune arete vers les sous-classes (R-341, P-02).
#[test]
fn bounded_polymorphism_is_resolved_and_unbounded_is_not() {
    let vehicle = method("Calls", "vehicle", "(Lfixtures/Vehicle;)I");
    let graph = build(&vehicle);
    assert_eq!(
        resolved_from(&graph, &vehicle),
        vec![
            (
                "fixtures/Bike.wheels()I".to_owned(),
                Resolution::Polymorphic
            ),
            ("fixtures/Car.wheels()I".to_owned(), Resolution::Polymorphic),
            (
                "fixtures/Truck.wheels()I".to_owned(),
                Resolution::Polymorphic
            ),
        ]
    );

    let tool = method("Calls", "tool", "(Lfixtures/Tool;)V");
    let graph = build(&tool);
    assert!(resolved_from(&graph, &tool).is_empty());
    assert_eq!(
        unresolved_from(&graph, &tool),
        vec![Unresolved::Megamorphic]
    );
    assert_eq!(graph.nodes().len(), 1, "aucune sous-classe explorée");
}

/// Zero faux resolu : une interface peut etre implementee par des lambdas, des `Proxy`
/// ou des classes generees que l'index ne verra jamais. Meme avec trois
/// implementations connues, l'appel n'est pas resolu (ADR-051).
#[test]
fn calls_through_an_interface_are_never_resolved() {
    for (name, descriptor) in [
        ("bounded", "(Lfixtures/Shape;)D"),
        ("unbounded", "(Lfixtures/Handler;)V"),
        ("greet", "(Lfixtures/Greeter;)Ljava/lang/String;"),
        ("label", "(Lfixtures/Named;)Ljava/lang/String;"),
        ("runJob", "(Lfixtures/Job;)V"),
    ] {
        let root = method("Calls", name, descriptor);
        let graph = build(&root);
        assert!(resolved_from(&graph, &root).is_empty(), "{name}");
        assert_eq!(
            unresolved_from(&graph, &root),
            vec![Unresolved::OpenInterface],
            "{name}"
        );
    }
}

/// Methode par defaut heritee par la seule sous-classe chargee, appelee par une classe.
#[test]
fn default_methods_are_selected_through_interfaces() {
    let speak = method("Calls", "speak", "(Lfixtures/Speaker;)Ljava/lang/String;");
    assert_eq!(
        resolved_from(&build(&speak), &speak),
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

/// Zero faux resolu : la seule sous-classe chargee herite sa methode d'une classe du
/// JDK, absente de l'index. Rien n'est affirme.
#[test]
fn a_method_inherited_from_an_absent_class_is_not_resolved() {
    let work = method("Calls", "work", "(Lfixtures/Worker;)V");
    let graph = build(&work);
    assert!(resolved_from(&graph, &work).is_empty());
    assert_eq!(
        unresolved_from(&graph, &work),
        vec![Unresolved::UnknownClass]
    );
}

/// Zero faux resolu : une interface du JDK, absente de l'index, pourrait fournir une
/// methode par defaut plus specifique que celle qu'on connait.
#[test]
fn a_default_method_next_to_an_absent_interface_is_not_resolved() {
    let label = method(
        "Calls",
        "labelled",
        "(Lfixtures/Labelled;)Ljava/lang/String;",
    );
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

/// Par `java/lang/Object`, une methode redefinissable est megamorphe, sans que le
/// bytecode d'`Object` ni ses sous-classes soient necessaires (ADR-052).
#[test]
fn an_overridable_call_through_object_is_megamorphic() {
    let root = method("Calls", "objectHash", "(Ljava/lang/Object;)I");
    let graph = build(&root);
    assert_eq!(
        unresolved_from(&graph, &root),
        vec![Unresolved::Megamorphic]
    );
}

/// Deux hierarchies de classes dans le meme graphe : les sous-types memorises pendant
/// la construction (ADR-052) restent ceux de chaque classe.
#[test]
fn each_call_enumerates_the_subtypes_of_its_own_class() {
    let root = method(
        "Calls",
        "twoHierarchies",
        "(Lfixtures/Vehicle;Lfixtures/Tool;)I",
    );
    let graph = build(&root);
    let tool = method("Calls", "tool", "(Lfixtures/Tool;)V");
    let vehicle = method("Calls", "vehicle", "(Lfixtures/Vehicle;)I");
    assert_eq!(
        unresolved_from(&graph, &tool),
        vec![Unresolved::Megamorphic],
        "cinq outils"
    );
    let targets: Vec<String> = resolved_from(&graph, &vehicle)
        .into_iter()
        .map(|(target, _)| target)
        .collect();
    assert_eq!(
        targets,
        vec![
            "fixtures/Bike.wheels()I",
            "fixtures/Car.wheels()I",
            "fixtures/Truck.wheels()I"
        ],
        "les vehicules, pas les outils enumeres juste avant"
    );
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
