//! C-21 : Command Buffer — les effets d'une tache, ecrits sans toucher le jeu.
//!
//! Cahier des charges : PARTIE 5.21 (R-460 a R-463), DM-16, SM-05. Tests : T-300 a
//! T-302. Maturite : `STABLE`.
//!
//! Un buffer s'ouvre, recoit ses commandes dans l'ordre ou la tache les produit, puis se
//! scelle : plus rien ne s'y ajoute (R-460). Le commit les trie par [`OrderKey`], cle
//! derivee de l'unite traitee et non de l'ordre d'achevement des taches (R-461).

use rfx_model::{AccessSet, BlobRef, Command, Region, WorkId};

use crate::Epoch;

/// Commandes au plus par buffer, par defaut (`commit.max_commands_per_buffer`, R-462).
pub const DEFAULT_MAX_COMMANDS: usize = 4_096;

/// Cle d'ordre total d'un buffer au commit : priorite, puis cle stable (index d'entite,
/// position de bloc, index de batch), puis rang dans le lot (R-461).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OrderKey {
    /// Priorite : la plus faible passe d'abord.
    pub priority: i32,
    /// Cle stable de l'element traite.
    pub key: u64,
    /// Rang dans un lot qui partage la meme cle.
    pub index: u32,
}

/// Etat d'un buffer (SM-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferState {
    /// Recoit des commandes.
    Open,
    /// Scelle : ne change plus.
    Sealed,
    /// Accepte par la detection de conflits.
    Validated,
    /// Applique au jeu.
    Applied,
    /// Jete, avec son motif.
    Discarded(DiscardReason),
}

/// Pourquoi un buffer est jete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscardReason {
    /// Trop de commandes (R-462) : repli sur Java.
    BudgetExceeded,
    /// Une region lue a change depuis la capture (R-441).
    Stale,
    /// Conflit avec un buffer passe avant lui (C-23).
    Conflict,
    /// Une commande est invalide (bloc hors monde, entite disparue).
    Invalid,
    /// Execution fantome : toujours jete (C-25).
    Shadow,
    /// Retour arriere du workload (C-26).
    Rollback,
}

/// Refus d'une operation sur un buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferError {
    /// Le buffer n'est pas ouvert : il est append-only puis scelle (R-460).
    NotOpen,
    /// La borne de commandes est atteinte ; le buffer est jete (R-462).
    BudgetExceeded,
    /// Transition d'etat interdite.
    InvalidTransition,
    /// Plus de 4 Gio de donnees dans un buffer : la plage ne tiendrait pas sur 32 bits.
    BlobTooLarge,
}

/// C-21 : les effets d'une tache.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandBuffer {
    owner_task: u32,
    work: WorkId,
    epoch: Epoch,
    read_set: AccessSet,
    read_regions: Vec<Region>,
    commands: Vec<Command>,
    blobs: Vec<u8>,
    order_key: OrderKey,
    max_commands: usize,
    state: BufferState,
}

impl CommandBuffer {
    /// Ouvre un buffer pour la tache `owner_task` du workload `work`, qui lit un
    /// snapshot pris a `epoch`.
    #[must_use]
    pub fn open(
        owner_task: u32,
        work: WorkId,
        epoch: Epoch,
        order_key: OrderKey,
        max_commands: usize,
    ) -> Self {
        Self {
            owner_task,
            work,
            epoch,
            read_set: AccessSet::default(),
            read_regions: Vec::new(),
            commands: Vec::new(),
            blobs: Vec::new(),
            order_key,
            max_commands,
            state: BufferState::Open,
        }
    }

    fn require_open(&self) -> Result<(), BufferError> {
        if self.state == BufferState::Open {
            Ok(())
        } else {
            Err(BufferError::NotOpen)
        }
    }

    /// Ajoute une commande a la fin. Au-dela de la borne, le buffer entier est jete
    /// (R-462) : un buffer tronque serait un effet partiel.
    ///
    /// # Erreurs
    ///
    /// [`BufferError::NotOpen`] si le buffer est scelle ou jete ;
    /// [`BufferError::BudgetExceeded`] si la borne est atteinte.
    pub fn push(&mut self, command: Command) -> Result<(), BufferError> {
        self.require_open()?;
        if self.commands.len() >= self.max_commands {
            self.state = BufferState::Discarded(DiscardReason::BudgetExceeded);
            return Err(BufferError::BudgetExceeded);
        }
        self.commands.push(command);
        Ok(())
    }

