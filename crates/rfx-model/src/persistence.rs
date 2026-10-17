//! Blobs de la base des unites de travail (C-09, C-48) echanges avec Java.
//!
//! Cahier des charges : PARTIE 5.9, DM-05 (empreinte), PARTIE 4.18 (emplacement).
//! Decision : ADR-049. Maturite : `STABLE`.

use serde::{Deserialize, Serialize};

/// Ouverture de la base : ce que seul Java connait de l'environnement.
///
/// Les chaines sont hachees cote natif : Java n'a aucune regle de hachage a tenir en
/// accord avec Rust. La classe de materiel et la configuration, le natif les connait
/// deja.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbOpen {
    /// Version du schema du blob. Doit valoir [`MODEL_SCHEMA_VERSION`](crate::MODEL_SCHEMA_VERSION).
    pub schema: u32,
    /// Repertoire de la base, `<gameDir>/rustforgex/cache` (PARTIE 4.18).
    pub dir: String,
    /// Version de Minecraft.
    pub minecraft: String,
    /// Version de Forge.
    pub forge: String,
    /// Fournisseur et version de la JVM.
    pub java: String,
    /// Un element par mod : `modid@version@debut du sha256 du jar` (DM-05). L'ordre est
    /// indifferent : le natif trie avant de hacher.
    pub mods: Vec<String>,
}

/// Etat de la base, publie dans le statut (`rfx.db.*`).
///
/// Tous les champs sont nuls tant que la base n'est pas ouverte (R-660).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbStatus {
    /// `cache.enabled`.
    pub enabled: bool,
    /// `true` une fois la base ouverte.
    pub open: bool,
    /// Ouvertures echouees, faute de pouvoir creer le fil d'ecriture : la base reste
    /// fermee, le jeu tourne normalement.
    pub open_failures: u64,
    /// Issue de la relecture : 0 en cours, 1 aucun fichier, 2 autre environnement,
    /// 3 fichier mis de cote, 4 relu, 5 lecture refusee par le systeme.
    pub load: u8,
    /// Unites de travail relues du fichier.
    pub loaded_entries: u64,
    /// Entrees ecartees a la relecture (`rfx.db.load_rejected_entries`).
    pub rejected_entries: u64,
    /// Unites de travail dans la base (`rfx.db.entries`).
    pub entries: u64,
    /// Memoire estimee de la base, en octets.
    pub estimated_bytes: u64,
    /// Unites de travail evincees (`rfx.db.evictions`).
    pub evictions: u64,
    /// Lots confies au fil d'ecriture.
    pub syncs: u64,
    /// Lots rendus, file pleine : repris au cycle suivant.
    pub handed_back: u64,
    /// Unites de travail en attente d'etre confiees.
    pub pending: u64,
    /// Unites de travail decouvertes avant l'ouverture et perdues, faute de place.
    pub pending_dropped: u64,
    /// Fichiers ecrits.
    pub writes: u64,
    /// Echecs d'ecriture (le premier desactive l'ecriture, R-692).
    pub write_failures: u64,
    /// Duree de la derniere ecriture, en ms (`rfx.db.persist_ms`).
    pub persist_ms: u64,
    /// `true` si l'ecriture est desactivee apres un echec.
    pub disabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_blob_round_trips() {
        let open = DbOpen {
            schema: crate::MODEL_SCHEMA_VERSION,
            dir: "C:/jeu/rustforgex/cache".to_owned(),
            minecraft: "1.20.1".to_owned(),
            forge: "47.4.0".to_owned(),
            java: "Eclipse Adoptium 17.0.12".to_owned(),
            mods: vec!["create@0.5.1@0123456789abcdef".to_owned()],
        };
        let bytes = crate::to_cbor(&open).unwrap();
        assert_eq!(crate::from_cbor::<DbOpen>(&bytes).unwrap(), open);
    }

    #[test]
    fn the_status_is_readable_by_java() {
        let status = DbStatus {
            enabled: true,
            open: true,
            load: 4,
            entries: 12,
            ..DbStatus::default()
        };
        crate::assert_java_readable(&crate::to_cbor(&status).unwrap()).unwrap();
    }
}
