//! Instructions d'une methode, avec leur effet sur la pile (JVMS §6.5).
//!
//! Cahier des charges : PARTIE 5.11 (analyse abstraite sur bytecode), FM-20. Decision :
//! ADR-055. Maturite : `STABLE`.
//!
//! Chaque instruction est decrite par ce qu'une interpretation abstraite doit savoir :
//! les valeurs qu'elle depile et empile, en valeurs et non en emplacements (une valeur
//! `long` ou `double` est une seule valeur de categorie 2), les champs, classes et
//! appels qu'elle designe, et ses successeurs. [`stack_depths`] rejoue ces effets sur le
//! graphe de flot de controle : sur du code verifie par la JVM, une erreur dans la table
//! des effets s'y trahit par un depilement impossible, deux piles differentes a une
//! jonction, ou un depassement de `max_stack`.
//!
//! `jsr` et `ret`, interdits depuis la version 51 du format, rendent la methode non
//! analysable : leur effet depend d'une adresse de retour que l'analyse ne suit pas.

use crate::class::BootstrapMethod;
use crate::code::{dynamic_target_of, instruction_length, CallTarget, InvokeKind};
use crate::constant::{Constant, ConstantPool, MemberRef};
use crate::ParseError;

/// Nature d'une valeur de pile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    /// Primitive de categorie 1 : `int`, `float`, et leurs formes courtes.
    Prim,
    /// Primitive de categorie 2 : `long`, `double` (deux emplacements).
    Wide,
    /// Reference.
    Ref,
}

impl Slot {
    /// Emplacements occupes : 2 pour une valeur de categorie 2.
    #[must_use]
    pub fn size(self) -> u16 {
        match self {
            Self::Wide => 2,
            Self::Prim | Self::Ref => 1,
        }
    }

    /// Nature d'une valeur d'apres son descripteur de champ (`I`, `J`, `Lx;`, `[I`).
    #[must_use]
    pub fn of_field(descriptor: &str) -> Self {
        match descriptor.as_bytes().first() {
            Some(b'J' | b'D') => Self::Wide,
            Some(b'L' | b'[') => Self::Ref,
            _ => Self::Prim,
        }
    }
}

/// Parametres et retour d'un descripteur de methode, ou `None` s'il est mal forme.
#[must_use]
pub fn method_signature(descriptor: &str) -> Option<(Vec<Slot>, Option<Slot>)> {
    let bytes = descriptor.as_bytes();
    if bytes.first() != Some(&b'(') {
        return None;
    }
    let mut params = Vec::new();
    let mut at = 1;
    while *bytes.get(at)? != b')' {
        let start = at;
        while bytes.get(at) == Some(&b'[') {
            at += 1;
        }
        match bytes.get(at)? {
            b'L' => at += bytes[at..].iter().position(|&b| b == b';')? + 1,
            b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z' => at += 1,
            _ => return None,
        }
        params.push(Slot::of_field(&descriptor[start..at]));
    }
    let ret = match &descriptor[at + 1..] {
        "V" => None,
        "" => return None,
        other => Some(Slot::of_field(other)),
    };
    Some((params, ret))
}

/// Operations de pile qui ne dependent que des categories (JVMS §6.5 `pop`..`swap`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackOp {
    /// `pop`.
    Pop,
    /// `pop2`.
    Pop2,
    /// `dup`.
    Dup,
    /// `dup_x1`.
    DupX1,
    /// `dup_x2`.
    DupX2,
    /// `dup2`.
    Dup2,
    /// `dup2_x1`.
    Dup2X1,
    /// `dup2_x2`.
    Dup2X2,
    /// `swap`.
    Swap,
}

