//! C-23 : Conflict Detection — un buffer n'est applique que si ce qu'il a lu n'a pas
//! change.
//!
//! Cahier des charges : PARTIE 5.23 (R-480, R-481), R-441. Tests : T-320 a T-323.
//! Maturite : `STABLE`.
//!
//! Deux mecanismes :
//!
//! - **la peremption** : chaque region garde l'epoch de sa derniere ecriture appliquee ;
//!   un buffer dont une region lue a ete ecrite apres son snapshot est perime ;
//! - **le conflit dans le tick** : dans l'ordre du commit, un buffer qui lit ou ecrit ce
//!   qu'un buffer accepte avant lui a ecrit est rejete. Le premier en ordre gagne.
//!
//! La detection est prudente (R-480) : une ecriture d'un domaine entier, de
//! `Domain::Unknown` ou de `Any` recouvre toutes les regions qu'elle peut toucher.

use std::collections::HashMap;

use rfx_model::{Domain, Region};

use crate::buffer::CommandBuffer;
use crate::Epoch;

/// Verifications au plus par commit, par defaut (`commit.max_conflict_checks`, R-481).
pub const DEFAULT_MAX_CHECKS: u64 = 1_000_000;

/// Issue de la verification d'un buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Applicable.
    Accept,
    /// Une region lue a ete ecrite apres le snapshot (R-441).
    Stale(Region),
    /// Une region lue ou ecrite l'a deja ete par un buffer accepte avant lui.
    Conflict(Region),
    /// Le plafond de verifications est atteint : rejet par prudence (R-481).
    CheckBudgetExceeded,
}

/// Epoch de la derniere ecriture appliquee, par region et par domaine.
#[derive(Debug, Default, Clone)]
pub struct EpochTable {
    exact: HashMap<Region, Epoch>,
    /// Derniere ecriture d'une region quelconque de chaque domaine.
    by_domain: HashMap<Domain, Epoch>,
    /// Derniere ecriture qui recouvre tout (`Any`, `Domain::Unknown`).
    wide: Epoch,
    /// Derniere ecriture, toutes regions confondues.
    latest: Epoch,
}

fn is_wide(region: Region) -> bool {
    matches!(region, Region::Any | Region::Domain(Domain::Unknown))
}

impl EpochTable {
    /// Table vide : rien n'a jamais ete ecrit.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enregistre une ecriture appliquee a `epoch`.
    pub fn record(&mut self, region: Region, epoch: Epoch) {
        let raise = |slot: &mut Epoch| *slot = (*slot).max(epoch);
        raise(self.exact.entry(region).or_default());
        raise(&mut self.latest);
        if is_wide(region) {
            raise(&mut self.wide);
        } else if let Some(domain) = region.domain() {
            raise(self.by_domain.entry(domain).or_default());
        }
    }

    /// Derniere ecriture qui a pu toucher `region`.
    #[must_use]
    pub fn last_write(&self, region: Region) -> Epoch {
        if is_wide(region) {
            return self.latest;
        }
        let mut last = self.wide;
        if let Some(epoch) = self.exact.get(&region) {
            last = last.max(*epoch);
        }
        match region {
            // Un domaine entier a pu etre touche par n'importe laquelle de ses regions.
            Region::Domain(domain) => {
                if let Some(epoch) = self.by_domain.get(&domain) {
                    last = last.max(*epoch);
                }
            }
            // Une region fine : par une ecriture d'elle-meme ou de son domaine entier.
            _ => {
                if let Some(domain) = region.domain() {
                    if let Some(epoch) = self.exact.get(&Region::Domain(domain)) {
                        last = last.max(*epoch);
                    }
                }
            }
        }
        last
    }
}

/// C-23 : verification des buffers d'un commit, dans l'ordre du commit.
#[derive(Debug)]
pub struct ConflictDetector<'a> {
    epochs: &'a EpochTable,
    written: Vec<Region>,
    checks: u64,
    max_checks: u64,
}

impl<'a> ConflictDetector<'a> {
    /// Detecteur d'un commit, sur l'etat des epochs avant ce commit.
    #[must_use]
    pub fn new(epochs: &'a EpochTable, max_checks: u64) -> Self {
        Self {
            epochs,
            written: Vec::new(),
            checks: 0,
            max_checks,
        }
    }

    /// Verifications faites jusqu'ici.
    #[must_use]
    pub fn checks(&self) -> u64 {
        self.checks
    }

    /// Regions ecrites par les buffers acceptes, dans l'ordre.
    #[must_use]
    pub fn written(&self) -> &[Region] {
        &self.written
    }

    fn spend(&mut self, n: usize) -> bool {
        self.checks = self.checks.saturating_add(n as u64);
        self.checks <= self.max_checks
    }

    fn already_written(&self, region: Region) -> Option<Region> {
        self.written.iter().copied().find(|w| w.overlaps(region))
    }

