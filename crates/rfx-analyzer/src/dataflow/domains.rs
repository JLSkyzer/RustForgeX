//! Table des domaines memoire, derivee de la hierarchie des classes (PARTIE 5.11,
//! ADR-055).
//!
//! Un objet recoit les domaines des classes d'ancrage qui figurent dans sa chaine de
//! super-classes : toute sous-classe de l'entite de Minecraft est `EntityState`, toute
//! sous-classe de `BlockEntity` est `BlockEntityState`. Les ancres sont des classes du
//! jeu, de Forge ou du JDK, jamais d'un mod (INV-12).
//!
//! # Pourquoi c'est sur
//!
//! L'etiquette d'un objet ne depend que de sa classe : deux unites qui touchent le meme
//! objet lui donnent des etiquettes tirees de classes de sa chaine. Une sous-classe
//! herite des ancres de ses super-classes, si bien que l'etiquette d'une classe de
//! reference est incluse dans celle de la classe reelle de l'objet : deux etiquettes
//! non vides d'un meme objet se recoupent toujours. Une classe sans ancre, ou dont la
//! chaine passe par une classe inconnue, n'a pas d'etiquette : l'acces est alors
//! `Unknown`, en conflit avec tout (R-211).
//!
//! La hierarchie declaree ne donne que la super-classe, pas les interfaces : une
//! interface d'ancrage ne vaut donc que pour la classe nommee elle-meme (le type statique
//! d'un receveur), jamais par heritage.

use std::cell::RefCell;
use std::collections::HashMap;

use rfx_model::{Domain, Domains};

use crate::hierarchy::{ClassIndex, OBJECT};

/// Classes d'ancrage : elles et toutes leurs sous-classes portent ce domaine. Noms
/// verifies dans les classes de Minecraft 1.20.1, de Forge et du JDK 17 (ADR-055).
pub const CLASS_ANCHORS: [(&str, Domain); 21] = [
    ("net/minecraft/world/entity/Entity", Domain::EntityState),
    (
        "net/minecraft/world/entity/player/Inventory",
        Domain::EntityInventory,
    ),
    (
        "net/minecraft/world/level/block/entity/BlockEntity",
        Domain::BlockEntityState,
    ),
    (
        "net/minecraft/world/level/chunk/ChunkAccess",
        Domain::ChunkData,
    ),
    (
        "net/minecraft/world/level/chunk/LevelChunkSection",
        Domain::WorldBlocks,
    ),
    (
        "net/minecraft/world/level/chunk/PalettedContainer",
        Domain::WorldBlocks,
    ),
    (
        "net/minecraft/world/level/lighting/LevelLightEngine",
        Domain::WorldLighting,
    ),
    (
        "net/minecraft/world/level/lighting/LightEngine",
        Domain::WorldLighting,
    ),
    (
        "net/minecraft/world/level/chunk/DataLayer",
        Domain::WorldLighting,
    ),
    (
        "net/minecraft/server/level/ChunkHolder",
        Domain::ChunkStatus,
    ),
    ("net/minecraft/server/level/ChunkMap", Domain::ChunkStatus),
    (
        "net/minecraft/server/level/DistanceManager",
        Domain::ChunkStatus,
    ),
    ("net/minecraft/world/level/Level", Domain::LevelGlobals),
    ("net/minecraft/core/MappedRegistry", Domain::Registries),
    (
        "net/minecraftforge/common/capabilities/CapabilityDispatcher",
        Domain::Capabilities,
    ),
    ("java/lang/ThreadLocal", Domain::ThreadLocals),
    ("java/util/Random", Domain::Random),
    ("net/minecraft/network/Connection", Domain::Network),
    ("java/io/OutputStream", Domain::FileSystem),
    ("java/io/InputStream", Domain::FileSystem),
    ("java/nio/channels/FileChannel", Domain::FileSystem),
];

/// Interfaces d'ancrage : elles ne valent que pour la classe nommee elle-meme.
pub const INTERFACE_ANCHORS: [(&str, Domain); 5] = [
    ("net/minecraft/core/Registry", Domain::Registries),
    ("net/minecraft/util/RandomSource", Domain::Random),
    ("java/util/random/RandomGenerator", Domain::Random),
    (
        "net/minecraft/world/level/storage/LevelData",
        Domain::LevelGlobals,
    ),
    ("io/netty/channel/Channel", Domain::Network),
];

/// Profondeur au-dela de laquelle une chaine de super-classes est tenue pour
/// incoherente (cycle dans une hierarchie mal formee).
const MAX_DEPTH: usize = 256;

/// Domaines par classe, calcules a la demande et memorises.
///
/// Seuls les resultats complets sont memorises : une classe encore inconnue peut etre
/// declaree plus tard et completer une chaine. Une table sert pour une hierarchie ; si
/// des classes deja vues sont redeclarees autrement, il faut une table neuve.
#[derive(Debug, Default)]
pub struct DomainTable {
    memo: RefCell<HashMap<String, Domains>>,
}

