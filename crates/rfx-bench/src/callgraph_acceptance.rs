//! Acceptance de C-07 : 200 methodes de reference, taux de resolution et sites
//! resolus a confronter a un oracle (ADR-053).
//!
//! Cahier des charges : PARTIE 5.7, acceptance — « sur un corpus de 200 methodes de
//! reference, taux de resolution superieur a 80 % et zero faux resolu ». Maturite :
//! `STABLE`.
//!
//! Ce module produit la moitie native de l'acceptance : il construit le graphe de
//! chaque methode de reference comme le fait le jeu (hierarchie complete, bytecode a la
//! demande, tours bornes), mesure le taux de resolution, et ecrit chaque site resolu de
//! tous les graphes dans un fichier que l'oracle Java
//! (`benchmarks/CallGraphOracle.java`) confronte a la JVM. Format, une ligne par site :
//! appelant, position, nature, membre, resolution, cibles separees par `|` (un
//! descripteur contient des `;`), separes par des tabulations.
//!
//! Le taux exige est mesure sur les sites d'appel des methodes de reference
//! elles-memes ; le taux sur les graphes entiers est rapporte a cote. La verification
//! des faux resolus porte, elle, sur tous les sites resolus de tous les graphes : plus
//! de sites, plus de chances d'en trouver un faux.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;

use rfx_analyzer::hierarchy::Skeleton;
use rfx_analyzer::{CallGraph, ClassIndex, EdgeKind, Limits, MethodRef, Resolution};
use rfx_bytecode::{access, CallTarget, ClassFile, InvokeKind, MemberRef};
use rfx_core::analysis::{MAX_BYTECODE_CLASSES, MAX_ROUNDS};
use serde::Serialize;

use crate::callgraph_cost::Corpus;

/// Taille du corpus de reference.
pub const REFERENCE_METHODS: usize = 200;

/// Methode pont generee par le compilateur (JVMS §4.6, `ACC_BRIDGE`).
const ACC_BRIDGE: u16 = 0x0040;

/// Paquets d'ou sont tirees les methodes de reference : le serveur et le monde.
const REFERENCE_PACKAGES: [&str; 2] = ["net/minecraft/server/", "net/minecraft/world/"];

/// Les `count` methodes de reference : parmi les methodes concretes, ni synthetiques
/// ni ponts ni initialiseurs de classe, qui appellent au moins une methode, dans les
/// paquets de [`REFERENCE_PACKAGES`], triees, un echantillon regulier.
#[must_use]
pub fn reference_methods(corpus: &Corpus, count: usize) -> Vec<MethodRef> {
    let mut names: Vec<&String> = corpus
        .classes
        .keys()
        .filter(|name| REFERENCE_PACKAGES.iter().any(|p| name.starts_with(p)))
        .collect();
    names.sort();
    let mut eligible = Vec::new();
    for name in names {
        let Ok(class) = ClassFile::parse(&corpus.classes[name]) else {
            continue;
        };
        let mut methods: Vec<_> = class
            .methods
            .iter()
            .filter(|m| {
                m.code.is_some()
                    && m.name != "<clinit>"
                    && m.access & (access::SYNTHETIC | ACC_BRIDGE) == 0
                    && class.call_sites(m).is_ok_and(|sites| !sites.is_empty())
            })
            .map(|m| MethodRef::new(&class.name, &m.name, &m.descriptor))
            .collect();
        methods.sort();
        eligible.extend(methods);
    }
    let take = count.min(eligible.len());
    (0..take)
        .map(|i| eligible[i * eligible.len() / take].clone())
        .collect()
}

/// Un site d'appel d'un graphe.
#[derive(Debug, Clone)]
pub struct Site {
    /// Methode appelante.
    pub from: MethodRef,
    /// Position du site dans son code.
    pub offset: u32,
    /// Resolution, ou `None` si le site n'est pas resolu.
    pub resolution: Option<Resolution>,
    /// Cibles, triees.
    pub targets: Vec<MethodRef>,
}

/// Bilan de l'acceptance cote natif.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    /// Methodes de reference.
    pub methods: usize,
    /// Sites des methodes de reference.
    pub reference_sites: u64,
    /// Dont resolus.
    pub reference_resolved: u64,
    /// Sites de tous les graphes, sans doublon.
    pub graph_sites: u64,
    /// Dont resolus.
    pub graph_resolved: u64,
    /// Graphes arretes par une borne.
    pub truncated_graphs: u64,
    /// Motifs des sites non resolus des methodes de reference.
    pub reference_unresolved_by_reason: BTreeMap<String, u64>,
    /// Lignes ecrites pour l'oracle : sites resolus de tous les graphes.
    pub oracle_lines: u64,
    /// Sites resolus que le bytecode relu n'a pas permis de decrire : non verifies, donc
    /// a zero pour que l'acceptance vaille.
    pub undescribed: u64,
}

