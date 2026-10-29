//! C-09 : format du fichier `workload.db`.
//!
//! Cahier des charges : PARTIE 4.17 (DM-17, R-223), R-360, acceptance de la PARTIE 5.9
//! (chargement en moins de 200 ms). Tests : T-180, T-182. Decision : ADR-048.
//! Maturite : `STABLE`.
//!
//! ```text
//! magic           8 octets     "RFX-WDB\0"
//! schema_version  u32 LE       SCHEMA_VERSION
//! fp_len          u32 LE       longueur de l'empreinte encodee
//! fingerprint     fp_len       empreinte d'environnement, CBOR
//! payload_len     u64 LE
//! checksum        u64 LE       xxh3-64 du contenu
//! payload         payload_len  contenu :
//!   chunk_count   u32 LE
//!   chunk_len     u64 LE       une par bloc
//!   chunks                     blocs, chacun un tableau CBOR d'au plus CHUNK_RECORDS
//!                              enregistrements, dans l'ordre
//! ```
//!
//! L'en-tete permet de rejeter un fichier sans decoder son contenu (R-223, R-360) ; la
//! somme de controle detecte un contenu altere, meme d'un seul octet (T-182). Un fichier
//! illisible n'est jamais lu en partie : c'est tout ou rien.
//!
//! # Des blocs, pour decoder en parallele
//!
//! Le decodage CBOR fait l'essentiel du temps de chargement : en serie, 20 000 unites au
//! pire demandaient plus que les 200 ms de l'acceptance. Les blocs se decodent chacun sur
//! un fil, au demarrage, hors tick. Si un fil ne peut pas etre cree, le decodage se fait
//! en serie : plus lent, jamais faux.

use std::ops::Range;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use rfx_model::Fingerprint;
use xxhash_rust::xxh3::xxh3_64;

use crate::exec::Fork;
use crate::record::WorkloadRecord;

/// Signature d'un fichier `workload.db`.
pub const MAGIC: [u8; 8] = *b"RFX-WDB\0";

/// Version du schema des enregistrements. Changee a chaque ajout de champ (ADR-048).
/// Schema 2 : faits d'analyse (ADR-064).
pub const SCHEMA_VERSION: u32 = 2;

/// Plus ancien schema encore lu. Un schema 1 est connu : ses enregistrements n'ont pas
/// d'analyse, champ absent, et se lisent tels quels (ADR-064). Plus ancien ou plus
/// recent, le fichier est mis de cote (R-223).
pub const MIN_SCHEMA_VERSION: u32 = 1;

/// Enregistrements par bloc.
pub const CHUNK_RECORDS: usize = 1_024;

/// Fils de decodage au plus : au-dela, le gain ne paie plus les fils d'un jeu qui
/// demarre et d'une machine qui fait autre chose.
const MAX_DECODE_THREADS: usize = 8;

/// Pourquoi un fichier ne peut pas etre lu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Ce n'est pas un fichier `workload.db`.
    BadMagic,
    /// Schema inconnu de cette version : le fichier est mis de cote (R-223).
    UnknownSchema(u32),
    /// Le fichier s'arrete avant la fin annoncee.
    Truncated,
    /// Le contenu ne correspond pas a sa somme de controle.
    ChecksumMismatch,
    /// L'en-tete ou le contenu ne se decode pas.
    Malformed(String),
}

/// Ce qu'un fichier contient.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    /// Empreinte d'environnement ecrite dans l'en-tete.
    pub fingerprint: Fingerprint,
    /// Enregistrements.
    pub records: Vec<WorkloadRecord>,
}