impl DomainTable {
    /// Table vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Domaines des objets de `class`, ou `None` si la classe n'en a aucun ou si sa
    /// chaine passe par une classe inconnue : l'objet est alors d'un domaine inconnu.
    #[must_use]
    pub fn of_class(&self, index: &ClassIndex, class: &str) -> Option<Domains> {
        if let Some(known) = self.memo.borrow().get(class) {
            return Some(*known);
        }
        if class.starts_with('[') {
            return None; // tableau : sa classe d'element n'est pas suivie
        }
        let mut domains = Domains::EMPTY;
        for (anchor, domain) in INTERFACE_ANCHORS {
            if anchor == class {
                domains = domains.with(domain);
            }
        }
        let mut current = class;
        for _ in 0..MAX_DEPTH {
            for (anchor, domain) in CLASS_ANCHORS {
                if anchor == current {
                    domains = domains.with(domain);
                }
            }
            if current == OBJECT {
                return self.remember(class, domains);
            }
            match index.superclass(current)? {
                Some(parent) => current = parent,
                None => return self.remember(class, domains),
            }
        }
        None
    }

    fn remember(&self, class: &str, domains: Domains) -> Option<Domains> {
        if domains.is_empty() {
            return None;
        }
        self.memo.borrow_mut().insert(class.to_owned(), domains);
        Some(domains)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hierarchy::Skeleton;

    fn declare(index: &mut ClassIndex, name: &str, parent: &str) {
        index.declare(
            name,
            Skeleton {
                super_name: Some(parent.to_owned()),
                access: 0,
            },
        );
    }

    fn hierarchy() -> ClassIndex {
        let mut index = ClassIndex::new();
        declare(&mut index, "net/minecraft/world/entity/Entity", OBJECT);
        declare(
            &mut index,
            "net/minecraft/world/entity/Mob",
            "net/minecraft/world/entity/Entity",
        );
        declare(
            &mut index,
            "example/Golem",
            "net/minecraft/world/entity/Mob",
        );
        declare(&mut index, "example/Plain", OBJECT);
        declare(&mut index, "example/Orphan", "example/Missing");
        declare(&mut index, "example/Tricky", "java/util/Random");
        index
    }

    #[test]
    fn subclasses_inherit_their_anchor_domain() {
        let index = hierarchy();
        let table = DomainTable::new();
        let entity = Domains::EMPTY.with(Domain::EntityState);
        assert_eq!(table.of_class(&index, "example/Golem"), Some(entity));
        assert_eq!(
            table.of_class(&index, "net/minecraft/world/entity/Entity"),
            Some(entity)
        );
        assert_eq!(
            table.of_class(&index, "example/Golem"),
            Some(entity),
            "memorise"
        );
    }

    #[test]
    fn unanchored_broken_and_array_classes_have_no_domain() {
        let index = hierarchy();
        let table = DomainTable::new();
        assert_eq!(
            table.of_class(&index, "example/Plain"),
            None,
            "aucune ancre"
        );
        assert_eq!(
            table.of_class(&index, "example/Orphan"),
            None,
            "chaine rompue"
        );
        assert_eq!(
            table.of_class(&index, "example/Nowhere"),
            None,
            "classe inconnue"
        );
        assert_eq!(
            table.of_class(&index, "[Lnet/minecraft/world/entity/Entity;"),
            None
        );
        // java/util/Random n'est pas declaree ici : la chaine de Tricky est rompue.
        assert_eq!(table.of_class(&index, "example/Tricky"), None);
    }

    #[test]
    fn a_late_declaration_completes_a_broken_chain() {
        let mut index = hierarchy();
        let table = DomainTable::new();
        assert_eq!(table.of_class(&index, "example/Tricky"), None);
        declare(&mut index, "java/util/Random", OBJECT);
        assert_eq!(
            table.of_class(&index, "example/Tricky"),
            Some(Domains::EMPTY.with(Domain::Random)),
            "un echec n'est pas memorise"
        );
    }

    #[test]
    fn interface_anchors_apply_to_the_named_type_only() {
        let index = hierarchy();
        let table = DomainTable::new();
        // Non declaree, l'interface est inconnue : rien n'est affirme.
        assert_eq!(
            table.of_class(&index, "net/minecraft/util/RandomSource"),
            None
        );
        let mut index = index;
        index.declare(
            "net/minecraft/util/RandomSource",
            Skeleton {
                super_name: Some(OBJECT.to_owned()),
                access: rfx_bytecode::access::INTERFACE,
            },
        );
        assert_eq!(
            table.of_class(&index, "net/minecraft/util/RandomSource"),
            Some(Domains::EMPTY.with(Domain::Random))
        );
    }

    #[test]
    fn anchor_names_are_unique() {
        let mut names: Vec<&str> = CLASS_ANCHORS
            .iter()
            .chain(INTERFACE_ANCHORS.iter())
            .map(|(name, _)| *name)
            .collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
    }
}
