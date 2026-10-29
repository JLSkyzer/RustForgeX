//! Diagnostic : que rapporterait de fermer les interfaces sur les classes connues, et
//! cette fermeture tiendrait-elle sur l'installation (ADR-051, ADR-055) ?
//!
//! Cahier des charges : PARTIE 5.7, R-341. Maturite : `EXPERIMENTAL` — outil de
//! decision, rien ne s'en sert en jeu.
//!
//! C-07 tient tout appel par interface pour ouvert : une classe cachee
//! (`Lookup.defineHiddenClass`) peut implementer n'importe quelle interface sans passer
//! par les transformateurs, donc sans etre vue (essai du 2026-10-18, Java 17.0.20.1).
//! Ce module mesure, sur les graphes des methodes de reference, combien de sites
//! « interface ouverte » n'auraient qu'une a quatre implementations connues (classes
//! concretes et sites de lambda), et qui, dans l'installation, peut definir des classes
//! cachees ou des implementations generees.

use std::collections::{BTreeMap, HashMap, HashSet};

use rfx_analyzer::hierarchy::{ClassIndex, Skeleton};
use rfx_analyzer::{EdgeKind, MethodRef, Unresolved};
use rfx_bytecode::{access, CallTarget, ClassFile};

use crate::callgraph_acceptance::build_like_the_game;
use crate::callgraph_cost::Corpus;

/// Appels qui definissent des classes cachees, anonymes, ordinaires ou des
/// implementations generees d'une interface. Les proxys et `defineClass` comptent :
/// leurs classes apparaissent a l'execution, apres les graphes, et un proxy
/// implemente l'interface que son appelant choisit (ADR-055, 2026-10-29).
const DEFINERS: [(&str, &str); 9] = [
    ("java/lang/ClassLoader", "defineClass"),
    ("java/security/SecureClassLoader", "defineClass"),
    ("java/lang/invoke/MethodHandles$Lookup", "defineClass"),
    ("java/lang/reflect/Proxy", "newProxyInstance"),
    ("java/lang/reflect/Proxy", "getProxyClass"),
    ("java/lang/invoke/MethodHandles$Lookup", "defineHiddenClass"),
    (
        "java/lang/invoke/MethodHandles$Lookup",
        "defineHiddenClassWithClassData",
    ),
    ("sun/misc/Unsafe", "defineAnonymousClass"),
    (
        "java/lang/invoke/MethodHandleProxies",
        "asInterfaceInstance",
    ),
];

const LAMBDA_FACTORY: &str = "java/lang/invoke/LambdaMetafactory";

/// Resultat.
#[derive(Debug, Default)]
pub struct Report {
    /// Sites « interface ouverte » des graphes, par nombre d'implementations connues :
    /// `0`, `1`, `2-4`, `>4`.
    pub open_sites: BTreeMap<&'static str, u64>,
    /// Appelants de [`DEFINERS`], par paquet de premier niveau, avec un exemple.
    pub definers: BTreeMap<String, (u64, String)>,
    /// Interfaces implementees par au moins une lambda de l'installation.
    pub lambda_interfaces: usize,
}

fn bucket(count: usize) -> &'static str {
    match count {
        0 => "0",
        1 => "1",
        2..=4 => "2-4",
        _ => ">4",
    }
}

fn package(name: &str) -> String {
    name.split('/').take(2).collect::<Vec<_>>().join("/")
}

/// Mesure sur le corpus, pour les graphes de racines `roots`.
#[must_use]
pub fn run(corpus: &Corpus, roots: &[MethodRef]) -> Report {
    let mut report = Report::default();
    let mut classes: HashMap<String, (Option<String>, Vec<String>, u16)> = HashMap::new();
    let mut lambdas: HashMap<String, usize> = HashMap::new();
    for bytes in corpus.classes.values() {
        let Ok(class) = ClassFile::parse(bytes) else {
            continue;
        };
        for method in &class.methods {
            let Ok(sites) = class.call_sites(method) else {
                continue;
            };
            for site in sites {
                match &site.target {
                    CallTarget::Dynamic {
                        descriptor,
                        bootstrap,
                        ..
                    } if bootstrap.member.owner == LAMBDA_FACTORY => {
                        if let Some(iface) = descriptor
                            .rsplit_once(")L")
                            .and_then(|(_, rest)| rest.strip_suffix(';'))
                        {
                            *lambdas.entry(iface.to_owned()).or_default() += 1;
                        }
                    }
                    CallTarget::Member(member)
                        if DEFINERS.contains(&(member.owner.as_str(), member.name.as_str())) =>
                    {
                        let entry = report
                            .definers
                            .entry(package(&class.name))
                            .or_insert((0, format!("{}.{}", class.name, method.name)));
                        entry.0 += 1;
                    }
                    _ => {}
                }
            }
        }
        classes.insert(
            class.name,
            (class.super_name, class.interfaces, class.access),
        );
    }
    report.lambda_interfaces = lambdas.len();

    // Implementations concretes de chaque interface, transitivement.
    let mut implementors: HashMap<String, usize> = HashMap::new();
    for (name, (_, _, flags)) in &classes {
        if flags & (access::INTERFACE | access::ABSTRACT) != 0 {
            continue;
        }
        let mut seen: HashSet<String> = HashSet::new();
        let mut pending: Vec<String> = vec![name.clone()];
        while let Some(current) = pending.pop() {
            let Some((parent, ifaces, _)) = classes.get(&current) else {
                continue;
            };
            pending.extend(parent.iter().cloned());
            for iface in ifaces {
                if seen.insert(iface.clone()) {
                    pending.push(iface.clone());
                }
            }
        }
        for iface in seen {
            *implementors.entry(iface).or_default() += 1;
        }
    }

    let mut index = ClassIndex::new();
    for (name, (parent, _, flags)) in &classes {
        index.declare(
            name,
            Skeleton {
                super_name: parent.clone(),
                access: *flags,
            },
        );
    }
    for root in roots {
        let graph = build_like_the_game(&mut index, corpus, root);
        for edge in graph.edges() {
            if edge.kind != EdgeKind::VirtualUnresolved(Unresolved::OpenInterface) {
                continue;
            }
            let Some(reference) = &edge.reference else {
                continue;
            };
            let known = implementors.get(&reference.owner).copied().unwrap_or(0)
                + lambdas.get(&reference.owner).copied().unwrap_or(0);
            *report.open_sites.entry(bucket(known)).or_default() += 1;
        }
    }
    report
}
