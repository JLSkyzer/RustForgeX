//! C-07 : graphe d'appel partiel d'une unite de travail.
//!
//! Cahier des charges : PARTIE 5.7. Exigences : R-340 (bornes), R-341 (polymorphisme
//! non borne = non resolu, P-02), FM-15. Tests : T-160 a T-164. Decision : ADR-050.
//! Maturite : `STABLE`.
//!
//! # Ce que « resolu » veut dire ici
//!
//! Une arete resolue designe la methode qui s'executera, sans supposition sur des
//! classes absentes. Trois cas :
//!
//! - l'appel est lie statiquement (`invokestatic`, `invokespecial`, methode privee ou
//!   finale, classe finale) : la cible est sure, meme dans une classe hors de l'index
//!   (elle est alors un noeud externe, non explore) ;
//! - l'appel est virtuel, par une **classe**, et les classes concretes connues n'offrent
//!   qu'une a quatre cibles : une arete par cible. Ce resultat vaut pour l'ensemble de classes donne ;
//!   charger une nouvelle sous-classe peut l'invalider, d'ou un graphe a recalculer
//!   quand l'index change (ADR-050) ;
//! - une lambda dont la methode d'implementation est lisible dans le reservoir.
//!
//! Tout le reste — appel par une interface, classe absente, plus de quatre cibles,
//! aucune implementation, reflexion, autre `invokedynamic` — est une arete non
//! resolue, comptee.
//!
//! # Interfaces ouvertes
//!
//! Une interface peut etre implementee par des classes que l'index ne verra jamais :
//! les classes cachees des lambdas (`LambdaMetafactory`), les `Proxy`, les classes
//! generees a l'execution. Aucune enumeration de ses implementations n'est donc
//! complete, et un appel par une interface n'est jamais declare resolu — sauf vers une
//! methode privee de l'interface, qui ne peut pas etre redefinie (ADR-051).

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use rfx_bytecode::{CallSite, CallTarget, InvokeKind, MemberRef};

use crate::hierarchy::{ClassIndex, MethodRef, Selection};

/// Profondeur d'exploration par defaut (`analysis.max_call_depth`, R-340).
pub const DEFAULT_MAX_CALL_DEPTH: u32 = 12;

/// Noeuds par graphe par defaut (`analysis.max_nodes_per_workload`, R-340).
pub const DEFAULT_MAX_NODES: usize = 2_000;

/// Bytecode demande au plus par site d'appel et par tour (ADR-051).
pub const MAX_REQUESTS_PER_SITE: usize = 8;

/// Cibles au plus d'un appel polymorphe borne ; au-dela, il est non resolu (R-341).
pub const MAX_POLYMORPHIC_TARGETS: usize = 4;

/// Bornes de l'exploration (R-340).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Profondeur au plus, la racine etant a 0.
    pub max_call_depth: u32,
    /// Noeuds au plus.
    pub max_nodes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_call_depth: DEFAULT_MAX_CALL_DEPTH,
            max_nodes: DEFAULT_MAX_NODES,
        }
    }
}

/// Comment une arete resolue l'a ete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Resolution {
    /// `invokestatic`.
    Static,
    /// `invokespecial` : constructeur, `super`, methode privee.
    Special,
    /// Appel virtuel dont la cible ne peut pas etre redefinie : methode privee ou
    /// finale, classe finale.
    Exact,
    /// Une seule cible parmi les classes chargees.
    Monomorphic,
    /// Deux a quatre cibles parmi les classes chargees.
    Polymorphic,
    /// Methode d'implementation d'une lambda ou d'une reference de methode.
    Lambda,
}

/// Pourquoi un appel n'est pas resolu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unresolved {
    /// Une classe necessaire a la resolution est absente de l'index (FM-15).
    UnknownClass,
    /// Appel par une interface : des classes invisibles a l'index (lambdas, `Proxy`,
    /// classes generees) peuvent l'implementer (ADR-051).
    OpenInterface,
    /// Plus de [`MAX_POLYMORPHIC_TARGETS`] cibles (R-341).
    Megamorphic,
    /// Aucune implementation concrete chargee.
    NoImplementation,
    /// Plusieurs methodes par defaut candidates.
    Ambiguous,
    /// `invokedynamic` qui n'est pas une lambda.
    Dynamic,
    /// Reflexion : `Method.invoke`, `MethodHandle.invoke*`, `newInstance`.
    Reflection,
    /// Code illisible : la methode n'a pas pu etre parcourue.
    Malformed,
}

