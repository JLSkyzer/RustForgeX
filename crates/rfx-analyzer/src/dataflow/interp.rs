//! Interpretation abstraite d'une methode : point fixe sur les blocs de base, puis une
//! passe qui consigne les faits depuis les etats stables (ADR-055).
//!
//! Les etats ne sont gardes qu'a l'entree des blocs : a l'interieur, ils se deduisent
//! en rejouant les instructions. Une exception peut partir de toute instruction couverte
//! par un gestionnaire, avec les variables locales d'avant l'instruction ; ces
//! variables ne changent qu'apres un `store`, et l'arete vers le gestionnaire n'est
//! donc emise qu'en entree de bloc, apres un `store`, ou a l'entree de sa plage.

use rfx_bytecode::{apply_stack_op, Handler, Instruction, Op, Slot};

use super::facts::{
    Access, Allocation, CallFact, MethodFacts, Monitor, Target, Throw, Unanalysable, Work,
};
use super::value::{Frame, Source, Sources, Value};

/// Budget de l'analyse d'une methode. Le depasser rend la methode non analysee, donc
/// d'effet inconnu : jamais un resultat partiel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataflowLimits {
    /// Emplacements d'etat gardes au plus : blocs × (variables locales + pile).
    pub max_cells: usize,
    /// Passages au plus par bloc avant d'abandonner le point fixe.
    pub max_visits_per_block: u32,
}

impl DataflowLimits {
    /// Budget par defaut, fixe sur le corpus reel (`rfx-bench dataflow`, ADR-055) :
    /// sur 599 749 methodes, au plus 17 440 emplacements d'etat et 14 passages dans un
    /// meme bloc. La marge laisse passer des methodes plus grosses que celles du jeu de
    /// base, et borne l'etat d'une methode a 65 536 valeurs de 52 octets (3,4 Mio).
    pub const DEFAULT: Self = Self {
        max_cells: 65_536,
        max_visits_per_block: 64,
    };
}

impl Default for DataflowLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Classe d'exception qui attrape tout.
const THROWABLE: &str = "java/lang/Throwable";

/// Code d'une methode, pret a interpreter.
pub(super) struct Body<'a> {
    pub instructions: &'a [Instruction],
    pub handlers: &'a [Handler],
    pub max_stack: u16,
}

fn incoherent(what: &'static str) -> Unanalysable {
    Unanalysable::Incoherent(what)
}

/// Blocs de base : debut de chaque bloc, et bloc de chaque instruction de tete.
struct Blocks {
    starts: Vec<usize>,
    block_of: Vec<u32>,
}

impl Blocks {
    fn of(body: &Body<'_>) -> Result<Self, Unanalysable> {
        let instructions = body.instructions;
        let n = instructions.len();
        let mut leader = vec![false; n];
        leader[0] = true;
        for (i, insn) in instructions.iter().enumerate() {
            for &target in insn.branch_targets() {
                leader[index_of(instructions, target)?] = true;
            }
            if (insn.ends_flow() || !insn.branch_targets().is_empty()) && i + 1 < n {
                leader[i + 1] = true;
            }
        }
        for handler in body.handlers {
            leader[index_of(instructions, handler.handler)?] = true;
        }
        let mut starts = Vec::new();
        let mut block_of = vec![u32::MAX; n];
        for (i, is_leader) in leader.iter().enumerate() {
            if *is_leader {
                block_of[i] = u32::try_from(starts.len()).map_err(|_| Unanalysable::Budget)?;
                starts.push(i);
            }
        }
        Ok(Self { starts, block_of })
    }

    fn end(&self, block: usize, n: usize) -> usize {
        self.starts.get(block + 1).copied().unwrap_or(n)
    }

    /// Bloc qui commence a l'instruction `index`.
    fn at(&self, index: usize) -> Result<usize, Unanalysable> {
        match self.block_of.get(index) {
            Some(&block) if block != u32::MAX => Ok(block as usize),
            _ => Err(incoherent("arete hors d'une tete de bloc")),
        }
    }
}

