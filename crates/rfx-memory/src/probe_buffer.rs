//! IF-03 : tampons de profilage, possedes par le natif.
//!
//! Composant : C-31 (allocation), C-05 (consommation). Cahier des charges :
//! PARTIE 6.4. Exigences : R-708 (le tampon appartient au natif, Java ne le libere
//! jamais), R-709 (un flush n'alloue ni ne bloque ; a saturation, les enregistrements
//! les plus anciens sont perdus et comptes). Maturite : `STABLE`.
//!
//! Un tampon par thread, alloue une fois puis reutilise a chaque tick. Java y ecrit
//! sequentiellement des enregistrements de trente-deux octets, puis signale au flush
//! combien d'octets il a ecrits. Le natif consomme, et rend la main : une seule
//! traversee de frontiere par tick et par thread.
//!
//! Le tampon est un `Vec<CacheLine>`, ce qui lui donne l'alignement voulu (R-531) sans
//! allocateur manuel. La lecture se fait par indexation octet par octet : le flush a
//! lieu une fois par tick, pas dans le chemin chaud, et cette forme evite toute
//! reinterpretation de memoire — donc tout `unsafe`.

use std::collections::BTreeMap;

use crate::{CacheLine, CACHE_LINE};

/// Taille d'un enregistrement de sonde (IF-03), en octets.
pub const PROBE_RECORD_SIZE: usize = 32;

/// Taille par defaut d'un tampon de thread : 64 Kio (PARTIE 5.5).
pub const DEFAULT_BUFFER_BYTES: usize = 64 * 1024;

/// Nature d'un enregistrement de sonde (IF-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// Entree dans une methode sondee.
    Enter,
    /// Sortie d'une methode sondee.
    Exit,
    /// Allocation observee.
    Alloc,
    /// Evenement Forge.
    Event,
    /// Echantillon de pile.
    Sample,
    /// Nature inconnue : l'enregistrement est conserve mais non interprete.
    Unknown(u8),
}

impl RecordKind {
    /// Convertit l'octet de nature.
    #[must_use]
    pub fn from_byte(value: u8) -> Self {
        match value {
            0 => Self::Enter,
            1 => Self::Exit,
            2 => Self::Alloc,
            3 => Self::Event,
            4 => Self::Sample,
            other => Self::Unknown(other),
        }
    }

    /// Octet de nature transmis a travers la frontiere.
    #[must_use]
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Enter => 0,
            Self::Exit => 1,
            Self::Alloc => 2,
            Self::Event => 3,
            Self::Sample => 4,
            Self::Unknown(v) => v,
        }
    }
}

/// Enregistrement de sonde decode (IF-03, `ProbeRecord`).
///
/// Disposition binaire, petit-boutiste, alignee sur huit octets :
///
/// ```text
/// offset  taille  champ
///      0       4  probe_id        u32
///      4       1  kind            u8
///      5       1  flags           u8
///      6       2  context_hash16  u16
///      8       8  timestamp_ns    u64
///     16       8  value           u64
///     24       8  reserved        u64
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProbeRecord {
    /// Identifiant local de la sonde, resolu en `WorkId` par C-04.
    pub probe_id: u32,
    /// Drapeaux specifiques a la nature.
    pub flags: u8,
    /// Hachage court du contexte d'appel (DM-02).
    pub context_hash16: u16,
    /// Horodatage monotone, en nanosecondes.
    pub timestamp_ns: u64,
    /// Duree, taille d'allocation, ou autre valeur selon la nature.
    pub value: u64,
    /// Nature de l'enregistrement.
    pub kind: RecordKindField,
}

/// Nature de l'enregistrement, stockee sous forme d'octet pour rester `Default`.
pub type RecordKindField = u8;

impl ProbeRecord {
    /// Nature decodee de l'enregistrement.
    #[must_use]
    pub fn kind(&self) -> RecordKind {
        RecordKind::from_byte(self.kind)
    }

    /// Decode un enregistrement depuis ses trente-deux octets.
    ///
    /// Renvoie `None` si la tranche est trop courte : un enregistrement tronque est
    /// ignore, jamais complete par des zeros.
    #[must_use]
    pub fn decode(bytes: &[u8; PROBE_RECORD_SIZE]) -> Self {
        Self {
            probe_id: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            kind: bytes[4],
            flags: bytes[5],
            context_hash16: u16::from_le_bytes([bytes[6], bytes[7]]),
            timestamp_ns: u64::from_le_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ]),
            value: u64::from_le_bytes([
                bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22],
                bytes[23],
            ]),
        }
    }

    /// Encode un enregistrement. Sert aux tests et au harnais de benchmark.
    #[must_use]
    pub fn encode(&self) -> [u8; PROBE_RECORD_SIZE] {
        let mut out = [0_u8; PROBE_RECORD_SIZE];
        out[0..4].copy_from_slice(&self.probe_id.to_le_bytes());
        out[4] = self.kind;
        out[5] = self.flags;
        out[6..8].copy_from_slice(&self.context_hash16.to_le_bytes());
        out[8..16].copy_from_slice(&self.timestamp_ns.to_le_bytes());
        out[16..24].copy_from_slice(&self.value.to_le_bytes());
        out
    }
}

