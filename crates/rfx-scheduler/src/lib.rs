//! C-17 Scheduler et C-18 Worker Runtime : executer des taches sur un pool de workers a
//! vol de travail, sans jamais degrader le fil autoritatif ni saturer la machine.
//!
//! Cahier des charges : PARTIE 5.17 et 5.18. Exigences : R-420 a R-423, R-434, R-524.
//! Codes d'erreur : `E-2401`. Maturite : `STABLE` pour le pool, la soumission, les
//! priorites, la contre-pression et `FORCE_SERIAL` ; l'execution d'un graphe, le
//! parallelisme imbrique, la surveillance des echeances et l'adaptation a la
//! sur-souscription viennent ensuite (voir `tasks/todo.md`).
//!
//! Au jalon M2, l'ordonnanceur n'execute que des taches internes au runtime (analyse,
//! persistance) : aucune transformation du jeu.
//!
//! # Sens des dependances
//!
//! `rfx-scheduler` depend de `rfx-taskgraph` et de `crossbeam-deque` (ADR-044), et de
//! rien d'autre du projet (INV-13). Il ne contient aucun `unsafe`.

#![doc(html_root_url = "https://example.invalid/rustforgex")]

pub mod config;
pub mod pool;
pub mod task;

pub use config::{default_workers, SchedulerConfig};
pub use pool::{Backpressure, JobOptions, Scheduler, SchedulerStats};
pub use task::{GroupTally, RejectReason, Task, TaskContext, TaskGroup, TaskOutcome};
