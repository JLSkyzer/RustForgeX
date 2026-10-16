//! C-09 : format du fichier `workload.db`.
//!
//! Cahier des charges : PARTIE 4.17 (DM-17, R-223), R-360. Tests : T-180, T-182.
//! Decision : ADR-048. Maturite : `STABLE`.
//!
//! ```text
//! magic           8 octets   "RFX-WDB\0"
//! schema_version  u32 LE     SCHEMA_VERSION
//! fp_len          u32 LE     longueur de l'empreinte encodee
//! fingerprint     fp_len     empreinte d'environnement, CBOR
//! payload_len     u64 LE
//! checksum        u64 LE     xxh3-64 du contenu
//! payload         payload_len  enregistrements, CBOR
//! ```
//!
//! L'en-tete permet de rejeter un fichier sans decoder son contenu (R-223, R-360) ; la
//! somme de controle detecte un contenu altere, meme d'un seul octet (T-182). Un fichier
//! illisible n'est jamais lu en partie : c'est tout ou rien.

use rfx_model::Fingerprint;
use xxhash_rust::xxh3::xxh3_64;

use crate::record::WorkloadRecord;

/// Signature d'un fichier `workload.db`.
pub const MAGIC: [u8; 8] = *b"RFX-WDB\0";

/// Version du schema des enregistrements. Changee a chaque ajout de champ (ADR-048).
pub const SCHEMA_VERSION: u32 = 1;

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
/// # Errors
///
/// Le message de l'encodeur CBOR, s'il echoue.
pub fn encode(fingerprint: &Fingerprint, records: &[WorkloadRecord]) -> Result<Vec<u8>, String> {
    let fp = rfx_model::to_cbor(&fingerprint.environment()).map_err(|e| e.to_string())?;
    let payload = rfx_model::to_cbor(&records).map_err(|e| e.to_string())?;
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
    let payload_len = usize::try_from(cursor.u64()?).map_err(|_| DecodeError::Truncated)?;
    let checksum = cursor.u64()?;
    let payload = cursor.take(payload_len)?;
    if xxh3_64(payload) != checksum {
        return Err(DecodeError::ChecksumMismatch);
    }
    let records =
        rfx_model::from_cbor(payload).map_err(|e| DecodeError::Malformed(e.to_string()))?;
    Ok(Decoded {
        fingerprint,
        records,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{HistoryEvent, HistoryKind};
    use rfx_model::{Heat, WorkDescriptor};

    fn records() -> Vec<WorkloadRecord> {
        (0..50)
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

    /// T-180 : ce qui est ecrit se relit a l'identique.
    #[test]
    fn t180_a_file_round_trips() {
        let written = records();
        let bytes = encode(&environment(), &written).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.records, written);
        assert_eq!(
            decoded.fingerprint,
            environment().environment(),
            "en-tete : environnement"
        );
        assert_eq!(decode_header(&bytes).unwrap(), environment().environment());
    }

    /// T-182 : un seul octet altere n'importe ou dans le contenu est detecte.
    #[test]
    fn t182_any_altered_byte_is_detected() {
        let bytes = encode(&environment(), &records()).unwrap();
        let payload_start = bytes.len() - rfx_model::to_cbor(&records()).unwrap().len();
        for at in (payload_start..bytes.len()).step_by(97) {
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
        let bytes = encode(&environment(), &records()).unwrap();
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
}
