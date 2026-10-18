//! Effets d'une unite de travail : sa methode racine et tout ce qu'elle appelle, sur le
//! graphe d'appel de C-07 (ADR-055, phase interprocedurale ; R-380, R-381).
//!
//! Chaque methode du graphe recoit un resume ([`Summary`]). Les sites d'appel prennent
//! les cibles que C-07 a resolues, et seulement elles :
//!
//! - cibles resolues : union de leurs resumes ;
//! - lambda (`LambdaMetafactory`) : la creation n'a pas d'effet, les valeurs capturees
//!   s'echappent dans l'objet cree ; le corps de la lambda s'executera plus tard, par un
//!   appel d'interface, qui sera lui-meme non resolu ;
//! - concatenation de chaines (`StringConcatFactory`) dont tous les arguments sont
//!   primitifs ou `String` : aucun effet, aucun `toString` arbitraire n'est appele ;
//! - `java/lang/Object.<init>()V` : aucun effet ;
//! - tout le reste — site non resolu, site non explore (borne de profondeur), cible
//!   sans code ou non analysable — est inconnu (R-380).
//!
//! Les resumes partent de « aucun effet » et ne font que croitre jusqu'au point fixe :
//! une recursion converge vers ses effets reels. Faute de convergence sous la borne de
//! passages, l'unite est inconnue.

use std::collections::{HashMap, VecDeque};

use rfx_bytecode::{method_signature, CallTarget};
use rfx_model::{AccessSet, Domain, Domains, Precision, SideEffectSet};

use super::analyze;
use super::compose::{summarize, Sites};
use super::domains::DomainTable;
use super::effects::{union, WORLD};
use super::facts::{CallFact, MethodFacts};
use super::interp::DataflowLimits;
use super::summary::Summary;
use crate::callgraph::{CallGraph, Edge, EdgeKind, Resolution};
use crate::hierarchy::{ClassIndex, MethodRef, OBJECT};

/// Passages au plus par methode avant de renoncer au point fixe.
pub const MAX_PASSES_PER_METHOD: u32 = 64;

/// Fabrique de concatenation de chaines (JDK 9 et suivants).
const STRING_CONCAT: &str = "java/lang/invoke/StringConcatFactory";

/// Pourquoi une unite est, en tout ou partie, inconnue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnknownCauses {
    /// Sites d'appel non resolus par C-07.
    pub unresolved_sites: u32,
    /// Sites d'appel non explores : la borne de profondeur du graphe est atteinte.
    pub unexplored_sites: u32,
    /// Methodes cibles sans code dans l'index.
    pub external_methods: u32,
    /// Methodes cibles dont le code n'est pas analysable.
    pub unanalysable_methods: u32,
    /// Le graphe d'appel est tronque.
    pub truncated: bool,
    /// Le point fixe n'a pas converge sous la borne de passages.
    pub not_converged: bool,
}

/// Effets d'une unite de travail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadEffects {
    /// Lectures (DM-06).
    pub reads: AccessSet,
    /// Ecritures (DM-06).
    pub writes: AccessSet,
    /// Effets de bord (DM-07).
    pub side_effects: SideEffectSet,
    /// Un verrou est pris sur un objet non local.
    pub locks: bool,
    /// Causes d'inconnu.
    pub causes: UnknownCauses,
    /// Methodes du graphe analysees.
    pub analysed_methods: u32,
    /// Calculs de resume, jusqu'au point fixe.
    pub passes: u32,
}

/// Ce qu'un site d'appel fait, avant le point fixe.
enum Plan {
    /// Effet connu d'avance.
    Fixed(Summary),
    /// Union des resumes de ces methodes du graphe, pour `usize` arguments.
    Targets(Vec<usize>, usize),
}

/// Parametres logiques d'une methode, receveur compris s'il peut y en avoir un.
fn params_of(method: &MethodRef, is_static: Option<bool>) -> usize {
    let declared = method_signature(&method.descriptor).map_or(0, |(params, _)| params.len());
    declared + usize::from(is_static != Some(true))
}

/// Methode sans code dont l'effet est connu : aucun.
fn known_inert(method: &MethodRef) -> bool {
    method.owner == OBJECT && method.name == "<init>" && method.descriptor == "()V"
}

