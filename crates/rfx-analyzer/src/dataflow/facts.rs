//! Faits intraproceduraux d'une methode : ce que son code lit, ecrit, appelle et laisse
//! echapper, chaque base etant decrite par ses sources (ADR-055).
//!
//! Ces faits ne disent encore rien des domaines memoire (DM-06) : c'est la phase
//! suivante qui les en deduit, a partir des classes et des sources.

use rfx_bytecode::{CallTarget, InvokeKind, MemberRef, ParseError};

use super::value::Sources;

/// Ce qu'un acces atteint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Champ d'instance, sur l'objet de la base.
    Field(MemberRef),
    /// Champ statique ; la base est vide.
    Static(MemberRef),
    /// Element du tableau de la base.
    Element,
}

/// Une lecture ou une ecriture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    /// Position de l'instruction.
    pub offset: u32,
    /// Ce qui est atteint.
    pub target: Target,
    /// Objet ou tableau dont un champ ou un element est atteint ; vide pour un statique.
    pub base: Sources,
    /// Pour l'ecriture d'une reference : ce qui est ecrit, qui s'echappe ainsi.
    pub value: Option<Sources>,
}

/// Un appel, avec l'origine de chacun de ses arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallFact {
    /// Position de l'instruction.
    pub offset: u32,
    /// Nature de l'appel.
    pub kind: InvokeKind,
    /// Cible ecrite.
    pub target: CallTarget,
    /// Arguments, receveur en tete s'il y en a un ; `None` pour une primitive.
    pub args: Vec<Option<Sources>>,
}

/// Un objet alloue par la methode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allocation {
    /// Position de l'instruction.
    pub offset: u32,
    /// Classe instanciee ; `None` pour un tableau.
    pub class: Option<String>,
}

/// Une prise de verrou (`monitorenter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    /// Position de l'instruction.
    pub offset: u32,
    /// Objet verrouille.
    pub lock: Sources,
}

/// Un `athrow` qu'aucun gestionnaire universel de la methode ne couvre : l'exception
/// peut en sortir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Throw {
    /// Position de l'instruction.
    pub offset: u32,
    /// Exception levee.
    pub value: Sources,
}

/// Faits d'une methode, dans l'ordre du code, pour ses seules instructions
/// atteignables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MethodFacts {
    /// Lectures de champs, de statiques et d'elements.
    pub reads: Vec<Access>,
    /// Ecritures de champs, de statiques et d'elements.
    pub writes: Vec<Access>,
    /// Appels.
    pub calls: Vec<CallFact>,
    /// Allocations.
    pub allocations: Vec<Allocation>,
    /// Prises de verrou.
    pub monitors: Vec<Monitor>,
    /// Exceptions qui peuvent sortir par `athrow`.
    pub throws: Vec<Throw>,
    /// Origine des references rendues ; `None` si la methode ne rend pas de reference.
    pub returns: Option<Sources>,
    /// Unions que la borne de sources a rendues inconnues : perte de precision, jamais
    /// de surete.
    pub widened: u32,
    /// Travail de l'analyse.
    pub work: Work,
}

/// Travail d'une analyse, pour en borner le cout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Work {
    /// Blocs de base.
    pub blocks: u32,
    /// Emplacements d'etat gardes : blocs × (variables locales + pile).
    pub cells: u64,
    /// Passages dans les blocs pendant le point fixe.
    pub visits: u32,
    /// Plus grand nombre de passages dans un meme bloc.
    pub max_block_visits: u32,
}

/// Pourquoi une methode n'est pas analysee : son effet est alors inconnu (FM-20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unanalysable {
    /// Methode abstraite ou native : pas de code.
    NoCode,
    /// Code illisible, ou `jsr`/`ret`.
    Bytecode(ParseError),
    /// Code incoherent : pile impossible, jonction de formes differentes, variable
    /// locale hors bornes.
    Incoherent(&'static str),
    /// Le budget de l'analyse est depasse.
    Budget,
}

impl core::fmt::Display for Unanalysable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoCode => f.write_str("methode sans code"),
            Self::Bytecode(e) => write!(f, "bytecode non analysable : {e}"),
            Self::Incoherent(what) => write!(f, "code incoherent : {what}"),
            Self::Budget => f.write_str("budget d'analyse depasse"),
        }
    }
}

impl std::error::Error for Unanalysable {}