    /// Verifie le buffer suivant dans l'ordre du commit ; s'il est accepte, ses ecritures
    /// comptent pour les suivants. Un buffer sans commande n'a aucun effet : accepte.
    pub fn check(&mut self, buffer: &CommandBuffer) -> Verdict {
        let writes = buffer.write_regions();
        if writes.is_empty() {
            return Verdict::Accept;
        }
        let reads = buffer.read_regions();
        let cost = reads.len() + (reads.len() + writes.len()) * (self.written.len() + 1);
        if !self.spend(cost) {
            return Verdict::CheckBudgetExceeded;
        }
        for read in reads {
            if self.epochs.last_write(*read) > buffer.epoch() {
                return Verdict::Stale(*read);
            }
        }
        for region in reads.iter().chain(&writes) {
            if let Some(written) = self.already_written(*region) {
                return Verdict::Conflict(written);
            }
        }
        self.written.extend(writes);
        Verdict::Accept
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::{OrderKey, DEFAULT_MAX_COMMANDS};
    use rfx_model::{
        BlobRef, BlockPos, BlockStateId, Command, DimId, EntityKey, EntityPatch, WorkId,
    };

    fn chunk(cx: i32) -> Region {
        Region::Chunk {
            dim: DimId(0),
            cx,
            cz: 0,
        }
    }

    fn entity(id: i32) -> Region {
        Region::Entity(EntityKey { dim: DimId(0), id })
    }

    /// Une commande qui ecrit exactement `region`.
    fn writing(region: Region) -> Command {
        match region {
            Region::Chunk { dim, cx, cz } => Command::SetBlock {
                dim,
                pos: BlockPos {
                    x: cx * 16,
                    y: 64,
                    z: cz * 16,
                },
                state: BlockStateId(1),
                flags: 3,
            },
            Region::Entity(entity) => Command::UpdateEntity {
                entity,
                patch: EntityPatch(BlobRef { offset: 0, len: 0 }),
            },
            Region::Domain(Domain::EntityState) => Command::SpawnEntity {
                dim: DimId(0),
                proto: rfx_model::EntityProtoId(1),
                pos: rfx_model::Vec3d {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                nbt: None,
            },
            Region::Domain(Domain::Network) => Command::SendPacket {
                target: rfx_model::PacketTarget::All,
                payload: BlobRef { offset: 0, len: 0 },
            },
            _ => Command::CallbackJava {
                handle: rfx_model::JavaCallbackId(0),
                arg: BlobRef { offset: 0, len: 0 },
            },
        }
    }

    fn buffer(task: u32, epoch: u64, reads: &[Region], writes: &[Region]) -> CommandBuffer {
        let mut b = CommandBuffer::open(
            task,
            WorkId(u64::from(task)),
            Epoch(epoch),
            OrderKey {
                priority: 0,
                key: u64::from(task),
                index: 0,
            },
            DEFAULT_MAX_COMMANDS,
        );
        for r in reads {
            b.note_read(*r).unwrap();
        }
        for w in writes {
            b.push(writing(*w)).unwrap();
        }
        b.seal().unwrap();
        b
    }

    /// T-320 : deux ecritures de la meme region dans un commit : la seconde est rejetee.
    #[test]
    fn write_write_is_always_detected() {
        let epochs = EpochTable::new();
        let mut detector = ConflictDetector::new(&epochs, DEFAULT_MAX_CHECKS);
        assert_eq!(
            detector.check(&buffer(1, 5, &[], &[chunk(0)])),
            Verdict::Accept
        );
        assert_eq!(
            detector.check(&buffer(2, 5, &[], &[chunk(0)])),
            Verdict::Conflict(chunk(0))
        );
        assert_eq!(
            detector.check(&buffer(3, 5, &[], &[chunk(1)])),
            Verdict::Accept
        );
        // Un domaine entier recouvre ses regions, dans les deux sens.
        assert_eq!(
            detector.check(&buffer(4, 5, &[], &[Region::Domain(Domain::WorldBlocks)])),
            Verdict::Conflict(chunk(0))
        );
        assert_eq!(
            detector.check(&buffer(5, 5, &[], &[entity(3)])),
            Verdict::Accept
        );
        assert!(matches!(
            detector.check(&buffer(6, 5, &[], &[Region::Any])),
            Verdict::Conflict(_)
        ));
    }

    /// T-321 : une region lue, ecrite apres le snapshot, rend le buffer perime.
    #[test]
    fn a_stale_read_is_detected() {
        let mut epochs = EpochTable::new();
        epochs.record(entity(1), Epoch(8));
        epochs.record(Region::Domain(Domain::WorldBlocks), Epoch(6));
        let mut detector = ConflictDetector::new(&epochs, DEFAULT_MAX_CHECKS);
        assert_eq!(
            detector.check(&buffer(1, 7, &[entity(1)], &[chunk(9)])),
            Verdict::Stale(entity(1))
        );
        assert_eq!(
            detector.check(&buffer(2, 8, &[entity(1)], &[chunk(9)])),
            Verdict::Accept,
            "ecrite au tick du snapshot : vue par lui"
        );
        assert_eq!(
            detector.check(&buffer(3, 5, &[chunk(4)], &[entity(2)])),
            Verdict::Stale(chunk(4)),
            "le domaine entier a ete ecrit apres le snapshot"
        );
    }

    #[test]
    fn a_buffer_without_commands_is_harmless() {
        let mut epochs = EpochTable::new();
        epochs.record(Region::Any, Epoch(100));
        let mut detector = ConflictDetector::new(&epochs, DEFAULT_MAX_CHECKS);
        assert_eq!(
            detector.check(&buffer(1, 1, &[Region::Any], &[])),
            Verdict::Accept
        );
    }

    /// T-323 : le cout est borne ; au-dela, le buffer est rejete, jamais accepte sans
    /// verification.
    #[test]
    fn the_check_cost_is_bounded() {
        let epochs = EpochTable::new();
        let mut detector = ConflictDetector::new(&epochs, 50);
        let mut accepted = 0;
        let mut refused = 0;
        for task in 0..100 {
            let b = buffer(
                task,
                1,
                &[entity(1000 + i32::try_from(task).unwrap())],
                &[chunk(i32::try_from(task).unwrap())],
            );
            match detector.check(&b) {
                Verdict::Accept => accepted += 1,
                Verdict::CheckBudgetExceeded => refused += 1,
                other => panic!("{other:?}"),
            }
        }
        assert!(accepted > 0 && refused > 0);
        assert!(detector.checks() > 50, "le depassement est constate");
        assert_eq!(
            detector.written().len(),
            accepted,
            "seuls les acceptes comptent"
        );
    }

    /// Generateur deterministe pour la suite synthetique.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }

        fn region(&mut self) -> Region {
            #[allow(clippy::cast_possible_truncation)]
            match self.below(12) {
                0..=3 => chunk(self.below(4) as i32),
                4..=7 => entity(self.below(4) as i32),
                8 => Region::Domain(Domain::WorldBlocks),
                9 => Region::Domain(Domain::EntityState),
                10 => Region::Domain(Domain::Network),
                _ => Region::Any,
            }
        }

        fn regions(&mut self, max: u64) -> Vec<Region> {
            (0..self.below(max + 1)).map(|_| self.region()).collect()
        }
    }

    /// Oracle naif, ecrit a part : un buffer est sain si aucune ecriture passee posterieure
    /// a son snapshot ne recouvre une region qu'il lit, et si aucun buffer accepte avant
    /// lui dans le commit n'ecrit une region qu'il lit ou ecrit.
    fn oracle_sound(
        history: &[(Region, u64)],
        accepted: &[CommandBuffer],
        candidate: &CommandBuffer,
    ) -> bool {
        if candidate.commands().is_empty() {
            return true;
        }
        let epoch = candidate.epoch().0;
        let stale = candidate.read_regions().iter().any(|read| {
            history
                .iter()
                .any(|(written, at)| *at > epoch && written.overlaps(*read))
        });
        let touched: Vec<Region> = candidate
            .read_regions()
            .iter()
            .copied()
            .chain(candidate.commands().iter().map(Command::region))
            .collect();
        let conflict = accepted.iter().any(|earlier| {
            earlier
                .commands()
                .iter()
                .any(|c| touched.iter().any(|t| c.region().overlaps(*t)))
        });
        !stale && !conflict
    }

    /// T-322 : aucun faux negatif sur 10 000 cas synthetiques. Chaque cas : un passe
    /// d'ecritures datees, puis un commit de plusieurs buffers. Tout buffer accepte par le
    /// detecteur doit etre sain selon l'oracle ; et l'oracle ne doit refuser aucun buffer
    /// que le detecteur accepte. Les faux positifs sont comptes.
    #[test]
    fn no_false_negative_on_ten_thousand_synthetic_cases() {
        let mut rng = Rng(0x5DEE_CE66_D1CE_4E5B);
        let mut false_positives = 0_u32;
        let mut decisions = 0_u32;
        for _ in 0..10_000 {
            let mut epochs = EpochTable::new();
            let mut history = Vec::new();
            for _ in 0..rng.below(4) {
                let (region, at) = (rng.region(), 1 + rng.below(10));
                epochs.record(region, Epoch(at));
                history.push((region, at));
            }
            let mut detector = ConflictDetector::new(&epochs, DEFAULT_MAX_CHECKS);
            let mut accepted: Vec<CommandBuffer> = Vec::new();
            for task in 0..(1 + rng.below(5)) {
                let reads = rng.regions(3);
                let writes = rng.regions(3);
                #[allow(clippy::cast_possible_truncation)]
                let candidate = buffer(task as u32, rng.below(12), &reads, &writes);
                let sound = oracle_sound(&history, &accepted, &candidate);
                decisions += 1;
                match detector.check(&candidate) {
                    Verdict::Accept => {
                        assert!(sound, "faux negatif : {candidate:?}");
                        accepted.push(candidate);
                    }
                    Verdict::CheckBudgetExceeded => panic!("plafond atteint"),
                    _ => {
                        if sound {
                            false_positives += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(false_positives, 0, "sur {decisions} decisions");
    }
}
