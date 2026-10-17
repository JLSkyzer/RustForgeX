//! Parcours du code d'une methode : sites d'appel `invoke*` (JVMS §6.5).
//!
//! Le parcours suit la longueur de chaque instruction, sans interpreter la pile. Une
//! instruction inconnue, ou qui deborde du code, arrete le parcours par une erreur :
//! un site d'appel manque serait pire qu'une methode refusee, car l'analyse en
//! deduirait une methode qui n'appelle rien.

use crate::class::BootstrapMethod;
use crate::constant::{Constant, ConstantPool, MemberRef, MethodHandleRef};
use crate::cursor::Cursor;
use crate::ParseError;

/// Nature d'un appel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvokeKind {
    /// `invokevirtual`.
    Virtual,
    /// `invokespecial` : constructeur, methode privee, `super`.
    Special,
    /// `invokestatic`.
    Static,
    /// `invokeinterface`.
    Interface,
    /// `invokedynamic`.
    Dynamic,
}

/// Cible d'un site d'appel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTarget {
    /// Methode designee par une reference de membre.
    Member(MemberRef),
    /// Site `invokedynamic`.
    Dynamic {
        /// Nom du site (`apply`, `makeConcatWithConstants`).
        name: String,
        /// Descripteur du site.
        descriptor: String,
        /// Methode d'amorcage.
        bootstrap: MethodHandleRef,
        /// Arguments statiques qui sont des handles de methode, dans l'ordre. Pour
        /// `LambdaMetafactory`, le premier est la methode qui implemente la lambda.
        handle_arguments: Vec<MethodHandleRef>,
    },
}

/// Un site d'appel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    /// Position dans le code.
    pub offset: u32,
    /// Nature de l'appel.
    pub kind: InvokeKind,
    /// Cible.
    pub target: CallTarget,
}

/// Longueur fixe des instructions, code d'operation compris ; 0 : longueur variable
/// (`tableswitch`, `lookupswitch`, `wide`) ou code inconnu dans un fichier `.class`.
const LENGTHS: [u8; 256] = lengths();

const fn lengths() -> [u8; 256] {
    let mut table = [0_u8; 256];
    let mut op = 0;
    while op < 256 {
        table[op] = match op {
            0x00..=0x0f => 1, // nop, constantes
            0x10 => 2,        // bipush
            0x11 => 3,        // sipush
            0x12 => 2,        // ldc
            0x13 | 0x14 => 3, // ldc_w, ldc2_w
            0x15..=0x19 => 2, // xload
            0x1a..=0x35 => 1, // xload_n, xaload
            0x36..=0x3a => 2, // xstore
            0x3b..=0x83 => 1, // xstore_n, xastore, pile, arithmetique
            0x84 => 3,        // iinc
            0x85..=0x98 => 1, // conversions, comparaisons
            0x99..=0xa8 => 3, // if*, goto, jsr
            0xa9 => 2,        // ret
            0xaa | 0xab => 0, // tableswitch, lookupswitch
            0xac..=0xb1 => 1, // retours
            0xb2..=0xb8 => 3, // champs, invokevirtual/special/static
            0xb9 | 0xba => 5, // invokeinterface, invokedynamic
            0xbb => 3,        // new
            0xbc => 2,        // newarray
            0xbd => 3,        // anewarray
            0xbe | 0xbf => 1, // arraylength, athrow
            0xc0 | 0xc1 => 3, // checkcast, instanceof
            0xc2 | 0xc3 => 1, // monitorenter, monitorexit
            0xc4 => 0,        // wide
            0xc5 => 4,        // multianewarray
            0xc6 | 0xc7 => 3, // ifnull, ifnonnull
            0xc8 | 0xc9 => 5, // goto_w, jsr_w
            _ => 0,           // breakpoint, impdep, codes reserves
        };
        op += 1;
    }
    table
}

