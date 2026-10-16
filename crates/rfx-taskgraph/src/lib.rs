//! C-16 : Task Graph — le travail d'un tick sous forme de graphe acyclique executable.
//!
//! Cahier des charges : PARTIE 5.16. Invariant : INV-01 (acyclique). Code d'erreur :
//! `E-2301`. Tests : T-250 a T-253. Maturite : `STABLE`.
//!
//! Le graphe decrit le travail, il ne l'execute pas : dependances explicites, priorites,
//! echeances, cout estime, et une charge utile que l'executant interprete. C'est C-17
//! qui l'execute, vague par vague. Au jalon M2, il ne porte que des taches internes au
//! runtime (analyse, persistance) : aucune transformation du jeu.
//!
//! # Sens des dependances
//!
//! `rfx-taskgraph` depend de `rfx-model` et de rien d'autre (INV-13). Il ne journalise
//! pas : il refuse et compte, et c'est `rfx-core` qui transforme le refus en `E-2301`
//! journalise.

#![doc(html_root_url = "https://example.invalid/rustforgex")]

pub mod graph;
pub mod node;
mod transform;

pub use graph::{DrainReport, GraphError, GraphStats, TaskGraph};
pub use node::{
    CancelToken, Deadline, Priority, TaskId, TaskKind, TaskNode, TaskPayload, TaskSpec,
};