/// `true` si le site est une concatenation de chaines dont aucun argument ne peut
/// executer de code : primitives et `String` seulement.
fn inert_concat(call: &CallFact) -> bool {
    let CallTarget::Dynamic {
        descriptor,
        bootstrap,
        ..
    } = &call.target
    else {
        return false;
    };
    if bootstrap.member.owner != STRING_CONCAT {
        return false;
    }
    let Some(params) = descriptor
        .strip_prefix('(')
        .and_then(|rest| rest.split_once(')'))
        .map(|(params, _)| params)
    else {
        return false;
    };
    let mut rest = params;
    while let Some(first) = rest.chars().next() {
        match first {
            'B' | 'C' | 'D' | 'F' | 'I' | 'J' | 'S' | 'Z' => rest = &rest[1..],
            'L' => match rest.strip_prefix("Ljava/lang/String;") {
                Some(after) => rest = after,
                None => return false,
            },
            _ => return false,
        }
    }
    true
}

fn inert(args: usize) -> Summary {
    let mut summary = Summary::bottom(args);
    summary.returns_shared = true;
    summary
}

fn lambda(args: usize) -> Summary {
    let mut summary = inert(args);
    summary.escaping_params = vec![true; args];
    summary
}

/// Effets de l'unite de travail dont `graph` est le graphe d'appel ; sa racine est le
/// premier noeud.
#[must_use]
pub fn workload_effects(
    graph: &CallGraph,
    index: &ClassIndex,
    table: &DomainTable,
    limits: &DataflowLimits,
) -> WorkloadEffects {
    workload_effects_from(graph, index, table, &analyze_graph(graph, index, limits))
}

/// Faits des methodes d'un graphe d'appel, par noeud : calcules une fois, partages par
/// C-11 (effets) et C-12 (fils).
#[derive(Debug, Clone)]
pub struct GraphFacts {
    /// Faits du code de chaque noeud ; `None` sans code ou non analysable.
    pub facts: Vec<Option<MethodFacts>>,
    /// Parametres logiques de chaque noeud, receveur compris s'il peut y en avoir un.
    pub params: Vec<usize>,
    /// Noeuds sans code dans l'index, hors methodes connues sans effet.
    pub external_methods: u32,
    /// Noeuds dont le code n'est pas analysable.
    pub unanalysable_methods: u32,
}

/// Analyse le code de chaque methode du graphe.
#[must_use]
pub fn analyze_graph(graph: &CallGraph, index: &ClassIndex, limits: &DataflowLimits) -> GraphFacts {
    let nodes = graph.nodes();
    let mut external_methods = 0;
    let mut unanalysable_methods = 0;
    let mut facts: Vec<Option<MethodFacts>> = Vec::with_capacity(nodes.len());
    let mut params = Vec::with_capacity(nodes.len());
    for node in nodes {
        let found = index.class(&node.method.owner).and_then(|class| {
            class
                .method(&node.method.name, &node.method.descriptor)
                .map(|method| (class, method))
        });
        let is_static = found.map(|(_, method)| method.is_static());
        params.push(params_of(&node.method, is_static));
        let analysed = match found {
            Some((class, method)) if method.code.is_some() => {
                match analyze(class, method, limits) {
                    Ok(facts) => Some(facts),
                    Err(_) => {
                        unanalysable_methods += 1;
                        None
                    }
                }
            }
            _ => {
                if !known_inert(&node.method) {
                    external_methods += 1;
                }
                None
            }
        };
        facts.push(analysed);
    }
    GraphFacts {
        facts,
        params,
        external_methods,
        unanalysable_methods,
    }
}

