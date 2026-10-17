//! Analyse intraprocedurale de C-11 sur des classes compilees par `javac` (ADR-055).
//!
//! Les classes sont celles de `rfx-bytecode/tests/fixtures`, `Effects` en tete. Les
//! positions d'instructions ne sont jamais ecrites en dur : une source qui designe un
//! site est verifiee par le fait que la methode consigne a ce site.

use std::path::PathBuf;

use rfx_analyzer::dataflow::{
    analyze, Access, DataflowLimits, MethodFacts, Source, Sources, Target, Unanalysable,
};
use rfx_bytecode::{CallTarget, ClassFile};

fn class(name: &str) -> ClassFile {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rfx-bytecode/tests/fixtures/classes/fixtures")
        .join(format!("{name}.class"));
    ClassFile::parse(&std::fs::read(path).expect("classe de test")).expect("lisible")
}

fn facts(name: &str, descriptor: &str) -> MethodFacts {
    let class = class("Effects");
    let method = class.method(name, descriptor).expect("methode");
    analyze(&class, method, &DataflowLimits::DEFAULT).expect("analysable")
}

fn sources(list: &[Source]) -> Sources {
    list.iter()
        .fold(Sources::NULL, |acc, s| acc.union(&Sources::one(*s)).0)
}

fn field_name(access: &Access) -> Option<&str> {
    match &access.target {
        Target::Field(field) | Target::Static(field) => Some(field.name.as_str()),
        Target::Element => None,
    }
}

/// Lecture consignee au site `offset`.
fn read_at(facts: &MethodFacts, offset: u32) -> &Access {
    facts
        .reads
        .iter()
        .find(|read| read.offset == offset)
        .expect("lecture a ce site")
}

#[test]
fn a_method_without_memory_access_has_no_facts() {
    let pure = facts("pureAdd", "(II)I");
    assert!(pure.reads.is_empty() && pure.writes.is_empty() && pure.calls.is_empty());
    assert!(pure.allocations.is_empty() && pure.monitors.is_empty());
    assert!(pure.throws.is_empty());
    assert_eq!(pure.returns, None, "rend un int");
}

#[test]
fn field_writes_carry_their_base() {
    // T-201 : l'ecriture est vue, et sur le bon objet.
    let own = facts("setValue", "(I)V");
    assert_eq!(own.writes.len(), 1);
    assert_eq!(field_name(&own.writes[0]), Some("value"));
    assert_eq!(
        own.writes[0].base,
        Sources::one(Source::Param(0)),
        "receveur"
    );
    assert_eq!(own.writes[0].value, None, "un int n'est pas une reference");

    let other = facts("setOther", "(Lfixtures/Effects;I)V");
    assert_eq!(
        other.writes[0].base,
        Sources::one(Source::Param(0)),
        "statique : 0 est `other`"
    );

    let joined = facts("chooseAndSet", "(Lfixtures/Effects;Lfixtures/Effects;Z)V");
    assert_eq!(
        joined.writes[0].base,
        sources(&[Source::Param(0), Source::Param(1)]),
        "l'un ou l'autre parametre"
    );
}

#[test]
fn array_aliases_lead_back_to_the_field_read() {
    // T-203 : le tableau ecrit par la variable locale est celui du champ `numbers`.
    let alias = facts("aliasArray", "(I)V");
    assert_eq!(alias.writes.len(), 1);
    let write = &alias.writes[0];
    assert_eq!(write.target, Target::Element);
    let [Source::Field(site)] = write.base.iter().collect::<Vec<_>>()[..] else {
        panic!("base attendue : une lecture de champ, vu {:?}", write.base);
    };
    let read = read_at(&alias, site);
    assert_eq!(field_name(read), Some("numbers"));
    assert_eq!(read.base, Sources::one(Source::Param(0)));

    let fresh = facts("fresh", "(I)[I");
    let [Source::New(site)] = fresh.writes[0].base.iter().collect::<Vec<_>>()[..] else {
        panic!("tableau alloue attendu");
    };
    assert!(fresh
        .allocations
        .iter()
        .any(|a| a.offset == site && a.class.is_none()));
    assert_eq!(fresh.returns, Some(Sources::one(Source::New(site))));
}

#[test]
fn chained_and_static_accesses_are_traced() {
    let chain = facts("chain", "()V");
    let [Source::Field(site)] = chain.writes[0].base.iter().collect::<Vec<_>>()[..] else {
        panic!("base attendue : la valeur de `next`");
    };
    assert_eq!(field_name(read_at(&chain, site)), Some("next"));

    let bump = facts("bump", "()V");
    assert_eq!(field_name(&bump.reads[0]), Some("counter"));
    assert_eq!(field_name(&bump.writes[0]), Some("counter"));
    assert!(matches!(bump.writes[0].target, Target::Static(_)));
    assert!(bump.writes[0].base.is_null(), "un statique n'a pas de base");

    let escape = facts("escape", "(Ljava/lang/Object;)V");
    assert_eq!(field_name(&escape.writes[0]), Some("shared"));
    assert_eq!(
        escape.writes[0].value,
        Some(Sources::one(Source::Param(0))),
        "le parametre s'echappe dans le statique"
    );
}

