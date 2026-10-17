//! Resume d'une methode a partir de ses faits et des resumes de ses sites d'appel
//! (ADR-055, phase interprocedurale).
//!
//! # Racines
//!
//! Chaque source d'une reference est ramenee a ses racines : un parametre, une
//! allocation (site `new`, ou appel qui rend un objet frais), ou un objet partage
//! (lu dans un champ, un statique, un tableau, rendu partage par un appel, attrape,
//! constante, inconnu). Une valeur rendue par un appel qui rend l'un de ses parametres
//! a pour racines celles de l'argument passe.
//!
//! # Echappement
//!
//! Une racine s'echappe si elle est passee a un parametre qui s'echappe chez l'appele,
//! levee, ou ecrite dans une base partagee, un parametre ou une allocation qui
//! s'echappe ; jusqu'au point fixe. Un parametre ecrit dans une allocation que la
//! methode rend s'echappe aussi : l'appelant ne doit pas croire local un objet
//! atteignable par ce qu'on lui rend. Une allocation seulement rendue reste locale ici :
//! avant le retour, personne d'autre ne la voit.

use std::collections::{BTreeSet, HashMap, HashSet};

use rfx_model::{Domain, Domains};

use super::domains::DomainTable;
use super::effects::{target_domains, union};
use super::facts::{Access, CallFact, MethodFacts, Target};
use super::summary::Summary;
use super::value::{Source, Sources};
use crate::hierarchy::ClassIndex;

/// Racine d'une reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Root {
    Param(u16),
    Alloc(u32),
    Shared,
}

/// Effet d'un site d'appel : le resume de sa cible, ou de l'union de ses cibles.
pub(super) type Sites = HashMap<u32, Summary>;

struct Roots<'a> {
    calls: HashMap<u32, &'a CallFact>,
    sites: &'a Sites,
}

impl Roots<'_> {
    fn of(&self, sources: &Sources) -> BTreeSet<Root> {
        let mut out = BTreeSet::new();
        let mut seen = HashSet::new();
        self.collect(sources, &mut out, &mut seen);
        out
    }

    fn collect(&self, sources: &Sources, out: &mut BTreeSet<Root>, seen: &mut HashSet<u32>) {
        for source in sources.iter() {
            match source {
                Source::Param(i) => {
                    out.insert(Root::Param(i));
                }
                Source::New(site) => {
                    out.insert(Root::Alloc(site));
                }
                Source::Return(site) => {
                    if !seen.insert(site) {
                        continue;
                    }
                    let (Some(summary), Some(call)) =
                        (self.sites.get(&site), self.calls.get(&site))
                    else {
                        out.insert(Root::Shared);
                        continue;
                    };
                    if summary.returns_shared || summary.is_unknown() {
                        out.insert(Root::Shared);
                    }
                    if summary.returns_fresh {
                        out.insert(Root::Alloc(site));
                    }
                    for (p, returned) in summary.returns_params.iter().enumerate() {
                        if !returned {
                            continue;
                        }
                        match call.args.get(p) {
                            Some(Some(arg)) => self.collect(arg, out, seen),
                            _ => {
                                out.insert(Root::Shared);
                            }
                        }
                    }
                }
                Source::Static(_)
                | Source::Field(_)
                | Source::Element(_)
                | Source::Caught(_)
                | Source::Constant
                | Source::Unknown => {
                    out.insert(Root::Shared);
                }
            }
        }
    }
}

/// Ce qui s'echappe d'une methode.
struct Escape {
    allocs: BTreeSet<u32>,
    params: BTreeSet<u16>,
    returned: BTreeSet<u32>,
}

impl Escape {
    fn mark(&mut self, roots: &BTreeSet<Root>) -> bool {
        let mut changed = false;
        for root in roots {
            changed |= match *root {
                Root::Alloc(site) => self.allocs.insert(site),
                Root::Param(p) => self.params.insert(p),
                Root::Shared => false,
            };
        }
        changed
    }

    fn is_local(&self, root: Root) -> bool {
        matches!(root, Root::Alloc(site) if !self.allocs.contains(&site))
    }
}

/// Domaines que l'appele touche par son parametre `i` ; au-dela de son resume, rien,
/// sauf s'il est inconnu.
fn site_param(domains: &[Domains], i: usize, unknown: bool) -> Domains {
    domains.get(i).copied().unwrap_or(if unknown {
        Domains::EMPTY.with(Domain::Unknown)
    } else {
        Domains::EMPTY
    })
}