/// Effets de l'unite de travail dont `graph` est le graphe d'appel, ses methodes deja
/// analysees ([`analyze_graph`]).
#[must_use]
#[allow(clippy::too_many_lines)] // trois etapes lineaires : plans, point fixe, racine
pub fn workload_effects_from(
    graph: &CallGraph,
    index: &ClassIndex,
    table: &DomainTable,
    analysed: &GraphFacts,
) -> WorkloadEffects {
    let nodes = graph.nodes();
    let (facts, params) = (&analysed.facts, &analysed.params);
    let causes_start = UnknownCauses {
        truncated: graph.truncated(),
        external_methods: analysed.external_methods,
        unanalysable_methods: analysed.unanalysable_methods,
        ..UnknownCauses::default()
    };
    let mut causes = causes_start;

    // 2. Plan de chaque site d'appel.
    let mut by_site: HashMap<(usize, u32), Vec<&Edge>> = HashMap::new();
    for edge in graph.edges() {
        if let Some(offset) = edge.offset {
            by_site.entry((edge.from, offset)).or_default().push(edge);
        }
    }
    let mut callers: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut plans: Vec<Vec<(u32, Plan)>> = Vec::with_capacity(nodes.len());
    for (id, method_facts) in facts.iter().enumerate() {
        let mut plan = Vec::new();
        for call in method_facts.iter().flat_map(|f| &f.calls) {
            let args = call.args.len();
            let site = match by_site.get(&(id, call.offset)) {
                None => {
                    causes.unexplored_sites += 1;
                    Plan::Fixed(Summary::top(args))
                }
                Some(edges) => {
                    let resolved: Option<Vec<(usize, Resolution)>> = edges
                        .iter()
                        .map(|edge| match (edge.kind, edge.to) {
                            (EdgeKind::Resolved(resolution), Some(to)) => Some((to, resolution)),
                            _ => None,
                        })
                        .collect();
                    match resolved {
                        None if inert_concat(call) => Plan::Fixed(inert(args)),
                        None => {
                            causes.unresolved_sites += 1;
                            Plan::Fixed(Summary::top(args))
                        }
                        Some(targets) if targets.iter().all(|(_, r)| *r == Resolution::Lambda) => {
                            Plan::Fixed(lambda(args))
                        }
                        Some(targets) => {
                            let ids: Vec<usize> = targets.iter().map(|(to, _)| *to).collect();
                            for &to in &ids {
                                callers[to].push(id);
                            }
                            Plan::Targets(ids, args)
                        }
                    }
                }
            };
            plan.push((call.offset, site));
        }
        plans.push(plan);
    }

    // 3. Point fixe, des methodes les plus profondes vers la racine.
    let mut summaries: Vec<Summary> = nodes
        .iter()
        .zip(facts)
        .zip(params)
        .map(|((node, facts), &count)| match facts {
            Some(_) => Summary::bottom(count),
            None if known_inert(&node.method) => Summary::bottom(count),
            None => Summary::top(count),
        })
        .collect();
    let mut order: Vec<usize> = (0..nodes.len()).filter(|&id| facts[id].is_some()).collect();
    order.sort_by_key(|&id| std::cmp::Reverse(nodes[id].depth));
    let mut queued = vec![false; nodes.len()];
    for &id in &order {
        queued[id] = true;
    }
    let mut pending: VecDeque<usize> = order.into_iter().collect();
    let mut passes_of = vec![0_u32; nodes.len()];
    let mut passes = 0_u32;
    while let Some(id) = pending.pop_front() {
        queued[id] = false;
        passes_of[id] += 1;
        passes += 1;
        if passes_of[id] > MAX_PASSES_PER_METHOD {
            causes.not_converged = true;
            break;
        }
        let Some(method_facts) = &facts[id] else {
            continue;
        };
        let sites: Sites = plans[id]
            .iter()
            .map(|(offset, plan)| {
                let summary = match plan {
                    Plan::Fixed(summary) => summary.clone(),
                    Plan::Targets(targets, args) => {
                        let mut joined = Summary::bottom(*args);
                        for &to in targets {
                            joined.join(&summaries[to]);
                        }
                        joined
                    }
                };
                (*offset, summary)
            })
            .collect();
        let computed = summarize(method_facts, params[id], &sites, index, table);
        if summaries[id].join(&computed) {
            for &caller in &callers[id] {
                if !queued[caller] {
                    queued[caller] = true;
                    pending.push_back(caller);
                }
            }
        }
    }

    // 4. Effets de la racine : ses parametres viennent du jeu, ils sont partages.
    let root = match (summaries.first(), causes.not_converged) {
        (Some(summary), false) => summary.clone(),
        _ => Summary::top(params.first().copied().unwrap_or(0)),
    };
    let unknown = root.is_unknown()
        || causes.truncated
        || causes.not_converged
        || facts.first().is_none_or(Option::is_none);
    let mut reads = root
        .param_reads
        .iter()
        .fold(root.reads, |a, b| union(a, *b));
    let mut writes = root
        .param_writes
        .iter()
        .fold(root.writes, |a, b| union(a, *b));
    if unknown {
        reads = reads.with(Domain::Unknown);
        writes = writes.with(Domain::Unknown);
    }
    let precision = |domains: Domains| {
        if domains.contains(Domain::Unknown) {
            Precision::Unknown
        } else if root.over {
            Precision::OverApprox
        } else {
            Precision::Exact
        }
    };
    let mut side_effects = root.side_effects;
    side_effects.unknown |= unknown;
    side_effects.mutates_world =
        WORLD.iter().any(|d| writes.contains(*d)) || writes.contains(Domain::Unknown);
    WorkloadEffects {
        reads: AccessSet {
            domains: reads,
            precision: precision(reads),
        },
        writes: AccessSet {
            domains: writes,
            precision: precision(writes),
        },
        side_effects,
        locks: root.locks,
        causes,
        analysed_methods: u32::try_from(facts.iter().filter(|f| f.is_some()).count())
            .unwrap_or(u32::MAX),
        passes,
    }
}