/// Tampon de profilage d'un thread.
#[derive(Debug)]
pub struct ProbeBuffer {
    lines: Vec<CacheLine>,
    flushes: u64,
    records_consumed: u64,
    records_lost: u64,
}

impl ProbeBuffer {
    /// Alloue un tampon d'au moins `bytes` octets, arrondi a la ligne de cache.
    #[must_use]
    pub fn new(bytes: usize) -> Self {
        let lines = bytes.div_ceil(CACHE_LINE).max(1);
        Self {
            lines: vec![CacheLine::default(); lines],
            flushes: 0,
            records_consumed: 0,
            records_lost: 0,
        }
    }

    /// Capacite du tampon, en octets.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.lines.len() * CACHE_LINE
    }

    /// Nombre d'enregistrements que le tampon peut contenir.
    #[must_use]
    pub fn capacity_records(&self) -> usize {
        self.capacity() / PROBE_RECORD_SIZE
    }

    /// Adresse du tampon, transmise a Java pour y construire un `DirectByteBuffer`.
    ///
    /// Le tampon reste la propriete du natif : Java y ecrit et ne le libere jamais
    /// (R-708). L'adresse reste valide tant que le tampon vit, c'est-a-dire tant que
    /// le handle du runtime vit.
    pub fn address(&mut self) -> *mut u8 {
        self.lines.as_mut_ptr().cast::<u8>()
    }

    /// Lit un octet du tampon.
    fn byte_at(&self, index: usize) -> u8 {
        self.lines[index / CACHE_LINE].0[index % CACHE_LINE]
    }

    /// Ecrit un octet dans le tampon.
    fn set_byte_at(&mut self, index: usize, value: u8) {
        self.lines[index / CACHE_LINE].0[index % CACHE_LINE] = value;
    }

    /// Ecrit des octets a l'offset donne.
    ///
    /// Renvoie `false` si l'ecriture depasserait la capacite. En production, c'est
    /// Java qui ecrit directement dans le tampon ; cette methode sert aux tests et au
    /// harnais.
    pub fn write_at(&mut self, offset: usize, data: &[u8]) -> bool {
        if offset.saturating_add(data.len()) > self.capacity() {
            return false;
        }
        for (index, byte) in data.iter().enumerate() {
            self.set_byte_at(offset + index, *byte);
        }
        true
    }

    /// Lit l'enregistrement d'indice `index`, s'il tient dans le tampon.
    #[must_use]
    pub fn record_at(&self, index: usize) -> Option<ProbeRecord> {
        let start = index.checked_mul(PROBE_RECORD_SIZE)?;
        if start + PROBE_RECORD_SIZE > self.capacity() {
            return None;
        }
        let mut bytes = [0_u8; PROBE_RECORD_SIZE];
        for (offset, byte) in bytes.iter_mut().enumerate() {
            *byte = self.byte_at(start + offset);
        }
        Some(ProbeRecord::decode(&bytes))
    }

    /// Consomme les `used` premiers octets et rend les enregistrements complets.
    ///
    /// Un `used` superieur a la capacite signale une saturation cote Java : les
    /// enregistrements qui n'ont pas tenu sont comptes comme perdus (R-709) et la
    /// lecture se limite a ce que le tampon contient reellement. Un `used` qui ne
    /// tombe pas sur un multiple de trente-deux octets laisse un enregistrement
    /// partiel, ignore.
    pub fn flush(&mut self, used: usize) -> Vec<ProbeRecord> {
        self.flushes = self.flushes.saturating_add(1);

        if used > self.capacity() {
            let overflow = used - self.capacity();
            self.records_lost = self
                .records_lost
                .saturating_add((overflow / PROBE_RECORD_SIZE) as u64);
        }

        let readable = used.min(self.capacity());
        let count = readable / PROBE_RECORD_SIZE;
        let mut records = Vec::with_capacity(count);
        for index in 0..count {
            if let Some(record) = self.record_at(index) {
                records.push(record);
            }
        }
        self.records_consumed = self.records_consumed.saturating_add(records.len() as u64);
        records
    }

    /// Nombre de flushes effectues.
    #[must_use]
    pub fn flushes(&self) -> u64 {
        self.flushes
    }

    /// Enregistrements consommes depuis la creation.
    #[must_use]
    pub fn records_consumed(&self) -> u64 {
        self.records_consumed
    }

    /// Enregistrements perdus par saturation (`rfx.profiler.samples_lost`, R-709).
    #[must_use]
    pub fn records_lost(&self) -> u64 {
        self.records_lost
    }
}