/// Attribut d'une arete (PARTIE 5.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// Cible sure.
    Resolved(Resolution),
    /// Cible inconnue.
    VirtualUnresolved(Unresolved),
    /// Arete observee a l'execution (contexte d'appel reel).
    DynamicObserved,
}

/// Une arete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// Noeud appelant.
    pub from: usize,
    /// Noeud appele ; absent pour une arete non resolue.
    pub to: Option<usize>,
    /// Methode designee par le site d'appel, telle qu'ecrite ; absente pour un
    /// `invokedynamic` et une arete observee.
    pub reference: Option<MethodRef>,
    /// Position du site d'appel dans le code de l'appelant.
    pub offset: Option<u32>,
    /// Attribut.
    pub kind: EdgeKind,
}

/// Un noeud : une methode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Methode.
    pub method: MethodRef,
    /// Distance a la racine, en appels.
    pub depth: u32,
    /// `true` si sa classe ou son code est absent de l'index : non explore.
    pub external: bool,
    /// `true` si la methode appartient a un cycle d'appels (Tarjan).
    pub recursive: bool,
}

/// Graphe d'appel partiel.
#[derive(Debug, Clone, Default)]
pub struct CallGraph {
    nodes: Vec<Node>,
    by_method: HashMap<MethodRef, usize>,
    edges: Vec<Edge>,
    truncated: bool,
    resolved_calls: u32,
    unresolved_calls: u32,
    reflection: bool,
    build_ns: u64,
}

/// Appels reflexifs : proprietaire et nom.
const REFLECTIVE: [(&str, &str); 6] = [
    ("java/lang/reflect/Method", "invoke"),
    ("java/lang/reflect/Constructor", "newInstance"),
    ("java/lang/Class", "newInstance"),
    ("java/lang/invoke/MethodHandle", "invoke"),
    ("java/lang/invoke/MethodHandle", "invokeExact"),
    ("java/lang/invoke/MethodHandle", "invokeWithArguments"),
];

const LAMBDA_FACTORY: &str = "java/lang/invoke/LambdaMetafactory";

impl CallGraph {
    /// Construit le graphe depuis `root`, dans les bornes donnees.
    #[must_use]
    pub fn build(classes: &ClassIndex, root: &MethodRef, limits: Limits) -> Self {
        let started = Instant::now();
        let mut graph = Self::default();
        let mut queue = VecDeque::from([graph.add_node(root.clone(), 0)]);
        while let Some(id) = queue.pop_front() {
            let node = graph.nodes[id].clone();
            let found = classes.class(&node.method.owner).and_then(|class| {
                class
                    .method(&node.method.name, &node.method.descriptor)
                    .map(|method| (class, method))
            });
            let Some((class, method)) = found else {
                graph.nodes[id].external = true;
                continue;
            };
            let Ok(sites) = class.call_sites(method) else {
                graph.push_unresolved(id, None, None, Unresolved::Malformed);
                continue;
            };
            if sites.is_empty() {
                continue;
            }
            if node.depth >= limits.max_call_depth {
                graph.truncated = true;
                continue;
            }
            for site in sites {
                graph.add_site(classes, id, &site, node.depth + 1, limits, &mut queue);
            }
        }
        graph.mark_recursion();
        graph.build_ns = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        graph
    }

    fn add_node(&mut self, method: MethodRef, depth: u32) -> usize {
        let id = self.nodes.len();
        self.by_method.insert(method.clone(), id);
        self.nodes.push(Node {
            method,
            depth,
            external: false,
            recursive: false,
        });
        id
    }

    fn push_unresolved(
        &mut self,
        from: usize,
        reference: Option<MethodRef>,
        offset: Option<u32>,
        reason: Unresolved,
    ) {
        if reason == Unresolved::Reflection {
            self.reflection = true;
        }
        self.unresolved_calls = self.unresolved_calls.saturating_add(1);
        self.edges.push(Edge {
            from,
            to: None,
            reference,
            offset,
            kind: EdgeKind::VirtualUnresolved(reason),
        });
    }