    /// Range des octets dans la zone de donnees du buffer et rend leur plage.
    ///
    /// # Erreurs
    ///
    /// [`BufferError::NotOpen`] si le buffer n'est plus ouvert ;
    /// [`BufferError::BlobTooLarge`] si la zone depasserait 4 Gio.
    pub fn add_blob(&mut self, bytes: &[u8]) -> Result<BlobRef, BufferError> {
        self.require_open()?;
        let offset = u32::try_from(self.blobs.len()).map_err(|_| BufferError::BlobTooLarge)?;
        let len = u32::try_from(bytes.len()).map_err(|_| BufferError::BlobTooLarge)?;
        offset.checked_add(len).ok_or(BufferError::BlobTooLarge)?;
        self.blobs.extend_from_slice(bytes);
        Ok(BlobRef { offset, len })
    }

    /// Enregistre une region effectivement lue (R-463), et ses domaines dans le read set.
    ///
    /// # Erreurs
    ///
    /// [`BufferError::NotOpen`] si le buffer n'est plus ouvert.
    pub fn note_read(&mut self, region: Region) -> Result<(), BufferError> {
        self.require_open()?;
        let domain = region.domain().unwrap_or(rfx_model::Domain::Unknown);
        self.read_set.domains = self.read_set.domains.with(domain);
        if !self.read_regions.contains(&region) {
            self.read_regions.push(region);
        }
        Ok(())
    }

    /// Scelle le buffer : plus aucune commande, plus aucune lecture (R-460).
    ///
    /// # Erreurs
    ///
    /// [`BufferError::NotOpen`] si le buffer n'est pas ouvert.
    pub fn seal(&mut self) -> Result<(), BufferError> {
        self.require_open()?;
        self.state = BufferState::Sealed;
        Ok(())
    }

    fn transition(&mut self, from: BufferState, to: BufferState) -> Result<(), BufferError> {
        if self.state == from {
            self.state = to;
            Ok(())
        } else {
            Err(BufferError::InvalidTransition)
        }
    }

    /// `SEALED -> VALIDATED`, apres la detection de conflits.
    ///
    /// # Erreurs
    ///
    /// [`BufferError::InvalidTransition`] depuis un autre etat que `SEALED`.
    pub fn mark_validated(&mut self) -> Result<(), BufferError> {
        self.transition(BufferState::Sealed, BufferState::Validated)
    }

    /// `VALIDATED -> APPLIED`, apres l'application par le commit.
    ///
    /// # Erreurs
    ///
    /// [`BufferError::InvalidTransition`] depuis un autre etat que `VALIDATED`.
    pub fn mark_applied(&mut self) -> Result<(), BufferError> {
        self.transition(BufferState::Validated, BufferState::Applied)
    }

    /// Jette le buffer. Un buffer deja applique ne se jette pas : ses effets sont
    /// commites (R-511).
    ///
    /// # Erreurs
    ///
    /// [`BufferError::InvalidTransition`] si le buffer est applique ou deja jete.
    pub fn discard(&mut self, reason: DiscardReason) -> Result<(), BufferError> {
        match self.state {
            BufferState::Applied | BufferState::Discarded(_) => Err(BufferError::InvalidTransition),
            _ => {
                self.state = BufferState::Discarded(reason);
                Ok(())
            }
        }
    }

    /// Tache qui a produit le buffer.
    #[must_use]
    pub fn owner_task(&self) -> u32 {
        self.owner_task
    }

    /// Workload.
    #[must_use]
    pub fn work(&self) -> WorkId {
        self.work
    }

    /// Epoch du snapshot lu.
    #[must_use]
    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    /// Domaines lus (DM-06).
    #[must_use]
    pub fn read_set(&self) -> AccessSet {
        self.read_set
    }

    /// Regions effectivement lues, dans l'ordre de leur premiere lecture.
    #[must_use]
    pub fn read_regions(&self) -> &[Region] {
        &self.read_regions
    }

