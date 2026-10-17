//! Effets intraproceduraux de C-11 ramenes aux domaines memoire (ADR-055).
//!
//! Les classes de test ne peuvent pas heriter des vraies classes de Minecraft, qu'on ne
//! redistribue pas : la place de `fixtures/Effects` dans la hierarchie est donc
//! declaree par le test, sous l'entite de Minecraft ou sous `Object`. Seul le squelette
//! est declare, jamais le bytecode, pour que cette declaration fasse foi.

use std::path::PathBuf;

use rfx_analyzer::dataflow::{
    analyze, intraprocedural_effects, DataflowLimits, DomainTable, MethodEffects,
};
use rfx_analyzer::hierarchy::{ClassIndex, Skeleton};
use rfx_bytecode::ClassFile;
use rfx_model::{Domain, Domains, Precision};

const ENTITY: &str = "net/minecraft/world/entity/Entity";
const OBJECT: &str = "java/lang/Object";

fn effects_class() -> ClassFile {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rfx-bytecode/tests/fixtures/classes/fixtures/Effects.class");
    ClassFile::parse(&std::fs::read(path).expect("classe de test")).expect("lisible")
}

/// Hierarchie ou `fixtures/Effects` descend de `parent`.
fn hierarchy(parent: &str) -> ClassIndex {
    let mut index = ClassIndex::new();
    let declare = |index: &mut ClassIndex, name: &str, parent: &str| {
        index.declare(
            name,
            Skeleton {
                super_name: Some(parent.to_owned()),
                access: 0,
            },
        );
    };
    declare(&mut index, ENTITY, OBJECT);
    declare(&mut index, "fixtures/Effects", parent);
    index
}

fn effects_under(parent: &str, name: &str, descriptor: &str) -> MethodEffects {
    let class = effects_class();
    let method = class.method(name, descriptor).expect("methode");
    let facts = analyze(&class, method, &DataflowLimits::DEFAULT).expect("analysable");
    intraprocedural_effects(&facts, &hierarchy(parent), &DomainTable::new())
}

fn effects(name: &str, descriptor: &str) -> MethodEffects {
    effects_under(OBJECT, name, descriptor)
}

fn only(domain: Domain) -> Domains {
    Domains::EMPTY.with(domain)
}

#[test]
fn a_pure_method_touches_nothing_exactly() {
    let pure = effects("pureAdd", "(II)I");
    assert_eq!(pure.reads.domains, Domains::EMPTY);
    assert_eq!(pure.writes.domains, Domains::EMPTY);
    assert_eq!(pure.writes.precision, Precision::Exact);
    assert!(!pure.side_effects.any() && !pure.side_effects.unknown);
}

#[test]
fn a_field_takes_the_domain_of_its_class() {
    // T-201 : sous l'entite, l'ecriture est EntityState, exacte, et modifie le monde.
    let entity = effects_under(ENTITY, "setValue", "(I)V");
    assert_eq!(entity.writes.domains, only(Domain::EntityState));
    assert_eq!(entity.writes.precision, Precision::Exact);
    assert!(entity.side_effects.mutates_world);

    // Sans ancre, la classe n'a pas de domaine : inconnu, en conflit avec tout.
    let plain = effects("setValue", "(I)V");
    assert_eq!(plain.writes.domains, only(Domain::Unknown));
    assert_eq!(plain.writes.precision, Precision::Unknown);
    assert!(plain.side_effects.mutates_world, "pire cas (R-206)");
}

#[test]
fn statics_are_mod_or_java_statics() {
    let bump = effects("bump", "()V");
    assert_eq!(bump.reads.domains, only(Domain::ModStatics));
    assert_eq!(bump.writes.domains, only(Domain::ModStatics));
    assert_eq!(bump.writes.precision, Precision::Exact);
    assert!(!bump.side_effects.mutates_world);
}

#[test]
fn local_objects_touch_no_domain() {
    let local = effects("localOnly", "()I");
    assert!(local.escaping.is_empty());
    assert_eq!(local.writes.domains, Domains::EMPTY);
    assert_eq!(local.writes.precision, Precision::Exact);
    assert!(!local.side_effects.mutates_world);
}

#[test]
fn escape_is_transitive_and_arrays_are_unknown() {
    // T-203 : l'interieur s'echappe parce que l'exterieur, qui le contient, s'echappe.
    let nested = effects("nested", "()V");
    assert_eq!(nested.escaping.len(), 2, "les deux tableaux");
    assert!(nested.writes.domains.contains(Domain::Unknown));
    assert!(nested.writes.domains.contains(Domain::ModStatics));

    let fresh = effects("fresh", "(I)[I");
    assert_eq!(fresh.escaping.len(), 1, "rendu");
    assert_eq!(fresh.writes.domains, only(Domain::Unknown));
}

#[test]
fn calls_make_the_sets_unknown() {
    // R-380 : un appel non suivi rend les ensembles inconnus.
    let via = effects("viaCall", "(Lfixtures/Effects;)Ljava/lang/Object;");
    assert_eq!(via.reads.precision, Precision::Unknown);
    assert_eq!(via.writes.precision, Precision::Unknown);
    assert!(via.side_effects.unknown);
}

#[test]
fn throws_and_locks_are_reported() {
    let thrower = effects("thrower", "(Ljava/lang/Object;)V");
    assert!(thrower.side_effects.throws_observable);

    let locked = effects_under(ENTITY, "locked", "()V");
    assert!(locked.locks);
    assert_eq!(locked.writes.domains, only(Domain::EntityState));
    assert!(
        locked.side_effects.throws_observable,
        "relance du gestionnaire"
    );
}
