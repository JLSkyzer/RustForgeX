//! Decodage des instructions des classes compilees par `javac` (C-11, ADR-055).
//!
//! Le compilateur calcule lui-meme `max_stack` : rejouer la table des effets sur le
//! graphe de flot de controle doit retrouver exactement sa valeur, pour chaque methode.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rfx_bytecode::{stack_depths, ClassFile, Instruction, InvokeKind, Op, Slot, StackOp};

fn fixtures() -> BTreeMap<String, ClassFile> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/classes/fixtures");
    let mut classes = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("classes de test") {
        let path = entry.expect("entree").path();
        if path.extension().is_some_and(|ext| ext == "class") {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let bytes = std::fs::read(&path).expect("lecture");
            classes.insert(name, ClassFile::parse(&bytes).expect("lisible"));
        }
    }
    classes
}

fn flow(name: &str, descriptor: &str) -> (ClassFile, Vec<Instruction>) {
    let class = fixtures().remove("Flow").expect("Flow");
    let method = class.method(name, descriptor).expect("methode");
    let instructions = class.instructions(method).expect("decodable");
    (class, instructions)
}

fn ops(instructions: &[Instruction]) -> Vec<&Op> {
    instructions.iter().map(|insn| &insn.op).collect()
}

/// Variable locale touchee et sa taille en emplacements.
fn local_of(op: &Op) -> Option<(u16, u16)> {
    match op {
        Op::Load { local, slot } | Op::Store { local, slot } => Some((*local, slot.size())),
        Op::Iinc { local } => Some((*local, 1)),
        _ => None,
    }
}

#[test]
fn every_method_replays_to_the_compiler_max_stack() {
    let mut methods = 0;
    for (name, class) in fixtures() {
        for method in &class.methods {
            let Some(code) = &method.code else {
                assert_eq!(method.max_stack, 0, "{name}.{} sans code", method.name);
                continue;
            };
            let instructions = class.instructions(method).expect("decodable");
            // Les instructions se suivent et couvrent tout le code.
            let mut next = 0;
            for insn in &instructions {
                assert_eq!(insn.offset, next, "{name}.{} : trou", method.name);
                next += insn.len;
            }
            assert_eq!(next as usize, code.len(), "{name}.{}", method.name);
            for insn in &instructions {
                if let Some((local, size)) = local_of(&insn.op) {
                    assert!(
                        local + size <= method.max_locals,
                        "{name}.{} : variable {local} hors de max_locals",
                        method.name
                    );
                }
            }
            let depth = stack_depths(&instructions, &method.handlers).expect("pile coherente");
            assert_eq!(
                depth, method.max_stack,
                "{name}.{}{} : hauteur rejouee",
                method.name, method.descriptor
            );
            methods += 1;
        }
    }
    assert!(methods >= 150, "toutes les fixtures : {methods} methodes");
}

#[test]
fn category_two_stack_operations_are_decoded() {
    let cases = [
        ("addToLong", "([JI)V", StackOp::Dup2),
        ("chainLongArray", "([JIJ)J", StackOp::Dup2X2),
        ("chainField", "(I)I", StackOp::DupX1),
        ("chainIntArray", "([III)I", StackOp::DupX2),
        ("chainWideField", "(J)J", StackOp::Dup2X1),
        (
            "discard",
            "(Ljava/lang/Long;Ljava/lang/Integer;)V",
            StackOp::Pop2,
        ),
        (
            "discard",
            "(Ljava/lang/Long;Ljava/lang/Integer;)V",
            StackOp::Pop,
        ),
    ];
    for (name, descriptor, expected) in cases {
        let (_, instructions) = flow(name, descriptor);
        assert!(
            ops(&instructions).contains(&&Op::Stack(expected)),
            "{name} : {expected:?}"
        );
    }
}

