//! Index paresseux (ADR-051) : la hierarchie d'abord, le bytecode a la demande.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rfx_analyzer::callgraph::MAX_REQUESTS_PER_SITE;
use rfx_analyzer::hierarchy::Skeleton;
use rfx_analyzer::{CallGraph, ClassIndex, EdgeKind, Limits, MethodRef, Unresolved};
use rfx_bytecode::{access, ClassFile};

fn fixture_bytes() -> BTreeMap<String, Vec<u8>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rfx-bytecode/tests/fixtures/classes/fixtures");
    std::fs::read_dir(&dir)
        .expect("classes de test")
        .map(|entry| entry.expect("entree").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "class"))
        .map(|path| {
            let bytes = std::fs::read(&path).expect("lecture");
            let name = ClassFile::parse(&bytes).expect("lisible").name;
            (name, bytes)
        })
        .collect()
}

fn parse(bytes: &[u8]) -> ClassFile {
    ClassFile::parse(bytes).expect("lisible")
}

/// Index complet : tout le bytecode, d'emblee.
fn eager(bytes: &BTreeMap<String, Vec<u8>>) -> ClassIndex {
    ClassIndex::from_classes(bytes.values().map(|b| parse(b)))
}

/// Index paresseux : la hierarchie de toutes les classes, aucun bytecode.
fn lazy(bytes: &BTreeMap<String, Vec<u8>>) -> ClassIndex {
    let mut index = ClassIndex::new();
    for b in bytes.values() {
        let class = parse(b);
        index.declare(
            &class.name,
            Skeleton {
                super_name: class.super_name.clone(),
                access: class.access,
            },
        );
    }
    index
}

/// Construit en fournissant ce qui est demande, jusqu'a ce que plus rien ne le soit.
/// Une classe sans bytecode est declaree absente. Rend le graphe et le nombre de tours.
fn build_lazily(
    index: &mut ClassIndex,
    bytes: &BTreeMap<String, Vec<u8>>,
    root: &MethodRef,
) -> (CallGraph, usize) {
    for round in 0..64 {
        let graph = CallGraph::build(index, root, Limits::default());
        let requests = index.take_requests();
        if requests.is_empty() {
            return (graph, round);
        }
        for name in requests {
            match bytes.get(&name) {
                Some(b) => index.insert(parse(b)),
                None => index.declare_absent(&name),
            }
        }
    }
    panic!("la construction ne converge pas");
}

/// Forme comparable d'un graphe : `(appelant, appele, attribut)`, triee.
fn shape(graph: &CallGraph) -> Vec<(String, Option<String>, String)> {
    let mut edges: Vec<_> = graph
        .edges()
        .iter()
        .map(|edge| {
            (
                graph.nodes()[edge.from].method.to_string(),
                edge.to.map(|to| graph.nodes()[to].method.to_string()),
                format!("{:?}", edge.kind),
            )
        })
        .collect();
    edges.sort();
    edges
}

fn method(owner: &str, name: &str, descriptor: &str) -> MethodRef {
    MethodRef::new(&format!("fixtures/{owner}"), name, descriptor)
}

/// A connaissance egale, le graphe construit a la demande est celui de l'index complet.
#[test]
fn lazy_and_eager_graphs_agree() {
    let bytes = fixture_bytes();
    let roots = [
        method("Calls", "vehicle", "(Lfixtures/Vehicle;)I"),
        method("Calls", "tool", "(Lfixtures/Tool;)V"),
        method(
            "Calls",
            "monomorphic",
            "(Lfixtures/Animal;)Ljava/lang/String;",
        ),
        method("Calls", "direct", "()I"),
        method("Calls", "speak", "(Lfixtures/Speaker;)Ljava/lang/String;"),
        method("Calls", "lambda", "()I"),
        method("Calls", "isEven", "(I)Z"),
        method("Bounds", "fan", "()I"),
        method("Calls", "inherited", "(Lfixtures/Top;)I"),
    ];
    for root in roots {
        let expected = CallGraph::build(&eager(&bytes), &root, Limits::default());
        let mut index = lazy(&bytes);
        let (graph, rounds) = build_lazily(&mut index, &bytes, &root);
        assert_eq!(shape(&graph), shape(&expected), "{root}");
        assert!(rounds >= 1, "{root} : rien n'a ete demande");
        assert!(
            index.len() < bytes.len(),
            "{root} : seul le necessaire est fourni"
        );
    }
}