/// Ce que fait une instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// `nop`.
    Nop,
    /// `aconst_null`.
    Null,
    /// Constante primitive empilee (`iconst`, `lconst`, `bipush`…).
    Const(Slot),
    /// `ldc`, `ldc_w`, `ldc2_w` : nombre, chaine, classe, handle ou constante dynamique.
    Ldc(Slot),
    /// Lecture d'une variable locale.
    Load {
        /// Indice de la variable.
        local: u16,
        /// Nature.
        slot: Slot,
    },
    /// Ecriture d'une variable locale.
    Store {
        /// Indice de la variable.
        local: u16,
        /// Nature.
        slot: Slot,
    },
    /// `iinc` : variable locale entiere modifiee en place.
    Iinc {
        /// Indice de la variable.
        local: u16,
    },
    /// Operation primitive : depile `pops` valeurs, empile `push`.
    Prim {
        /// Valeurs depilees.
        pops: u8,
        /// Valeur empilee.
        push: Option<Slot>,
    },
    /// Lecture d'un element de tableau : depile tableau et indice.
    ArrayLoad(Slot),
    /// Ecriture d'un element de tableau : depile tableau, indice et valeur.
    ArrayStore(Slot),
    /// `arraylength`.
    ArrayLength,
    /// `getfield`.
    GetField(MemberRef),
    /// `putfield`.
    PutField(MemberRef),
    /// `getstatic`.
    GetStatic(MemberRef),
    /// `putstatic`.
    PutStatic(MemberRef),
    /// `invoke*`.
    Invoke {
        /// Nature de l'appel.
        kind: InvokeKind,
        /// Cible.
        target: CallTarget,
        /// Arguments, receveur exclu.
        args: Vec<Slot>,
        /// Un receveur est depile avant les arguments.
        receiver: bool,
        /// Valeur rendue.
        ret: Option<Slot>,
    },
    /// `new`.
    New(String),
    /// `newarray`, `anewarray` : depile la taille.
    NewArray,
    /// `multianewarray` : depile une taille par dimension.
    MultiNewArray(u8),
    /// `checkcast` : la reference reste sur la pile.
    CheckCast,
    /// `instanceof`.
    InstanceOf,
    /// Operation de pile.
    Stack(StackOp),
    /// Branchement conditionnel : depile `pops` valeurs, puis `target` ou la suite.
    If {
        /// Valeurs depilees.
        pops: u8,
        /// Cible si la condition tient.
        target: u32,
    },
    /// `goto`, `goto_w`.
    Goto(u32),
    /// `tableswitch`, `lookupswitch` : depile la cle ; toutes les cibles, defaut compris.
    Switch(Vec<u32>),
    /// Retour, avec ou sans valeur.
    Return(Option<Slot>),
    /// `athrow`.
    Throw,
    /// `monitorenter`.
    MonitorEnter,
    /// `monitorexit`.
    MonitorExit,
}

/// Une instruction a sa position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    /// Position dans le code.
    pub offset: u32,
    /// Longueur, operandes compris.
    pub len: u32,
    /// Effet.
    pub op: Op,
}

impl Instruction {
    /// `true` si l'execution ne continue jamais a l'instruction suivante.
    #[must_use]
    pub fn ends_flow(&self) -> bool {
        matches!(
            self.op,
            Op::Goto(_) | Op::Switch(_) | Op::Return(_) | Op::Throw
        )
    }

    /// Cibles de branchement, hors instruction suivante.
    #[must_use]
    pub fn branch_targets(&self) -> &[u32] {
        match &self.op {
            Op::If { target, .. } | Op::Goto(target) => core::slice::from_ref(target),
            Op::Switch(targets) => targets,
            _ => &[],
        }
    }
}

/// Gestionnaire d'exceptions d'un code (JVMS §4.7.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handler {
    /// Debut de la plage couverte, inclus.
    pub start: u32,
    /// Fin de la plage couverte, exclue.
    pub end: u32,
    /// Debut du gestionnaire.
    pub handler: u32,
    /// Classe interceptee ; `None` pour toutes (`finally`).
    pub catch_type: Option<String>,
}

fn read_u16(code: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(code.get(at..at + 2)?.try_into().ok()?))
}

fn read_i32(code: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_be_bytes(code.get(at..at + 4)?.try_into().ok()?))
}

