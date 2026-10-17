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

use std::thread;

use rfx_model::Fingerprint;
use xxhash_rust::xxh3::xxh3_64;

use crate::record::WorkloadRecord;

/// Signature d'un fichier `workload.db`.
pub const MAGIC: [u8; 8] = *b"RFX-WDB\0";

/// Version du schema des enregistrements. Changee a chaque ajout de champ (ADR-048).
pub const SCHEMA_VERSION: u32 = 1;

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
    if schema != SCHEMA_VERSION {
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
    let mut cursor = Cursor { bytes, at: 0 };
    let fingerprint = read_header(&mut cursor)?;
    let payload_len = cursor.len()?;
    let checksum = cursor.u64()?;
    let payload = cursor.take(payload_len)?;
    if xxh3_64(payload) != checksum {
        return Err(DecodeError::ChecksumMismatch);
    }
    let chunks = split_chunks(payload)?;
    let records = decode_chunks(&chunks)?;
    Ok(Decoded {
        fingerprint,
        records,
    })
}

/// Decoupe le contenu en blocs, d'apres sa table. Le contenu doit se terminer
/// exactement avec le dernier bloc.
fn split_chunks(payload: &[u8]) -> Result<Vec<&[u8]>, DecodeError> {
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
        .map(|len| cursor.take(len))
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

/// Decode les blocs sur plusieurs fils, et rend les enregistrements dans l'ordre.
fn decode_chunks(chunks: &[&[u8]]) -> Result<Vec<WorkloadRecord>, DecodeError> {
    let threads = thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(MAX_DECODE_THREADS)
        .min(chunks.len());
    let decoded: Vec<Result<Vec<WorkloadRecord>, DecodeError>> = if threads <= 1 {
        chunks.iter().map(|chunk| decode_chunk(chunk)).collect()
    } else {
        decode_in_parallel(chunks, threads)
    };
    let mut records = Vec::new();
    for chunk in decoded {
        records.extend(chunk?);
    }
    Ok(records)
}

/// Le fil `t` decode les blocs `t`, `t + threads`, `t + 2 * threads`... ; ce que le fil
/// appelant n'a pas pu confier, il le decode lui-meme.
fn decode_in_parallel(
    chunks: &[&[u8]],
    threads: usize,
) -> Vec<Result<Vec<WorkloadRecord>, DecodeError>> {
    let mut results: Vec<Option<Result<Vec<WorkloadRecord>, DecodeError>>> =
        (0..chunks.len()).map(|_| None).collect();
    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for first in 0..threads {
            let spawned = thread::Builder::new()
                .name("RFX-DB-Load".to_owned())
                .spawn_scoped(scope, move || {
                    (first..chunks.len())
                        .step_by(threads)
                        .map(|index| (index, decode_chunk(chunks[index])))
                        .collect::<Vec<_>>()
                });
            match spawned {
                Ok(handle) => handles.push(handle),
                Err(_) => {
                    for index in (first..chunks.len()).step_by(threads) {
                        results[index] = Some(decode_chunk(chunks[index]));
                    }
                }
            }
        }
        for handle in handles {
            // Une panic du decodeur est confinee a son fil : son resultat manque, et le
            // fichier est refuse ci-dessous.
            if let Ok(done) = handle.join() {
                for (index, result) in done {
                    results[index] = Some(result);
                }
            }
        }
    });
    results
        .into_iter()
        .map(|result| {
            result.unwrap_or_else(|| {
                Err(DecodeError::Malformed(
                    "un fil de decodage a echoue".to_owned(),
                ))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// Le decodage en serie et en parallele rendent la meme chose.
    #[test]
    fn parallel_and_serial_decoding_agree() {
        let written = records(5 * CHUNK_RECORDS as u64 + 3);
        let bytes = encode(&environment(), &written).unwrap();
        let start = payload_start(&bytes);
        let chunks = split_chunks(&bytes[start..]).unwrap();
        assert_eq!(chunks.len(), 6);
        for threads in [2, 3, 8] {
            let parallel: Vec<WorkloadRecord> = decode_in_parallel(&chunks, threads)
                .into_iter()
                .flat_map(Result::unwrap)
                .collect();
            assert_eq!(parallel, written, "{threads} fils");
        }
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
