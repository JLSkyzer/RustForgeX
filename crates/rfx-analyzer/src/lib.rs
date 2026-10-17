//! Analyse statique des unites de travail : C-07 (Call Graph), puis C-08 et C-10 a C-13.
//!
//! Cahier des charges : PARTIE 3 (crate `rfx-analyzer`), PARTIE 5.7. Decision :
//! ADR-050. Maturite : `STABLE`.
//!
//! - [`hierarchy::ClassIndex`] : les classes chargees, et la methode qu'un appel
//!   virtuel executerait pour un receveur donne ;
//! - [`callgraph::CallGraph`] : le graphe d'appel partiel d'une methode, borne, avec
//!   ses aretes resolues, non resolues et observees, et ses cycles.
//!
//! # Sens des dependances
//!
//! `rfx-analyzer` ne depend que de `rfx-bytecode`. Il ne connait ni le runtime ni la
//! frontiere FFI : c'est `rfx-core` qui lui donnera les classes et lira ses resultats
//! (INV-13).

pub mod callgraph;
pub mod hierarchy;

pub use callgraph::{CallGraph, Edge, EdgeKind, Limits, Node, Resolution, Unresolved};
pub use hierarchy::{ClassIndex, MethodRef, Selection};