fn relative(offset: usize, delta: i64) -> Option<u32> {
    u32::try_from(i64::try_from(offset).ok()? + delta).ok()
}

/// Valeur d'une constante empilee par `ldc`.
fn ldc_slot(pool: &ConstantPool, index: u16) -> Result<Slot, ParseError> {
    Ok(match pool.get(index)? {
        Constant::Integer(_) | Constant::Float(_) => Slot::Prim,
        Constant::Long(_) | Constant::Double(_) => Slot::Wide,
        Constant::String { .. }
        | Constant::Class { .. }
        | Constant::MethodHandle { .. }
        | Constant::MethodType { .. } => Slot::Ref,
        Constant::Dynamic { name_and_type, .. } => {
            Slot::of_field(pool.name_and_type(*name_and_type)?.1)
        }
        _ => {
            return Err(ParseError::WrongKind {
                index,
                expected: "constante chargeable",
            })
        }
    })
}

/// Lecture (`xload`) ou ecriture (`xstore`) d'une variable locale, par code d'operation.
fn local_op(opcode: u8, local: u16) -> Option<Op> {
    let slot = |n: u8| match n {
        1 | 3 => Slot::Wide,
        4 => Slot::Ref,
        _ => Slot::Prim,
    };
    match opcode {
        0x15..=0x19 => Some(Op::Load {
            local,
            slot: slot(opcode - 0x15),
        }),
        0x36..=0x3a => Some(Op::Store {
            local,
            slot: slot(opcode - 0x36),
        }),
        _ => None,
    }
}

