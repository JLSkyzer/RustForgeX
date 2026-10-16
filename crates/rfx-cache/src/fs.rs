//! C-48 : acces au disque, derriere une interface.
//!
//! Cahier des charges : PARTIE 5.46 (R-690 a R-692), R-224 (ecriture atomique).
//! Decision : ADR-048. Maturite : `STABLE`.
//!
//! Toute entree-sortie de la persistance passe par [`FileSystem`]. C'est ce qui permet
//! de verifier par instrumentation qu'aucune n'a lieu sur le fil appelant (T-500), et de
//! simuler un disque plein (T-502), sans toucher au vrai disque.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Un fichier d'un repertoire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    /// Chemin.
    pub path: PathBuf,
    /// Taille, en octets.
    pub len: u64,
    /// Date de derniere modification.
    pub modified: SystemTime,
}

/// Operations de la persistance sur le disque.
pub trait FileSystem: Send + Sync {
    /// Lit un fichier entier.
    ///
    /// # Errors
    ///
    /// L'erreur du systeme, dont `NotFound` si le fichier n'existe pas.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Ecrit un fichier de facon atomique (R-224) : un lecteur voit l'ancien contenu ou
    /// le nouveau, jamais un melange, meme si le processus s'arrete au milieu.
    ///
    /// # Errors
    ///
    /// L'erreur du systeme ; le fichier d'origine est alors intact.
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;

    /// Renomme un fichier, en remplacant la destination si elle existe.
    ///
    /// # Errors
    ///
    /// L'erreur du systeme.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// Supprime un fichier.
    ///
    /// # Errors
    ///
    /// L'erreur du systeme.
    fn remove(&self, path: &Path) -> io::Result<()>;

    /// Fichiers d'un repertoire, sans les sous-repertoires.
    ///
    /// # Errors
    ///
    /// L'erreur du systeme.
    fn list(&self, dir: &Path) -> io::Result<Vec<FileInfo>>;
}

/// Le vrai disque.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdFileSystem;

/// Chemin du fichier temporaire d'une ecriture atomique : meme repertoire, pour que le
/// renommage final ne traverse pas de volume.
#[must_use]
pub fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

impl FileSystem for StdFileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = temporary_path(path);
        let written = (|| {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(bytes)?;
            // Sur disque avant le renommage : sinon, une coupure pourrait laisser le
            // nouveau nom pointer sur un contenu incomplet.
            file.sync_all()
        })();
        if let Err(error) = written {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        fs::rename(&temporary, path).inspect_err(|_| {
            let _ = fs::remove_file(&temporary);
        })
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<FileInfo>> {
        let mut files = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_file() {
                files.push(FileInfo {
                    path: entry.path(),
                    len: metadata.len(),
                    modified: metadata.modified()?,
                });
            }
        }
        Ok(files)
    }
}
