//! C-24 : oracle de correction — une execution candidate est-elle identique a la
//! reference ?
//!
//! Cahier des charges : PARTIE 5.24 (« Oracle de correction »), R-490. Tests : T-332,
//! T-333. Maturite : `STABLE`.
//!
//! Tout est compare strictement : entiers egaux, reels **bit a bit** (aucune tolerance
//! par defaut), sequence de commandes egale apres tri par cle d'ordre, empreintes des
//! regions ecrites egales, meme exception levee, meme sequence d'evenements. Seuls les
//! identifiants et horodatages internes de RF-X sont normalises — et ils ne figurent pas
//! dans une [`Outcome`]. Toute autre normalisation est une faute.

use rfx_model::{Command, Region, Vec3d};

/// Une valeur rendue.
#[derive(Debug, Clone, Copy)]
pub enum Value {
    /// Entier, compare exactement.
    Int(i64),
    /// Reel simple, compare bit a bit.
    Float(f32),
    /// Reel double, compare bit a bit.
    Double(f64),
    /// Booleen.
    Bool(bool),
    /// Aucune valeur (methode `void`, ou reference nulle).
    None,
}

impl Value {
    /// Egalite stricte : `0.0` et `-0.0` different, deux `NaN` de memes bits sont egaux.
    #[must_use]
    pub fn same(self, other: Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a.to_bits() == b.to_bits(),
            (Self::Double(a), Self::Double(b)) => a.to_bits() == b.to_bits(),
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::None, Self::None) => true,
            _ => false,
        }
    }
}

/// Exception levee : son type et la condition de sa levee, identifies par l'appelant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thrown {
    /// Type de l'exception.
    pub type_id: u32,
    /// Condition de levee (site, ou cause).
    pub condition: u32,
}

/// Ce qu'une execution a produit.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    /// Valeurs rendues.
    pub returned: Vec<Value>,
    /// Commandes, chacune avec la cle d'ordre de son buffer et son rang dans le buffer.
    pub commands: Vec<((i32, u64, u32), u32, Command)>,
    /// Empreinte de chaque region ecrite.
    pub region_hashes: Vec<(Region, u64)>,
    /// Exception levee, s'il y en a une.
    pub thrown: Option<Thrown>,
    /// Evenements emis, dans l'ordre.
    pub events: Vec<u32>,
}

/// Ce qui differe entre la reference et la candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mismatch {
    /// Les valeurs rendues different.
    Returned,
    /// Les sequences de commandes different, apres tri par cle d'ordre.
    Commands,
    /// Une region ecrite differe, ou l'ensemble des regions ecrites.
    Regions,
    /// L'exception differe, ou l'une seulement en leve une.
    Exception,
    /// La sequence d'evenements differe.
    Events,
}

fn same_vec(a: Vec3d, b: Vec3d) -> bool {
    a.x.to_bits() == b.x.to_bits()
        && a.y.to_bits() == b.y.to_bits()
        && a.z.to_bits() == b.z.to_bits()
}

/// Egalite stricte de deux commandes : les positions reelles se comparent bit a bit.
#[must_use]
pub fn same_command(a: &Command, b: &Command) -> bool {
    match (a, b) {
        (
            Command::SpawnEntity {
                dim: da,
                proto: pa,
                pos: va,
                nbt: na,
            },
            Command::SpawnEntity {
                dim: db,
                proto: pb,
                pos: vb,
                nbt: nb,
            },
        ) => da == db && pa == pb && na == nb && same_vec(*va, *vb),
        _ => a == b,
    }
}

fn sorted_commands(outcome: &Outcome) -> Vec<&((i32, u64, u32), u32, Command)> {
    let mut sorted: Vec<_> = outcome.commands.iter().collect();
    sorted.sort_by_key(|(key, rank, _)| (*key, *rank));
    sorted
}

/// Cle de tri stable d'une region : nature, dimension, coordonnees ou identifiant.
type RegionKey = (u8, u16, i32, i32, u8);

fn region_key(region: Region) -> RegionKey {
    match region {
        Region::Chunk { dim, cx, cz } => (0, dim.0, cx, cz, 0),
        Region::Entity(entity) => (1, entity.dim.0, entity.id, 0, 0),
        Region::Domain(domain) => (2, 0, 0, 0, domain as u8),
        Region::Any => (3, 0, 0, 0, 0),
    }
}

fn sorted_regions(outcome: &Outcome) -> Vec<(RegionKey, u64)> {
    let mut regions: Vec<_> = outcome
        .region_hashes
        .iter()
        .map(|(region, hash)| (region_key(*region), *hash))
        .collect();
    regions.sort_unstable();
    regions
}