fn index_of(instructions: &[Instruction], offset: u32) -> Result<usize, Unanalysable> {
    instructions
        .binary_search_by_key(&offset, |insn| insn.offset)
        .map_err(|_| incoherent("cible hors instruction"))
}

/// Interprete le code depuis `entry`, et rend ses faits.
pub(super) fn run(
    body: &Body<'_>,
    entry: Frame,
    returns_ref: bool,
    limits: &DataflowLimits,
) -> Result<MethodFacts, Unanalysable> {
    let instructions = body.instructions;
    if instructions.is_empty() {
        return Err(incoherent("code vide"));
    }
    let blocks = Blocks::of(body)?;
    let cells = blocks
        .starts
        .len()
        .saturating_mul(entry.locals.len() + usize::from(body.max_stack));
    if cells > limits.max_cells {
        return Err(Unanalysable::Budget);
    }
    let handler_blocks = body
        .handlers
        .iter()
        .map(|h| blocks.at(index_of(instructions, h.handler)?))
        .collect::<Result<Vec<_>, _>>()?;

    let count = blocks.starts.len();
    let mut entries: Vec<Option<Frame>> = vec![None; count];
    let mut queued = vec![false; count];
    let mut visits = vec![0_u32; count];
    let mut pending = vec![0_usize];
    let mut widened = 0_u32;
    entries[0] = Some(entry);
    queued[0] = true;

    while let Some(block) = pending.pop() {
        queued[block] = false;
        visits[block] += 1;
        if visits[block] > limits.max_visits_per_block {
            return Err(Unanalysable::Budget);
        }
        let mut frame = entries[block].clone().ok_or(incoherent("bloc sans etat"))?;
        let mut edge = |to: usize, incoming: &Frame| -> Result<(), Unanalysable> {
            let changed = match &mut entries[to] {
                Some(known) => known
                    .join(incoming, &mut widened)
                    .map_err(|_| incoherent("piles differentes a une jonction"))?,
                slot @ None => {
                    *slot = Some(incoming.clone());
                    true
                }
            };
            if changed && !queued[to] {
                queued[to] = true;
                pending.push(to);
            }
            Ok(())
        };
        walk(
            body,
            &blocks,
            &handler_blocks,
            block,
            &mut frame,
            None,
            &mut edge,
        )?;
    }

    let mut facts = MethodFacts {
        returns: returns_ref.then_some(Sources::NULL),
        ..MethodFacts::default()
    };
    for (block, entry) in entries.iter().enumerate() {
        let Some(entry) = entry else {
            continue; // inatteignable : ne s'execute jamais
        };
        let mut frame = entry.clone();
        walk(
            body,
            &blocks,
            &handler_blocks,
            block,
            &mut frame,
            Some(&mut facts),
            &mut |_, _| Ok(()),
        )?;
    }
    facts.widened += widened;
    facts.work = Work {
        blocks: u32::try_from(count).unwrap_or(u32::MAX),
        cells: cells as u64,
        visits: visits.iter().sum(),
        max_block_visits: visits.iter().copied().max().unwrap_or(0),
    };
    Ok(facts)
}

/// Rejoue un bloc ; signale chaque arete sortante, gestionnaires compris.
fn walk(
    body: &Body<'_>,
    blocks: &Blocks,
    handler_blocks: &[usize],
    block: usize,
    frame: &mut Frame,
    mut facts: Option<&mut MethodFacts>,
    edge: &mut impl FnMut(usize, &Frame) -> Result<(), Unanalysable>,
) -> Result<(), Unanalysable> {
    let instructions = body.instructions;
    let start = blocks.starts[block];
    let end = blocks.end(block, instructions.len());
    for i in start..end {
        let insn = &instructions[i];
        let after_store = i > start && matches!(instructions[i - 1].op, Op::Store { .. });
        for (handler, &to) in body.handlers.iter().zip(handler_blocks) {
            if !covers(handler, insn.offset) {
                continue;
            }
            let entering =
                i == start || after_store || !covers(handler, instructions[i - 1].offset);
            if entering {
                let caught = Frame {
                    locals: frame.locals.clone(),
                    stack: vec![Value::Ref(Sources::one(Source::Caught(handler.handler)))],
                };
                edge(to, &caught)?;
            }
        }
        let caught_all = body.handlers.iter().any(|h| {
            covers(h, insn.offset) && h.catch_type.as_deref().is_none_or(|t| t == THROWABLE)
        });
        step(frame, insn, facts.as_deref_mut(), caught_all)?;
    }
    let last = &instructions[end - 1];
    for &target in last.branch_targets() {
        edge(blocks.at(index_of(instructions, target)?)?, frame)?;
    }
    if !last.ends_flow() {
        if end >= instructions.len() {
            return Err(incoherent("le code se termine sans retour"));
        }
        edge(blocks.at(end)?, frame)?;
    }
    Ok(())
}