    fn add_site(
        &mut self,
        classes: &ClassIndex,
        from: usize,
        site: &CallSite,
        depth: u32,
        limits: Limits,
        queue: &mut VecDeque<usize>,
    ) {
        let reference = match &site.target {
            CallTarget::Member(member) => Some(reference_of(member)),
            CallTarget::Dynamic { .. } => None,
        };
        match resolve(classes, site) {
            Err(reason) => self.push_unresolved(from, reference, Some(site.offset), reason),
            Ok(targets) => {
                self.resolved_calls = self.resolved_calls.saturating_add(1);
                for (target, resolution) in targets {
                    let to = match self.by_method.get(&target) {
                        Some(&known) => known,
                        None if self.nodes.len() >= limits.max_nodes => {
                            self.truncated = true;
                            continue;
                        }
                        None => {
                            let added = self.add_node(target, depth);
                            queue.push_back(added);
                            added
                        }
                    };
                    self.edges.push(Edge {
                        from,
                        to: Some(to),
                        reference: reference.clone(),
                        offset: Some(site.offset),
                        kind: EdgeKind::Resolved(resolution),
                    });
                }
            }
        }
    }

    /// Fusionne une arete observee a l'execution (PARTIE 5.7, etape 6). Rend `false`
    /// si elle etait deja connue, ou si le plafond de noeuds l'empeche.
    pub fn add_observed(&mut self, from: &MethodRef, to: &MethodRef, limits: Limits) -> bool {
        let from_id = match self.by_method.get(from) {
            Some(&id) => id,
            None if self.nodes.len() >= limits.max_nodes => return false,
            None => self.add_node(from.clone(), 0),
        };
        let to_id = match self.by_method.get(to) {
            Some(&id) => id,
            None if self.nodes.len() >= limits.max_nodes => return false,
            None => {
                let depth = self.nodes[from_id].depth.saturating_add(1);
                let id = self.add_node(to.clone(), depth);
                self.nodes[id].external = true;
                id
            }
        };
        if self
            .edges
            .iter()
            .any(|edge| edge.from == from_id && edge.to == Some(to_id))
        {
            return false;
        }
        self.edges.push(Edge {
            from: from_id,
            to: Some(to_id),
            reference: None,
            offset: None,
            kind: EdgeKind::DynamicObserved,
        });
        self.mark_recursion();
        true
    }

    /// Noeuds, la racine en premier.
    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Aretes, dans l'ordre de leur decouverte.
    #[must_use]
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// Noeud d'une methode.
    #[must_use]
    pub fn find(&self, method: &MethodRef) -> Option<&Node> {
        self.by_method.get(method).map(|&id| &self.nodes[id])
    }

    /// Aretes qui partent d'une methode.
    pub fn edges_from<'a>(&'a self, method: &MethodRef) -> impl Iterator<Item = &'a Edge> + 'a {
        let id = self.by_method.get(method).copied();
        self.edges.iter().filter(move |edge| Some(edge.from) == id)
    }

    /// `true` si une borne a arrete l'exploration (`DependencySet.truncated`, R-340).
    #[must_use]
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// `true` si un appel reflexif a ete rencontre.
    #[must_use]
    pub fn reflection(&self) -> bool {
        self.reflection
    }

    /// Sites d'appel non resolus.
    #[must_use]
    pub fn unresolved_calls(&self) -> u32 {
        self.unresolved_calls
    }

    /// Part des sites d'appel non resolus, de 0 a 1 (`rfx.callgraph.unresolved_ratio`).
    #[must_use]
    pub fn unresolved_ratio(&self) -> f64 {
        let total = self.resolved_calls + self.unresolved_calls;
        if total == 0 {
            0.0
        } else {
            f64::from(self.unresolved_calls) / f64::from(total)
        }
    }

    /// Duree de la construction, en nanosecondes (`rfx.callgraph.build_ns`).
    #[must_use]
    pub fn build_ns(&self) -> u64 {
        self.build_ns
    }

    /// Marque les noeuds des cycles : composantes fortement connexes de plus d'un
    /// noeud, ou noeud qui s'appelle lui-meme (Tarjan, iteratif : aucune pile d'appels
    /// a la merci de la profondeur du graphe).
    fn mark_recursion(&mut self) {
        let count = self.nodes.len();
        let mut successors = vec![Vec::new(); count];
        for edge in &self.edges {
            if let Some(to) = edge.to {
                successors[edge.from].push(to);
                if edge.from == to {
                    self.nodes[to].recursive = true;
                }
            }
        }
        for component in strongly_connected(&successors) {
            if component.len() > 1 {
                for id in component {
                    self.nodes[id].recursive = true;
                }
            }
        }
    }
}