/// Encode un fichier complet. Seule la partie « environnement » de l'empreinte est
/// ecrite dans l'en-tete.
///
/// Les enregistrements sont pris par reference : le fil d'ecriture encode la base qu'il
/// detient sans la copier.
///
/// # Errors
///
/// Le message de l'encodeur CBOR, s'il echoue.
pub fn encode<'a>(
    fingerprint: &Fingerprint,
    records: impl IntoIterator<Item = &'a WorkloadRecord>,
) -> Result<Vec<u8>, String> {
    let fp = rfx_model::to_cbor(&fingerprint.environment()).map_err(|e| e.to_string())?;
    let mut chunks = Vec::new();
    let mut chunk: Vec<&WorkloadRecord> = Vec::with_capacity(CHUNK_RECORDS);
    for record in records {
        chunk.push(record);
        if chunk.len() == CHUNK_RECORDS {
            chunks.push(rfx_model::to_cbor(&chunk).map_err(|e| e.to_string())?);
            chunk.clear();
        }
    }
    if !chunk.is_empty() {
        chunks.push(rfx_model::to_cbor(&chunk).map_err(|e| e.to_string())?);
    }
    let chunk_count = u32::try_from(chunks.len()).map_err(|e| e.to_string())?;
    let mut payload =
        Vec::with_capacity(4 + 8 * chunks.len() + chunks.iter().map(Vec::len).sum::<usize>());
    payload.extend_from_slice(&chunk_count.to_le_bytes());
    for chunk in &chunks {
        payload.extend_from_slice(&(chunk.len() as u64).to_le_bytes());
    }
    for chunk in &chunks {
        payload.extend_from_slice(chunk);
    }

    let fp_len = u32::try_from(fp.len()).map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity(32 + fp.len() + payload.len());
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
    bytes.extend_from_slice(&fp_len.to_le_bytes());
    bytes.extend_from_slice(&fp);
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&xxh3_64(&payload).to_le_bytes());
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Lecteur d'octets qui refuse de lire au-dela de la fin.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.at.checked_add(len).ok_or(DecodeError::Truncated)?;
        let slice = self.bytes.get(self.at..end).ok_or(DecodeError::Truncated)?;
        self.at = end;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        let mut raw = [0_u8; 4];
        raw.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(raw))
    }

    fn u64(&mut self) -> Result<u64, DecodeError> {
        let mut raw = [0_u8; 8];
        raw.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(raw))
    }

    fn len(&mut self) -> Result<usize, DecodeError> {
        usize::try_from(self.u64()?).map_err(|_| DecodeError::Truncated)
    }
}

/// Lit l'en-tete seul : signature, schema, empreinte.
///
/// # Errors
///
/// [`DecodeError`] si l'en-tete est invalide.
pub fn decode_header(bytes: &[u8]) -> Result<Fingerprint, DecodeError> {
    let mut cursor = Cursor { bytes, at: 0 };
    read_header(&mut cursor)
}

fn read_header(cursor: &mut Cursor<'_>) -> Result<Fingerprint, DecodeError> {
    if cursor
        .take(MAGIC.len())
        .map_err(|_| DecodeError::BadMagic)?
        != MAGIC
    {
        return Err(DecodeError::BadMagic);
    }
    let schema = cursor.u32()?;
    if !(MIN_SCHEMA_VERSION..=SCHEMA_VERSION).contains(&schema) {
        return Err(DecodeError::UnknownSchema(schema));
    }
    let fp_len = cursor.u32()? as usize;
    let fp = cursor.take(fp_len)?;
    rfx_model::from_cbor(fp).map_err(|e| DecodeError::Malformed(e.to_string()))
}

/// Decode un fichier complet.
///
/// # Errors
///
/// [`DecodeError`] si le fichier est invalide, tronque ou altere.
pub fn decode(bytes: &[u8]) -> Result<Decoded, DecodeError> {
    let (fingerprint, start, ranges) = layout(bytes)?;
    let mut records = Vec::new();
    for range in ranges {
        records.extend(decode_chunk(
            &bytes[start + range.start..start + range.end],
        )?);
    }
    Ok(Decoded {
        fingerprint,
        records,
    })
}

/// Decode un fichier complet, ses blocs en sous-travaux de `fork` (ADR-061) : en jeu,
/// des sous-taches de l'ordonnanceur, sans fil cree pour l'occasion.
///
/// # Errors
///
/// [`DecodeError`] si le fichier est invalide, tronque ou altere, ou si un sous-travail
/// n'a pas rendu son bloc.
pub fn decode_with(bytes: Arc<Vec<u8>>, fork: &dyn Fork) -> Result<Decoded, DecodeError> {
    let (fingerprint, start, ranges) = layout(&bytes)?;
    let records = decode_chunks(&bytes, start, ranges, fork)?;
    Ok(Decoded {
        fingerprint,
        records,
    })
}