/// Decode tout le code d'une methode.
///
/// # Errors
///
/// [`ParseError::BadInstruction`] pour une instruction inconnue, tronquee, ou `jsr` et
/// `ret` (non analysables, FM-20) ; une autre [`ParseError`] si une constante designee
/// est incoherente.
#[allow(clippy::too_many_lines)] // une table : une branche par famille d'instructions
pub fn decode(
    code: &[u8],
    pool: &ConstantPool,
    bootstraps: &[BootstrapMethod],
) -> Result<Vec<Instruction>, ParseError> {
    let mut instructions = Vec::new();
    let mut offset = 0_usize;
    while offset < code.len() {
        let opcode = code[offset];
        let bad = || ParseError::BadInstruction { offset, opcode };
        let len = instruction_length(code, offset).ok_or_else(bad)?;
        if code.len() < offset + len {
            return Err(bad());
        }
        let u8_operand = || code[offset + 1];
        let u16_operand = || read_u16(code, offset + 1).ok_or_else(bad);
        let branch16 = || {
            let delta = read_u16(code, offset + 1).ok_or_else(bad)? as i16;
            relative(offset, i64::from(delta)).ok_or_else(bad)
        };
        let p = Slot::Prim;
        let w = Slot::Wide;
        // Familles typees : `int` puis `long`, ou `int long float double` : les codes
        // impairs de la famille sont de categorie 2.
        let typed = |base: u8| if (opcode - base) % 2 == 1 { w } else { p };
        let prim = |pops: u8, push: Slot| Op::Prim {
            pops,
            push: Some(push),
        };
        let op = match opcode {
            0x00 => Op::Nop,
            0x01 => Op::Null,
            0x02..=0x08 | 0x0b..=0x0d | 0x10 | 0x11 => Op::Const(p),
            0x09 | 0x0a | 0x0e | 0x0f => Op::Const(w),
            0x12 => Op::Ldc(ldc_slot(pool, u16::from(u8_operand()))?),
            0x13 | 0x14 => Op::Ldc(ldc_slot(pool, u16_operand()?)?),
            0x15..=0x19 | 0x36..=0x3a => {
                local_op(opcode, u16::from(u8_operand())).ok_or_else(bad)?
            }
            0x1a..=0x2d => {
                let n = opcode - 0x1a;
                local_op(0x15 + n / 4, u16::from(n % 4)).ok_or_else(bad)?
            }
            0x3b..=0x4e => {
                let n = opcode - 0x3b;
                local_op(0x36 + n / 4, u16::from(n % 4)).ok_or_else(bad)?
            }
            0x2e | 0x30 | 0x33..=0x35 => Op::ArrayLoad(p),
            0x2f | 0x31 => Op::ArrayLoad(w),
            0x32 => Op::ArrayLoad(Slot::Ref),
            0x4f | 0x51 | 0x54..=0x56 => Op::ArrayStore(p),
            0x50 | 0x52 => Op::ArrayStore(w),
            0x53 => Op::ArrayStore(Slot::Ref),
            0x57 => Op::Stack(StackOp::Pop),
            0x58 => Op::Stack(StackOp::Pop2),
            0x59 => Op::Stack(StackOp::Dup),
            0x5a => Op::Stack(StackOp::DupX1),
            0x5b => Op::Stack(StackOp::DupX2),
            0x5c => Op::Stack(StackOp::Dup2),
            0x5d => Op::Stack(StackOp::Dup2X1),
            0x5e => Op::Stack(StackOp::Dup2X2),
            0x5f => Op::Stack(StackOp::Swap),
            0x60..=0x73 => prim(2, typed(0x60)),
            0x74..=0x77 => prim(1, typed(0x74)),
            0x78..=0x83 => prim(2, typed(0x78)),
            0x84 => Op::Iinc {
                local: u16::from(u8_operand()),
            },
            0x85 | 0x87 | 0x8a | 0x8c | 0x8d | 0x8f => prim(1, w),
            0x86 | 0x88 | 0x89 | 0x8b | 0x8e | 0x90..=0x93 => prim(1, p),
            0x94..=0x98 => prim(2, p),
            0x99..=0x9e | 0xc6 | 0xc7 => Op::If {
                pops: 1,
                target: branch16()?,
            },
            0x9f..=0xa6 => Op::If {
                pops: 2,
                target: branch16()?,
            },
            0xa7 => Op::Goto(branch16()?),
            0xc8 => {
                let delta = read_i32(code, offset + 1).ok_or_else(bad)?;
                Op::Goto(relative(offset, i64::from(delta)).ok_or_else(bad)?)
            }
            0xaa | 0xab => Op::Switch(switch_targets(code, offset).ok_or_else(bad)?),
            0xac | 0xae => Op::Return(Some(p)),
            0xad | 0xaf => Op::Return(Some(w)),
            0xb0 => Op::Return(Some(Slot::Ref)),
            0xb1 => Op::Return(None),
            0xb2 => Op::GetStatic(pool.member(u16_operand()?)?),
            0xb3 => Op::PutStatic(pool.member(u16_operand()?)?),
            0xb4 => Op::GetField(pool.member(u16_operand()?)?),
            0xb5 => Op::PutField(pool.member(u16_operand()?)?),
            0xb6..=0xba => invoke(opcode, u16_operand()?, pool, bootstraps)?,
            0xbb => Op::New(pool.class_name(u16_operand()?)?.to_owned()),
            0xbc | 0xbd => Op::NewArray,
            0xbe => Op::ArrayLength,
            0xbf => Op::Throw,
            0xc0 => Op::CheckCast,
            0xc1 => Op::InstanceOf,
            0xc2 => Op::MonitorEnter,
            0xc3 => Op::MonitorExit,
            0xc4 => {
                let local = read_u16(code, offset + 2).ok_or_else(bad)?;
                match code[offset + 1] {
                    0x84 => Op::Iinc { local },
                    wide => local_op(wide, local).ok_or_else(bad)?,
                }
            }
            0xc5 => Op::MultiNewArray(code[offset + 3]),
            // jsr, ret, jsr_w : non analysables ; le reste : inconnu.
            _ => return Err(bad()),
        };
        instructions.push(Instruction {
            offset: u32::try_from(offset).map_err(|_| bad())?,
            len: u32::try_from(len).map_err(|_| bad())?,
            op,
        });
        offset += len;
    }
    Ok(instructions)
}