#[test]
fn calls_carry_argument_sources_and_returns_follow_them() {
    let via = facts("viaCall", "(Lfixtures/Effects;)Ljava/lang/Object;");
    assert_eq!(via.calls.len(), 1);
    let call = &via.calls[0];
    assert!(matches!(&call.target, CallTarget::Member(m) if m.name == "self"));
    assert_eq!(call.args, vec![Some(Sources::one(Source::Param(0)))]);
    assert_eq!(via.returns, Some(Sources::one(Source::Return(call.offset))));

    let own = facts("self", "()Lfixtures/Effects;");
    assert_eq!(own.returns, Some(Sources::one(Source::Param(0))));
}

#[test]
fn monitors_handlers_and_throws_are_reported() {
    let locked = facts("locked", "()V");
    assert_eq!(locked.monitors.len(), 1);
    assert_eq!(locked.monitors[0].lock, Sources::one(Source::Param(0)));
    // La relance du gestionnaire implicite n'est couverte par aucun gestionnaire :
    // l'exception attrapee peut sortir.
    assert_eq!(locked.throws.len(), 1);
    assert!(matches!(
        locked.throws[0].value.iter().collect::<Vec<_>>()[..],
        [Source::Caught(_)]
    ));

    let thrower = facts("thrower", "(Ljava/lang/Object;)V");
    let [Source::New(site)] = thrower.throws[0].value.iter().collect::<Vec<_>>()[..] else {
        panic!("exception allouee attendue");
    };
    assert!(
        thrower
            .allocations
            .iter()
            .any(|a| a.offset == site
                && a.class.as_deref() == Some("java/lang/IllegalStateException"))
    );

    let caught = facts("caught", "()V");
    let stored = caught
        .writes
        .iter()
        .find(|w| field_name(w) == Some("ref"))
        .expect("ecriture de `ref`");
    let handler = class("Effects")
        .method("caught", "()V")
        .expect("methode")
        .handlers[0]
        .handler;
    assert_eq!(stored.value, Some(Sources::one(Source::Caught(handler))));
    assert!(caught.throws.is_empty());
}

#[test]
fn a_handler_sees_locals_stored_inside_its_protected_range() {
    // `target` vaut `a` a l'entree du bloc protege, puis `b` : le gestionnaire doit
    // envisager les deux.
    let stored = facts("storeInTry", "(Lfixtures/Effects;Lfixtures/Effects;)V");
    assert_eq!(stored.writes.len(), 1);
    assert_eq!(
        stored.writes[0].base,
        sources(&[Source::Param(0), Source::Param(1)])
    );
}

#[test]
fn only_a_catch_all_handler_retains_a_throw() {
    // Le gestionnaire ne prend que IllegalArgumentException : l'IllegalStateException
    // levee peut sortir. Sans connaitre le type leve, seul un gestionnaire universel
    // suffit a dire le contraire.
    let narrow = facts("rethrowNarrow", "()V");
    assert_eq!(narrow.throws.len(), 1);
}

#[test]
fn every_return_contributes_to_the_returned_sources() {
    let pick = facts(
        "pick",
        "(Ljava/lang/Object;Ljava/lang/Object;Z)Ljava/lang/Object;",
    );
    assert_eq!(
        pick.returns,
        Some(sources(&[Source::Param(0), Source::Param(1)]))
    );
}

#[test]
fn loops_reach_a_fixed_point_and_null_adds_no_source() {
    let last = facts("last", "([Ljava/lang/Object;)Ljava/lang/Object;");
    let returned: Vec<Source> = last.returns.expect("rend une reference").iter().collect();
    assert!(
        matches!(returned[..], [Source::Element(_)]),
        "null ou un element : {returned:?}"
    );
    assert!(last.work.max_block_visits >= 2, "la boucle est repassee");
}

#[test]
fn methods_without_code_or_beyond_budget_are_unanalysable() {
    let effects = class("Effects");
    let method = effects.method("aliasArray", "(I)V").expect("methode");
    let tight = DataflowLimits {
        max_cells: 1,
        max_visits_per_block: 64,
    };
    assert_eq!(analyze(&effects, method, &tight), Err(Unanalysable::Budget));

    let loop_method = effects
        .method("last", "([Ljava/lang/Object;)Ljava/lang/Object;")
        .expect("methode");
    let once = DataflowLimits {
        max_cells: usize::MAX,
        max_visits_per_block: 1,
    };
    assert_eq!(
        analyze(&effects, loop_method, &once),
        Err(Unanalysable::Budget)
    );

    let shape = class("Shape");
    let area = shape
        .methods
        .iter()
        .find(|m| m.code.is_none())
        .expect("methode abstraite");
    assert_eq!(
        analyze(&shape, area, &DataflowLimits::DEFAULT),
        Err(Unanalysable::NoCode)
    );
}

#[test]
fn every_fixture_method_is_analysable() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rfx-bytecode/tests/fixtures/classes/fixtures");
    let mut analysed = 0;
    for entry in std::fs::read_dir(dir).expect("classes de test") {
        let path = entry.expect("entree").path();
        let class = ClassFile::parse(&std::fs::read(&path).expect("lecture")).expect("lisible");
        for method in class.methods.iter().filter(|m| m.code.is_some()) {
            analyze(&class, method, &DataflowLimits::DEFAULT)
                .unwrap_or_else(|e| panic!("{}.{} : {e}", class.name, method.name));
            analysed += 1;
        }
    }
    assert!(analysed >= 150, "{analysed} methodes");
}
