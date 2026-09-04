//! Modele de donnees canonique de RUSTFORGE-X.
//!
//! Composant : aucun (crate transverse). Modeles : DM-14 (`HardwareClass`).
//! Cahier des charges : PARTIE 4.
//!
//! Toutes les structures qui traversent la frontiere FFI sont definies ici, une seule
//! fois, et serialisees en CBOR avec un numero de schema explicite (R-704, R-708).
//! Ce crate est la couche la plus basse du systeme : il ne depend d'aucun autre crate
//! `rfx-*` (INV-13).
//!
//! Maturite : `STABLE` pour les modeles implementes ci-dessous. Les modeles DM-01 a
//! DM-13 et DM-15 a DM-17 seront ajoutes aux jalons qui les mobilisent.

#![doc(html_root_url = "https://example.invalid/rustforgex")]

pub mod config;
pub mod hardware;
pub mod maturity;
pub mod status;

pub use config::{RuntimeConfig, RuntimeMode};
pub use hardware::{CoreTopology, HardwareClass, SimdCaps};
pub use maturity::Maturity;
pub use status::{ComponentStatus, ProbeCoverage, RuntimeStatus};

/// Version du schema de serialisation des blobs CBOR echanges avec Java.
///
/// Toute modification incompatible d'une structure serialisee incremente cette
/// valeur. Le cote Java refuse un blob dont le schema est inconnu.
pub const MODEL_SCHEMA_VERSION: u32 = 1;

/// Erreur de serialisation ou de deserialisation d'un blob CBOR.
#[derive(Debug)]
pub enum CodecError {
    /// Le blob n'a pas pu etre decode : octets tronques, types incompatibles.
    Decode(String),
    /// La valeur n'a pas pu etre encodee, ou le tampon de destination est trop petit.
    Encode(String),
    /// Le blob porte un numero de schema que cette version ne sait pas lire.
    UnsupportedSchema {
        /// Numero de schema lu dans le blob.
        found: u32,
        /// Numero de schema attendu par cette version du modele.
        expected: u32,
    },
}

impl core::fmt::Display for CodecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Decode(m) => write!(f, "decodage CBOR impossible : {m}"),
            Self::Encode(m) => write!(f, "encodage CBOR impossible : {m}"),
            Self::UnsupportedSchema { found, expected } => {
                write!(
                    f,
                    "schema de modele {found} non supporte (attendu {expected})"
                )
            }
        }
    }
}

impl std::error::Error for CodecError {}

/// Encode une valeur en CBOR.
///
/// # Erreurs
///
/// Renvoie [`CodecError::Encode`] si la valeur ne peut pas etre serialisee.
pub fn to_cbor<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    ciborium::into_writer(value, &mut out).map_err(|e| CodecError::Encode(e.to_string()))?;
    Ok(out)
}

/// Decode une valeur depuis un blob CBOR.
///
/// # Erreurs
///
/// Renvoie [`CodecError::Decode`] si le blob est tronque ou incompatible.
pub fn from_cbor<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    ciborium::from_reader(bytes).map_err(|e| CodecError::Decode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cbor_roundtrip_preserve_la_configuration() {
        let cfg = RuntimeConfig::default();
        let bytes = to_cbor(&cfg).expect("encodage");
        let back: RuntimeConfig = from_cbor(&bytes).expect("decodage");
        assert_eq!(cfg, back);
    }

    #[test]
    fn decodage_d_un_blob_tronque_echoue_sans_paniquer() {
        let bytes = to_cbor(&RuntimeConfig::default()).expect("encodage");
        let tronque = &bytes[..bytes.len() / 2];
        assert!(from_cbor::<RuntimeConfig>(tronque).is_err());
    }
}
