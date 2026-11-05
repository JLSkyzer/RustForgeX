//! C-22 : Commit Engine, cote Rust — le protocole qui applique les buffers d'un tick.
//!
//! Cahier des charges : PARTIE 5.22 (protocole normatif, R-470 a R-473), FM-29 a FM-31.
//! Tests : T-310 a T-315. Maturite : `STABLE` pour le protocole ; l'application reelle au
//! jeu passera par des applicateurs Java, apres accord de Killian (ADR-073).
//!
//! Collecte des buffers scelles, tri dans l'ordre total, validation par C-23, application
//! atomique par buffer avec compensation, publication des epochs. Le budget de temps est
//! dur : un buffer qui ne tient pas est rejete, jamais reporte (R-470). L'horloge et le
//! monde sont fournis par l'appelant.

use rfx_model::{Command, Region, WorkId};

use crate::buffer::{sort_for_commit, BufferState, CommandBuffer, DiscardReason};
use crate::conflict::{ConflictDetector, EpochTable, Verdict};
use crate::Epoch;

/// Budget de commit par tick, par defaut (`commit.max_ns_per_tick`, R-470) : 2 ms.
pub const DEFAULT_MAX_NS: u64 = 2_000_000;

/// Ce qui applique les commandes au monde (R-220, R-472).
pub trait Applicator {
    /// Jeton qui permet d'annuler une commande appliquee.
    type Undo;

    /// `true` si la region est chargee ; une commande vers une region dechargee est
    /// rejetee, jamais l'occasion d'un chargement (R-471).
    fn is_loaded(&self, region: Region) -> bool;

    /// `true` si la commande est applicable telle quelle (bloc dans le monde, entite
    /// presente). Sans effet.
    fn validate(&self, command: &Command, buffer: &CommandBuffer) -> bool;

    /// `true` si ce genre de commande sait etre annule (R-473).
    fn compensable(&self, command: &Command) -> bool;

    /// Applique la commande ; rend de quoi l'annuler si elle est compensable.
    ///
    /// # Erreurs
    ///
    /// `Err(())` si l'application echoue ; rien ne doit alors avoir change.
    #[allow(clippy::result_unit_err)]
    fn apply(
        &mut self,
        command: &Command,
        buffer: &CommandBuffer,
    ) -> Result<Option<Self::Undo>, ()>;

    /// Annule une commande appliquee.
    fn compensate(&mut self, undo: Self::Undo);
}

/// Issue d'un buffer au commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Applique en entier.
    Applied,
    /// Jete, sans aucun effet visible.
    Discarded(DiscardReason),
    /// Echec d'application non compensable : des effets partiels peuvent subsister. Le
    /// workload passe en `JAVA_ONLY` definitif (FM-30, `E-2601`).
    Fatal,
}

/// Bilan d'un commit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitReport {
    /// Issue de chaque buffer, dans l'ordre du commit.
    pub outcomes: Vec<(WorkId, Outcome)>,
    /// Commandes appliquees.
    pub commands_applied: u64,
    /// Buffers rejetes faute de budget (`rfx.commit.budget_exceeded`).
    pub budget_exceeded: u64,
}

/// Commite les buffers scelles de `buffers` a l'epoch `tick` (PARTIE 5.22). Les buffers
/// qui ne sont pas scelles sont ignores. `now_ns` lit une horloge monotone.
pub fn commit<A: Applicator>(
    buffers: &mut [CommandBuffer],
    epochs: &mut EpochTable,
    tick: Epoch,
    applicator: &mut A,
    now_ns: &mut dyn FnMut() -> u64,
    max_ns: u64,
    max_checks: u64,
) -> CommitReport {
    let start = now_ns();
    sort_for_commit(buffers);
    let mut report = CommitReport::default();
    let mut written: Vec<Region> = Vec::new();
    {
        let mut detector = ConflictDetector::new(epochs, max_checks);
        for buffer in buffers.iter_mut() {
            if buffer.state() != BufferState::Sealed {
                continue;
            }
            let outcome = commit_one(buffer, &mut detector, applicator, now_ns, start, max_ns);
            match outcome {
                Outcome::Applied => {
                    report.commands_applied += buffer.commands().len() as u64;
                    written.extend(buffer.write_regions());
                }
                Outcome::Discarded(DiscardReason::BudgetExceeded) => report.budget_exceeded += 1,
                _ => {}
            }
            report.outcomes.push((buffer.work(), outcome));
        }
    }
    // Publication : les regions ecrites prennent l'epoch du tick.
    for region in written {
        epochs.record(region, tick);
    }
    report
}

