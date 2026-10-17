//! Lecture des fichiers `.class` (JVMS §4) pour l'analyse statique (C-07).
//!
//! Cahier des charges : PARTIE 3 (crate `rfx-bytecode`), PARTIE 5.7, FM-15,
//! PARTIE 19.2 (entrees non fiables). Decision : ADR-050. Maturite : `STABLE`.
//!
//! N'est lu que ce que l'analyse utilise : la hierarchie (super-classe, interfaces,
//! drapeaux), les methodes et leur code, les sites d'appel `invoke*` et les methodes
//! d'amorcage des `invokedynamic` ; pour l'analyse de flux (C-11, ADR-055), chaque
//! instruction avec son effet sur la pile, et la table des exceptions. Les champs, les
//! annotations et les autres attributs sont sautes, jamais interpretes.
//!
//! # Entrees non fiables
//!
//! Les octets viennent d'un mod quelconque, eventuellement transforme par d'autres :
//! toute incoherence rend une [`ParseError`], jamais une panic. Un compteur lu dans le
//! fichier ne sert jamais a reserver plus de memoire que les octets restants ne peuvent
//! en remplir.

pub mod class;
pub mod code;
pub mod constant;
mod cursor;
pub mod insn;
mod mutf8;

pub use class::{access, BootstrapMethod, ClassFile, MethodInfo};
pub use code::{call_sites, CallSite, CallTarget, InvokeKind};
pub use constant::{Constant, ConstantPool, MemberRef, MethodHandleRef};
pub use insn::{
    apply_stack_op, method_signature, stack_depths, Handler, Instruction, Op, Slot, StackOp,
};

/// Pourquoi un fichier `.class` ne peut pas etre lu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Les quatre premiers octets ne sont pas `0xCAFEBABE`.
    BadMagic,
    /// Le fichier, ou un de ses attributs, s'arrete avant la fin annoncee.
    Truncated,
    /// Une constante porte une etiquette inconnue.
    UnknownTag {
        /// Indice de la constante.
        index: u16,
        /// Etiquette lue.
        tag: u8,
    },
    /// Un indice de constante est nul, hors du reservoir, ou designe la seconde moitie
    /// d'une constante longue.
    BadIndex {
        /// Indice lu.
        index: u16,
    },
    /// Une constante n'est pas de la nature attendue a cet endroit.
    WrongKind {
        /// Indice de la constante.
        index: u16,
        /// Nature attendue.
        expected: &'static str,
    },
    /// Une chaine n'est pas de l'UTF-8 modifie valide.
    InvalidUtf8 {
        /// Indice de la constante.
        index: u16,
    },
    /// Une instruction est inconnue, ou deborde du code.
    BadInstruction {
        /// Position dans le code de la methode.
        offset: usize,
        /// Code d'operation lu.
        opcode: u8,
    },
    /// Une autre incoherence de structure.
    Malformed(&'static str),
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadMagic => f.write_str("signature 0xCAFEBABE absente"),
            Self::Truncated => f.write_str("fichier tronque"),
            Self::UnknownTag { index, tag } => {
                write!(f, "constante {index} : etiquette inconnue {tag}")
            }
            Self::BadIndex { index } => write!(f, "indice de constante invalide : {index}"),
            Self::WrongKind { index, expected } => {
                write!(f, "constante {index} : {expected} attendu")
            }
            Self::InvalidUtf8 { index } => write!(f, "constante {index} : UTF-8 modifie invalide"),
            Self::BadInstruction { offset, opcode } => {
                write!(f, "instruction invalide 0x{opcode:02x} a l'octet {offset}")
            }
            Self::Malformed(what) => write!(f, "structure invalide : {what}"),
        }
    }
}

impl std::error::Error for ParseError {}
