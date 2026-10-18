//! C-12 : contraintes de fil d'une unite de travail (ADR-057).
//!
//! Cahier des charges : PARTIE 5.12, DM-09, R-390, R-391 ; tests T-210 a T-212.
//! Maturite : `EXPERIMENTAL` — l'observation des roles de fil (DM-02) manque ; seule
//! l'analyse statique est faite.
//!
//! Sur les methodes analysees du graphe d'appel :
//!
//! - `synchronized` : methode `ACC_SYNCHRONIZED` ou `monitorenter` ;
//! - verrous : appels vers `java/util/concurrent/locks/*`, `Semaphore.acquire*`,
//!   `Object.wait` ;
//! - variables locales de fil : domaine `ThreadLocals`, ou appel vers une classe qui
//!   descend de `java/lang/ThreadLocal` ;
//! - API non sure : acces au champ d'une classe non sure entre fils
//!   ([`ThreadSafetyTable`]) sur un objet non local, a un statique d'une telle classe, a
//!   un element de tableau partage ; ou partie du graphe inconnue.
//!
//! `main_thread_required` : `YES` si l'unite touche une API non sure ou ecrit l'etat du
//! jeu ; `NO` seulement si elle est pure (lecture seule connue, sans effet de bord,
//! deterministe selon C-13) sans toucher d'API non sure ; `UNKNOWN` sinon (P-02).

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

use rfx_bytecode::{access, CallTarget, ClassFile, FieldInfo, MemberRef, Op};
use rfx_model::{DeterminismFacts, Domain, Precision, ThreadFacts, Tri};

use crate::callgraph::CallGraph;
use crate::dataflow::{
    escaping_allocations, DomainTable, GraphFacts, Source, Sources, Target, WorkloadEffects,
};
use crate::hierarchy::{ClassIndex, OBJECT};

/// Domaines de l'etat du jeu : les ecrire exige le fil autoritatif (DM-09).
const GAME_STATE: [Domain; 11] = [
    Domain::WorldBlocks,
    Domain::WorldFluids,
    Domain::WorldLighting,
    Domain::ChunkData,
    Domain::ChunkStatus,
    Domain::EntityState,
    Domain::EntityInventory,
    Domain::BlockEntityState,
    Domain::LevelGlobals,
    Domain::Registries,
    Domain::Capabilities,
];

/// Classe des variables locales de fil.
const THREAD_LOCAL: &str = "java/lang/ThreadLocal";

/// Profondeur au-dela de laquelle une chaine de super-classes est tenue pour incoherente.
const MAX_DEPTH: usize = 256;

/// Confinement derive des classes (etape 3 de la fiche), calcule a la demande et
/// memorise.
///
/// Une classe est non sure entre fils si elle, ou une de ses super-classes, declare un
/// champ ni `final` ni `volatile` qui est soit non prive, soit prive mais dans un nid
/// (des classes imbriquees peuvent l'ecrire), soit ecrit par une de ses methodes autre
/// que le constructeur (ou `<clinit>` pour un statique) et non `synchronized`. Une
/// classe dont le bytecode ou une super-classe manque est tenue pour non sure ; ce
/// resultat-la n'est pas memorise, le bytecode pouvant arriver plus tard.
#[derive(Debug, Default)]
pub struct ThreadSafetyTable {
    memo: RefCell<HashMap<String, bool>>,
}

impl ThreadSafetyTable {
    /// Table vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` si les objets de `class` ne sont pas surs entre fils, ou si on ne peut pas
    /// le prouver.
    #[must_use]
    pub fn is_unsafe(&self, index: &ClassIndex, class: &str) -> bool {
        if let Some(known) = self.memo.borrow().get(class) {
            return *known;
        }
        match compute_unsafe(index, class) {
            Some(found) => {
                self.memo.borrow_mut().insert(class.to_owned(), found);
                found
            }
            None => true,
        }
    }
}

/// `Some(true)` non sure, `Some(false)` sure, `None` invérifiable.
fn compute_unsafe(index: &ClassIndex, class: &str) -> Option<bool> {
    if class.starts_with('[') {
        return Some(true); // un tableau est toujours mutable
    }
    let mut current = class;
    for _ in 0..MAX_DEPTH {
        if current == OBJECT {
            return Some(false);
        }
        let file = index.class(current)?;
        let mutable = file.fields.iter().any(|field| {
            field.is_plain_mutable()
                && (field.access & access::PRIVATE == 0
                    || file.nested
                    || written_outside_construction(file, field))
        });
        if mutable {
            return Some(true);
        }
        current = file.super_name.as_deref()?;
    }
    None
}