fn reference_of(member: &MemberRef) -> MethodRef {
    MethodRef::new(&member.owner, &member.name, &member.descriptor)
}

/// Resout un site d'appel en ses cibles.
fn resolve(
    classes: &ClassIndex,
    site: &CallSite,
) -> Result<Vec<(MethodRef, Resolution)>, Unresolved> {
    match (&site.target, site.kind) {
        (CallTarget::Member(member), InvokeKind::Static) => {
            Ok(vec![(declared_up(classes, member), Resolution::Static)])
        }
        (CallTarget::Member(member), InvokeKind::Special) => {
            Ok(vec![(declared_up(classes, member), Resolution::Special)])
        }
        (CallTarget::Member(member), _) => resolve_virtual(classes, member),
        (
            CallTarget::Dynamic {
                bootstrap,
                handle_arguments,
                ..
            },
            _,
        ) => {
            let lambda = bootstrap.member.owner == LAMBDA_FACTORY
                && matches!(
                    bootstrap.member.name.as_str(),
                    "metafactory" | "altMetafactory"
                );
            let Some(implementation) = handle_arguments.first().filter(|_| lambda) else {
                return Err(Unresolved::Dynamic);
            };
            match implementation.kind {
                // invokeStatic, invokeSpecial, newInvokeSpecial : cible sure.
                6..=8 => Ok(vec![(
                    reference_of(&implementation.member),
                    Resolution::Lambda,
                )]),
                // invokeVirtual, invokeInterface : la cible depend du receveur.
                5 | 9 => resolve_virtual(classes, &implementation.member),
                _ => Err(Unresolved::Dynamic),
            }
        }
    }
}

/// Classe qui declare la methode, en remontant depuis le proprietaire ecrit ; a defaut,
/// la reference telle qu'ecrite (la JVM la resoudra a l'identique).
fn declared_up(classes: &ClassIndex, member: &MemberRef) -> MethodRef {
    let mut current = Some(member.owner.as_str());
    while let Some(name) = current {
        let Some(class) = classes.class(name) else {
            break;
        };
        if class.method(&member.name, &member.descriptor).is_some() {
            return MethodRef::new(name, &member.name, &member.descriptor);
        }
        current = class.super_name.as_deref();
    }
    reference_of(member)
}