/// Ensemble des tampons de profilage, indexes par thread.
#[derive(Debug)]
pub struct ProbeBufferPool {
    buffers: BTreeMap<i32, ProbeBuffer>,
    bytes_per_buffer: usize,
    max_buffers: usize,
    rejected_threads: u64,
}

impl ProbeBufferPool {
    /// Construit un pool.
    ///
    /// `max_buffers` borne le nombre de threads suivis : un modpack qui creerait des
    /// threads sans fin ne doit pas faire croitre la memoire native sans fin (R-533).
    #[must_use]
    pub fn new(bytes_per_buffer: usize, max_buffers: usize) -> Self {
        Self {
            buffers: BTreeMap::new(),
            bytes_per_buffer,
            max_buffers,
            rejected_threads: 0,
        }
    }

    /// Rend le tampon d'un thread, en l'allouant au premier appel.
    ///
    /// Renvoie `None` si le pool a atteint son plafond de threads.
    pub fn acquire(&mut self, thread_id: i32) -> Option<&mut ProbeBuffer> {
        if !self.buffers.contains_key(&thread_id) {
            if self.buffers.len() >= self.max_buffers {
                self.rejected_threads = self.rejected_threads.saturating_add(1);
                return None;
            }
            self.buffers
                .insert(thread_id, ProbeBuffer::new(self.bytes_per_buffer));
        }
        self.buffers.get_mut(&thread_id)
    }

    /// Tampon d'un thread, s'il a deja ete alloue.
    pub fn get_mut(&mut self, thread_id: i32) -> Option<&mut ProbeBuffer> {
        self.buffers.get_mut(&thread_id)
    }

    /// Nombre de tampons alloues.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    /// Indique si aucun tampon n'est alloue.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }

    /// Octets totaux occupes par les tampons.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.buffers.values().map(|b| b.capacity() as u64).sum()
    }

    /// Threads refuses faute de place.
    #[must_use]
    pub fn rejected_threads(&self) -> u64 {
        self.rejected_threads
    }

    /// Enregistrements perdus, tous tampons confondus.
    #[must_use]
    pub fn records_lost(&self) -> u64 {
        self.buffers.values().map(ProbeBuffer::records_lost).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(probe_id: u32, kind: RecordKind, value: u64) -> ProbeRecord {
        ProbeRecord {
            probe_id,
            kind: kind.to_byte(),
            flags: 0,
            context_hash16: 0x1234,
            timestamp_ns: 42_000,
            value,
        }
    }

    #[test]
    fn a_record_round_trips_through_its_binary_form() {
        let original = record(7, RecordKind::Exit, 1_500);
        let decoded = ProbeRecord::decode(&original.encode());
        assert_eq!(original, decoded);
        assert_eq!(decoded.kind(), RecordKind::Exit);
    }

    #[test]
    fn record_layout_matches_the_specification() {
        let bytes = record(0x0102_0304, RecordKind::Alloc, 0).encode();
        assert_eq!(bytes.len(), PROBE_RECORD_SIZE);
        // probe_id en petit-boutiste sur les quatre premiers octets.
        assert_eq!(&bytes[0..4], &[0x04, 0x03, 0x02, 0x01]);
        assert_eq!(bytes[4], 2, "kind = ALLOC");
        assert_eq!(&bytes[6..8], &[0x34, 0x12], "context_hash16 petit-boutiste");
    }

    #[test]
    fn an_unknown_kind_is_preserved_not_guessed() {
        assert_eq!(RecordKind::from_byte(200), RecordKind::Unknown(200));
        assert_eq!(RecordKind::Unknown(200).to_byte(), 200);
    }

    #[test]
    fn a_buffer_is_rounded_up_to_whole_cache_lines() {
        assert_eq!(ProbeBuffer::new(1).capacity(), 64);
        assert_eq!(ProbeBuffer::new(64).capacity(), 64);
        assert_eq!(ProbeBuffer::new(65).capacity(), 128);
        assert_eq!(ProbeBuffer::new(DEFAULT_BUFFER_BYTES).capacity(), 65_536);
        assert_eq!(
            ProbeBuffer::new(DEFAULT_BUFFER_BYTES).capacity_records(),
            2_048
        );
    }

    #[test]
    fn a_buffer_address_is_aligned_on_a_cache_line() {
        let mut buffer = ProbeBuffer::new(DEFAULT_BUFFER_BYTES);
        assert_eq!(buffer.address() as usize % CACHE_LINE, 0);
    }

    #[test]
    fn written_records_are_read_back_in_order() {
        let mut buffer = ProbeBuffer::new(1024);
        for index in 0..4_u32 {
            let written = buffer.write_at(
                index as usize * PROBE_RECORD_SIZE,
                &record(index, RecordKind::Enter, u64::from(index) * 10).encode(),
            );
            assert!(written);
        }

        let records = buffer.flush(4 * PROBE_RECORD_SIZE);

        assert_eq!(records.len(), 4);
        for (index, record) in records.iter().enumerate() {
            assert_eq!(record.probe_id, index as u32);
            assert_eq!(record.value, index as u64 * 10);
            assert_eq!(record.kind(), RecordKind::Enter);
        }
        assert_eq!(buffer.records_consumed(), 4);
        assert_eq!(buffer.records_lost(), 0);
        assert_eq!(buffer.flushes(), 1);
    }

    #[test]
    fn a_partial_record_is_ignored() {
        let mut buffer = ProbeBuffer::new(1024);
        buffer.write_at(0, &record(1, RecordKind::Enter, 5).encode());

        // Un enregistrement complet plus seize octets : le fragment est ignore.
        let records = buffer.flush(PROBE_RECORD_SIZE + 16);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].probe_id, 1);
    }

    /// R-709 : la saturation perd les enregistrements excedentaires et les compte.
    #[test]
    fn saturation_loses_records_and_counts_them() {
        let mut buffer = ProbeBuffer::new(64); // deux enregistrements
        assert_eq!(buffer.capacity_records(), 2);

        // Java annonce avoir voulu ecrire dix enregistrements : huit n'ont pas tenu.
        let records = buffer.flush(10 * PROBE_RECORD_SIZE);

        assert_eq!(records.len(), 2, "seule la capacite reelle est lue");
        assert_eq!(buffer.records_lost(), 8);
    }

    #[test]
    fn a_write_beyond_capacity_is_refused() {
        let mut buffer = ProbeBuffer::new(64);
        assert!(buffer.write_at(0, &[0_u8; 64]));
        assert!(!buffer.write_at(1, &[0_u8; 64]), "depassement refuse");
        assert!(
            !buffer.write_at(usize::MAX, &[0_u8; 1]),
            "offset aberrant refuse"
        );
    }

    #[test]
    fn an_empty_flush_yields_nothing() {
        let mut buffer = ProbeBuffer::new(1024);
        assert!(buffer.flush(0).is_empty());
        assert_eq!(buffer.flushes(), 1, "un flush vide reste un flush");
        assert_eq!(buffer.records_lost(), 0);
    }

    #[test]
    fn the_pool_allocates_one_buffer_per_thread() {
        let mut pool = ProbeBufferPool::new(1024, 4);
        assert!(pool.is_empty());

        assert!(pool.acquire(1).is_some());
        assert!(pool.acquire(2).is_some());
        assert_eq!(pool.len(), 2);
        assert_eq!(pool.total_bytes(), 2048);

        // Le second appel pour un thread connu rend le meme tampon.
        pool.acquire(1)
            .expect("tampon existant")
            .write_at(0, &[1, 2, 3]);
        assert_eq!(pool.len(), 2, "aucun tampon supplementaire");
    }

    /// R-533 : le nombre de threads suivis est borne.
    #[test]
    fn the_pool_refuses_threads_beyond_its_limit() {
        let mut pool = ProbeBufferPool::new(64, 2);
        assert!(pool.acquire(1).is_some());
        assert!(pool.acquire(2).is_some());

        assert!(pool.acquire(3).is_none(), "au-dela du plafond");
        assert_eq!(pool.rejected_threads(), 1);
        assert_eq!(pool.len(), 2);

        // Un thread deja connu reste servi malgre le plafond.
        assert!(pool.acquire(1).is_some());
    }

    #[test]
    fn the_pool_totals_lost_records() {
        let mut pool = ProbeBufferPool::new(64, 4);
        pool.acquire(1)
            .expect("tampon")
            .flush(10 * PROBE_RECORD_SIZE);
        pool.acquire(2)
            .expect("tampon")
            .flush(5 * PROBE_RECORD_SIZE);
        assert_eq!(pool.records_lost(), 8 + 3);
    }
}
