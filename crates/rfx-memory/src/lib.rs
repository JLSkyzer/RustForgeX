//! C-31 : gestion memoire native bornee et sans contention.
//!
//! Cahier des charges : PARTIE 5.28. Exigences : R-531 (structures partagees alignees
//! sur 64 octets), R-533 (budget memoire natif borne), R-534 (memoire rapportee dans
//! les diagnostics). Tests : T-370 a T-373. Maturite : `STABLE`.
//!
//! Ce crate ne contient, a ce jalon, que ce que le jalon mobilise : le budget memoire
//! et les tampons de profilage. Les arenes de worker et de snapshot decrites en
//! PARTIE 5.28 arriveront avec l'ordonnanceur et les snapshots ; les declarer
//! maintenant reviendrait a annoncer une gestion memoire qui n'existe pas.
//!
//! Aucun `unsafe` : l'alignement vient du type [`CacheLine`], pas d'un allocateur
//! manuel. Un `Vec<CacheLine>` est aligne sur 64 octets par construction.

pub mod budget;
pub mod probe_buffer;

pub use budget::{BudgetError, MemoryBudget, Pool};
pub use probe_buffer::{ProbeBuffer, ProbeBufferPool, ProbeRecord, RecordKind, PROBE_RECORD_SIZE};

/// Taille d'une ligne de cache sur les architectures visees.
pub const CACHE_LINE: usize = 64;

/// Bloc aligne sur une ligne de cache.
///
/// Sert d'unite d'allocation a tous les tampons partages : deux tampons voisins ne
/// peuvent pas se retrouver sur la meme ligne de cache, ce qui elimine le faux partage
/// entre workers (R-531).
#[repr(align(64))]
#[derive(Debug, Clone, Copy)]
pub struct CacheLine(pub [u8; CACHE_LINE]);

impl Default for CacheLine {
    fn default() -> Self {
        Self([0; CACHE_LINE])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T-372 : l'alignement est bien celui d'une ligne de cache.
    #[test]
    fn a_cache_line_is_aligned_on_64_bytes() {
        assert_eq!(core::mem::align_of::<CacheLine>(), 64);
        assert_eq!(core::mem::size_of::<CacheLine>(), 64);
    }

    /// R-531 : un tampon fait de lignes de cache est aligne, quelle que soit sa taille.
    #[test]
    fn a_buffer_of_cache_lines_is_aligned() {
        for lines in [1_usize, 2, 17, 1024] {
            let buffer = vec![CacheLine::default(); lines];
            let address = buffer.as_ptr() as usize;
            assert_eq!(address % 64, 0, "tampon de {lines} lignes mal aligne");
        }
    }
}