/// Resume de la methode dont `facts` decrit le code, `params` parametres logiques, ses
/// sites d'appel decrits par `sites` (un site absent est inconnu).
pub(super) fn summarize(
    facts: &MethodFacts,
    params: usize,
    sites: &Sites,
    index: &ClassIndex,
    table: &DomainTable,
) -> Summary {
    let unknown_sites: Sites = facts
        .calls
        .iter()
        .filter(|call| !sites.contains_key(&call.offset))
        .map(|call| (call.offset, Summary::top(call.args.len())))
        .collect();
    let mut all_sites = sites.clone();
    all_sites.extend(unknown_sites);
    let roots = Roots {
        calls: facts.calls.iter().map(|call| (call.offset, call)).collect(),
        sites: &all_sites,
    };

    let mut summary = Summary::bottom(params);
    let mut escape = Escape {
        allocs: BTreeSet::new(),
        params: BTreeSet::new(),
        returned: BTreeSet::new(),
    };

    // Echappements directs : appels, exceptions, retour.
    for call in &facts.calls {
        let callee = &all_sites[&call.offset];
        for (i, arg) in call.args.iter().enumerate() {
            let escapes = callee
                .escaping_params
                .get(i)
                .copied()
                .unwrap_or(callee.is_unknown());
            if let (Some(arg), true) = (arg, escapes) {
                escape.mark(&roots.of(arg));
            }
        }
    }
    for throw in &facts.throws {
        escape.mark(&roots.of(&throw.value));
    }
    if let Some(returned) = &facts.returns {
        for root in roots.of(returned) {
            match root {
                Root::Param(p) => {
                    if let Some(flag) = summary.returns_params.get_mut(usize::from(p)) {
                        *flag = true;
                    }
                }
                Root::Alloc(site) => {
                    escape.returned.insert(site);
                }
                Root::Shared => summary.returns_shared = true,
            }
        }
    }

    // Echappement par les ecritures, jusqu'au point fixe.
    let stores: Vec<(Option<BTreeSet<Root>>, BTreeSet<Root>)> = facts
        .writes
        .iter()
        .filter_map(|write| {
            let value = write.value.as_ref()?;
            let base = match write.target {
                Target::Static(_) => None,
                Target::Field(_) | Target::Element => Some(roots.of(&write.base)),
            };
            Some((base, roots.of(value)))
        })
        .collect();
    loop {
        let mut changed = false;
        for (base, value) in &stores {
            let shared = base
                .as_ref()
                .is_none_or(|base| base.iter().any(|root| !escape.is_local(*root)));
            if shared {
                changed |= escape.mark(value);
            } else if base.as_ref().is_some_and(|base| {
                base.iter()
                    .any(|root| matches!(root, Root::Alloc(site) if escape.returned.contains(site)))
            }) {
                for root in value {
                    if let Root::Param(p) = root {
                        changed |= escape.params.insert(*p);
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    summary.returns_fresh = escape.returned.iter().any(|a| !escape.allocs.contains(a));
    summary.returns_shared |= escape.returned.iter().any(|a| escape.allocs.contains(a));
    for p in &escape.params {
        if let Some(flag) = summary.escaping_params.get_mut(usize::from(*p)) {
            *flag = true;
        }
    }

    // Acces de la methode.
    let account = |access: &Access, write: bool, summary: &mut Summary| {
        let domains = target_domains(&access.target, index, table);
        let (shared, per_param) = if write {
            (&mut summary.writes, &mut summary.param_writes)
        } else {
            (&mut summary.reads, &mut summary.param_reads)
        };
        if let Target::Static(_) = access.target {
            *shared = union(*shared, domains);
            return;
        }
        let mut local = false;
        let mut other = false;
        for root in roots.of(&access.base) {
            match root {
                Root::Param(p) => {
                    other = true;
                    if let Some(slot) = per_param.get_mut(usize::from(p)) {
                        *slot = union(*slot, domains);
                    }
                }
                root if escape.is_local(root) => local = true,
                _ => {
                    other = true;
                    *shared = union(*shared, domains);
                }
            }
        }
        summary.over |= local && other;
    };
    for read in &facts.reads {
        account(read, false, &mut summary);
    }
    for write in &facts.writes {
        account(write, true, &mut summary);
    }

    // Effets des appeles, ramenes aux arguments.
    for call in &facts.calls {
        let callee = &all_sites[&call.offset];
        summary.reads = union(summary.reads, callee.reads);
        summary.writes = union(summary.writes, callee.writes);
        for (i, arg) in call.args.iter().enumerate() {
            let Some(arg) = arg else { continue };
            let reads = site_param(&callee.param_reads, i, callee.is_unknown());
            let writes = site_param(&callee.param_writes, i, callee.is_unknown());
            if reads.is_empty() && writes.is_empty() {
                continue;
            }
            let mut local = false;
            let mut other = false;
            for root in roots.of(arg) {
                match root {
                    Root::Param(p) => {
                        other = true;
                        let p = usize::from(p);
                        if let Some(slot) = summary.param_reads.get_mut(p) {
                            *slot = union(*slot, reads);
                        }
                        if let Some(slot) = summary.param_writes.get_mut(p) {
                            *slot = union(*slot, writes);
                        }
                    }
                    root if escape.is_local(root) => local = true,
                    _ => {
                        other = true;
                        summary.reads = union(summary.reads, reads);
                        summary.writes = union(summary.writes, writes);
                    }
                }
            }
            summary.over |= local && other;
        }
        let (side, theirs) = (&mut summary.side_effects, &callee.side_effects);
        side.throws_observable |= theirs.throws_observable;
        side.unknown |= theirs.unknown;
        side.spawns_or_removes_entity |= theirs.spawns_or_removes_entity;
        side.sends_packets |= theirs.sends_packets;
        side.schedules_tasks |= theirs.schedules_tasks;
        side.fires_events |= theirs.fires_events;
        side.io |= theirs.io;
        side.native_call |= theirs.native_call;
        side.cancels_event |= theirs.cancels_event;
        summary.locks |= callee.locks;
        summary.over |= callee.over;
    }

    summary.side_effects.throws_observable |= !facts.throws.is_empty();
    summary.locks |= facts.monitors.iter().any(|monitor| {
        roots
            .of(&monitor.lock)
            .into_iter()
            .any(|root| !escape.is_local(root))
    });
    summary
}
