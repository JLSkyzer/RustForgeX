//! T-204 : corpus annote de 150 methodes pour C-11 (ADR-071).
//!
//! Chaque methode du corpus porte sa verite (`@W.<Domaine>`), deduite de la memoire
//! reellement ecrite. Le test echoue sur toute sous-approximation d'ecriture, tout faux
//! `EXACT`, toute methode non annotee, et un ratio `EXACT` sous 60 % (PARTIE 5.11).

use std::path::{Path, PathBuf};

use rfx_analyzer::dataflow::{workload_effects, DataflowLimits, DomainTable};
use rfx_analyzer::{CallGraph, ClassIndex, Limits, MethodRef};
use rfx_bytecode::ClassFile;
use rfx_model::{AccessSet, Domain, Domains, Precision};

/// Familles du corpus et leur taille fixee d'avance (ADR-071, decision 1).
const FAMILIES: [(&str, usize); 7] = [
    ("corpus/Pure", 25),
    ("corpus/ModState", 25),
    ("corpus/Game", 25),
    ("corpus/Arrays", 20),
    ("corpus/Calls", 25),
    ("corpus/Poly", 15),
    ("corpus/Hard", 15),
];

const ANNOTATION_PREFIX: &str = "Lcorpus/W$";
const ACC_SYNTHETIC: u16 = 0x1000;

fn classes_under(dir: &Path, out: &mut Vec<ClassFile>) {
    for entry in std::fs::read_dir(dir).expect("repertoire de classes") {
        let path = entry.expect("entree").path();
        if path.is_dir() {
            classes_under(&path, out);
        } else if path.extension().is_some_and(|e| e == "class") {
            let bytes = std::fs::read(&path).expect("lecture");
            out.push(ClassFile::parse(&bytes).expect("lisible"));
        }
    }
}

fn corpus_classes() -> Vec<ClassFile> {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../rfx-bytecode/tests/fixtures/classes");
    let mut classes = Vec::new();
    classes_under(&root.join("corpus"), &mut classes);
    classes_under(&root.join("net"), &mut classes);
    classes
}

/// Verite d'une methode, depuis ses annotations ; `None` si elle n'en porte aucune.
fn truth(annotations: &[String]) -> Option<Domains> {
    let mut domains = Domains::EMPTY;
    let mut annotated = false;
    for annotation in annotations {
        let Some(name) = annotation
            .strip_prefix(ANNOTATION_PREFIX)
            .and_then(|rest| rest.strip_suffix(';'))
        else {
            continue;
        };
        annotated = true;
        domains = match name {
            "Nothing" => domains,
            "EntityState" => domains.with(Domain::EntityState),
            "EntityInventory" => domains.with(Domain::EntityInventory),
            "BlockEntityState" => domains.with(Domain::BlockEntityState),
            "LevelGlobals" => domains.with(Domain::LevelGlobals),
            "ModStatics" => domains.with(Domain::ModStatics),
            "Unknown" => domains.with(Domain::Unknown),
            other => panic!("annotation de verite inconnue : {other}"),
        };
    }
    annotated.then_some(domains)
}

/// `true` si l'ensemble calcule couvre toute la verite (ADR-071, decision 3).
fn covers(writes: &AccessSet, truth: Domains) -> bool {
    if writes.precision == Precision::Unknown || writes.domains.contains(Domain::Unknown) {
        return true;
    }
    Domain::ALL
        .iter()
        .filter(|d| truth.contains(**d))
        .all(|d| writes.domains.contains(*d))
}

#[derive(Default)]
struct Tally {
    methods: usize,
    exact: usize,
    exact_unanchored: usize,
    under: Vec<String>,
    false_exact: Vec<String>,
}

