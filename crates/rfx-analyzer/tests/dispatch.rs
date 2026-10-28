//! Rappels possibles : unites dont un repartiteur peut decider l'appel, et dont
//! `order_deps` reste inconnu (ADR-060).

use std::path::PathBuf;

use rfx_analyzer::dependency::may_be_dispatched;
use rfx_analyzer::{ClassIndex, MethodRef};
use rfx_bytecode::ClassFile;

fn index() -> ClassIndex {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rfx-bytecode/tests/fixtures/classes/fixtures");
    ClassIndex::from_classes(
        std::fs::read_dir(dir)
            .expect("classes de test")
            .map(|entry| entry.expect("entree").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "class"))
            .map(|path| ClassFile::parse(&std::fs::read(path).expect("lecture")).expect("lisible")),
    )
}

/// Index ou la methode `name` de `Chance` porte en plus `ACC_SYNTHETIC`, comme un corps
/// de lambda ou un pont genere par le compilateur.
fn with_synthetic(name: &str) -> ClassIndex {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rfx-bytecode/tests/fixtures/classes/fixtures");
    let mut chance = ClassFile::parse(&std::fs::read(dir.join("Chance.class")).expect("lecture"))
        .expect("lisible");
    for method in chance.methods.iter_mut().filter(|m| m.name == name) {
        method.access |= rfx_bytecode::access::SYNTHETIC;
    }
    ClassIndex::from_classes([chance])
}

fn dispatched(owner: &str, name: &str, descriptor: &str) -> bool {
    may_be_dispatched(
        &index(),
        &MethodRef::new(&format!("fixtures/{owner}"), name, descriptor),
    )
}

#[test]
fn only_static_methods_with_primitive_parameters_have_a_known_order() {
    assert!(
        !dispatched("Chance", "pure", "(II)I"),
        "statique, primitifs"
    );
    assert!(!dispatched("Chance", "now", "()J"), "sans parametre");
}

#[test]
fn possible_callbacks_keep_an_unknown_order() {
    assert!(
        dispatched("Chance", "roll", "(Ljava/util/Random;)I"),
        "objet en parametre"
    );
    assert!(
        dispatched("Chance", "keys", "(Ljava/util/HashMap;)I"),
        "objet en parametre"
    );
    assert!(
        dispatched("Dog", "sound", "()Ljava/lang/String;"),
        "methode d'instance"
    );
    assert!(
        dispatched("Chance", "doubled", "(I)I"),
        "designee par une reference de methode"
    );
    assert!(
        dispatched("Chance", "first", "([I)I"),
        "tableau en parametre"
    );
    assert!(
        may_be_dispatched(
            &with_synthetic("pure"),
            &MethodRef::new("fixtures/Chance", "pure", "(II)I")
        ),
        "methode synthetique"
    );
    assert!(dispatched("Chance", "missing", "()V"), "methode inconnue");
    assert!(dispatched("Absent", "run", "()V"), "classe inconnue");
}