#[test]
fn invoke_carries_arguments_receiver_and_return() {
    let (_, instructions) = flow("discard", "(Ljava/lang/Long;Ljava/lang/Integer;)V");
    let calls: Vec<_> = instructions
        .iter()
        .filter_map(|insn| match &insn.op {
            Op::Invoke {
                kind,
                args,
                receiver,
                ret,
                ..
            } => Some((*kind, args.clone(), *receiver, *ret)),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        vec![
            (InvokeKind::Virtual, vec![], true, Some(Slot::Wide)),
            (InvokeKind::Virtual, vec![], true, Some(Slot::Prim)),
        ]
    );
    let (_, instructions) = flow("manyLocals", "(I)I");
    assert!(instructions.iter().any(|insn| matches!(
        &insn.op,
        Op::Invoke { kind: InvokeKind::Static, args, receiver: false, ret: Some(Slot::Ref), .. }
            if *args == vec![Slot::Prim]
    )));
}

#[test]
fn constants_have_their_category() {
    let (_, instructions) = flow("constants", "(I)Ljava/lang/Object;");
    let loaded: Vec<Slot> = instructions
        .iter()
        .filter_map(|insn| match insn.op {
            Op::Ldc(slot) => Some(slot),
            _ => None,
        })
        .collect();
    // int, float, String, Class, long, double — dans l'ordre du source.
    assert_eq!(
        loaded,
        vec![
            Slot::Prim,
            Slot::Prim,
            Slot::Ref,
            Slot::Ref,
            Slot::Wide,
            Slot::Wide
        ]
    );
    let switch = instructions
        .iter()
        .find_map(|insn| match &insn.op {
            Op::Switch(targets) => Some(targets.len()),
            _ => None,
        })
        .expect("tableswitch");
    assert_eq!(switch, 6, "defaut et cas 0 a 4");
}

#[test]
fn lookupswitch_and_type_tests_are_decoded() {
    let (_, instructions) = flow("dispatch", "(Ljava/lang/Object;I)I");
    let all = ops(&instructions);
    let switch = all
        .iter()
        .find_map(|op| match op {
            Op::Switch(targets) => Some(targets.len()),
            _ => None,
        })
        .expect("lookupswitch");
    assert_eq!(switch, 4, "defaut et trois cles");
    assert!(all.contains(&&Op::InstanceOf));
    assert!(all.contains(&&Op::CheckCast));
    assert!(all.contains(&&Op::Throw));
    assert!(all.contains(&&Op::New("java/lang/IllegalArgumentException".to_owned())));
}

#[test]
fn exception_table_is_read_with_catch_types() {
    let class = fixtures().remove("Flow").expect("Flow");
    let method = class
        .method("guarded", "(Ljava/lang/String;)I")
        .expect("methode");
    let types: Vec<Option<&str>> = method
        .handlers
        .iter()
        .map(|handler| handler.catch_type.as_deref())
        .collect();
    assert!(types.contains(&Some("java/lang/NumberFormatException")));
    assert!(types.contains(&Some("java/lang/NullPointerException")));
    assert!(types.contains(&None), "finally");
    for handler in &method.handlers {
        assert!(handler.start < handler.end);
    }

    let method = class
        .method("locked", "(Ljava/lang/Object;)V")
        .expect("methode");
    let instructions = class.instructions(method).expect("decodable");
    let all = ops(&instructions);
    assert!(all.contains(&&Op::MonitorEnter));
    assert_eq!(all.iter().filter(|op| ***op == Op::MonitorExit).count(), 2);
    assert!(
        !method.handlers.is_empty(),
        "sortie du moniteur sur exception"
    );
}

#[test]
fn arrays_and_static_fields_are_decoded() {
    let (_, instructions) = flow("arrays", "(I)I");
    let all = ops(&instructions);
    assert!(all.contains(&&Op::MultiNewArray(2)));
    assert_eq!(all.iter().filter(|op| ***op == Op::NewArray).count(), 2);
    assert_eq!(all.iter().filter(|op| ***op == Op::ArrayLength).count(), 3);

    let (_, instructions) = flow("statics", "(I)J");
    let puts: Vec<&str> = instructions
        .iter()
        .filter_map(|insn| match &insn.op {
            Op::PutStatic(field) => Some(field.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(puts, vec!["counter", "total"]);
}

#[test]
fn wide_prefix_reaches_locals_beyond_255() {
    let (class, instructions) = flow("manyLocals", "(I)I");
    let method = class.method("manyLocals", "(I)I").expect("methode");
    assert!(method.max_locals > 256);
    let all = ops(&instructions);
    assert!(all.contains(&&Op::Iinc { local: 265 }));
    assert!(all.contains(&&Op::Load {
        local: 265,
        slot: Slot::Prim
    }));
    assert!(all.contains(&&Op::Store {
        local: 266,
        slot: Slot::Wide
    }));
}
