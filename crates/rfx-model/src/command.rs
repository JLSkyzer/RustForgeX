//! DM-16 : contenu des command buffers, et regions ecrites (C-21, C-23).
//!
//! Cahier des charges : PARTIE 4.16, PARTIE 5.21, PARTIE 5.23. Decision : ADR-073.
//! Maturite : `STABLE` pour le modele ; aucun applicateur n'existe encore (C-22).
//!
//! Une commande decrit un effet sur le jeu sans le produire : seul un applicateur Java,
//! sur le fil autoritatif, le produira (R-220, R-222). Les types que le cahier nomme sans
//! les decrire prennent la forme la plus simple qui les rende applicables et
//! comparables ; un buffer ne se persiste pas.

use crate::classification::Domain;

/// Dimension, par son indice dans la table des dimensions de la partie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DimId(pub u16);

/// Position d'un bloc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockPos {
    /// Abscisse.
    pub x: i32,
    /// Hauteur.
    pub y: i32,
    /// Profondeur.
    pub z: i32,
}

impl BlockPos {
    /// Coordonnees du chunk qui contient le bloc.
    #[must_use]
    pub fn chunk(self) -> (i32, i32) {
        (self.x >> 4, self.z >> 4)
    }
}

/// Etat de bloc, par son identifiant dans le registre de la partie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockStateId(pub u32);

/// Entite : sa dimension et son identifiant dans la partie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityKey {
    /// Dimension.
    pub dim: DimId,
    /// Identifiant de l'entite, stable pendant sa vie.
    pub id: i32,
}

/// Type d'entite, par son identifiant dans le registre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityProtoId(pub u32);

/// Vecteur de reels doubles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3d {
    /// Abscisse.
    pub x: f64,
    /// Hauteur.
    pub y: f64,
    /// Profondeur.
    pub z: f64,
}

/// Plage d'octets dans la zone de donnees du buffer qui porte la commande.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlobRef {
    /// Debut, en octets.
    pub offset: u32,
    /// Longueur, en octets.
    pub len: u32,
}

/// Modification d'une entite, encodee pour son applicateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityPatch(pub BlobRef);

/// Destinataire d'un paquet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PacketTarget {
    /// Un joueur, par son entite.
    Player(EntityKey),
    /// Tous les joueurs d'une dimension.
    Dimension(DimId),
    /// Tous les joueurs.
    All,
}

/// Rappel Java enregistre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JavaCallbackId(pub u32);

/// Commande du SDK (R-221).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CustomCommandId(pub u32);

/// DM-16 : un effet a produire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// Pose un etat de bloc.
    SetBlock {
        /// Dimension.
        dim: DimId,
        /// Position.
        pos: BlockPos,
        /// Etat pose.
        state: BlockStateId,
        /// Drapeaux de `Level.setBlock`.
        flags: u8,
    },
    /// Modifie une entite.
    UpdateEntity {
        /// Entite.
        entity: EntityKey,
        /// Modification.
        patch: EntityPatch,
    },
    /// Fait apparaitre une entite.
    SpawnEntity {
        /// Dimension.
        dim: DimId,
        /// Type.
        proto: EntityProtoId,
        /// Position.
        pos: Vec3d,
        /// Donnees initiales.
        nbt: Option<BlobRef>,
    },
    /// Retire une entite.
    RemoveEntity {
        /// Entite.
        entity: EntityKey,
        /// Motif.
        reason: u8,
    },
    /// Envoie un paquet.
    SendPacket {
        /// Destinataire.
        target: PacketTarget,
        /// Contenu.
        payload: BlobRef,
    },
    /// Programme une mise a jour de bloc.
    ScheduleTick {
        /// Dimension.
        dim: DimId,
        /// Position.
        pos: BlockPos,
        /// Delai, en ticks.
        delay: u32,
        /// Priorite.
        priority: i32,
    },
    /// Marque un chunk a sauvegarder.
    MarkChunkDirty {
        /// Dimension.
        dim: DimId,
        /// Chunk, abscisse.
        cx: i32,
        /// Chunk, profondeur.
        cz: i32,
    },
    /// Appelle un rappel Java.
    CallbackJava {
        /// Rappel.
        handle: JavaCallbackId,
        /// Argument.
        arg: BlobRef,
    },
    /// Commande d'un module du SDK.
    Custom {
        /// Identifiant.
        id: CustomCommandId,
        /// Contenu.
        payload: BlobRef,
    },
}