/// La methode executee est dans une classe intermediaire, demandee en remontant : la
/// resolution aboutit une fois cette classe fournie.
#[test]
fn an_intermediate_superclass_is_requested_then_resolves() {
    let bytes = fixture_bytes();
    let mut index = lazy(&bytes);
    let root = method("Calls", "inherited", "(Lfixtures/Top;)I");
    let (graph, rounds) = build_lazily(&mut index, &bytes, &root);
    assert!(
        rounds >= 2,
        "proprietaire, receveur, puis classe intermediaire"
    );
    let targets: Vec<(Option<String>, EdgeKind)> = graph
        .edges_from(&root)
        .map(|edge| {
            (
                edge.to.map(|to| graph.nodes()[to].method.to_string()),
                edge.kind,
            )
        })
        .collect();
    assert_eq!(
        targets,
        vec![(
            Some("fixtures/Middle.level()I".to_owned()),
            EdgeKind::Resolved(rfx_analyzer::Resolution::Monomorphic)
        )]
    );
}

/// Zero faux resolu : une sous-classe concrete dont le bytecode ne sera jamais fourni
/// (cible de mixin) rend l'appel non resolu, et non « polymorphe a deux cibles ».
#[test]
fn an_absent_concrete_subclass_makes_the_call_unresolved() {
    let bytes = fixture_bytes();
    let mut index = lazy(&bytes);
    index.declare_absent("fixtures/Truck");
    let root = method("Calls", "vehicle", "(Lfixtures/Vehicle;)I");
    let (graph, _) = build_lazily(&mut index, &bytes, &root);
    let kinds: Vec<EdgeKind> = graph.edges_from(&root).map(|edge| edge.kind).collect();
    assert_eq!(
        kinds,
        vec![EdgeKind::VirtualUnresolved(Unresolved::UnknownClass)]
    );
}

fn declare_fakes(index: &mut ClassIndex, parent: &str, count: usize) {
    for n in 0..count {
        index.declare(
            &format!("fixtures/Fake{n:02}"),
            Skeleton {
                super_name: Some(format!("fixtures/{parent}")),
                access: access::FINAL,
            },
        );
    }
}

/// Une classe aux nombreuses sous-classes ne fait pas demander tout leur bytecode d'un
/// coup : quelques-unes par site et par tour.
#[test]
fn requests_per_site_are_capped() {
    let bytes = fixture_bytes();
    let mut index = eager(&bytes);
    for b in bytes.values() {
        let class = parse(b);
        index.declare(
            &class.name,
            Skeleton {
                super_name: class.super_name.clone(),
                access: class.access,
            },
        );
    }
    declare_fakes(&mut index, "Vehicle", 50);
    let root = method("Calls", "vehicle", "(Lfixtures/Vehicle;)I");
    let graph = CallGraph::build(&index, &root, Limits::default());
    let fakes = index
        .take_requests()
        .into_iter()
        .filter(|name| name.starts_with("fixtures/Fake"))
        .count();
    assert_eq!(fakes, MAX_REQUESTS_PER_SITE);
    assert_eq!(
        graph
            .edges_from(&root)
            .map(|edge| edge.kind)
            .collect::<Vec<_>>(),
        vec![EdgeKind::VirtualUnresolved(Unresolved::UnknownClass)],
        "rien n'est affirme tant que des sous-classes manquent"
    );
}

/// Des sous-classes encore inconnues ne peuvent qu'ajouter des cibles : au-dela de
/// quatre cibles connues, la conclusion est sure.
#[test]
fn megamorphism_is_concluded_before_every_subclass_is_known() {
    let bytes = fixture_bytes();
    let mut index = eager(&bytes);
    for b in bytes.values() {
        let class = parse(b);
        index.declare(
            &class.name,
            Skeleton {
                super_name: class.super_name.clone(),
                access: class.access,
            },
        );
    }
    declare_fakes(&mut index, "Tool", 50);
    let root = method("Calls", "tool", "(Lfixtures/Tool;)V");
    let graph = CallGraph::build(&index, &root, Limits::default());
    assert_eq!(
        graph
            .edges_from(&root)
            .map(|edge| edge.kind)
            .collect::<Vec<_>>(),
        vec![EdgeKind::VirtualUnresolved(Unresolved::Megamorphic)]
    );
}

/// Retirer le bytecode pour la memoire garde la hierarchie : les sous-classes restent
/// connues, et seront demandees de nouveau.
#[test]
fn clearing_bytecode_keeps_the_hierarchy() {
    let bytes = fixture_bytes();
    let mut index = lazy(&bytes);
    let root = method("Calls", "vehicle", "(Lfixtures/Vehicle;)I");
    let (before, _) = build_lazily(&mut index, &bytes, &root);
    index.clear_bytecode();
    assert!(index.is_empty());
    assert_eq!(
        index.concrete_subtypes("fixtures/Vehicle").unwrap(),
        vec!["fixtures/Bike", "fixtures/Car", "fixtures/Truck"]
    );
    let (after, rounds) = build_lazily(&mut index, &bytes, &root);
    assert!(rounds >= 1);
    assert_eq!(shape(&after), shape(&before));
}