/// `invokevirtual`, `invokespecial`, `invokestatic`, `invokeinterface`,
/// `invokedynamic`.
fn invoke(
    opcode: u8,
    index: u16,
    pool: &ConstantPool,
    bootstraps: &[BootstrapMethod],
) -> Result<Op, ParseError> {
    let (kind, target, descriptor) = if opcode == 0xba {
        let target = dynamic_target_of(pool, bootstraps, index)?;
        let CallTarget::Dynamic { descriptor, .. } = &target else {
            return Err(ParseError::Malformed("invokedynamic sans site dynamique"));
        };
        let descriptor = descriptor.clone();
        (InvokeKind::Dynamic, target, descriptor)
    } else {
        let member = pool.member(index)?;
        let kind = match opcode {
            0xb6 => InvokeKind::Virtual,
            0xb7 => InvokeKind::Special,
            0xb8 => InvokeKind::Static,
            _ => InvokeKind::Interface,
        };
        let descriptor = member.descriptor.clone();
        (kind, CallTarget::Member(member), descriptor)
    };
    let (args, ret) = method_signature(&descriptor)
        .ok_or(ParseError::Malformed("descripteur de methode invalide"))?;
    Ok(Op::Invoke {
        receiver: !matches!(kind, InvokeKind::Static | InvokeKind::Dynamic),
        kind,
        target,
        args,
        ret,
    })
}

/// Cibles d'un `tableswitch` ou d'un `lookupswitch`, defaut en tete.
fn switch_targets(code: &[u8], offset: usize) -> Option<Vec<u32>> {
    let padding = (4 - (offset + 1) % 4) % 4;
    let mut at = offset + 1 + padding;
    let mut targets = vec![relative(offset, i64::from(read_i32(code, at)?))?];
    at += 4;
    if code[offset] == 0xaa {
        let low = read_i32(code, at)?;
        let high = read_i32(code, at + 4)?;
        at += 8;
        for _ in 0..=(i64::from(high) - i64::from(low)) {
            targets.push(relative(offset, i64::from(read_i32(code, at)?))?);
            at += 4;
        }
    } else {
        let pairs = read_i32(code, at)?;
        at += 4;
        for _ in 0..pairs {
            targets.push(relative(offset, i64::from(read_i32(code, at + 4)?))?);
            at += 8;
        }
    }
    Some(targets)
}

/// Applique une operation de pile (JVMS §6.5 `pop`..`swap`) a une pile de valeurs
/// quelconques, dont `slot_of` donne la nature : ces operations ne lisent que la
/// categorie. `None` si une categorie ne convient pas, ou s'il manque des valeurs.
pub fn apply_stack_op<T: Copy>(
    stack: &mut Vec<T>,
    op: StackOp,
    slot_of: impl Fn(&T) -> Slot,
) -> Option<()> {
    let wide = |value: &T| slot_of(value) == Slot::Wide;
    let narrow = |value: T| (!wide(&value)).then_some(value);
    match op {
        StackOp::Pop => {
            narrow(stack.pop()?)?;
        }
        StackOp::Pop2 => {
            if !wide(&stack.pop()?) {
                narrow(stack.pop()?)?;
            }
        }
        StackOp::Dup => {
            let v1 = narrow(*stack.last()?)?;
            stack.push(v1);
        }
        StackOp::DupX1 => {
            let v1 = narrow(stack.pop()?)?;
            let v2 = narrow(stack.pop()?)?;
            stack.extend([v1, v2, v1]);
        }
        StackOp::DupX2 => {
            let v1 = narrow(stack.pop()?)?;
            let v2 = stack.pop()?;
            if wide(&v2) {
                stack.extend([v1, v2, v1]);
            } else {
                let v3 = narrow(stack.pop()?)?;
                stack.extend([v1, v3, v2, v1]);
            }
        }
        StackOp::Dup2 => {
            let v1 = stack.pop()?;
            if wide(&v1) {
                stack.extend([v1, v1]);
            } else {
                let v2 = narrow(stack.pop()?)?;
                stack.extend([v2, v1, v2, v1]);
            }
        }
        StackOp::Dup2X1 => {
            let v1 = stack.pop()?;
            if wide(&v1) {
                let v2 = narrow(stack.pop()?)?;
                stack.extend([v1, v2, v1]);
            } else {
                let v2 = narrow(stack.pop()?)?;
                let v3 = narrow(stack.pop()?)?;
                stack.extend([v2, v1, v3, v2, v1]);
            }
        }
        StackOp::Dup2X2 => {
            let v1 = stack.pop()?;
            if wide(&v1) {
                let v2 = stack.pop()?;
                if wide(&v2) {
                    stack.extend([v1, v2, v1]);
                } else {
                    let v3 = narrow(stack.pop()?)?;
                    stack.extend([v1, v3, v2, v1]);
                }
            } else {
                let v2 = narrow(stack.pop()?)?;
                let v3 = stack.pop()?;
                if wide(&v3) {
                    stack.extend([v2, v1, v3, v2, v1]);
                } else {
                    let v4 = narrow(stack.pop()?)?;
                    stack.extend([v2, v1, v4, v3, v2, v1]);
                }
            }
        }
        StackOp::Swap => {
            let v1 = narrow(stack.pop()?)?;
            let v2 = narrow(stack.pop()?)?;
            stack.extend([v1, v2]);
        }
    }
    Some(())
}