/// Compare une execution candidate a la reference ; `Ok(())` si elles sont identiques,
/// sinon la premiere difference trouvee, dans l'ordre de l'oracle.
///
/// # Erreurs
///
/// Le premier [`Mismatch`] constate.
pub fn compare(reference: &Outcome, candidate: &Outcome) -> Result<(), Mismatch> {
    let returned = reference.returned.len() == candidate.returned.len()
        && reference
            .returned
            .iter()
            .zip(&candidate.returned)
            .all(|(a, b)| a.same(*b));
    if !returned {
        return Err(Mismatch::Returned);
    }
    let (a, b) = (sorted_commands(reference), sorted_commands(candidate));
    let commands = a.len() == b.len()
        && a.iter()
            .zip(&b)
            .all(|(x, y)| x.0 == y.0 && x.1 == y.1 && same_command(&x.2, &y.2));
    if !commands {
        return Err(Mismatch::Commands);
    }
    if sorted_regions(reference) != sorted_regions(candidate) {
        return Err(Mismatch::Regions);
    }
    if reference.thrown != candidate.thrown {
        return Err(Mismatch::Exception);
    }
    if reference.events != candidate.events {
        return Err(Mismatch::Events);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfx_model::{BlockPos, BlockStateId, DimId, EntityProtoId};

    fn set_block(x: i32) -> Command {
        Command::SetBlock {
            dim: DimId(0),
            pos: BlockPos { x, y: 0, z: 0 },
            state: BlockStateId(1),
            flags: 3,
        }
    }

    fn spawn(x: f64) -> Command {
        Command::SpawnEntity {
            dim: DimId(0),
            proto: EntityProtoId(1),
            pos: Vec3d { x, y: 0.0, z: 0.0 },
            nbt: None,
        }
    }

    fn outcome() -> Outcome {
        Outcome {
            returned: vec![Value::Int(4), Value::Double(0.1 + 0.2)],
            commands: vec![((0, 1, 0), 0, set_block(1)), ((0, 2, 0), 0, set_block(2))],
            region_hashes: vec![(Region::Any, 7)],
            thrown: None,
            events: vec![1, 2],
        }
    }

    #[test]
    fn identical_outcomes_match() {
        assert_eq!(compare(&outcome(), &outcome()), Ok(()));
    }

    /// T-332 : les reels se comparent bit a bit, sans tolerance.
    #[test]
    fn floats_are_compared_bit_for_bit() {
        let mut close = outcome();
        close.returned[1] = Value::Double(0.3);
        assert_eq!(
            compare(&outcome(), &close),
            Err(Mismatch::Returned),
            "0.1 + 0.2 != 0.3"
        );
        assert!(!Value::Double(0.0).same(Value::Double(-0.0)));
        assert!(Value::Double(f64::NAN).same(Value::Double(f64::NAN)));
        assert!(
            !Value::Float(1.0).same(Value::Double(1.0)),
            "types differents"
        );
        assert!(!same_command(&spawn(0.0), &spawn(-0.0)));
        assert!(same_command(&spawn(f64::NAN), &spawn(f64::NAN)));
    }

    /// T-333 : l'exception se compare, type et condition.
    #[test]
    fn exceptions_are_compared() {
        let thrown = |type_id, condition| Outcome {
            thrown: Some(Thrown { type_id, condition }),
            ..outcome()
        };
        assert_eq!(compare(&thrown(1, 2), &thrown(1, 2)), Ok(()));
        assert_eq!(
            compare(&thrown(1, 2), &thrown(1, 3)),
            Err(Mismatch::Exception)
        );
        assert_eq!(compare(&thrown(1, 2), &outcome()), Err(Mismatch::Exception));
    }

    #[test]
    fn commands_match_after_sorting_by_order_key_only() {
        let mut shuffled = outcome();
        shuffled.commands.reverse();
        assert_eq!(
            compare(&outcome(), &shuffled),
            Ok(()),
            "l'ordre d'arrivee ne compte pas"
        );
        let mut changed = outcome();
        changed.commands[1].2 = set_block(3);
        assert_eq!(compare(&outcome(), &changed), Err(Mismatch::Commands));
        let mut missing = outcome();
        missing.commands.pop();
        assert_eq!(compare(&outcome(), &missing), Err(Mismatch::Commands));
        let mut reordered = outcome();
        reordered.commands[0].0 = (0, 3, 0);
        assert_eq!(
            compare(&outcome(), &reordered),
            Err(Mismatch::Commands),
            "meme contenu, autre ordre au commit"
        );
    }

    #[test]
    fn regions_and_events_are_compared() {
        let mut region = outcome();
        region.region_hashes[0].1 = 8;
        assert_eq!(compare(&outcome(), &region), Err(Mismatch::Regions));
        let mut events = outcome();
        events.events.swap(0, 1);
        assert_eq!(compare(&outcome(), &events), Err(Mismatch::Events));
    }
}