fn resolve_virtual(
    classes: &ClassIndex,
    member: &MemberRef,
) -> Result<Vec<(MethodRef, Resolution)>, Unresolved> {
    if REFLECTIVE.contains(&(member.owner.as_str(), member.name.as_str())) {
        return Err(Unresolved::Reflection);
    }
    let Some(owner) = classes.class(&member.owner) else {
        // Absente, ou demandee : dans les deux cas, rien n'est affirme a ce tour.
        return Err(Unresolved::UnknownClass);
    };

    // Cible qui ne peut pas etre redefinie : classe finale, methode privee ou finale.
    let declared = declared_up(classes, member);
    let fixed = owner.is_final()
        || classes
            .class(&declared.owner)
            .and_then(|class| class.method(&declared.name, &declared.descriptor))
            .is_some_and(|method| method.is_private() || method.is_final());
    if fixed {
        let receiver = if owner.is_final() {
            member.owner.as_str()
        } else {
            declared.owner.as_str()
        };
        return match classes.select(receiver, &member.name, &member.descriptor) {
            Selection::Found(target) => Ok(vec![(target, Resolution::Exact)]),
            Selection::Missing => Err(Unresolved::NoImplementation),
            Selection::Unknown | Selection::Pending => Err(Unresolved::UnknownClass),
            Selection::Ambiguous => Err(Unresolved::Ambiguous),
        };
    }

    if owner.is_interface() {
        return Err(Unresolved::OpenInterface);
    }

    let receivers = classes
        .concrete_subtypes(&member.owner)
        .ok_or(Unresolved::UnknownClass)?;
    let mut targets: Vec<MethodRef> = Vec::new();
    let mut pending = false;
    let mut asked = 0;
    for receiver in receivers {
        if classes.get(&receiver).is_none() {
            if !classes.is_fetchable(&receiver) {
                // Sous-classe concrete dont le bytecode ne sera jamais fourni (cible de
                // mixin) : sa cible est inconnue, l'appel aussi.
                return Err(Unresolved::UnknownClass);
            }
            // Pas plus de quelques demandes par site et par tour : une classe aux
            // milliers de sous-classes se revele megamorphe bien avant.
            if asked < MAX_REQUESTS_PER_SITE {
                classes.request(&receiver);
                asked += 1;
            }
            pending = true;
            continue;
        }
        match classes.select(&receiver, &member.name, &member.descriptor) {
            Selection::Found(target) => {
                if !targets.contains(&target) {
                    targets.push(target);
                }
                // Des receveurs encore inconnus ne peuvent qu'ajouter des cibles :
                // au-dela du seuil, la conclusion est deja sure.
                if targets.len() > MAX_POLYMORPHIC_TARGETS {
                    return Err(Unresolved::Megamorphic);
                }
            }
            // Ce receveur leverait AbstractMethodError : il n'apporte pas de cible.
            Selection::Missing => {}
            Selection::Pending => pending = true,
            Selection::Unknown => return Err(Unresolved::UnknownClass),
            Selection::Ambiguous => return Err(Unresolved::Ambiguous),
        }
    }
    if pending {
        return Err(Unresolved::UnknownClass);
    }
    targets.sort();
    match targets.len() {
        0 => Err(Unresolved::NoImplementation),
        1 => Ok(vec![(targets.remove(0), Resolution::Monomorphic)]),
        n if n <= MAX_POLYMORPHIC_TARGETS => Ok(targets
            .into_iter()
            .map(|target| (target, Resolution::Polymorphic))
            .collect()),
        _ => Err(Unresolved::Megamorphic),
    }
}

/// Composantes fortement connexes (Tarjan, version iterative).
fn strongly_connected(successors: &[Vec<usize>]) -> Vec<Vec<usize>> {
    const UNVISITED: usize = usize::MAX;
    let count = successors.len();
    let mut index = vec![UNVISITED; count];
    let mut lowlink = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut components = Vec::new();
    let mut next = 0;
    for start in 0..count {
        if index[start] != UNVISITED {
            continue;
        }
        // (noeud, prochain successeur a visiter)
        let mut work = vec![(start, 0_usize)];
        index[start] = next;
        lowlink[start] = next;
        next += 1;
        stack.push(start);
        on_stack[start] = true;
        while let Some(&(node, child)) = work.last() {
            if let Some(&succ) = successors[node].get(child) {
                if let Some(top) = work.last_mut() {
                    top.1 += 1;
                }
                if index[succ] == UNVISITED {
                    index[succ] = next;
                    lowlink[succ] = next;
                    next += 1;
                    stack.push(succ);
                    on_stack[succ] = true;
                    work.push((succ, 0));
                } else if on_stack[succ] {
                    lowlink[node] = lowlink[node].min(index[succ]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                lowlink[parent] = lowlink[parent].min(lowlink[node]);
            }
            if lowlink[node] == index[node] {
                let mut component = Vec::new();
                while let Some(member) = stack.pop() {
                    on_stack[member] = false;
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                components.push(component);
            }
        }
    }
    components
}

#[cfg(test)]
mod tests {
    use super::strongly_connected;

    #[test]
    fn tarjan_finds_cycles_and_singletons() {
        // 0 -> 1 -> 2 -> 0 ; 2 -> 3 ; 3 seul.
        let successors = vec![vec![1], vec![2], vec![0, 3], vec![]];
        let mut components: Vec<Vec<usize>> = strongly_connected(&successors)
            .into_iter()
            .map(|mut c| {
                c.sort_unstable();
                c
            })
            .collect();
        components.sort();
        assert_eq!(components, vec![vec![0, 1, 2], vec![3]]);
    }

    #[test]
    fn a_long_chain_does_not_exhaust_the_stack() {
        let successors: Vec<Vec<usize>> = (0..200_000)
            .map(|n| if n + 1 < 200_000 { vec![n + 1] } else { vec![] })
            .collect();
        assert_eq!(strongly_connected(&successors).len(), 200_000);
    }
}