fn discard(buffer: &mut CommandBuffer, reason: DiscardReason) -> Outcome {
    // Un buffer scelle ou valide se jette toujours.
    let _ = buffer.discard(reason);
    Outcome::Discarded(reason)
}

fn commit_one<A: Applicator>(
    buffer: &mut CommandBuffer,
    detector: &mut ConflictDetector<'_>,
    applicator: &mut A,
    now_ns: &mut dyn FnMut() -> u64,
    start: u64,
    max_ns: u64,
) -> Outcome {
    if now_ns().saturating_sub(start) > max_ns {
        return discard(buffer, DiscardReason::BudgetExceeded);
    }
    // Validation de chaque commande avant la detection : un buffer invalide ne doit pas
    // compter comme ecrivain pour les suivants.
    let commands = buffer.commands();
    if commands.iter().any(|c| !applicator.is_loaded(c.region())) {
        return discard(buffer, DiscardReason::Invalid);
    }
    if commands.iter().any(|c| !applicator.validate(c, buffer)) {
        return discard(buffer, DiscardReason::Invalid);
    }
    // R-473 : une commande non compensable n'est admise que seule.
    if commands.len() > 1 && commands.iter().any(|c| !applicator.compensable(c)) {
        return discard(buffer, DiscardReason::Invalid);
    }
    match detector.check(buffer) {
        Verdict::Accept => {}
        Verdict::Stale(_) => return discard(buffer, DiscardReason::Stale),
        Verdict::Conflict(_) => return discard(buffer, DiscardReason::Conflict),
        Verdict::CheckBudgetExceeded => return discard(buffer, DiscardReason::BudgetExceeded),
    }
    if buffer.mark_validated().is_err() {
        return discard(buffer, DiscardReason::Invalid);
    }
    let mut undos = Vec::with_capacity(buffer.commands().len());
    for command in buffer.commands().to_vec() {
        match applicator.apply(&command, buffer) {
            Ok(undo) => undos.push(undo),
            Err(()) => {
                // Atomicite par buffer : on defait ce qui a ete fait, a rebours.
                let mut complete = true;
                for undo in undos.into_iter().rev() {
                    match undo {
                        Some(undo) => applicator.compensate(undo),
                        None => complete = false,
                    }
                }
                let _ = buffer.discard(DiscardReason::Invalid);
                return if complete {
                    Outcome::Discarded(DiscardReason::Invalid)
                } else {
                    Outcome::Fatal
                };
            }
        }
    }
    let _ = buffer.mark_applied();
    Outcome::Applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::{OrderKey, DEFAULT_MAX_COMMANDS};
    use crate::conflict::DEFAULT_MAX_CHECKS;
    use rfx_model::{BlobRef, BlockPos, BlockStateId, DimId, JavaCallbackId};
    use std::collections::{BTreeMap, BTreeSet};

    /// Monde factice : des blocs, des chunks charges, des pannes injectables.
    #[derive(Default)]
    struct World {
        blocks: BTreeMap<(i32, i32, i32), u32>,
        unloaded: BTreeSet<i32>,
        fail_on_state: Option<u32>,
        log: Vec<(i32, u32)>,
        loads_requested: u32,
    }

    impl Applicator for World {
        type Undo = ((i32, i32, i32), Option<u32>);

        fn is_loaded(&self, region: Region) -> bool {
            match region {
                Region::Chunk { cx, .. } => !self.unloaded.contains(&cx),
                _ => true,
            }
        }

        fn validate(&self, command: &Command, _: &CommandBuffer) -> bool {
            match command {
                Command::SetBlock { pos, .. } => (-64..320).contains(&pos.y),
                _ => true,
            }
        }

        fn compensable(&self, command: &Command) -> bool {
            matches!(command, Command::SetBlock { .. })
        }

        fn apply(
            &mut self,
            command: &Command,
            _: &CommandBuffer,
        ) -> Result<Option<Self::Undo>, ()> {
            match *command {
                Command::SetBlock { pos, state, .. } => {
                    if self.unloaded.contains(&(pos.x >> 4)) {
                        // Ne doit jamais arriver : le commit l'a filtre (R-471).
                        self.loads_requested += 1;
                    }
                    if self.fail_on_state == Some(state.0) {
                        return Err(());
                    }
                    let key = (pos.x, pos.y, pos.z);
                    let previous = self.blocks.insert(key, state.0);
                    self.log.push((pos.x, state.0));
                    Ok(Some((key, previous)))
                }
                Command::CallbackJava { .. } => {
                    self.log.push((i32::MIN, 0));
                    Ok(None)
                }
                _ => Ok(None),
            }
        }

        fn compensate(&mut self, (key, previous): Self::Undo) {
            match previous {
                Some(state) => self.blocks.insert(key, state),
                None => self.blocks.remove(&key),
            };
        }
    }

    fn set(x: i32, state: u32) -> Command {
        Command::SetBlock {
            dim: DimId(0),
            pos: BlockPos { x, y: 64, z: 0 },
            state: BlockStateId(state),
            flags: 3,
        }
    }

    fn sealed(
        task: u32,
        key: u64,
        epoch: u64,
        reads: &[Region],
        commands: &[Command],
    ) -> CommandBuffer {
        let mut b = CommandBuffer::open(
            task,
            WorkId(u64::from(task)),
            Epoch(epoch),
            OrderKey {
                priority: 0,
                key,
                index: 0,
            },
            DEFAULT_MAX_COMMANDS,
        );
        for r in reads {
            b.note_read(*r).unwrap();
        }
        for c in commands {
            b.push(*c).unwrap();
        }
        b.seal().unwrap();
        b
    }

    fn run(
        world: &mut World,
        buffers: &mut [CommandBuffer],
        epochs: &mut EpochTable,
    ) -> CommitReport {
        let mut clock = 0_u64;
        let mut now = || {
            clock += 1;
            clock
        };
        commit(
            buffers,
            epochs,
            Epoch(10),
            world,
            &mut now,
            DEFAULT_MAX_NS,
            DEFAULT_MAX_CHECKS,
        )
    }

    fn chunk(cx: i32) -> Region {
        Region::Chunk {
            dim: DimId(0),
            cx,
            cz: 0,
        }
    }

    /// T-310 : le commit est deterministe, quel que soit l'ordre de remise des buffers.
    #[test]
    fn the_commit_is_deterministic() {
        let make = || -> Vec<CommandBuffer> {
            (0..20_u32)
                .map(|i| {
                    let x = i32::try_from(i % 6).unwrap() * 16;
                    sealed(i, u64::from(i * 7 % 13), 10, &[], &[set(x, i)])
                })
                .collect()
        };
        let mut reference_world = World::default();
        let mut reference = make();
        let expected = run(&mut reference_world, &mut reference, &mut EpochTable::new());
        for rotation in 1..20 {
            let mut world = World::default();
            let mut buffers = make();
            buffers.rotate_left(rotation);
            buffers.reverse();
            let report = run(&mut world, &mut buffers, &mut EpochTable::new());
            assert_eq!(report, expected, "remise {rotation}");
            assert_eq!(world.log, reference_world.log);
            assert_eq!(world.blocks, reference_world.blocks);
        }
    }

    /// T-311 et T-312 : un buffer perime ou en conflit est jete sans effet.
    #[test]
    fn stale_and_conflicting_buffers_are_discarded() {
        let mut epochs = EpochTable::new();
        epochs.record(chunk(5), Epoch(9));
        let mut world = World::default();
        let mut buffers = vec![
            sealed(1, 1, 8, &[chunk(5)], &[set(0, 1)]),
            sealed(2, 2, 10, &[], &[set(32, 2)]),
            sealed(3, 3, 10, &[], &[set(33, 3)]),
        ];
        let report = run(&mut world, &mut buffers, &mut epochs);
        assert_eq!(
            report.outcomes,
            vec![
                (WorkId(1), Outcome::Discarded(DiscardReason::Stale)),
                (WorkId(2), Outcome::Applied),
                (WorkId(3), Outcome::Discarded(DiscardReason::Conflict)),
            ]
        );
        assert_eq!(world.log, vec![(32, 2)]);
        assert_eq!(epochs.last_write(chunk(2)), Epoch(10), "epoch publiee");
    }

    /// T-313 : atomicite par buffer. Une commande qui echoue fait defaire les
    /// precedentes ; si l'une n'est pas compensable, l'incident est fatal.
    #[test]
    fn a_buffer_is_applied_entirely_or_not_at_all() {
        let mut world = World::default();
        world.blocks.insert((0, 64, 0), 99);
        world.fail_on_state = Some(3);
        let mut buffers = vec![sealed(1, 1, 10, &[], &[set(0, 1), set(16, 2), set(32, 3)])];
        let report = run(&mut world, &mut buffers, &mut EpochTable::new());
        assert_eq!(
            report.outcomes,
            vec![(WorkId(1), Outcome::Discarded(DiscardReason::Invalid))]
        );
        assert_eq!(world.blocks.get(&(0, 64, 0)), Some(&99), "restaure");
        assert_eq!(world.blocks.get(&(16, 64, 0)), None, "defait");
        assert_eq!(report.commands_applied, 0);

        // R-473 : une commande non compensable n'est admise que seule.
        let callback = Command::CallbackJava {
            handle: JavaCallbackId(1),
            arg: BlobRef { offset: 0, len: 0 },
        };
        let mut mixed = vec![sealed(2, 1, 10, &[], &[set(0, 5), callback])];
        let report = run(&mut World::default(), &mut mixed, &mut EpochTable::new());
        assert_eq!(
            report.outcomes,
            vec![(WorkId(2), Outcome::Discarded(DiscardReason::Invalid))]
        );
        let mut alone = vec![sealed(3, 1, 10, &[], &[callback])];
        let report = run(&mut World::default(), &mut alone, &mut EpochTable::new());
        assert_eq!(report.outcomes, vec![(WorkId(3), Outcome::Applied)]);
    }

    /// T-314 : le budget est dur ; au-dela, les buffers sont rejetes, pas reportes.
    #[test]
    fn the_budget_rejects_instead_of_deferring() {
        let mut buffers: Vec<CommandBuffer> = (0..10_u32)
            .map(|i| {
                sealed(
                    i,
                    u64::from(i),
                    10,
                    &[],
                    &[set(i32::try_from(i).unwrap() * 16, i)],
                )
            })
            .collect();
        let mut world = World::default();
        let mut clock = 0_u64;
        let mut now = || {
            clock += 300_000;
            clock
        };
        let report = commit(
            &mut buffers,
            &mut EpochTable::new(),
            Epoch(10),
            &mut world,
            &mut now,
            DEFAULT_MAX_NS,
            DEFAULT_MAX_CHECKS,
        );
        assert!(report.budget_exceeded > 0);
        let applied = report
            .outcomes
            .iter()
            .filter(|(_, o)| *o == Outcome::Applied)
            .count();
        assert!(applied > 0 && applied < 10, "{applied}");
        assert!(buffers
            .iter()
            .all(|b| matches!(b.state(), BufferState::Applied | BufferState::Discarded(_))));
        assert_eq!(world.log.len(), applied, "aucun effet des buffers rejetes");
    }

    /// T-315 : aucune commande vers un chunk decharge n'atteint le monde.
    #[test]
    fn no_command_reaches_an_unloaded_chunk() {
        let mut world = World::default();
        world.unloaded.insert(3);
        let mut buffers = vec![
            sealed(1, 1, 10, &[], &[set(48, 1)]),
            sealed(2, 2, 10, &[], &[set(0, 2), set(49, 2)]),
            sealed(3, 3, 10, &[], &[set(64, 3)]),
        ];
        let report = run(&mut world, &mut buffers, &mut EpochTable::new());
        assert_eq!(world.loads_requested, 0);
        assert_eq!(
            report.outcomes,
            vec![
                (WorkId(1), Outcome::Discarded(DiscardReason::Invalid)),
                (WorkId(2), Outcome::Discarded(DiscardReason::Invalid)),
                (WorkId(3), Outcome::Applied),
            ]
        );
    }

    #[test]
    fn invalid_commands_discard_the_whole_buffer_before_any_effect() {
        let mut world = World::default();
        let below = Command::SetBlock {
            dim: DimId(0),
            pos: BlockPos {
                x: 0,
                y: -100,
                z: 0,
            },
            state: BlockStateId(1),
            flags: 3,
        };
        let mut buffers = vec![sealed(1, 1, 10, &[], &[set(16, 1), below])];
        let report = run(&mut world, &mut buffers, &mut EpochTable::new());
        assert_eq!(
            report.outcomes,
            vec![(WorkId(1), Outcome::Discarded(DiscardReason::Invalid))]
        );
        assert!(world.log.is_empty());
        // Un buffer encore ouvert n'est pas collecte.
        let mut open = CommandBuffer::open(
            9,
            WorkId(9),
            Epoch(10),
            OrderKey {
                priority: 0,
                key: 0,
                index: 0,
            },
            8,
        );
        open.push(set(0, 1)).unwrap();
        let report = run(
            &mut world,
            std::slice::from_mut(&mut open),
            &mut EpochTable::new(),
        );
        assert!(report.outcomes.is_empty());
    }

    /// FM-30 : un echec apres une commande non compensable est fatal.
    #[test]
    fn an_uncompensable_failure_is_fatal() {
        struct Fragile;
        impl Applicator for Fragile {
            type Undo = ();
            fn is_loaded(&self, _: Region) -> bool {
                true
            }
            fn validate(&self, _: &Command, _: &CommandBuffer) -> bool {
                true
            }
            fn compensable(&self, _: &Command) -> bool {
                true
            }
            fn apply(&mut self, command: &Command, _: &CommandBuffer) -> Result<Option<()>, ()> {
                match command {
                    Command::SetBlock { state, .. } if state.0 == 2 => Err(()),
                    _ => Ok(None),
                }
            }
            fn compensate(&mut self, (): ()) {}
        }
        let mut buffers = vec![sealed(1, 1, 10, &[], &[set(0, 1), set(16, 2)])];
        let mut clock = 0_u64;
        let mut now = || {
            clock += 1;
            clock
        };
        let report = commit(
            &mut buffers,
            &mut EpochTable::new(),
            Epoch(10),
            &mut Fragile,
            &mut now,
            DEFAULT_MAX_NS,
            DEFAULT_MAX_CHECKS,
        );
        assert_eq!(report.outcomes, vec![(WorkId(1), Outcome::Fatal)]);
    }
}