fn covers(handler: &Handler, offset: u32) -> bool {
    handler.start <= offset && offset < handler.end
}

fn pop(stack: &mut Vec<Value>) -> Result<Value, Unanalysable> {
    stack.pop().ok_or(incoherent("pile vide"))
}

fn reference(value: Value) -> Result<Sources, Unanalysable> {
    match value {
        Value::Ref(sources) => Ok(sources),
        _ => Err(incoherent("reference attendue")),
    }
}

fn pop_ref(stack: &mut Vec<Value>) -> Result<Sources, Unanalysable> {
    reference(pop(stack)?)
}

fn pop_n(stack: &mut Vec<Value>, n: usize) -> Result<(), Unanalysable> {
    let keep = stack.len().checked_sub(n).ok_or(incoherent("pile vide"))?;
    stack.truncate(keep);
    Ok(())
}

/// Reference ecrite, s'il s'agit d'une reference.
fn written(value: Value) -> Option<Sources> {
    match value {
        Value::Ref(sources) => Some(sources),
        _ => None,
    }
}

/// Ecrit une variable locale ; une valeur longue occupe deux emplacements.
pub(super) fn store(locals: &mut [Value], local: u16, value: Value) -> Result<(), Unanalysable> {
    let at = usize::from(local);
    let size = usize::from(value.slot().size());
    if at + size > locals.len() {
        return Err(incoherent("variable locale hors de max_locals"));
    }
    if at > 0 && locals[at - 1] == Value::Wide {
        locals[at - 1] = Value::Top;
    }
    locals[at] = value;
    if size == 2 {
        locals[at + 1] = Value::Top;
    }
    Ok(())
}

