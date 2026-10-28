//! Lecture des classes compilees par `javac` (`tests/fixtures`, ADR-050).

use std::collections::BTreeMap;
use std::path::PathBuf;

use rfx_bytecode::{CallTarget, ClassFile, InvokeKind, MemberRef, ParseError};

fn fixtures() -> BTreeMap<String, Vec<u8>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/classes/fixtures");
    let mut classes = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("classes de test") {
        let path = entry.expect("entree").path();
        if path.extension().is_some_and(|ext| ext == "class") {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            classes.insert(name, std::fs::read(&path).expect("lecture"));
        }
    }
    classes
}

fn parsed(name: &str) -> ClassFile {
    ClassFile::parse(&fixtures()[name]).expect("lisible")
}

/// Sites d'appel d'une methode, sous une forme lisible : `nature proprietaire.nom descripteur`.
fn calls(class: &ClassFile, name: &str, descriptor: &str) -> Vec<String> {
    let method = class.method(name, descriptor).expect("methode");
    class
        .call_sites(method)
        .expect("code lisible")
        .into_iter()
        .map(|site| match site.target {
            CallTarget::Member(MemberRef {
                owner,
                name,
                descriptor,
                ..
            }) => format!("{:?} {owner}.{name} {descriptor}", site.kind),
            CallTarget::Dynamic { name, .. } => format!("Dynamic {name}"),
        })
        .collect()
}

#[test]
fn every_fixture_parses_with_its_hierarchy() {
    let all = fixtures();
    assert_eq!(all.len(), 60, "classes : {:?}", all.keys());
    for (name, bytes) in &all {
        let class = ClassFile::parse(bytes).unwrap_or_else(|e| panic!("{name} : {e}"));
        assert_eq!(class.name, format!("fixtures/{name}"));
        assert_eq!(class.major_version, 61, "Java 17");
    }

    let calls = parsed("Calls");
    assert_eq!(calls.super_name.as_deref(), Some("java/lang/Object"));
    assert_eq!(parsed("Dog").super_name.as_deref(), Some("fixtures/Animal"));
    assert_eq!(
        parsed("Circle").interfaces,
        vec!["fixtures/Shape".to_owned()]
    );
    assert!(parsed("Shape").is_interface());
    assert!(parsed("Animal").is_abstract() && !parsed("Animal").is_final());
    assert!(parsed("Circle").is_final());
    let sound = parsed("Animal");
    assert!(sound
        .method("sound", "()Ljava/lang/String;")
        .unwrap()
        .is_abstract());
    assert!(sound
        .method("sound", "()Ljava/lang/String;")
        .unwrap()
        .code
        .is_none());
}

#[test]
fn direct_calls_come_out_in_code_order() {
    assert_eq!(
        calls(&parsed("Calls"), "direct", "()I"),
        vec![
            "Special fixtures/Calls.<init> ()V",
            "Static fixtures/Calls.helper (I)I",
            // Depuis Java 11 (JEP 181), un appel prive dans la meme classe est un
            // `invokevirtual`, pas un `invokespecial` : verifie par `javap`.
            "Virtual fixtures/Calls.secret ()I",
            "Virtual fixtures/Calls.sealed ()I",
        ]
    );
    assert_eq!(
        calls(&parsed("Derived"), "value", "()I"),
        vec!["Special fixtures/Base.value ()I"],
        "appel a super"
    );
    assert_eq!(
        calls(&parsed("Calls"), "bounded", "(Lfixtures/Shape;)D"),
        vec!["Interface fixtures/Shape.area ()D"]
    );
}

/// `tableswitch`, `lookupswitch` et `iinc_w` : une longueur fausse decalerait la lecture
/// et ferait perdre ou inventer des appels.
#[test]
fn variable_length_instructions_are_walked_exactly() {
    assert_eq!(
        calls(&parsed("Calls"), "switches", "(I)I"),
        vec![
            "Static fixtures/Calls.helper (I)I",
            "Static fixtures/Calls.helper (I)I",
            "Static fixtures/Calls.helper (I)I",
            "Static fixtures/Calls.helper (I)I",
            "Static fixtures/Calls.factorial (I)J",
        ]
    );
}

/// Les `switch` decales, precedes de deux octets de remplissage : les positions viennent
/// de `javap`, et une erreur d'alignement les decalerait toutes.
#[test]
fn padded_switches_keep_every_offset_exact() {
    let class = parsed("Calls");
    let offsets = |name: &str| -> Vec<u32> {
        let method = class.method(name, "(I)I").unwrap();
        class
            .call_sites(method)
            .unwrap()
            .iter()
            .map(|site| site.offset)
            .collect()
    };
    assert_eq!(offsets("padded"), vec![34, 40, 46, 51]);
    assert_eq!(offsets("sparse"), vec![33, 38]);
}

