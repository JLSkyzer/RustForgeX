//! Effets d'une methode ramenes aux domaines memoire (DM-06, DM-07), sans suivre ses
//! appels : phase intraprocedurale (ADR-055).
//!
//! # Regles
//!
//! - Un objet alloue par la methode et qui ne s'en echappe pas est local : ses acces ne
//!   touchent aucun domaine. Il s'echappe s'il est ecrit dans un champ, un statique ou
//!   un tableau non local, passe a un appel, rendu ou leve ; ce qui est ecrit dans un
//!   objet qui s'echappe s'echappe aussi.
//! - Un champ d'instance prend les domaines de sa classe de reference
//!   ([`DomainTable`]) ; sans domaine, `Unknown`.
//! - Un statique est `JavaStatics` s'il appartient au JDK, `ModStatics` sinon.
//! - Un element de tableau non local est `Unknown` : la classe d'un tableau n'est pas
//!   suivie, et son contenu peut etre partage avec n'importe quoi.
//! - Un appel n'est pas suivi a cette phase : il rend l'ensemble `UNKNOWN` (R-380).
//!
//! La precision est `UNKNOWN` des que le domaine `Unknown` apparait ou qu'un appel
//! existe ; `OVER_APPROX` si une base melange objets locaux et partages ; `EXACT`
//! sinon.

use std::collections::BTreeSet;

use rfx_model::{AccessSet, Domain, Domains, Precision, SideEffectSet};

use super::domains::DomainTable;
use super::facts::{Access, MethodFacts, Target};
use super::value::{Source, Sources};
use crate::hierarchy::ClassIndex;

/// Paquets du JDK, dont les statiques sont `JavaStatics`.
const JDK_PACKAGES: [&str; 5] = ["java/", "javax/", "jdk/", "sun/", "com/sun/"];

/// Domaines qu'une ecriture rend « modification du monde » (DM-07).
pub(super) const WORLD: [Domain; 9] = [
    Domain::WorldBlocks,
    Domain::WorldFluids,
    Domain::WorldLighting,
    Domain::ChunkData,
    Domain::ChunkStatus,
    Domain::EntityState,
    Domain::EntityInventory,
    Domain::BlockEntityState,
    Domain::LevelGlobals,
];

/// Effets d'une methode, ses appels non suivis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodEffects {
    /// Lectures.
    pub reads: AccessSet,
    /// Ecritures.
    pub writes: AccessSet,
    /// Effets de bord.
    pub side_effects: SideEffectSet,
    /// Un verrou est pris sur un objet non local.
    pub locks: bool,
    /// Sites d'allocation dont l'objet s'echappe.
    pub escaping: BTreeSet<u32>,
}

/// Portee d'une base d'acces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// `null` seul : l'acces leve une exception, il n'atteint rien.
    Null,
    /// Uniquement des objets locaux.
    Local,
    /// Uniquement des objets partages.
    Shared,
    /// Les deux selon le chemin.
    Mixed,
}

fn reach(base: &Sources, escaping: &BTreeSet<u32>) -> Reach {
    if base.is_null() {
        return Reach::Null;
    }
    let local = |source: Source| matches!(source, Source::New(site) if !escaping.contains(&site));
    let locals = base.iter().filter(|s| local(*s)).count();
    match locals {
        0 => Reach::Shared,
        n if n == base.iter().count() => Reach::Local,
        _ => Reach::Mixed,
    }
}

fn new_sites(sources: &Sources) -> impl Iterator<Item = u32> + '_ {
    sources.iter().filter_map(|source| match source {
        Source::New(site) => Some(site),
        _ => None,
    })
}

/// Sites d'allocation dont l'objet s'echappe de la methode, jusqu'au point fixe : un
/// objet ecrit dans un objet qui s'echappe s'echappe a son tour.
#[must_use]
pub fn escaping_allocations(facts: &MethodFacts) -> BTreeSet<u32> {
    let mut escaping = BTreeSet::new();
    let direct = facts
        .calls
        .iter()
        .flat_map(|call| call.args.iter().flatten())
        .chain(facts.returns.iter())
        .chain(facts.throws.iter().map(|throw| &throw.value));
    for sources in direct {
        escaping.extend(new_sites(sources));
    }
    loop {
        let before = escaping.len();
        for write in &facts.writes {
            let Some(value) = &write.value else { continue };
            let into_shared = match write.target {
                Target::Static(_) => true,
                Target::Field(_) | Target::Element => {
                    matches!(reach(&write.base, &escaping), Reach::Shared | Reach::Mixed)
                }
            };
            if into_shared {
                escaping.extend(new_sites(value));
            }
        }
        if escaping.len() == before {
            return escaping;
        }
    }
}

/// Domaines d'un acces, ou `None` s'il n'atteint aucun domaine ; le booleen dit si la
/// base melange objets locaux et partages.
fn domains_of(
    access: &Access,
    escaping: &BTreeSet<u32>,
    index: &ClassIndex,
    table: &DomainTable,
) -> Option<(Domains, bool)> {
    let domains = target_domains(&access.target, index, table);
    match &access.target {
        Target::Static(_) => Some((domains, false)),
        Target::Field(_) | Target::Element => match reach(&access.base, escaping) {
            Reach::Null | Reach::Local => None,
            shared => Some((domains, shared == Reach::Mixed)),
        },
    }
}