/// Effet d'une instruction ; consigne ses faits si `facts` est fourni.
#[allow(clippy::too_many_lines)] // une branche par famille d'instructions
fn step(
    frame: &mut Frame,
    insn: &Instruction,
    facts: Option<&mut MethodFacts>,
    caught_all: bool,
) -> Result<(), Unanalysable> {
    let offset = insn.offset;
    let stack = &mut frame.stack;
    let access = |target: Target, base: Sources, value: Option<Sources>| Access {
        offset,
        target,
        base,
        value,
    };
    match &insn.op {
        Op::Nop | Op::Iinc { .. } | Op::Goto(_) => {}
        Op::Null => stack.push(Value::Ref(Sources::NULL)),
        Op::Const(slot) | Op::Ldc(slot) => stack.push(Value::of(*slot, Source::Constant)),
        Op::Load { local, slot } => {
            let value = *frame
                .locals
                .get(usize::from(*local))
                .ok_or(incoherent("variable locale hors de max_locals"))?;
            stack.push(match slot {
                Slot::Ref => Value::Ref(reference(value)?),
                Slot::Prim => Value::Prim,
                Slot::Wide => Value::Wide,
            });
        }
        Op::Store { local, .. } => {
            let value = pop(stack)?;
            store(&mut frame.locals, *local, value)?;
        }
        Op::Prim { pops, push } => {
            pop_n(stack, usize::from(*pops))?;
            if let Some(slot) = push {
                stack.push(Value::of(*slot, Source::Constant));
            }
        }
        Op::ArrayLoad(slot) => {
            pop(stack)?;
            let base = pop_ref(stack)?;
            if let Some(facts) = facts {
                facts.reads.push(access(Target::Element, base, None));
            }
            stack.push(Value::of(*slot, Source::Element(offset)));
        }
        Op::ArrayStore(_) => {
            let value = pop(stack)?;
            pop(stack)?;
            let base = pop_ref(stack)?;
            if let Some(facts) = facts {
                facts
                    .writes
                    .push(access(Target::Element, base, written(value)));
            }
        }
        Op::ArrayLength | Op::InstanceOf => {
            pop(stack)?;
            stack.push(Value::Prim);
        }
        Op::GetField(field) => {
            let base = pop_ref(stack)?;
            let slot = Slot::of_field(&field.descriptor);
            if let Some(facts) = facts {
                facts
                    .reads
                    .push(access(Target::Field(field.clone()), base, None));
            }
            stack.push(Value::of(slot, Source::Field(offset)));
        }
        Op::PutField(field) => {
            let value = pop(stack)?;
            let base = pop_ref(stack)?;
            if let Some(facts) = facts {
                facts
                    .writes
                    .push(access(Target::Field(field.clone()), base, written(value)));
            }
        }
        Op::GetStatic(field) => {
            if let Some(facts) = facts {
                facts
                    .reads
                    .push(access(Target::Static(field.clone()), Sources::NULL, None));
            }
            stack.push(Value::of(
                Slot::of_field(&field.descriptor),
                Source::Static(offset),
            ));
        }
        Op::PutStatic(field) => {
            let value = pop(stack)?;
            if let Some(facts) = facts {
                facts.writes.push(access(
                    Target::Static(field.clone()),
                    Sources::NULL,
                    written(value),
                ));
            }
        }
        Op::Invoke {
            kind,
            target,
            args,
            receiver,
            ret,
        } => {
            let mut values = Vec::with_capacity(args.len() + 1);
            for slot in args.iter().rev() {
                let value = pop(stack)?;
                values.push(match slot {
                    Slot::Ref => Some(reference(value)?),
                    Slot::Prim | Slot::Wide => None,
                });
            }
            if *receiver {
                values.push(Some(pop_ref(stack)?));
            }
            values.reverse();
            if let Some(facts) = facts {
                facts.calls.push(CallFact {
                    offset,
                    kind: *kind,
                    target: target.clone(),
                    args: values,
                });
            }
            if let Some(slot) = ret {
                stack.push(Value::of(*slot, Source::Return(offset)));
            }
        }
        Op::New(class) => {
            if let Some(facts) = facts {
                facts.allocations.push(Allocation {
                    offset,
                    class: Some(class.clone()),
                });
            }
            stack.push(Value::Ref(Sources::one(Source::New(offset))));
        }
        Op::NewArray | Op::MultiNewArray(_) => {
            let dims = match insn.op {
                Op::MultiNewArray(dims) => usize::from(dims),
                _ => 1,
            };
            pop_n(stack, dims)?;
            if let Some(facts) = facts {
                facts.allocations.push(Allocation {
                    offset,
                    class: None,
                });
            }
            stack.push(Value::Ref(Sources::one(Source::New(offset))));
        }
        Op::CheckCast => {
            let value = pop_ref(stack)?;
            stack.push(Value::Ref(value));
        }
        Op::Stack(op) => {
            apply_stack_op(stack, *op, Value::slot)
                .ok_or(incoherent("effet de pile impossible"))?;
        }
        Op::If { pops, .. } => pop_n(stack, usize::from(*pops))?,
        Op::Switch(_) | Op::MonitorExit => {
            pop(stack)?;
        }
        Op::MonitorEnter => {
            let lock = pop_ref(stack)?;
            if let Some(facts) = facts {
                facts.monitors.push(Monitor { offset, lock });
            }
        }
        Op::Return(slot) => {
            if *slot == Some(Slot::Ref) {
                let value = pop_ref(stack)?;
                if let Some(facts) = facts {
                    let (union, lost) = facts.returns.unwrap_or(Sources::NULL).union(&value);
                    facts.widened += u32::from(lost);
                    facts.returns = Some(union);
                }
            }
            stack.clear();
        }
        Op::Throw => {
            let value = pop_ref(stack)?;
            if !caught_all {
                if let Some(facts) = facts {
                    facts.throws.push(Throw { offset, value });
                }
            }
            stack.clear();
        }
    }
    Ok(())
}