#[test]
fn invokedynamic_sites_carry_their_bootstrap_and_handles() {
    let class = parsed("Calls");
    let method = class.method("lambda", "()I").unwrap();
    let sites = class.call_sites(method).unwrap();
    let dynamic: Vec<_> = sites
        .iter()
        .filter_map(|site| match &site.target {
            CallTarget::Dynamic {
                bootstrap,
                handle_arguments,
                ..
            } => Some((site.kind, bootstrap, handle_arguments)),
            CallTarget::Member(_) => None,
        })
        .collect();
    assert_eq!(dynamic.len(), 2);
    for (kind, bootstrap, _) in &dynamic {
        assert_eq!(*kind, InvokeKind::Dynamic);
        assert_eq!(bootstrap.member.owner, "java/lang/invoke/LambdaMetafactory");
        assert_eq!(bootstrap.member.name, "metafactory");
    }
    let lambda_body = &dynamic[0].2[0].member;
    assert_eq!(lambda_body.owner, "fixtures/Calls");
    assert!(
        lambda_body.name.starts_with("lambda$"),
        "{}",
        lambda_body.name
    );
    let method_ref = &dynamic[1].2[0];
    assert_eq!(method_ref.kind, 6, "REF_invokeStatic");
    assert_eq!(method_ref.member.owner, "java/lang/Integer");
    assert_eq!(method_ref.member.name, "parseInt");

    let concat = class
        .method("concat", "(Ljava/lang/String;I)Ljava/lang/String;")
        .unwrap();
    match &class.call_sites(concat).unwrap()[0].target {
        CallTarget::Dynamic {
            bootstrap,
            handle_arguments,
            ..
        } => {
            assert_eq!(
                bootstrap.member.owner,
                "java/lang/invoke/StringConcatFactory"
            );
            assert!(handle_arguments.is_empty());
        }
        other => panic!("invokedynamic attendu : {other:?}"),
    }
}

#[test]
fn reflective_calls_are_visible() {
    let found = calls(&parsed("Calls"), "reflective", "()Ljava/lang/Object;");
    assert!(found.contains(
        &"Virtual java/lang/reflect/Method.invoke (Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;"
            .to_owned()
    ));
    assert!(found
        .iter()
        .any(|call| call.starts_with("Virtual java/lang/invoke/MethodHandle.invokeExact")));
}

#[test]
fn foreign_bytes_are_refused() {
    assert_eq!(ClassFile::parse(b""), Err(ParseError::BadMagic));
    assert_eq!(
        ClassFile::parse(b"PK\x03\x04 zip"),
        Err(ParseError::BadMagic)
    );
    let mut extended = fixtures()["Calls"].clone();
    extended.push(0);
    assert!(matches!(
        ClassFile::parse(&extended),
        Err(ParseError::Malformed(_))
    ));
}

/// PARTIE 19.2 : toute troncature de toute classe est une erreur, jamais une panic.
#[test]
fn every_truncation_is_an_error() {
    for (name, bytes) in fixtures() {
        for len in 0..bytes.len() {
            assert!(
                ClassFile::parse(&bytes[..len]).is_err(),
                "{name} tronquee a {len} octets lue comme valide"
            );
        }
    }
}

/// PARTIE 19.2 : des octets alteres au hasard ne font jamais paniquer la lecture ni le
/// parcours du code.
#[test]
fn random_corruption_never_panics() {
    let original = fixtures()["Calls"].clone();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut parsed = 0;
    for _ in 0..5_000 {
        let mut bytes = original.clone();
        for _ in 0..1 + next() % 4 {
            let at = (next() % bytes.len() as u64) as usize;
            bytes[at] = next() as u8;
        }
        if let Ok(class) = ClassFile::parse(&bytes) {
            parsed += 1;
            for method in &class.methods {
                let _ = class.call_sites(method);
            }
        }
    }
    // Une alteration tombe souvent dans une chaine ou une constante inutilisee : une
    // partie des fichiers reste lisible, et leur code est parcouru lui aussi.
    assert!(
        parsed > 0,
        "aucun fichier altere relu : le parcours n'est pas exerce"
    );
}

#[test]
fn annotation_types_are_read_on_the_class_its_fields_and_its_methods() {
    // ADR-059 : seuls les types sont gardes, quelles que soient la retention et la
    // forme des valeurs (enumeration, annotation imbriquee, tableau, classe).
    let visible = "Lfixtures/Annotated$Visible;";
    let invisible = "Lfixtures/Annotated$Invisible;";
    let class = parsed("Annotated");
    assert_eq!(class.annotations, vec![visible.to_owned()]);
    let field = |name: &str| class.fields.iter().find(|f| f.name == name).expect("champ");
    assert_eq!(field("marked").annotations, vec![invisible.to_owned()]);
    assert!(field("plain").annotations.is_empty());
    let both = class.method("both", "()V").expect("methode");
    assert_eq!(
        both.annotations,
        vec![visible.to_owned(), invisible.to_owned()]
    );
    assert!(class
        .method("none", "()V")
        .expect("methode")
        .annotations
        .is_empty());
    assert!(class.mentions_annotation(invisible));
    assert!(
        !class.mentions_annotation("Lfixtures/Annotated$Leaf;"),
        "imbriquee seulement"
    );
    assert!(!parsed("Calls").mentions_annotation(visible));
}
