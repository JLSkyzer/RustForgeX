//! Greffons de lancement de Forge reproduits a la fourniture du bytecode (ADR-059).
//!
//! Les classes de test sont modifiees en memoire pour prendre la forme que chaque
//! greffon selectionne : aucune classe de Forge n'est copiee ici.

use std::path::PathBuf;

use rfx_analyzer::hierarchy::Skeleton;
use rfx_analyzer::platform::{
    as_loaded, CAPABILITY_TOKEN, EVENT, EXTENSIBLE_ENUM, OBJECT_HOLDER, VANILLA_HOLDERS,
};
use rfx_analyzer::ClassIndex;
use rfx_bytecode::{access, ClassFile};

fn fixture(name: &str) -> ClassFile {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../rfx-bytecode/tests/fixtures/classes/fixtures/{name}.class"
    ));
    ClassFile::parse(&std::fs::read(path).expect("lecture")).expect("lisible")
}

fn declared(entries: &[(&str, Option<&str>)]) -> ClassIndex {
    let mut index = ClassIndex::new();
    for (name, parent) in entries {
        index.declare(
            name,
            Skeleton {
                super_name: parent.map(str::to_owned),
                access: access::PUBLIC,
            },
        );
    }
    index
}

#[test]
fn rewritten_classes_are_absent() {
    let index = ClassIndex::new();
    let mut extensible = fixture("Calls");
    extensible.access |= 0x4000;
    extensible.interfaces.push(EXTENSIBLE_ENUM.to_owned());
    assert!(
        as_loaded(extensible, &index).is_none(),
        "RuntimeEnumExtender"
    );

    let mut token_child = fixture("Calls");
    token_child.super_name = Some(CAPABILITY_TOKEN.to_owned());
    assert!(
        as_loaded(token_child, &index).is_none(),
        "sous-classe directe du jeton"
    );
    let mut token = fixture("Calls");
    token.name = CAPABILITY_TOKEN.to_owned();
    assert!(as_loaded(token, &index).is_none(), "le jeton lui-meme");

    let mut event = fixture("Calls");
    event.name = EVENT.to_owned();
    assert!(as_loaded(event, &index).is_none(), "Event lui-meme");

    // Un enum ordinaire, ou une classe qui implemente l'interface sans etre un enum,
    // reste lisible tel quel.
    let mut plain_enum = fixture("Calls");
    plain_enum.access |= 0x4000;
    assert!(as_loaded(plain_enum, &index).is_some());
}

#[test]
fn descendants_of_event_and_unknown_chains_are_absent() {
    let index = declared(&[
        ("mod/BaseEvent", Some(EVENT)),
        ("mod/Plain", Some("java/lang/Object")),
        ("mod/Orphan", None),
    ]);
    let with_parent = |parent: &str| {
        let mut class = fixture("Calls");
        class.super_name = Some(parent.to_owned());
        class
    };
    assert!(
        as_loaded(with_parent("mod/BaseEvent"), &index).is_none(),
        "petite-fille d'Event"
    );
    assert!(
        as_loaded(with_parent(EVENT), &index).is_none(),
        "fille d'Event"
    );
    assert!(
        as_loaded(with_parent("mod/Missing"), &index).is_none(),
        "chaine inconnue"
    );
    assert!(
        as_loaded(with_parent("mod/Orphan"), &index).is_none(),
        "ancetre declare sans super-classe, qui n'est pas Object"
    );
    assert!(as_loaded(with_parent("mod/Plain"), &index).is_some());
    assert!(
        as_loaded(fixture("Calls"), &index).is_some(),
        "fille directe d'Object"
    );
}

#[test]
fn object_holders_lose_final_like_the_plugin_does() {
    let index = ClassIndex::new();
    let constant = access::PUBLIC | access::STATIC | access::FINAL;
    let shaped = |name: &str, annotated: bool, descriptor: &str, flags: u16| {
        let mut class = fixture("Threads");
        class.name = name.to_owned();
        let field = class
            .fields
            .iter_mut()
            .find(|f| f.name == "LOCAL")
            .expect("champ");
        field.access = flags;
        field.descriptor = descriptor.to_owned();
        if annotated {
            field.annotations.push(OBJECT_HOLDER.to_owned());
        }
        let loaded = as_loaded(class, &index).expect("lisible");
        let field = loaded
            .fields
            .iter()
            .find(|f| f.name == "LOCAL")
            .expect("champ");
        field.access & access::FINAL != 0
    };
    let object = "Ljava/lang/ThreadLocal;";
    assert!(
        !shaped("mod/Holders", true, object, constant),
        "@ObjectHolder"
    );
    assert!(
        shaped("mod/Holders", false, object, constant),
        "sans annotation"
    );
    assert!(shaped("mod/Holders", true, "I", constant), "type primitif");
    assert!(
        shaped("mod/Holders", true, object, access::STATIC | access::FINAL),
        "non public"
    );
    assert!(
        !shaped(VANILLA_HOLDERS[0], false, object, constant),
        "classe de Minecraft"
    );
}