/// En-tete verifie, debut du contenu et plages de ses blocs (relatives au contenu).
fn layout(bytes: &[u8]) -> Result<(Fingerprint, usize, Vec<Range<usize>>), DecodeError> {
    let mut cursor = Cursor { bytes, at: 0 };
    let fingerprint = read_header(&mut cursor)?;
    let payload_len = cursor.len()?;
    let checksum = cursor.u64()?;
    let start = cursor.at;
    let payload = cursor.take(payload_len)?;
    if xxh3_64(payload) != checksum {
        return Err(DecodeError::ChecksumMismatch);
    }
    Ok((fingerprint, start, chunk_ranges(payload)?))
}

/// Decoupe le contenu en blocs, d'apres sa table. Le contenu doit se terminer
/// exactement avec le dernier bloc.
#[cfg(test)]
fn split_chunks(payload: &[u8]) -> Result<Vec<&[u8]>, DecodeError> {
    Ok(chunk_ranges(payload)?
        .into_iter()
        .map(|range| &payload[range])
        .collect())
}

/// Plages des blocs du contenu, d'apres sa table. Le contenu doit se terminer
/// exactement avec le dernier bloc.
fn chunk_ranges(payload: &[u8]) -> Result<Vec<Range<usize>>, DecodeError> {
    let mut cursor = Cursor {
        bytes: payload,
        at: 0,
    };
    // Un compte invente ne reserve rien : la collecte d'un `Result` part d'un vecteur
    // vide, et la lecture s'arrete a la premiere longueur absente.
    let count = cursor.u32()? as usize;
    let lengths = (0..count)
        .map(|_| cursor.len())
        .collect::<Result<Vec<_>, _>>()?;
    let chunks = lengths
        .into_iter()
        .map(|len| {
            let at = cursor.at;
            cursor.take(len).map(|_| at..at + len)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if cursor.at != payload.len() {
        return Err(DecodeError::Malformed(
            "octets apres le dernier bloc".to_owned(),
        ));
    }
    Ok(chunks)
}

fn decode_chunk(chunk: &[u8]) -> Result<Vec<WorkloadRecord>, DecodeError> {
    rfx_model::from_cbor(chunk).map_err(|e| DecodeError::Malformed(e.to_string()))
}

/// Decode les blocs en `parts` sous-travaux de `fork`, et rend les enregistrements dans
/// l'ordre. Le sous-travail `p` decode les blocs `p`, `p + parts`, `p + 2 * parts`... ;
/// une panic du decodeur est confinee a son sous-travail : ses blocs manquent, et le
/// fichier est refuse.
fn decode_chunks(
    bytes: &Arc<Vec<u8>>,
    start: usize,
    ranges: Vec<Range<usize>>,
    fork: &dyn Fork,
) -> Result<Vec<WorkloadRecord>, DecodeError> {
    type Slots = Vec<Option<Result<Vec<WorkloadRecord>, DecodeError>>>;
    let parts = thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(MAX_DECODE_THREADS)
        .min(ranges.len())
        .max(1);
    let count = ranges.len();
    let ranges = Arc::new(ranges);
    let results: Arc<Mutex<Slots>> = Arc::new(Mutex::new((0..count).map(|_| None).collect()));
    for first in 0..parts {
        let (bytes, ranges, results) =
            (Arc::clone(bytes), Arc::clone(&ranges), Arc::clone(&results));
        fork.spawn(Box::new(move || {
            for index in (first..ranges.len()).step_by(parts) {
                let range = start + ranges[index].start..start + ranges[index].end;
                let decoded = catch_unwind(AssertUnwindSafe(|| decode_chunk(&bytes[range])));
                if let Ok(decoded) = decoded {
                    results.lock().unwrap_or_else(PoisonError::into_inner)[index] = Some(decoded);
                }
            }
        }));
    }
    fork.join();
    let slots = std::mem::take(&mut *results.lock().unwrap_or_else(PoisonError::into_inner));
    let mut records = Vec::new();
    for slot in slots {
        let chunk = slot.unwrap_or_else(|| {
            Err(DecodeError::Malformed(
                "un sous-travail de decodage a echoue".to_owned(),
            ))
        })?;
        records.extend(chunk);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::InlineFork;
    use crate::record::{HistoryEvent, HistoryKind};
    use rfx_model::{Heat, WorkDescriptor};

    fn records(count: u64) -> Vec<WorkloadRecord> {
        (0..count)
            .map(|n| {
                let descriptor = WorkDescriptor {
                    owner_id: format!("mod{}", n % 3),
                    class_internal_name: "net/example/Thing".to_owned(),
                    method_name: format!("method{n}"),
                    method_descriptor: "()V".to_owned(),
                    ..WorkDescriptor::default()
                };
                let fingerprint = Fingerprint {
                    bytecode_hash: n,
                    ..Fingerprint::default()
                };
                let mut record = WorkloadRecord::new(descriptor, fingerprint);
                record.dynamics.heat = Heat::Warm;
                record.dynamics.total_calls = n * 1_000;
                record.history.push(HistoryEvent {
                    tick: n,
                    kind: HistoryKind::ProbeLevelChanged { level: 2 },
                });
                record
            })
            .collect()
    }

    fn environment() -> Fingerprint {
        Fingerprint {
            schema_version: 1,
            mc_version: 0x1201,
            mod_set_hash: 99,
            bytecode_hash: 5,
            ..Fingerprint::default()
        }
    }

    /// Debut du contenu dans un fichier encode.
    fn payload_start(bytes: &[u8]) -> usize {
        let mut cursor = Cursor { bytes, at: 0 };
        read_header(&mut cursor).unwrap();
        cursor.at + 16
    }

    /// T-180 : ce qui est ecrit se relit a l'identique, dans l'ordre, quel que soit le
    /// nombre de blocs : aucun, un, un bloc plein, plusieurs dont le dernier partiel.
    #[test]
    fn t180_a_file_round_trips() {
        for count in [0, 1, CHUNK_RECORDS as u64, 3 * CHUNK_RECORDS as u64 + 17] {
            let written = records(count);
            let bytes = encode(&environment(), &written).unwrap();
            let decoded = decode(&bytes).unwrap();
            assert_eq!(decoded.records, written, "{count} enregistrements");
            assert_eq!(
                decoded.fingerprint,
                environment().environment(),
                "en-tete : environnement"
            );
        }
        let bytes = encode(&environment(), &records(5)).unwrap();
        assert_eq!(decode_header(&bytes).unwrap(), environment().environment());
    }

    /// Sous-travaux sur de vrais fils, pour eprouver le decodage concurrent.
    #[derive(Default)]
    struct ThreadFork(Mutex<Vec<thread::JoinHandle<()>>>);

    impl Fork for ThreadFork {
        fn spawn(&self, work: Box<dyn FnOnce() + Send>) {
            self.0.lock().unwrap().push(thread::spawn(work));
        }

        fn join(&self) {
            for handle in self.0.lock().unwrap().drain(..) {
                handle.join().unwrap();
            }
        }
    }

    /// Un sous-travail qui ne rend rien : le fichier est refuse, rien n'est invente.
    struct LosingFork;

    impl Fork for LosingFork {
        fn spawn(&self, _work: Box<dyn FnOnce() + Send>) {}

        fn join(&self) {}
    }

    #[test]
    fn a_missing_chunk_rejects_the_file() {
        let bytes = encode(&environment(), &records(3)).unwrap();
        assert!(matches!(
            decode_with(Arc::new(bytes), &LosingFork),
            Err(DecodeError::Malformed(_))
        ));
    }

    /// Le decodage en serie et en parallele rendent la meme chose.
    #[test]
    fn parallel_and_serial_decoding_agree() {
        let written = records(5 * CHUNK_RECORDS as u64 + 3);
        let bytes = encode(&environment(), &written).unwrap();
        let start = payload_start(&bytes);
        let chunks = split_chunks(&bytes[start..]).unwrap();
        assert_eq!(chunks.len(), 6);
        let shared = Arc::new(bytes.clone());
        let serial = decode_with(Arc::clone(&shared), &InlineFork)
            .unwrap()
            .records;
        assert_eq!(serial, written, "a la suite");
        let parallel = decode_with(shared, &ThreadFork::default()).unwrap().records;
        assert_eq!(parallel, written, "sur plusieurs fils");
        assert_eq!(decode(&bytes).unwrap().records, written);
    }

    /// T-182 : un seul octet altere n'importe ou dans le contenu est detecte : chaque
    /// octet de la table des blocs, puis un octet sur 997 des blocs eux-memes.
    #[test]
    fn t182_any_altered_byte_is_detected() {
        let bytes = encode(&environment(), &records(CHUNK_RECORDS as u64 + 50)).unwrap();
        let start = payload_start(&bytes);
        let table = start..start + 4 + 2 * 8;
        for at in table.chain((start + 20..bytes.len()).step_by(997)) {
            let mut altered = bytes.clone();
            altered[at] ^= 0x40;
            assert_eq!(
                decode(&altered),
                Err(DecodeError::ChecksumMismatch),
                "octet {at}"
            );
        }
    }

    #[test]
    fn truncated_foreign_and_future_files_are_refused() {
        let bytes = encode(&environment(), &records(50)).unwrap();
        assert_eq!(
            decode(&bytes[..bytes.len() - 1]),
            Err(DecodeError::Truncated)
        );
        assert_eq!(
            decode(b"PK\x03\x04 not a database"),
            Err(DecodeError::BadMagic)
        );
        assert_eq!(decode(b""), Err(DecodeError::BadMagic));
        let mut future = bytes.clone();
        future[8..12].copy_from_slice(&(SCHEMA_VERSION + 1).to_le_bytes());
        assert_eq!(
            decode(&future),
            Err(DecodeError::UnknownSchema(SCHEMA_VERSION + 1))
        );
    }

    /// ADR-064 : un fichier de schema 1 est connu, il se lit entier ; ses enregistrements
    /// n'ont pas d'analyse. Un schema plus ancien que le plus ancien lu est refuse.
    #[test]
    fn a_schema_1_file_is_read_whole_without_analyses() {
        let written = records(30);
        assert!(written.iter().all(|r| r.analysis.is_none()));
        // Sans analyse, le champ n'est pas ecrit : le contenu est celui qu'ecrivait le
        // schema 1, seul l'en-tete differe.
        let mut v1 = encode(&environment(), &written).unwrap();
        v1[8..12].copy_from_slice(&1_u32.to_le_bytes());

        let decoded = decode(&v1).unwrap();
        assert_eq!(decoded.records, written);
        assert!(decoded.fingerprint.same_environment(&environment()));

        let mut older = v1.clone();
        older[8..12].copy_from_slice(&(MIN_SCHEMA_VERSION - 1).to_le_bytes());
        assert_eq!(
            decode(&older),
            Err(DecodeError::UnknownSchema(MIN_SCHEMA_VERSION - 1))
        );
    }

    /// Une analyse ecrite se relit a l'identique.
    #[test]
    fn an_analysis_round_trips() {
        let mut written = records(3);
        written[1].analysis = Some(rfx_model::PersistedAnalysis {
            stamp: 42,
            bytecode_hash: Some(7),
            nodes: 12,
            resolved_calls: 9,
            unresolved_calls: 1,
            graph_truncated: false,
            reflection: false,
            recursive_root: true,
            rounds: 2,
            calls: vec!["a/B.c()V".to_owned()],
            depth: 3,
            truncated: false,
            external_locks: Some(Vec::new()),
            reads: rfx_model::AccessSet::default(),
            writes: rfx_model::AccessSet::default(),
            side_effects: rfx_model::SideEffectSet::default(),
            locks: false,
            thread: rfx_model::ThreadFacts::default(),
            determinism: rfx_model::DeterminismFacts::default(),
            may_be_dispatched: true,
        });
        let bytes = encode(&environment(), &written).unwrap();
        assert_eq!(decode(&bytes).unwrap().records, written);
    }

    /// Une table de blocs incoherente est refusee, meme quand la somme est juste : la
    /// somme protege du disque, pas d'un fichier mal forme des l'ecriture.
    #[test]
    fn an_inconsistent_chunk_table_is_refused() {
        let one = rfx_model::to_cbor(&records(1)).unwrap();
        let table = |count: u32, lengths: &[u64], tail: &[u8]| {
            let mut payload = count.to_le_bytes().to_vec();
            for len in lengths {
                payload.extend_from_slice(&len.to_le_bytes());
            }
            payload.extend_from_slice(tail);
            payload
        };
        let too_many = table(u32::MAX, &[], &[]);
        assert_eq!(split_chunks(&too_many), Err(DecodeError::Truncated));
        let too_long = table(1, &[one.len() as u64 + 1], &one);
        assert_eq!(split_chunks(&too_long), Err(DecodeError::Truncated));
        let trailing = table(1, &[one.len() as u64], &[one.as_slice(), &[0]].concat());
        assert!(matches!(
            split_chunks(&trailing),
            Err(DecodeError::Malformed(_))
        ));
        let exact = table(1, &[one.len() as u64], &one);
        assert_eq!(split_chunks(&exact).unwrap(), vec![one.as_slice()]);
    }
}
