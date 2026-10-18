//! Analyse statique des unites de travail : C-07 (Call Graph), puis C-08 et C-10 a C-13.
//!
//! Cahier des charges : PARTIE 3 (crate `rfx-analyzer`), PARTIE 5.7. Decision :
//! ADR-050. Maturite : `STABLE`.
//!
//! - [`hierarchy::ClassIndex`] : les classes chargees, et la methode qu'un appel
//!   virtuel executerait pour un receveur donne ;
//! - [`callgraph::CallGraph`] : le graphe d'appel partiel d'une methode, borne, avec
//!   ses aretes resolues, non resolues et observees, et ses cycles ;
//! - [`classify`] : les drapeaux et la confiance d'une unite de travail, fonctions pures
//!   des faits que les analyses ont produits (C-08, ADR-054) ;
//! - [`dataflow`] : ce que le code d'une methode lit, ecrit, appelle et laisse
//!   echapper, avec l'origine de chaque reference (C-11, ADR-055) ;
//! - [`dependency`] : de quelles autres unites chacune depend dans le tick, au niveau
//!   des domaines (C-10, ADR-056).
//!
//! # Sens des dependances
//!
//! `rfx-analyzer` ne depend que de `rfx-bytecode` et de `rfx-model`, pour les types de
//! faits. Il ne connait ni le runtime ni la frontiere FFI : c'est `rfx-core` qui lui
//! donne les classes et lit ses resultats (INV-13).

pub mod callgraph;
pub mod classify;
pub mod dataflow;
pub mod dependency;
pub mod hierarchy;

pub use callgraph::{CallGraph, Edge, EdgeKind, Limits, Node, Resolution, Unresolved};
pub use classify::{classify, confidence, Facts};
pub use hierarchy::{ClassIndex, MethodRef, Selection};