/// `true` si une methode de `class`, hors construction et non `synchronized`, ecrit
/// `field` ; une methode illisible est tenue pour l'ecrire.
fn written_outside_construction(class: &ClassFile, field: &FieldInfo) -> bool {
    let builder = if field.is_static() {
        "<clinit>"
    } else {
        "<init>"
    };
    class.methods.iter().any(|method| {
        if method.name == builder || method.access & access::SYNCHRONIZED != 0 {
            return false;
        }
        let Ok(instructions) = class.instructions(method) else {
            return true;
        };
        instructions.iter().any(|insn| match &insn.op {
            Op::PutField(target) | Op::PutStatic(target) => {
                target.owner == class.name && target.name == field.name
            }
            _ => false,
        })
    })
}

/// `true` si l'appel prend ou attend un verrou.
pub(crate) fn is_lock_api(target: &MemberRef) -> bool {
    target.owner.starts_with("java/util/concurrent/locks/")
        || (target.owner == "java/util/concurrent/Semaphore" && target.name.starts_with("acquire"))
        // `wait` est finale dans Object : quel que soit le type statique du receveur.
        || (target.name == "wait" && matches!(target.descriptor.as_str(), "()V" | "(J)V" | "(JI)V"))
}

/// `true` si la base n'atteint que des objets alloues par la methode et non echappes.
fn local(base: &Sources, escaping: &BTreeSet<u32>) -> bool {
    !base.is_null()
        && base
            .iter()
            .all(|source| matches!(source, Source::New(site) if !escaping.contains(&site)))
}

/// Faits de fil de l'unite dont `graph` est le graphe d'appel, ses methodes analysees
/// (`analysed`) et ses effets composes (`effects`, C-11) ; `determinism` vient de C-13
/// s'il existe.
#[must_use]
#[allow(clippy::too_many_arguments)] // les sorties des analyses dont C-12 depend
pub fn thread_facts(
    graph: &CallGraph,
    index: &ClassIndex,
    analysed: &GraphFacts,
    effects: &WorkloadEffects,
    domains: &DomainTable,
    safety: &ThreadSafetyTable,
    determinism: Option<&DeterminismFacts>,
) -> ThreadFacts {
    let incomplete = effects.causes.any();
    let mut synchronized = false;
    let mut locks = false;
    let mut thread_locals = effects.reads.domains.contains(Domain::ThreadLocals)
        || effects.writes.domains.contains(Domain::ThreadLocals);
    let mut touches = incomplete;

    for (node, facts) in graph.nodes().iter().zip(&analysed.facts) {
        let method = index
            .class(&node.method.owner)
            .and_then(|class| class.method(&node.method.name, &node.method.descriptor));
        synchronized |= method.is_some_and(|m| m.access & access::SYNCHRONIZED != 0);
        let Some(facts) = facts else {
            continue;
        };
        synchronized |= !facts.monitors.is_empty();
        for call in &facts.calls {
            if let CallTarget::Member(target) = &call.target {
                locks |= is_lock_api(target);
                thread_locals |= target.owner == THREAD_LOCAL
                    || domains
                        .of_class(index, &target.owner)
                        .is_some_and(|d| d.contains(Domain::ThreadLocals));
            }
        }
        if touches {
            continue;
        }
        let escaping = escaping_allocations(facts);
        touches = facts
            .reads
            .iter()
            .chain(&facts.writes)
            .any(|access| match &access.target {
                Target::Static(field) => safety.is_unsafe(index, &field.owner),
                Target::Field(field) => {
                    !access.base.is_null()
                        && !local(&access.base, &escaping)
                        && safety.is_unsafe(index, &field.owner)
                }
                Target::Element => !access.base.is_null() && !local(&access.base, &escaping),
            });
    }

    let writes = effects.writes;
    let writes_state = GAME_STATE.iter().any(|d| writes.domains.contains(*d));
    let pure = determinism.is_some_and(|d| d.deterministic == Tri::Yes)
        && writes.precision != Precision::Unknown
        && writes.domains.is_empty()
        && !effects.side_effects.unknown
        && !effects.side_effects.any();
    let main_thread_required = if touches || writes_state {
        Tri::Yes
    } else if pure {
        Tri::No
    } else {
        Tri::Unknown
    };
    let nodes = graph.nodes();
    let reentrant = if nodes.first().is_some_and(|root| root.recursive) {
        Tri::Yes
    } else if !incomplete && nodes.iter().all(|node| !node.recursive) {
        Tri::No
    } else {
        Tri::Unknown
    };
    ThreadFacts {
        main_thread_required,
        uses_synchronized: synchronized,
        uses_locks: locks,
        uses_thread_locals: thread_locals,
        touches_non_threadsafe_api: touches,
        reentrant,
        observed_roles: None,
    }
}