fn pop_values(stack: &mut Vec<Slot>, n: usize) -> Option<()> {
    let keep = stack.len().checked_sub(n)?;
    stack.truncate(keep);
    Some(())
}

/// Effet d'une instruction sur une pile de categories ; `None` s'il est impossible.
fn step(stack: &mut Vec<Slot>, op: &Op) -> Option<()> {
    match op {
        Op::Nop | Op::Iinc { .. } | Op::Goto(_) => {}
        Op::Null | Op::New(_) => stack.push(Slot::Ref),
        Op::Const(slot) | Op::Ldc(slot) | Op::Load { slot, .. } => stack.push(*slot),
        Op::Store { .. }
        | Op::MonitorEnter
        | Op::MonitorExit
        | Op::Switch(_)
        | Op::PutStatic(_) => pop_values(stack, 1)?,
        Op::If { pops, .. } => pop_values(stack, usize::from(*pops))?,
        Op::Prim { pops, push } => {
            pop_values(stack, usize::from(*pops))?;
            stack.extend(*push);
        }
        Op::ArrayLoad(slot) => {
            pop_values(stack, 2)?;
            stack.push(*slot);
        }
        Op::ArrayStore(_) => pop_values(stack, 3)?,
        Op::ArrayLength | Op::InstanceOf => {
            pop_values(stack, 1)?;
            stack.push(Slot::Prim);
        }
        Op::GetField(field) => {
            pop_values(stack, 1)?;
            stack.push(Slot::of_field(&field.descriptor));
        }
        Op::PutField(_) => pop_values(stack, 2)?,
        Op::GetStatic(field) => stack.push(Slot::of_field(&field.descriptor)),
        Op::Invoke {
            args,
            receiver,
            ret,
            ..
        } => {
            pop_values(stack, args.len() + usize::from(*receiver))?;
            stack.extend(*ret);
        }
        Op::NewArray | Op::CheckCast => {
            pop_values(stack, 1)?;
            stack.push(Slot::Ref);
        }
        Op::MultiNewArray(dims) => {
            pop_values(stack, usize::from(*dims))?;
            stack.push(Slot::Ref);
        }
        Op::Stack(stack_op) => apply_stack_op(stack, *stack_op, |slot| *slot)?,
        Op::Return(_) | Op::Throw => stack.clear(),
    }
    Some(())
}

fn depth(stack: &[Slot]) -> u16 {
    stack.iter().map(|slot| slot.size()).sum()
}

