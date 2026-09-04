//! DM-01 : `WorkId`, identifiant stable d'une unite de travail.
//!
//! Cahier des charges : PARTIE 4.1. Exigences : R-200 (reproductible entre deux
//! lancements identiques), R-201 (jamais fonde sur une adresse memoire, un
//! `identityHashCode` ou un ordre de chargement), R-202 (collision detectee).
//! Maturite : `STABLE`.
//!
//! L'algorithme `WORKID_V1` est implemente **une seule fois**, ici. Java ne calcule
//! aucun `WorkId` : il decrit l'unite de travail et le natif lui renvoie
//! l'identifiant. Deux implementations d'un meme hachage finiraient toujours par
//! diverger sur un detail d'encodage.

use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::Xxh3;

/// Cote d'execution d'une unite de travail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Side {
    /// Client uniquement.
    Client,
    /// Serveur uniquement.
    Server,
    /// Les deux cotes.
    #[default]
    Common,
}

impl Side {
    /// Libelle canonique, utilise dans le calcul du `WorkId` et dans les rapports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Client => "CLIENT",
            Self::Server => "SERVER",
            Self::Common => "COMMON",
        }
    }
}

impl core::fmt::Display for Side {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.label())
    }
}

/// Description d'une unite de travail, telle que Java la transmet.
///
/// Tous les champs sont textuels et proviennent du bytecode ou du loader : ils sont
/// donc identiques d'un lancement a l'autre pour une meme installation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WorkDescriptor {
    /// Identifiant du mod proprietaire, ou `minecraft`, `forge`, `unknown`.
    pub owner_id: String,
    /// Nom interne de la classe, par exemple `net/minecraft/world/entity/Mob`.
    pub class_internal_name: String,
    /// Nom de la methode.
    pub method_name: String,
    /// Descripteur JVM de la methode, par exemple `(Lnet/minecraft/world/level/Level;)V`.
    pub method_descriptor: String,
    /// Hachage du contexte d'appel (DM-02), ou `0` si le contexte n'est pas discriminant.
    pub call_context_hash: u64,
    /// Cote d'execution.
    pub side: Side,
}

/// DM-01 : identifiant stable d'une unite de travail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WorkId(pub u64);

impl WorkId {
    /// Calcule l'identifiant selon l'algorithme normatif `WORKID_V1`.
    ///
    /// Les composants sont concatenes en separant chaque champ par un octet `0x1f`
    /// (separateur d'unite ASCII), afin qu'aucune ambiguite ne soit possible entre
    /// deux decoupages differents des memes octets : sans separateur, les couples
    /// `("ab", "c")` et `("a", "bc")` produiraient le meme hachage.
    #[must_use]
    pub fn compute(descriptor: &WorkDescriptor) -> Self {
        const SEPARATOR: &[u8] = &[0x1f];

        let mut hasher = Xxh3::new();
        for field in [
            descriptor.owner_id.as_bytes(),
            descriptor.class_internal_name.as_bytes(),
            descriptor.method_name.as_bytes(),
            descriptor.method_descriptor.as_bytes(),
        ] {
            hasher.update(field);
            hasher.update(SEPARATOR);
        }
        hasher.update(&descriptor.call_context_hash.to_le_bytes());
        hasher.update(SEPARATOR);
        hasher.update(descriptor.side.label().as_bytes());

        Self(hasher.digest())
    }

    /// Representation hexadecimale utilisee par `/rfx why` et les rapports.
    #[must_use]
    pub fn to_hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

impl core::fmt::Display for WorkId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "0x{:016X}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> WorkDescriptor {
        WorkDescriptor {
            owner_id: "examplemod".to_owned(),
            class_internal_name: "com/example/Machine".to_owned(),
            method_name: "tickMachine".to_owned(),
            method_descriptor: "(Lnet/minecraft/world/level/Level;)V".to_owned(),
            call_context_hash: 0,
            side: Side::Server,
        }
    }

    /// R-200 : le meme descripteur produit toujours le meme identifiant.
    #[test]
    fn the_same_descriptor_always_yields_the_same_id() {
        assert_eq!(
            WorkId::compute(&descriptor()),
            WorkId::compute(&descriptor())
        );
    }

    /// R-201 : chaque composant participe reellement au hachage.
    #[test]
    fn every_component_changes_the_id() {
        let base = WorkId::compute(&descriptor());

        let mut owner = descriptor();
        owner.owner_id = "othermod".to_owned();
        assert_ne!(base, WorkId::compute(&owner));

        let mut class = descriptor();
        class.class_internal_name = "com/example/Other".to_owned();
        assert_ne!(base, WorkId::compute(&class));

        let mut method = descriptor();
        method.method_name = "tickOther".to_owned();
        assert_ne!(base, WorkId::compute(&method));

        let mut signature = descriptor();
        signature.method_descriptor = "()V".to_owned();
        assert_ne!(base, WorkId::compute(&signature));

        let mut context = descriptor();
        context.call_context_hash = 42;
        assert_ne!(base, WorkId::compute(&context));

        let mut side = descriptor();
        side.side = Side::Client;
        assert_ne!(base, WorkId::compute(&side));
    }

    /// Le separateur empeche deux decoupages differents de collisionner.
    #[test]
    fn field_boundaries_cannot_be_confused() {
        let mut left = descriptor();
        left.owner_id = "ab".to_owned();
        left.class_internal_name = "c".to_owned();

        let mut right = descriptor();
        right.owner_id = "a".to_owned();
        right.class_internal_name = "bc".to_owned();

        assert_ne!(WorkId::compute(&left), WorkId::compute(&right));
    }

    /// Valeur figee, relevee sur l'implementation de reference : `WorkId` sert de cle
    /// de cache persistante (PARTIE 14.1), toute evolution de `WORKID_V1` doit donc
    /// etre deliberee et versionnee.
    #[test]
    fn workid_v1_is_stable_across_versions() {
        assert_eq!(WorkId::compute(&descriptor()).0, 0x98d8_ef93_397f_0ecc);
    }

    #[test]
    fn hexadecimal_rendering_is_padded() {
        assert_eq!(WorkId(0x1827_A44C).to_hex(), "000000001827a44c");
        assert_eq!(WorkId(0x1827_A44C).to_string(), "0x000000001827A44C");
    }
}