/// Sites d'appel d'un code, dans l'ordre.
///
/// # Errors
///
/// [`ParseError::BadInstruction`] si une instruction est inconnue ou deborde ; une
/// autre [`ParseError`] si une constante designee est incoherente.
pub fn call_sites(
    code: &[u8],
    pool: &ConstantPool,
    bootstraps: &[BootstrapMethod],
) -> Result<Vec<CallSite>, ParseError> {
    let mut sites = Vec::new();
    let mut offset = 0_usize;
    while offset < code.len() {
        let opcode = code[offset];
        let bad = ParseError::BadInstruction { offset, opcode };
        let len = match opcode {
            0xaa => switch_length(code, offset, true).ok_or(bad.clone())?,
            0xab => switch_length(code, offset, false).ok_or(bad.clone())?,
            0xc4 => match code.get(offset + 1) {
                Some(0x84) => 6, // wide iinc
                Some(0x15..=0x19 | 0x36..=0x3a | 0xa9) => 4,
                _ => return Err(bad),
            },
            _ => match LENGTHS[usize::from(opcode)] {
                0 => return Err(bad),
                fixed => usize::from(fixed),
            },
        };
        let operands = code.get(offset + 1..offset + len).ok_or(bad)?;
        let kind = match opcode {
            0xb6 => Some(InvokeKind::Virtual),
            0xb7 => Some(InvokeKind::Special),
            0xb8 => Some(InvokeKind::Static),
            0xb9 => Some(InvokeKind::Interface),
            0xba => Some(InvokeKind::Dynamic),
            _ => None,
        };
        if let Some(kind) = kind {
            let index = u16::from_be_bytes([operands[0], operands[1]]);
            let target = if kind == InvokeKind::Dynamic {
                dynamic_target(pool, bootstraps, index)?
            } else {
                CallTarget::Member(pool.member(index)?)
            };
            sites.push(CallSite {
                offset: u32::try_from(offset).unwrap_or(u32::MAX),
                kind,
                target,
            });
        }
        offset += len;
    }
    Ok(sites)
}

/// Longueur d'un `tableswitch` ou d'un `lookupswitch` a `offset`, remplissage compris
/// (alignement sur quatre octets depuis le debut du code), ou `None` si incoherent.
fn switch_length(code: &[u8], offset: usize, table: bool) -> Option<usize> {
    let padding = (4 - (offset + 1) % 4) % 4;
    let start = offset + 1 + padding;
    let mut cursor = Cursor::new(code.get(start..)?);
    cursor.u32().ok()?; // defaut
    let entries = if table {
        let low = i32::from_be_bytes(cursor.u32().ok()?.to_be_bytes());
        let high = i32::from_be_bytes(cursor.u32().ok()?.to_be_bytes());
        if high < low {
            return None;
        }
        let count = usize::try_from(i64::from(high) - i64::from(low) + 1).ok()?;
        cursor.take(count.checked_mul(4)?).ok()?;
        12 + count * 4
    } else {
        let pairs = i32::from_be_bytes(cursor.u32().ok()?.to_be_bytes());
        let pairs = usize::try_from(pairs).ok()?;
        cursor.take(pairs.checked_mul(8)?).ok()?;
        8 + pairs * 8
    };
    Some(1 + padding + entries)
}

fn dynamic_target(
    pool: &ConstantPool,
    bootstraps: &[BootstrapMethod],
    index: u16,
) -> Result<CallTarget, ParseError> {
    let (bootstrap_index, name_and_type) = match pool.get(index)? {
        Constant::InvokeDynamic {
            bootstrap,
            name_and_type,
        } => (*bootstrap, *name_and_type),
        _ => {
            return Err(ParseError::WrongKind {
                index,
                expected: "InvokeDynamic",
            })
        }
    };
    let bootstrap = bootstraps
        .get(usize::from(bootstrap_index))
        .ok_or(ParseError::Malformed("methode d'amorcage absente"))?;
    let (name, descriptor) = pool.name_and_type(name_and_type)?;
    let mut handle_arguments = Vec::new();
    for &argument in &bootstrap.arguments {
        if let Constant::MethodHandle { .. } = pool.get(argument)? {
            handle_arguments.push(pool.method_handle(argument)?);
        }
    }
    Ok(CallTarget::Dynamic {
        name: name.to_owned(),
        descriptor: descriptor.to_owned(),
        bootstrap: pool.method_handle(bootstrap.method)?,
        handle_arguments,
    })
}
