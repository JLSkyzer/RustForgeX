//! C-09 : relire `workload.db`.
//!
//! Cahier des charges : PARTIE 5.9 (IF-12 `load`), R-223, R-360, FM-17. Tests : T-181,
//! T-182. Decision : ADR-048. Maturite : `STABLE`.
//!
//! La lecture se fait au demarrage, hors tick. L'ecriture, elle, passe par
//! [`crate::service::DbService`], sur son propre fil.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rfx_model::{Fingerprint, WorkId};

use crate::db::WorkloadDb;
use crate::exec::{Fork, InlineFork};
use crate::file::{self, DecodeError};
use crate::fs::FileSystem;

/// Issue d'une relecture.
#[derive(Debug)]
pub enum LoadOutcome {
    /// Aucun fichier : premiere execution.
    Missing,
    /// Le fichier a ete ecrit dans un autre environnement (versions, mods, JVM,
    /// materiel, configuration) : il est ignore en entier (R-360). Il reste en place et
    /// sera remplace a la prochaine ecriture.
    EnvironmentChanged,
    /// Le fichier est illisible : schema inconnu, signature, troncature ou contenu
    /// altere. Il est renomme en `.bak` et la base repart vide (R-223, FM-17).
    SetAside {
        /// Ce qui l'a rendu illisible.
        reason: DecodeError,
        /// Ou il a ete mis de cote.
        backup: PathBuf,
    },
    /// Le fichier a ete relu.
    Loaded {
        /// La base relue.
        db: WorkloadDb,
        /// Entrees ecartees parce que leur code a change (`rfx.db.load_rejected_entries`).
        rejected_entries: usize,
    },
}

impl LoadOutcome {
    /// La base a utiliser : celle relue, ou une base vide.
    #[must_use]
    pub fn into_db(self) -> WorkloadDb {
        match self {
            LoadOutcome::Loaded { db, .. } => db,
            _ => WorkloadDb::new(),
        }
    }
}

/// Chemin de mise de cote d'un fichier illisible.
#[must_use]
pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// Relit `path` (R-360).
///
/// `environment` est l'empreinte de l'environnement courant. `current` rend l'empreinte
/// courante d'une unite de travail si elle est deja connue : une entree dont le code a
/// change est ecartee. Une entree encore inconnue est gardee ; elle sera verifiee quand
/// l'unite de travail sera observee ([`WorkloadDb::observe`]).
///
/// # Errors
///
/// Une erreur du systeme autre que l'absence du fichier, ou l'echec de la mise de cote.
pub fn load(
    fs: &dyn FileSystem,
    path: &Path,
    environment: &Fingerprint,
    current: impl Fn(WorkId) -> Option<Fingerprint>,
) -> io::Result<LoadOutcome> {
    load_with(fs, path, environment, current, &InlineFork)
}

/// Comme [`load`], le decodage des blocs en sous-travaux de `fork` (ADR-061).
///
/// # Errors
///
/// Comme [`load`].
pub fn load_with(
    fs: &dyn FileSystem,
    path: &Path,
    environment: &Fingerprint,
    current: impl Fn(WorkId) -> Option<Fingerprint>,
    fork: &dyn Fork,
) -> io::Result<LoadOutcome> {
    let bytes = match fs.read(path) {
        Ok(bytes) => Arc::new(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(LoadOutcome::Missing),
        Err(error) => return Err(error),
    };
    // L'en-tete seul d'abord : un autre environnement est rejete sans decoder le reste.
    let decoded = file::decode_header(&bytes).and_then(|header| {
        if header.same_environment(environment) {
            file::decode_with(Arc::clone(&bytes), fork).map(Some)
        } else {
            Ok(None)
        }
    });
    match decoded {
        Ok(None) => Ok(LoadOutcome::EnvironmentChanged),
        Ok(Some(decoded)) => {
            let mut db = WorkloadDb::from_records(decoded.records);
            let rejected_entries = db.retain_matching(current);
            Ok(LoadOutcome::Loaded {
                db,
                rejected_entries,
            })
        }
        Err(reason) => {
            let backup = backup_path(path);
            fs.rename(path, &backup)?;
            Ok(LoadOutcome::SetAside { reason, backup })
        }
    }
}