    /// Commandes, dans l'ordre de production.
    #[must_use]
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// Octets d'une plage de la zone de donnees ; `None` hors de la zone.
    #[must_use]
    pub fn blob(&self, blob: BlobRef) -> Option<&[u8]> {
        let start = usize::try_from(blob.offset).ok()?;
        let end = start.checked_add(usize::try_from(blob.len).ok()?)?;
        self.blobs.get(start..end)
    }

    /// Cle d'ordre.
    #[must_use]
    pub fn order_key(&self) -> OrderKey {
        self.order_key
    }

    /// Etat.
    #[must_use]
    pub fn state(&self) -> BufferState {
        self.state
    }

    /// Regions ecrites, une fois chacune, dans l'ordre des commandes.
    #[must_use]
    pub fn write_regions(&self) -> Vec<Region> {
        let mut regions: Vec<Region> = Vec::new();
        for command in &self.commands {
            let region = command.region();
            if !regions.contains(&region) {
                regions.push(region);
            }
        }
        regions
    }
}

/// Trie les buffers dans l'ordre total du commit : cle d'ordre, puis workload, puis
/// tache (R-461). Le resultat ne depend pas de l'ordre d'arrivee.
pub fn sort_for_commit(buffers: &mut [CommandBuffer]) {
    buffers.sort_by(|a, b| {
        a.order_key
            .cmp(&b.order_key)
            .then_with(|| a.work.cmp(&b.work))
            .then_with(|| a.owner_task.cmp(&b.owner_task))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfx_model::{BlockPos, BlockStateId, DimId, EntityKey, EntityPatch};

    fn set_block(x: i32) -> Command {
        Command::SetBlock {
            dim: DimId(0),
            pos: BlockPos { x, y: 64, z: 0 },
            state: BlockStateId(7),
            flags: 3,
        }
    }

    fn key(key: u64) -> OrderKey {
        OrderKey {
            priority: 0,
            key,
            index: 0,
        }
    }

    fn open(task: u32, order: u64) -> CommandBuffer {
        CommandBuffer::open(task, WorkId(1), Epoch(10), key(order), DEFAULT_MAX_COMMANDS)
    }

    /// T-300 : append-only, puis scelle ; rien ne change apres le scellement.
    #[test]
    fn a_sealed_buffer_never_changes() {
        let mut buffer = open(1, 0);
        buffer.push(set_block(1)).unwrap();
        buffer.note_read(Region::Any).unwrap();
        let blob = buffer.add_blob(&[1, 2, 3]).unwrap();
        buffer.seal().unwrap();
        let before = buffer.clone();

        assert_eq!(buffer.push(set_block(2)), Err(BufferError::NotOpen));
        assert_eq!(buffer.note_read(Region::Any), Err(BufferError::NotOpen));
        assert_eq!(buffer.add_blob(&[9]), Err(BufferError::NotOpen));
        assert_eq!(buffer.seal(), Err(BufferError::NotOpen));
        assert_eq!(buffer, before, "aucune modification apres SEALED");
        assert_eq!(buffer.blob(blob), Some(&[1, 2, 3][..]));
        assert_eq!(buffer.blob(BlobRef { offset: 2, len: 5 }), None);
    }

    #[test]
    fn the_state_machine_only_moves_forward() {
        let mut buffer = open(1, 0);
        assert_eq!(buffer.mark_validated(), Err(BufferError::InvalidTransition));
        buffer.seal().unwrap();
        assert_eq!(buffer.mark_applied(), Err(BufferError::InvalidTransition));
        buffer.mark_validated().unwrap();
        buffer.mark_applied().unwrap();
        assert_eq!(
            buffer.discard(DiscardReason::Rollback),
            Err(BufferError::InvalidTransition),
            "un effet commite ne se jette pas (R-511)"
        );
        let mut other = open(2, 0);
        other.discard(DiscardReason::Shadow).unwrap();
        assert_eq!(other.state(), BufferState::Discarded(DiscardReason::Shadow));
        assert_eq!(other.push(set_block(0)), Err(BufferError::NotOpen));
    }

    /// T-302 : la borne est tenue, et la depasser jette le buffer entier.
    #[test]
    fn the_command_bound_discards_the_whole_buffer() {
        let mut buffer = CommandBuffer::open(1, WorkId(1), Epoch(0), key(0), 3);
        for x in 0..3 {
            buffer.push(set_block(x)).unwrap();
        }
        assert_eq!(buffer.push(set_block(3)), Err(BufferError::BudgetExceeded));
        assert_eq!(
            buffer.state(),
            BufferState::Discarded(DiscardReason::BudgetExceeded)
        );
        assert_eq!(buffer.commands().len(), 3);
        assert_eq!(buffer.push(set_block(4)), Err(BufferError::NotOpen));
    }

    /// Melange deterministe de Fisher-Yates, pour simuler des ordres d'achevement.
    fn shuffle<T>(items: &mut [T], mut seed: u64) {
        for i in (1..items.len()).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            #[allow(clippy::cast_possible_truncation)]
            let j = (seed % (i as u64 + 1)) as usize;
            items.swap(i, j);
        }
    }

    /// T-301 : l'ordre du commit ne depend pas de l'ordre d'achevement, sur 1000
    /// ordres d'arrivee differents.
    #[test]
    fn the_commit_order_does_not_depend_on_completion_order() {
        let produced: Vec<CommandBuffer> = (0..64)
            .map(|i| {
                // Des cles en partie egales : le departage par workload puis par tache
                // doit lui aussi etre stable.
                let mut b = CommandBuffer::open(
                    u32::try_from(i).unwrap(),
                    WorkId(i % 5),
                    Epoch(3),
                    key(i % 7),
                    DEFAULT_MAX_COMMANDS,
                );
                #[allow(clippy::cast_possible_truncation)]
                b.push(set_block(i as i32)).unwrap();
                b.seal().unwrap();
                b
            })
            .collect();
        let mut reference = produced.clone();
        sort_for_commit(&mut reference);
        let expected: Vec<(u32, Vec<Command>)> = reference
            .iter()
            .map(|b| (b.owner_task(), b.commands().to_vec()))
            .collect();
        for seed in 1..=1000_u64 {
            let mut arrival = produced.clone();
            shuffle(&mut arrival, seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            sort_for_commit(&mut arrival);
            let got: Vec<(u32, Vec<Command>)> = arrival
                .iter()
                .map(|b| (b.owner_task(), b.commands().to_vec()))
                .collect();
            assert_eq!(got, expected, "ordre d'arrivee {seed}");
        }
    }

    /// A cle d'ordre egale, le workload departage avant la tache : les numeros de tache
    /// suivent l'ordre de soumission, le workload non.
    #[test]
    fn ties_are_broken_by_workload_before_task() {
        let mut first = CommandBuffer::open(9, WorkId(1), Epoch(0), key(5), DEFAULT_MAX_COMMANDS);
        let mut second = CommandBuffer::open(3, WorkId(2), Epoch(0), key(5), DEFAULT_MAX_COMMANDS);
        first.seal().unwrap();
        second.seal().unwrap();
        let mut buffers = vec![second, first];
        sort_for_commit(&mut buffers);
        let works: Vec<WorkId> = buffers.iter().map(CommandBuffer::work).collect();
        assert_eq!(works, vec![WorkId(1), WorkId(2)]);
    }

    #[test]
    fn reads_and_writes_are_recorded_once_each() {
        let mut buffer = open(1, 0);
        let entity = EntityKey {
            dim: DimId(0),
            id: 4,
        };
        buffer.note_read(Region::Entity(entity)).unwrap();
        buffer.note_read(Region::Entity(entity)).unwrap();
        assert_eq!(buffer.read_regions(), &[Region::Entity(entity)]);
        assert!(buffer
            .read_set()
            .domains
            .contains(rfx_model::Domain::EntityState));
        buffer.push(set_block(1)).unwrap();
        buffer.push(set_block(2)).unwrap();
        buffer
            .push(Command::UpdateEntity {
                entity,
                patch: EntityPatch(BlobRef { offset: 0, len: 0 }),
            })
            .unwrap();
        assert_eq!(buffer.write_regions().len(), 2, "un chunk et une entite");
    }
}
