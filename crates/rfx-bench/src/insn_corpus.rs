//! Decodeur d'instructions de C-11 confronte au bytecode reel (ADR-055).
//!
//! Cahier des charges : PARTIE 5.11, FM-20. Maturite : `STABLE`.
//!
//! Pour chaque methode avec du code du corpus (Minecraft, Forge, mods), decode les
//! instructions et rejoue leur effet sur la pile. Le `max_stack` ecrit par le
//! compilateur sert de temoin : la JVM refuse une classe dont la pile depasse cette
//! borne, donc une hauteur rejouee superieure trahit une erreur dans la table des
//! effets. Une hauteur inferieure est legitime (un compilateur peut surestimer).

use std::collections::BTreeMap;

use rfx_bytecode::{stack_depths, ClassFile, MethodInfo, Op, ParseError};
use serde::Serialize;

use crate::callgraph_cost::Corpus;

/// Exemples gardes par categorie d'ecart.
const SAMPLES: usize = 10;

/// Resultat sur un corpus.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    /// Classes du corpus.
    pub classes: u64,
    /// Classes illisibles.
    pub unreadable_classes: u64,
    /// Methodes avec du code.
    pub methods: u64,
    /// Methodes decodees et rejouees sans incoherence.
    pub replayed: u64,
    /// Hauteur rejouee egale au `max_stack` du compilateur.
    pub depth_equal: u64,
    /// Hauteur rejouee inferieure : surestimation du compilateur, legitime.
    pub depth_below: u64,
    /// Hauteur rejouee superieure : erreur dans la table des effets.
    pub depth_above: u64,
    /// Variable locale hors de `max_locals` : erreur de decodage.
    pub locals_out_of_range: u64,
    /// Methodes avec `jsr` ou `ret` : non analysables (FM-20).
    pub subroutines: u64,
    /// Autres methodes non decodables.
    pub decode_errors: u64,
    /// Methodes decodees dont le rejeu echoue.
    pub replay_errors: u64,
    /// Exemples par categorie d'ecart : `classe.methode descripteur : detail`.
    pub samples: BTreeMap<String, Vec<String>>,
}

impl Report {
    fn sample(&mut self, category: &str, text: String) {
        let list = self.samples.entry(category.to_owned()).or_default();
        if list.len() < SAMPLES {
            list.push(text);
        }
    }

    /// `true` si aucun ecart ne trahit une erreur du decodeur.
    #[must_use]
    pub fn sound(&self) -> bool {
        self.depth_above == 0
            && self.locals_out_of_range == 0
            && self.decode_errors == 0
            && self.replay_errors == 0
    }
}

/// Decode et rejoue chaque methode du corpus.
#[must_use]
pub fn run(corpus: &Corpus) -> Report {
    let mut report = Report::default();
    let mut names: Vec<&String> = corpus.classes.keys().collect();
    names.sort();
    for name in names {
        report.classes += 1;
        let Ok(class) = ClassFile::parse(&corpus.classes[name]) else {
            report.unreadable_classes += 1;
            continue;
        };
        for method in &class.methods {
            if method.code.is_none() {
                continue;
            }
            report.methods += 1;
            check(&class, method, &mut report);
        }
    }
    report
}

fn check(class: &ClassFile, method: &MethodInfo, report: &mut Report) {
    let id = format!("{}.{} {}", class.name, method.name, method.descriptor);
    let instructions = match class.instructions(method) {
        Ok(instructions) => instructions,
        Err(ParseError::BadInstruction {
            opcode: 0xa8 | 0xa9 | 0xc9,
            ..
        }) => {
            report.subroutines += 1;
            return;
        }
        Err(e) => {
            report.decode_errors += 1;
            report.sample("decode", format!("{id} : {e}"));
            return;
        }
    };
    for insn in &instructions {
        let end = match &insn.op {
            Op::Load { local, slot } | Op::Store { local, slot } => {
                Some(u32::from(*local) + u32::from(slot.size()))
            }
            Op::Iinc { local } => Some(u32::from(*local) + 1),
            _ => None,
        };
        if end.is_some_and(|end| end > u32::from(method.max_locals)) {
            report.locals_out_of_range += 1;
            report.sample("locals", format!("{id} : octet {}", insn.offset));
            return;
        }
    }
    match stack_depths(&instructions, &method.handlers) {
        Ok(depth) => {
            report.replayed += 1;
            match depth.cmp(&method.max_stack) {
                core::cmp::Ordering::Equal => report.depth_equal += 1,
                core::cmp::Ordering::Less => {
                    report.depth_below += 1;
                    report.sample(
                        "below",
                        format!("{id} : {depth} < max_stack {}", method.max_stack),
                    );
                }
                core::cmp::Ordering::Greater => {
                    report.depth_above += 1;
                    report.sample(
                        "above",
                        format!("{id} : {depth} > max_stack {}", method.max_stack),
                    );
                }
            }
        }
        Err(e) => {
            report.replay_errors += 1;
            report.sample("replay", format!("{id} : {e}"));
        }
    }
}