/// Hauteur maximale de la pile, en emplacements, en rejouant l'effet de chaque
/// instruction sur le graphe de flot de controle (gestionnaires compris).
///
/// # Errors
///
/// [`ParseError::Malformed`] si un effet est impossible (depilement sous zero), si deux
/// chemins arrivent a une instruction avec des piles differentes, si une cible ne tombe
/// pas sur le debut d'une instruction, ou si le code se termine sans retour : le code
/// n'est pas compris, il n'est pas analysable (FM-20).
pub fn stack_depths(instructions: &[Instruction], handlers: &[Handler]) -> Result<u16, ParseError> {
    fn schedule(
        entry: &mut [Option<Vec<Slot>>],
        pending: &mut Vec<usize>,
        at: usize,
        stack: Vec<Slot>,
    ) -> Result<(), ParseError> {
        match &entry[at] {
            Some(known) if *known == stack => Ok(()),
            Some(_) => Err(ParseError::Malformed("piles differentes a une jonction")),
            None => {
                entry[at] = Some(stack);
                pending.push(at);
                Ok(())
            }
        }
    }
    if instructions.is_empty() {
        return Ok(0);
    }
    let index_of = |offset: u32| {
        instructions
            .binary_search_by_key(&offset, |insn| insn.offset)
            .ok()
    };
    let mut entry: Vec<Option<Vec<Slot>>> = vec![None; instructions.len()];
    let mut pending = Vec::new();
    schedule(&mut entry, &mut pending, 0, Vec::new())?;
    for handler in handlers {
        let at = index_of(handler.handler)
            .ok_or(ParseError::Malformed("gestionnaire hors instruction"))?;
        schedule(&mut entry, &mut pending, at, vec![Slot::Ref])?;
    }
    // Un gestionnaire commence avec l'exception sur la pile.
    let mut max = u16::from(!handlers.is_empty());
    while let Some(at) = pending.pop() {
        let insn = &instructions[at];
        let mut stack = entry[at].clone().unwrap_or_default();
        step(&mut stack, &insn.op).ok_or(ParseError::Malformed("effet de pile impossible"))?;
        max = max.max(depth(&stack));
        for &target in insn.branch_targets() {
            let to = index_of(target).ok_or(ParseError::Malformed("cible hors instruction"))?;
            schedule(&mut entry, &mut pending, to, stack.clone())?;
        }
        if !insn.ends_flow() {
            if at + 1 >= instructions.len() {
                return Err(ParseError::Malformed("le code se termine sans retour"));
            }
            schedule(&mut entry, &mut pending, at + 1, stack)?;
        }
    }
    Ok(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::Cursor;

    fn empty_pool() -> ConstantPool {
        ConstantPool::parse(&mut Cursor::new(&[0, 1])).expect("reservoir vide")
    }

    fn decoded(code: &[u8]) -> Result<Vec<Instruction>, ParseError> {
        decode(code, &empty_pool(), &[])
    }

    #[test]
    fn subroutines_and_unknown_opcodes_are_not_analysable() {
        // jsr, ret, jsr_w (FM-20) ; 0xcb n'existe pas dans un fichier .class.
        for code in [
            &[0xa8, 0x00, 0x03, 0xb1][..],
            &[0xa9, 0x01, 0xb1],
            &[0xc9, 0x00, 0x00, 0x00, 0x05, 0xb1],
            &[0xcb, 0xb1],
        ] {
            assert!(
                matches!(
                    decoded(code),
                    Err(ParseError::BadInstruction { offset: 0, .. })
                ),
                "{code:02x?}"
            );
        }
        // Operande tronque.
        assert!(matches!(
            decoded(&[0x10]),
            Err(ParseError::BadInstruction { .. })
        ));
    }

    #[test]
    fn stack_operations_respect_categories() {
        use Slot::{Prim as P, Ref as R, Wide as W};
        // Pile avant, operation, pile apres ou `None` si l'operation est impossible.
        type Case = (&'static [Slot], StackOp, Option<&'static [Slot]>);
        let cases: [Case; 14] = [
            (&[P, R], StackOp::Swap, Some(&[R, P])),
            (&[W, P], StackOp::Swap, None),
            (&[W], StackOp::Pop, None),
            (&[W], StackOp::Pop2, Some(&[])),
            (&[P, P], StackOp::Pop2, Some(&[])),
            (&[W], StackOp::Dup, None),
            (&[W], StackOp::Dup2, Some(&[W, W])),
            (&[R, P], StackOp::Dup2, Some(&[R, P, R, P])),
            (&[W, P], StackOp::DupX2, Some(&[P, W, P])),
            (&[R, R, P], StackOp::DupX2, Some(&[P, R, R, P])),
            (&[P, W], StackOp::Dup2X1, Some(&[W, P, W])),
            (&[W, W], StackOp::Dup2X2, Some(&[W, W, W])),
            (&[R, R, P, P], StackOp::Dup2X2, Some(&[P, P, R, R, P, P])),
            (&[W, R, P], StackOp::Dup2X2, Some(&[R, P, W, R, P])),
        ];
        for (before, op, after) in cases {
            let mut stack = before.to_vec();
            let result = apply_stack_op(&mut stack, op, |slot| *slot);
            match after {
                Some(after) => assert_eq!(stack, after, "{before:?} {op:?}"),
                None => assert!(result.is_none(), "{before:?} {op:?} doit echouer"),
            }
        }
        // Pas assez de valeurs.
        assert!(apply_stack_op(&mut vec![P], StackOp::DupX1, |slot| *slot).is_none());
    }

    #[test]
    fn incoherent_code_is_rejected_by_the_replay() {
        let at = |offset: u32, len: u32, op: Op| Instruction { offset, len, op };
        // Depilement sous zero.
        let underflow = [
            at(0, 1, Op::Stack(StackOp::Pop)),
            at(1, 1, Op::Return(None)),
        ];
        // Deux chemins arrivent en 6 avec un int ou avec un long.
        let mismatch = [
            at(0, 1, Op::Const(Slot::Prim)),
            at(1, 3, Op::If { pops: 1, target: 7 }),
            at(4, 1, Op::Const(Slot::Prim)),
            at(5, 3, Op::Goto(9)),
            at(8, 1, Op::Nop),
        ];
        let mismatch_to = [
            at(0, 1, Op::Const(Slot::Prim)),
            at(1, 3, Op::If { pops: 1, target: 5 }),
            at(4, 1, Op::Const(Slot::Prim)),
            at(5, 1, Op::Return(None)),
        ];
        let bad_target = [at(0, 3, Op::Goto(1)), at(3, 1, Op::Return(None))];
        let falls_off = [at(0, 1, Op::Nop)];
        for (name, code) in [
            ("depilement", &underflow[..]),
            ("cible au milieu", &mismatch),
            ("jonction", &mismatch_to),
            ("cible", &bad_target),
            ("fin", &falls_off),
        ] {
            assert!(
                matches!(stack_depths(code, &[]), Err(ParseError::Malformed(_))),
                "{name}"
            );
        }
        // Un gestionnaire qui ne tombe pas sur une instruction.
        let handler = Handler {
            start: 0,
            end: 1,
            handler: 7,
            catch_type: None,
        };
        assert!(stack_depths(&falls_off, &[handler]).is_err());
    }

    #[test]
    fn method_descriptors_give_argument_categories() {
        use Slot::{Prim as P, Ref as R, Wide as W};
        assert_eq!(method_signature("()V"), Some((vec![], None)));
        assert_eq!(
            method_signature("(IJLjava/lang/String;[[DZ)D"),
            Some((vec![P, W, R, R, P], Some(W)))
        );
        assert_eq!(method_signature("([J)[I"), Some((vec![R], Some(R))));
        for bad in ["", "V", "(I", "(X)V", "(Ljava/lang/String)V", "()"] {
            assert_eq!(method_signature(bad), None, "{bad}");
        }
    }
}
