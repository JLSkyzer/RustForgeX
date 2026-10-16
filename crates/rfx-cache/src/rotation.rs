//! C-48 : rotation des fichiers persistes, par age et par taille.
//!
//! Cahier des charges : PARTIE 5.46, R-691 (conservation par defaut : 7 jours ou
//! 256 Mio), R-225. Test : T-501. Decision : ADR-048. Maturite : `STABLE`.
//!
//! La rotation porte sur un repertoire : journaux, rapports, dumps d'incident. Elle
//! supprime d'abord ce qui a depasse l'age maximal, puis les plus anciens tant que le
//! total depasse la taille maximale. Elle s'execute sur le fil d'entree-sortie, jamais
//! sur le fil autoritatif (R-690).

use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::fs::FileSystem;

/// Conservation des fichiers d'un repertoire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotationPolicy {
    /// Age au-dela duquel un fichier est supprime.
    pub max_age: Duration,
    /// Taille totale au-dela de laquelle les plus anciens sont supprimes, en octets.
    pub max_total_bytes: u64,
}

impl Default for RotationPolicy {
    /// 7 jours ou 256 Mio (R-691).
    fn default() -> Self {
        Self {
            max_age: Duration::from_secs(7 * 24 * 3600),
            max_total_bytes: 256 << 20,
        }
    }
}

/// Bilan d'une rotation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RotationReport {
    /// Fichiers supprimes.
    pub removed: usize,
    /// Octets restants dans le repertoire.
    pub kept_bytes: u64,
}

/// Applique `policy` aux fichiers de `dir`, a la date `now`. Un repertoire absent n'a
/// rien a faire tourner.
///
/// # Errors
///
/// L'erreur du systeme a la lecture du repertoire ou a une suppression.
pub fn rotate(
    fs: &dyn FileSystem,
    dir: &Path,
    policy: &RotationPolicy,
    now: SystemTime,
) -> io::Result<RotationReport> {
    let mut files = match fs.list(dir) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(RotationReport::default())
        }
        Err(error) => return Err(error),
    };
    // Les plus anciens d'abord : ce sont eux que la taille fera partir.
    files.sort_by_key(|file| file.modified);
    let mut report = RotationReport::default();
    let mut kept = Vec::with_capacity(files.len());
    for file in files {
        let age = now.duration_since(file.modified).unwrap_or(Duration::ZERO);
        if age > policy.max_age {
            fs.remove(&file.path)?;
            report.removed += 1;
        } else {
            kept.push(file);
        }
    }
    let mut total: u64 = kept.iter().map(|file| file.len).sum();
    for file in &kept {
        if total <= policy.max_total_bytes {
            break;
        }
        fs.remove(&file.path)?;
        total -= file.len;
        report.removed += 1;
    }
    report.kept_bytes = total;
    Ok(report)
}