/// Domaines de ce qu'un acces atteint, sa base etant partagee : un statique est
/// `JavaStatics` ou `ModStatics`, un champ prend les domaines de sa classe ou `Unknown`,
/// un element de tableau est `Unknown`.
pub(super) fn target_domains(target: &Target, index: &ClassIndex, table: &DomainTable) -> Domains {
    let unknown = Domains::EMPTY.with(Domain::Unknown);
    match target {
        Target::Static(field) => {
            let jdk = JDK_PACKAGES.iter().any(|p| field.owner.starts_with(p));
            Domains::EMPTY.with(if jdk {
                Domain::JavaStatics
            } else {
                Domain::ModStatics
            })
        }
        Target::Field(field) => table.of_class(index, &field.owner).unwrap_or(unknown),
        Target::Element => unknown,
    }
}

pub(super) fn union(a: Domains, b: Domains) -> Domains {
    Domains(a.0 | b.0)
}

fn access_set(
    accesses: &[Access],
    escaping: &BTreeSet<u32>,
    index: &ClassIndex,
    table: &DomainTable,
    calls: bool,
) -> AccessSet {
    let mut domains = Domains::EMPTY;
    let mut mixed = false;
    for access in accesses {
        if let Some((found, partly_local)) = domains_of(access, escaping, index, table) {
            domains = union(domains, found);
            mixed |= partly_local;
        }
    }
    if calls {
        domains = domains.with(Domain::Unknown);
    }
    let precision = if domains.contains(Domain::Unknown) {
        Precision::Unknown
    } else if mixed {
        Precision::OverApprox
    } else {
        Precision::Exact
    };
    AccessSet { domains, precision }
}

/// Effets de la methode dont `facts` decrit le code, ses appels non suivis.
#[must_use]
pub fn intraprocedural_effects(
    facts: &MethodFacts,
    index: &ClassIndex,
    table: &DomainTable,
) -> MethodEffects {
    let escaping = escaping_allocations(facts);
    let calls = !facts.calls.is_empty();
    let reads = access_set(&facts.reads, &escaping, index, table, calls);
    let writes = access_set(&facts.writes, &escaping, index, table, calls);
    let world = WORLD.iter().any(|d| writes.domains.contains(*d));
    let side_effects = SideEffectSet {
        // Une ecriture d'un domaine inconnu peut modifier le monde : pire cas (R-206).
        mutates_world: world || writes.domains.contains(Domain::Unknown),
        throws_observable: !facts.throws.is_empty(),
        unknown: calls,
        ..SideEffectSet::default()
    };
    let locks = facts.monitors.iter().any(|monitor| {
        matches!(
            reach(&monitor.lock, &escaping),
            Reach::Shared | Reach::Mixed
        )
    });
    MethodEffects {
        reads,
        writes,
        side_effects,
        locks,
        escaping,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataflow::facts::Access;
    use crate::hierarchy::Skeleton;
    use rfx_bytecode::MemberRef;

    fn field(owner: &str) -> Target {
        Target::Field(MemberRef {
            owner: owner.to_owned(),
            name: "x".to_owned(),
            descriptor: "I".to_owned(),
            interface: false,
        })
    }

    fn write(offset: u32, target: Target, base: Sources, value: Option<Sources>) -> Access {
        Access {
            offset,
            target,
            base,
            value,
        }
    }

    fn entity_index() -> ClassIndex {
        let mut index = ClassIndex::new();
        index.declare(
            "net/minecraft/world/entity/Entity",
            Skeleton {
                super_name: Some("java/lang/Object".to_owned()),
                access: 0,
            },
        );
        index
    }

    #[test]
    fn a_base_mixing_local_and_shared_objects_is_over_approximated() {
        let base = Sources::one(Source::New(0))
            .union(&Sources::one(Source::Param(0)))
            .0;
        let facts = MethodFacts {
            writes: vec![write(
                4,
                field("net/minecraft/world/entity/Entity"),
                base,
                None,
            )],
            ..MethodFacts::default()
        };
        let effects = intraprocedural_effects(&facts, &entity_index(), &DomainTable::new());
        assert_eq!(
            effects.writes.domains,
            Domains::EMPTY.with(Domain::EntityState)
        );
        assert_eq!(effects.writes.precision, Precision::OverApprox);
    }

    #[test]
    fn a_null_base_reaches_nothing_and_lets_nothing_escape() {
        let facts = MethodFacts {
            writes: vec![write(
                4,
                field("example/Anything"),
                Sources::NULL,
                Some(Sources::one(Source::New(0))),
            )],
            ..MethodFacts::default()
        };
        let effects = intraprocedural_effects(&facts, &entity_index(), &DomainTable::new());
        assert_eq!(effects.writes.domains, Domains::EMPTY);
        assert_eq!(effects.writes.precision, Precision::Exact);
        assert!(effects.escaping.is_empty());
    }

    #[test]
    fn statics_of_the_jdk_are_java_statics() {
        let target = Target::Static(MemberRef {
            owner: "java/lang/System".to_owned(),
            name: "out".to_owned(),
            descriptor: "Ljava/io/PrintStream;".to_owned(),
            interface: false,
        });
        let facts = MethodFacts {
            reads: vec![write(0, target, Sources::NULL, None)],
            ..MethodFacts::default()
        };
        let effects = intraprocedural_effects(&facts, &entity_index(), &DomainTable::new());
        assert_eq!(
            effects.reads.domains,
            Domains::EMPTY.with(Domain::JavaStatics)
        );
    }
}