impl Command {
    /// Region que la commande ecrit (PARTIE 5.23). Un rappel Java ou une commande du SDK
    /// peut ecrire n'importe quoi : `Any`, au sens prudent (R-480).
    #[must_use]
    pub fn region(&self) -> Region {
        match *self {
            Self::SetBlock { dim, pos, .. } | Self::ScheduleTick { dim, pos, .. } => {
                let (cx, cz) = pos.chunk();
                Region::Chunk { dim, cx, cz }
            }
            Self::MarkChunkDirty { dim, cx, cz } => Region::Chunk { dim, cx, cz },
            // Une apparition change l'ensemble des entites : tout lecteur qui a parcouru
            // des entites peut en dependre, ou qu'elles soient. Le domaine entier, au sens
            // prudent (R-480).
            Self::SpawnEntity { .. } => Region::Domain(Domain::EntityState),
            Self::UpdateEntity { entity, .. } | Self::RemoveEntity { entity, .. } => {
                Region::Entity(entity)
            }
            Self::SendPacket { .. } => Region::Domain(Domain::Network),
            Self::CallbackJava { .. } | Self::Custom { .. } => Region::Any,
        }
    }
}

/// Granularite du versionnement (PARTIE 5.23) : le chunk pour les blocs, l'entite pour
/// les entites, le domaine entier pour le reste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Region {
    /// Un chunk d'une dimension.
    Chunk {
        /// Dimension.
        dim: DimId,
        /// Abscisse.
        cx: i32,
        /// Profondeur.
        cz: i32,
    },
    /// Une entite.
    Entity(EntityKey),
    /// Un domaine entier.
    Domain(Domain),
    /// Tout : en conflit avec toute region (R-480).
    Any,
}

impl Region {
    /// Domaine qui contient la region ; `None` pour `Any`.
    #[must_use]
    pub fn domain(self) -> Option<Domain> {
        match self {
            Self::Chunk { .. } => Some(Domain::WorldBlocks),
            Self::Entity(_) => Some(Domain::EntityState),
            Self::Domain(domain) => Some(domain),
            Self::Any => None,
        }
    }

    /// `true` si les deux regions peuvent designer la meme memoire. Un domaine recouvre
    /// toutes ses regions ; `Any` et `Domain::Unknown` recouvrent tout.
    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        let wide = |r: Self| matches!(r, Self::Any | Self::Domain(Domain::Unknown));
        if wide(self) || wide(other) || self == other {
            return true;
        }
        match (self, other) {
            (Self::Domain(d), r) | (r, Self::Domain(d)) => r.domain() == Some(d),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OVERWORLD: DimId = DimId(0);

    #[test]
    fn a_block_write_is_versioned_by_its_chunk() {
        let set = Command::SetBlock {
            dim: OVERWORLD,
            pos: BlockPos {
                x: -1,
                y: 64,
                z: 33,
            },
            state: BlockStateId(1),
            flags: 3,
        };
        assert_eq!(
            set.region(),
            Region::Chunk {
                dim: OVERWORLD,
                cx: -1,
                cz: 2
            }
        );
    }

    #[test]
    fn unknown_effects_are_regions_that_cover_everything() {
        let callback = Command::CallbackJava {
            handle: JavaCallbackId(1),
            arg: BlobRef { offset: 0, len: 0 },
        };
        assert_eq!(callback.region(), Region::Any);
        let spawn = Command::SpawnEntity {
            dim: OVERWORLD,
            proto: EntityProtoId(1),
            pos: Vec3d {
                x: 0.5,
                y: 64.0,
                z: 0.5,
            },
            nbt: None,
        };
        assert_eq!(spawn.region(), Region::Domain(Domain::EntityState));
        assert!(spawn.region().overlaps(Region::Entity(EntityKey {
            dim: OVERWORLD,
            id: 9
        })));
    }

    #[test]
    fn overlaps_follow_the_granularity() {
        let chunk = |cx| Region::Chunk {
            dim: OVERWORLD,
            cx,
            cz: 0,
        };
        let entity = |id| Region::Entity(EntityKey { dim: OVERWORLD, id });
        assert!(chunk(0).overlaps(chunk(0)));
        assert!(!chunk(0).overlaps(chunk(1)));
        assert!(!chunk(0).overlaps(entity(1)));
        assert!(entity(1).overlaps(Region::Domain(Domain::EntityState)));
        assert!(!entity(1).overlaps(Region::Domain(Domain::WorldBlocks)));
        assert!(chunk(5).overlaps(Region::Domain(Domain::WorldBlocks)));
        assert!(chunk(5).overlaps(Region::Any));
        assert!(entity(2).overlaps(Region::Domain(Domain::Unknown)));
        assert!(!Region::Domain(Domain::Network).overlaps(Region::Domain(Domain::Random)));
    }
}
