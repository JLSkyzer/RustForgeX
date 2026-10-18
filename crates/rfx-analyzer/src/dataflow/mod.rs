//! C-11 : analyse de flux de donnees, phase intraprocedurale (ADR-055).
//!
//! Cahier des charges : PARTIE 5.11, R-380, R-381, FM-20. Maturite : `EXPERIMENTAL` —
//! rien ne consomme encore ces resultats en jeu.
//!
//! - [`analyze`] : les faits du code d'une methode ([`MethodFacts`]) ;
//! - [`intraprocedural_effects`] : ces faits ramenes aux domaines memoire (DM-06) et
//!   aux effets de bord (DM-07), par une table derivee de la hierarchie
//!   ([`DomainTable`]) et une analyse d'echappement des objets alloues ;
//! - [`workload_effects`] : les effets d'une unite de travail, en composant les
//!   resumes ([`Summary`]) des methodes de son graphe d'appel (C-07).
//!
//! Chaque reference de la pile et des variables locales porte l'ensemble de ses
//! sources possibles ([`Source`]) : parametre, allocation, valeur lue par un champ, un
//! statique, un tableau, un appel, exception attrapee, constante, inconnu. Un point
//! fixe sur les blocs de base, gestionnaires d'exceptions compris, stabilise ces
//! ensembles ; une derniere passe consigne les faits de chaque instruction atteignable
//! ([`MethodFacts`]).
//!
//! # Surete
//!
//! Toute incertitude elargit, aucune ne reduit : une union qui depasse la borne de
//! sources devient [`Source::Unknown`], une variable locale de natures differentes
//! selon le chemin devient inutilisable, et une methode que l'analyse ne comprend pas
//! ou qui depasse son budget rend [`Unanalysable`], jamais des faits partiels.

mod compose;
mod domains;
mod effects;
mod facts;
mod interp;
mod interproc;
mod summary;
mod value;

pub use domains::{DomainTable, CLASS_ANCHORS, INTERFACE_ANCHORS};
pub use effects::{escaping_allocations, intraprocedural_effects, MethodEffects};
pub use facts::{
    Access, Allocation, CallFact, MethodFacts, Monitor, Target, Throw, Unanalysable, Work,
};
pub use interp::DataflowLimits;
pub use interproc::{
    analyze_graph, workload_effects, workload_effects_from, GraphFacts, UnknownCauses,
    WorkloadEffects, MAX_PASSES_PER_METHOD,
};
pub use summary::Summary;
pub use value::{Frame, Source, Sources, Value, MAX_SOURCES};

use rfx_bytecode::{access, method_signature, ClassFile, MethodInfo, Slot};

use interp::Body;

/// Analyse une methode de `class`.
///
/// # Errors
///
/// [`Unanalysable`] si la methode n'a pas de code, si son code est illisible, contient
/// `jsr`/`ret` ou est incoherent, ou si l'analyse depasse `limits` : son effet est alors
/// inconnu (FM-20).
pub fn analyze(
    class: &ClassFile,
    method: &MethodInfo,
    limits: &DataflowLimits,
) -> Result<MethodFacts, Unanalysable> {
    if method.code.is_none() {
        return Err(Unanalysable::NoCode);
    }
    let instructions = class.instructions(method).map_err(Unanalysable::Bytecode)?;
    let (params, ret) = method_signature(&method.descriptor)
        .ok_or(Unanalysable::Incoherent("descripteur de methode invalide"))?;
    let mut locals = vec![Value::Top; usize::from(method.max_locals)];
    let mut at = 0_u16;
    let mut logical = 0_u16;
    if method.access & access::STATIC == 0 {
        interp::store(&mut locals, 0, Value::Ref(Sources::one(Source::Param(0))))?;
        at = 1;
        logical = 1;
    }
    for slot in params {
        interp::store(&mut locals, at, Value::of(slot, Source::Param(logical)))?;
        at += slot.size();
        logical += 1;
    }
    let body = Body {
        instructions: &instructions,
        handlers: &method.handlers,
        max_stack: method.max_stack,
    };
    let entry = Frame {
        locals,
        stack: Vec::new(),
    };
    interp::run(&body, entry, ret == Some(Slot::Ref), limits)
}
