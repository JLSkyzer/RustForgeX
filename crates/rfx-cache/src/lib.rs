//! C-09 Workload Database et C-48 Persistence Store : retenir les unites de travail,
//! les relire au demarrage, et les ecrire sans jamais bloquer le jeu.
//!
//! Cahier des charges : PARTIE 5.9 (IF-12), PARTIE 5.46, PARTIE 4.17 (DM-17).
//! Exigences : R-223 a R-225, R-360 a R-362, R-690 a R-692. Tests : T-180 a T-184,
//! T-500 a T-502. Decisions : ADR-048, ADR-049. Maturite : `STABLE`.
//!
//! - [`db::WorkloadDb`] : la base en memoire, indexee et bornee.
//! - [`file`] : le format de `workload.db`, avec en-tete et somme de controle.
//! - [`store::load`] : la relecture, qui rejette un autre environnement et met de cote
//!   un fichier illisible.
//! - [`service::DbService`] : le fil `RFX-IO`, qui detient la base, la relit au
//!   demarrage et l'ecrit ; seul a toucher au disque.
//! - [`rotation`] : la conservation des fichiers par age et par taille.
//!
//! # Sens des dependances
//!
//! `rfx-cache` depend de `rfx-model` (et de `serde`, `xxhash-rust`, deja embarques). Il
//! ne connait ni l'ordonnanceur ni le runtime : c'est `rfx-core` qui decide quand
//! confier les lots et quand fermer (INV-13).

#![doc(html_root_url = "https://example.invalid/rustforgex")]

pub mod db;
pub mod file;
pub mod fs;
pub mod record;
pub mod rotation;
pub mod service;
pub mod store;

pub use db::{Idx, WorkloadDb};
pub use fs::{FileInfo, FileSystem, StdFileSystem};
pub use record::{History, HistoryEvent, HistoryKind, WorkloadRecord, HISTORY_CAPACITY};
pub use rotation::{RotationPolicy, RotationReport};
pub use service::{
    DbConfig, DbService, DbStats, DynamicsUpdate, LoadState, Refused, Rejected, SyncBatch,
};
pub use store::{load, LoadOutcome};