#[test]
fn the_annotated_corpus_has_no_write_under_approximation() {
    let classes = corpus_classes();
    let index = ClassIndex::from_classes(classes.iter().cloned());
    let table = DomainTable::new();

    let mut total = Tally::default();
    let mut report = Vec::new();
    for (family, expected) in FAMILIES {
        let class = classes
            .iter()
            .find(|c| c.name == family)
            .unwrap_or_else(|| panic!("{family} absente"));
        let mut tally = Tally::default();
        for method in &class.methods {
            let root = format!("{family}.{}{}", method.name, method.descriptor);
            let Some(expected_writes) = truth(&method.annotations) else {
                let helper = method.name.starts_with('<')
                    || method.is_private()
                    || method.access & ACC_SYNTHETIC != 0;
                assert!(helper, "{root} n'est pas annotee");
                continue;
            };
            let graph = CallGraph::build(
                &index,
                &MethodRef::new(family, &method.name, &method.descriptor),
                Limits::default(),
            );
            let effects = workload_effects(&graph, &index, &table, &DataflowLimits::DEFAULT);
            let writes = effects.writes;
            tally.methods += 1;
            if std::env::var_os("RFX_CORPUS_VERBOSE").is_some() {
                println!(
                    "{root:<70} vrai {expected_writes:?} calcule {:?} {:?}",
                    writes.domains, writes.precision
                );
            }
            if !covers(&writes, expected_writes) {
                tally.under.push(format!(
                    "{root} : vrai {expected_writes:?}, calcule {writes:?}"
                ));
            }
            if writes.precision == Precision::Exact {
                if writes.domains == expected_writes {
                    tally.exact += 1;
                    if expected_writes.contains(Domain::Unknown) {
                        tally.exact_unanchored += 1;
                    }
                } else {
                    tally.false_exact.push(format!(
                        "{root} : vrai {expected_writes:?}, calcule {writes:?}"
                    ));
                }
            }
        }
        assert_eq!(
            tally.methods, expected,
            "{family} : taille fixee par l'ADR-071"
        );
        report.push(format!(
            "{family:<16} {:>3} methodes, {:>3} EXACT, {} sous-approximee(s), {} faux EXACT",
            tally.methods,
            tally.exact,
            tally.under.len(),
            tally.false_exact.len()
        ));
        total.methods += tally.methods;
        total.exact += tally.exact;
        total.exact_unanchored += tally.exact_unanchored;
        total.under.extend(tally.under);
        total.false_exact.extend(tally.false_exact);
    }
    for line in &report {
        println!("{line}");
    }
    #[allow(clippy::cast_precision_loss)]
    let ratio = total.exact as f64 / total.methods as f64;
    println!(
        "total : {} methodes, {} EXACT ({:.1} %)",
        total.methods,
        total.exact,
        ratio * 100.0
    );
    // Un objet sans ancre est rendu `Unknown` : exact sur l'objet ecrit, pas sur son
    // domaine de jeu. Compte a part pour ne pas gonfler le ratio sans le dire.
    #[allow(clippy::cast_precision_loss)]
    let anchored = (total.exact - total.exact_unanchored) as f64 / total.methods as f64;
    println!(
        "dont {} EXACT sur un objet sans ancre ; hors ceux-la : {:.1} %",
        total.exact_unanchored,
        anchored * 100.0
    );
    for line in total.under.iter().chain(&total.false_exact) {
        println!("  {line}");
    }

    assert_eq!(total.methods, 150);
    assert!(
        total.under.is_empty(),
        "sous-approximations : {:#?}",
        total.under
    );
    assert!(
        total.false_exact.is_empty(),
        "faux EXACT : {:#?}",
        total.false_exact
    );
    assert!(ratio >= 0.60, "EXACT : {:.1} % < 60 %", ratio * 100.0);
}

/// R-380 : un graphe tronque laisse des appels non analyses ; les ecritures deviennent
/// inconnues, meme quand la partie analysee n'ecrit qu'un domaine connu. Aucune
/// methode du corpus n'atteint les bornes : ce cas se provoque en les abaissant.
#[test]
fn a_truncated_graph_makes_writes_unknown() {
    let classes = corpus_classes();
    let index = ClassIndex::from_classes(classes.iter().cloned());
    let root = MethodRef::new(
        "corpus/Calls",
        "deepChain",
        "(Lnet/minecraft/world/entity/Entity;)V",
    );
    let complete = CallGraph::build(&index, &root, Limits::default());
    let effects = workload_effects(
        &complete,
        &index,
        &DomainTable::new(),
        &DataflowLimits::DEFAULT,
    );
    assert_eq!(
        effects.writes.precision,
        Precision::Exact,
        "temoin : graphe complet"
    );

    let bounded = Limits {
        max_nodes: 2,
        ..Limits::default()
    };
    let truncated = CallGraph::build(&index, &root, bounded);
    let effects = workload_effects(
        &truncated,
        &index,
        &DomainTable::new(),
        &DataflowLimits::DEFAULT,
    );
    assert!(
        effects.causes.truncated || effects.causes.unexplored_sites > 0,
        "{:?}",
        effects.causes
    );
    assert!(
        effects.writes.domains.contains(Domain::Unknown),
        "{:?}",
        effects.writes
    );
    assert_eq!(effects.writes.precision, Precision::Unknown);
}