/// Construit le graphe de chaque methode de reference et rend le bilan et les lignes
/// destinees a l'oracle.
#[must_use]
pub fn run(corpus: &Corpus, roots: &[MethodRef]) -> (Report, String) {
    let mut index = ClassIndex::new();
    for bytes in corpus.classes.values() {
        if let Ok(class) = ClassFile::parse(bytes) {
            index.declare(
                &class.name,
                Skeleton {
                    super_name: class.super_name,
                    access: class.access,
                },
            );
        }
    }
    let mut report = Report {
        methods: roots.len(),
        ..Report::default()
    };
    let mut seen: HashSet<(MethodRef, u32)> = HashSet::new();
    let mut lines = String::new();
    for root in roots {
        let graph = build_like_the_game(&mut index, corpus, root);
        if graph.truncated() {
            report.truncated_graphs += 1;
        }
        for site in sites(&graph) {
            if &site.from == root {
                report.reference_sites += 1;
                if site.resolution.is_some() {
                    report.reference_resolved += 1;
                }
            }
            if !seen.insert((site.from.clone(), site.offset)) {
                continue;
            }
            report.graph_sites += 1;
            let Some(resolution) = site.resolution else {
                continue;
            };
            report.graph_resolved += 1;
            if let Some((kind, member)) = describe(&index, &site) {
                let targets: Vec<String> = site.targets.iter().map(ToString::to_string).collect();
                let _ = writeln!(
                    lines,
                    "{}\t{}\t{}\t{}\t{:?}\t{}",
                    site.from,
                    site.offset,
                    kind,
                    member,
                    resolution,
                    targets.join("|")
                );
                report.oracle_lines += 1;
            } else {
                report.undescribed += 1;
            }
        }
        for (reason, count) in unresolved_reasons(&graph, root) {
            *report
                .reference_unresolved_by_reason
                .entry(reason)
                .or_default() += count;
        }
    }
    (report, lines)
}

/// Meme pilotage que `rfx_core::analysis::Analysis` : purge au-dela de
/// [`MAX_BYTECODE_CLASSES`] au debut d'une racine, puis des tours tant que des classes
/// manquent, [`MAX_ROUNDS`] au plus ; une classe absente ou illisible est declaree
/// absente.
fn build_like_the_game(index: &mut ClassIndex, corpus: &Corpus, root: &MethodRef) -> CallGraph {
    if index.len() > MAX_BYTECODE_CLASSES {
        index.clear_bytecode();
    }
    let mut round = 0;
    loop {
        round += 1;
        let graph = CallGraph::build(index, root, Limits::default());
        let requests = index.take_requests();
        if requests.is_empty() || round >= MAX_ROUNDS {
            return graph;
        }
        for name in requests {
            let class = corpus
                .classes
                .get(&name)
                .and_then(|bytes| ClassFile::parse(bytes).ok())
                .filter(|class| class.name == name);
            match class {
                Some(class) => index.insert(class),
                None => index.declare_absent(&name),
            }
        }
    }
}

/// Sites d'un graphe : les aretes regroupees par appelant et position.
fn sites(graph: &CallGraph) -> Vec<Site> {
    let mut grouped: BTreeMap<(usize, u32), Site> = BTreeMap::new();
    for edge in graph.edges() {
        let Some(offset) = edge.offset else {
            continue;
        };
        let site = grouped.entry((edge.from, offset)).or_insert_with(|| Site {
            from: graph.nodes()[edge.from].method.clone(),
            offset,
            resolution: None,
            targets: Vec::new(),
        });
        if let (EdgeKind::Resolved(resolution), Some(to)) = (edge.kind, edge.to) {
            site.resolution = Some(resolution);
            site.targets.push(graph.nodes()[to].method.clone());
        }
    }
    let mut sites: Vec<Site> = grouped.into_values().collect();
    for site in &mut sites {
        site.targets.sort();
        site.targets.dedup();
    }
    sites
}

fn unresolved_reasons(graph: &CallGraph, root: &MethodRef) -> BTreeMap<String, u64> {
    let mut reasons = BTreeMap::new();
    for edge in graph.edges() {
        if graph.nodes()[edge.from].method != *root {
            continue;
        }
        if let EdgeKind::VirtualUnresolved(reason) = edge.kind {
            *reasons.entry(format!("{reason:?}")).or_default() += 1;
        }
    }
    reasons
}

/// Nature du site et membre que l'oracle doit resoudre, relus dans le bytecode de
/// l'appelant : pour une lambda, le handle de la methode d'implementation.
fn describe(index: &ClassIndex, site: &Site) -> Option<(String, String)> {
    let class = index.get(&site.from.owner)?;
    let method = class.method(&site.from.name, &site.from.descriptor)?;
    let call = class
        .call_sites(method)
        .ok()?
        .into_iter()
        .find(|call| call.offset == site.offset)?;
    let member = |m: &MemberRef| format!("{}.{}{}", m.owner, m.name, m.descriptor);
    match (&call.kind, &call.target) {
        (
            InvokeKind::Dynamic,
            CallTarget::Dynamic {
                handle_arguments, ..
            },
        ) => {
            let handle = handle_arguments.first()?;
            Some((format!("handle{}", handle.kind), member(&handle.member)))
        }
        (kind, CallTarget::Member(m)) => {
            let kind = match kind {
                InvokeKind::Virtual => "virtual",
                InvokeKind::Special => "special",
                InvokeKind::Static => "static",
                InvokeKind::Interface => "interface",
                InvokeKind::Dynamic => return None,
            };
            Some((kind.to_owned(), member(m)))
        }
        _ => None,
    }
}
