//! C-09 : la base des unites de travail, en memoire.
//!
//! Cahier des charges : PARTIE 5.9 (IF-12), R-362 (base bornee). Tests : T-183.
//! Decision : ADR-048. Maturite : `STABLE`.
//!
//! # Structure
//!
//! Les enregistrements vivent dans des emplacements stables : un indice ([`Idx`]) reste
//! valide tant que son enregistrement n'est pas retire, et un emplacement libere est
//! reutilise. Deux index l'accompagnent : par identifiant, et par proprietaire.
//!
//! Chaque enregistrement est dans sa propre allocation (`Box`) : un emplacement coute
//! 8 octets, pas la taille d'un enregistrement (environ 1,5 Kio, dont deux histogrammes).
//! Sans cela, la reserve que garde un `Vec` en doublant, et les emplacements liberes par
//! l'eviction, retiendraient chacun un enregistrement entier : mesure sur 20 000 unites,
//! la base depassait ainsi les 64 Mio de l'acceptance (`tests/acceptance_memory.rs`).
//!
//! # Ecarts a la fiche
//!
//! - `hot()` rend un `Vec`, calcule a la demande, et non une tranche maintenue : la
//!   chaleur change dans les dynamiques, que l'appelant modifie par `get_mut`, et un
//!   index tenu a jour par chaque appelant serait faux au premier oubli.
//! - L'eviction prend un nombre d'entrees et un nombre d'octets estimes, plutot que le
//!   `MemoryBudget` de C-31 : la base n'est pas encore branchee au budget du runtime.

use std::collections::HashMap;

use rfx_model::{Fingerprint, Heat, WorkId};

use crate::record::{HistoryEvent, WorkloadRecord};

/// Indice stable d'un enregistrement.
pub type Idx = usize;

/// Base des unites de travail.
#[derive(Debug, Default)]
pub struct WorkloadDb {
    slots: Vec<Option<Box<WorkloadRecord>>>,
    free: Vec<Idx>,
    by_id: HashMap<WorkId, Idx>,
    by_owner: HashMap<String, Vec<Idx>>,
    evictions: u64,
}

impl WorkloadDb {
    /// Base vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Base construite depuis des enregistrements, par exemple relus du disque.
    #[must_use]
    pub fn from_records(records: impl IntoIterator<Item = WorkloadRecord>) -> Self {
        let mut db = Self::new();
        for record in records {
            db.upsert(record);
        }
        db
    }

    /// Insere ou remplace un enregistrement.
    ///
    /// L'enregistrement fourni remplace l'ancien tel quel. Pour une unite de travail
    /// (re)decouverte, dont les mesures connues doivent etre gardees si son code n'a pas
    /// change, utiliser [`Self::observe`].
    pub fn upsert(&mut self, record: WorkloadRecord) -> Idx {
        if let Some(&idx) = self.by_id.get(&record.id) {
            self.unindex_owner(idx);
            let owner = record.descriptor.owner_id.clone();
            self.slots[idx] = Some(Box::new(record));
            self.by_owner.entry(owner).or_default().push(idx);
            return idx;
        }
        let idx = match self.free.pop() {
            Some(idx) => idx,
            None => {
                self.slots.push(None);
                self.slots.len() - 1
            }
        };
        self.by_id.insert(record.id, idx);
        self.by_owner
            .entry(record.descriptor.owner_id.clone())
            .or_default()
            .push(idx);
        self.slots[idx] = Some(Box::new(record));
        idx
    }

    /// Insere l'unite de travail si elle est inconnue, ou si son code a change depuis
    /// l'enregistrement connu ; garde sinon l'enregistrement existant et ses mesures.
    ///
    /// C'est l'entree a utiliser quand une unite de travail est (re)decouverte : le
    /// filtre de R-360 s'y applique, pour les entrees dont l'empreinte n'etait pas encore
    /// connue au chargement.
    pub fn observe(&mut self, record: WorkloadRecord) -> Idx {
        match self.get(record.id) {
            Some(known) if known.fingerprint.same_workload(&record.fingerprint) => {
                self.by_id[&record.id]
            }
            _ => self.upsert(record),
        }
    }

    fn unindex_owner(&mut self, idx: Idx) {
        let Some(record) = self.slots[idx].as_ref() else {
            return;
        };
        if let Some(list) = self.by_owner.get_mut(&record.descriptor.owner_id) {
            list.retain(|&other| other != idx);
            if list.is_empty() {
                let owner = record.descriptor.owner_id.clone();
                self.by_owner.remove(&owner);
            }
        }
    }

    /// Enregistrement d'une unite de travail.
    #[must_use]
    pub fn get(&self, id: WorkId) -> Option<&WorkloadRecord> {
        self.by_id.get(&id).and_then(|&idx| self.at(idx))
    }

    /// Enregistrement d'une unite de travail, modifiable. L'identite et l'empreinte ne
    /// doivent pas y etre changees : c'est le role de [`Self::upsert`].
    pub fn get_mut(&mut self, id: WorkId) -> Option<&mut WorkloadRecord> {
        let idx = *self.by_id.get(&id)?;
        self.slots[idx].as_deref_mut()
    }

    /// Enregistrement a un indice.
    #[must_use]
    pub fn at(&self, idx: Idx) -> Option<&WorkloadRecord> {
        self.slots.get(idx).and_then(Option::as_deref)
    }

    /// Indices des unites de travail d'un proprietaire.
    #[must_use]
    pub fn by_owner(&self, owner: &str) -> &[Idx] {
        self.by_owner.get(owner).map_or(&[], Vec::as_slice)
    }

    /// Indices des unites de travail `HOT` ou `CRITICAL`.
    #[must_use]
    pub fn hot(&self) -> Vec<Idx> {
        self.iter_indexed()
            .filter(|(_, record)| record.dynamics.heat >= Heat::Hot)
            .map(|(idx, _)| idx)
            .collect()
    }

    /// Ajoute un evenement a l'historique d'une unite de travail.
    ///
    /// Renvoie `false` si elle est inconnue.
    pub fn record(&mut self, id: WorkId, event: HistoryEvent) -> bool {
        match self.get_mut(id) {
            Some(record) => {
                record.history.push(event);
                true
            }
            None => false,
        }
    }

    /// Retire une unite de travail.
    pub fn remove(&mut self, id: WorkId) -> Option<WorkloadRecord> {
        let idx = self.by_id.remove(&id)?;
        self.unindex_owner(idx);
        self.free.push(idx);
        self.slots[idx].take().map(|record| *record)
    }

    /// Nombre d'enregistrements (`rfx.db.entries`).
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// `true` si la base est vide.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Enregistrements, par indice croissant.
    pub fn iter(&self) -> impl Iterator<Item = &WorkloadRecord> {
        self.slots.iter().filter_map(Option::as_deref)
    }

    fn iter_indexed(&self) -> impl Iterator<Item = (Idx, &WorkloadRecord)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| slot.as_deref().map(|record| (idx, record)))
    }

    /// Memoire estimee de la base, en octets : les enregistrements, les emplacements, et
    /// les deux index.
    ///
    /// Les tables de hachage sont estimees d'apres l'implementation actuelle de la
    /// bibliotheque standard (une case par paire et un octet de controle, pour une
    /// capacite utile des 7/8 des cases) ; ce n'est pas un contrat de `std`.
    /// `tests/acceptance_memory.rs` confronte cette estimation a l'allocateur.
    #[must_use]
    pub fn estimated_bytes(&self) -> usize {
        let records: usize = self.iter().map(WorkloadRecord::estimated_bytes).sum();
        let owners: usize = self
            .by_owner
            .iter()
            .map(|(owner, list)| owner.capacity() + list.capacity() * size_of::<Idx>())
            .sum();
        records
            + owners
            + self.slots.capacity() * size_of::<Option<Box<WorkloadRecord>>>()
            + self.free.capacity() * size_of::<Idx>()
            + table_bytes::<(WorkId, Idx)>(self.by_id.capacity())
            + table_bytes::<(String, Vec<Idx>)>(self.by_owner.capacity())
    }

    /// Unites de travail evincees depuis la creation (`rfx.db.evictions`).
    #[must_use]
    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    /// Evince jusqu'a tenir dans `max_entries` enregistrements et `max_bytes` octets
    /// estimes (R-362) : les plus froides d'abord, et a chaleur egale, les moins
    /// recemment vues.
    ///
    /// Les octets sont ceux de [`Self::estimated_bytes`], structure comprise. Les
    /// emplacements et les index gardent leur capacite apres une eviction : un plafond
    /// inferieur a leur taille viderait la base sans l'atteindre.
    ///
    /// Renvoie le nombre d'unites de travail evincees.
    pub fn evict(&mut self, max_entries: usize, max_bytes: usize) -> usize {
        let mut bytes = self.estimated_bytes();
        if self.len() <= max_entries && bytes <= max_bytes {
            return 0;
        }
        let mut order: Vec<(Heat, u64, WorkId, usize)> = self
            .iter()
            .map(|record| {
                (
                    record.dynamics.heat,
                    record.dynamics.last_seen_tick,
                    record.id,
                    record.estimated_bytes(),
                )
            })
            .collect();
        order.sort_unstable_by_key(|&(heat, seen, id, _)| (heat, seen, id));
        let mut evicted = 0;
        for (_, _, id, size) in order {
            if self.len() <= max_entries && bytes <= max_bytes {
                break;
            }
            self.remove(id);
            bytes = bytes.saturating_sub(size);
            evicted += 1;
        }
        self.evictions += evicted as u64;
        evicted
    }

    /// Copie de tous les enregistrements, pour une persistance hors du fil appelant.
    #[must_use]
    pub fn snapshot(&self) -> Vec<WorkloadRecord> {
        self.iter().cloned().collect()
    }

    /// Retire les enregistrements dont l'empreinte d'unite de travail differe de celle
    /// que `current` connait ; garde ceux dont l'empreinte courante est inconnue.
    ///
    /// Renvoie le nombre d'enregistrements retires.
    pub fn retain_matching(&mut self, current: impl Fn(WorkId) -> Option<Fingerprint>) -> usize {
        let stale: Vec<WorkId> = self
            .iter()
            .filter(|record| {
                current(record.id).is_some_and(|now| !now.same_workload(&record.fingerprint))
            })
            .map(|record| record.id)
            .collect();
        for id in &stale {
            self.remove(*id);
        }
        stale.len()
    }
}

/// Memoire estimee d'une table de hachage de `capacity` entrees utiles.
fn table_bytes<T>(capacity: usize) -> usize {
    if capacity == 0 {
        return 0;
    }
    let buckets = (capacity * 8 / 7).next_power_of_two();
    buckets * (size_of::<T>() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::HistoryKind;
    use rfx_model::WorkDescriptor;

    fn record(owner: &str, method: &str, heat: Heat, seen: u64) -> WorkloadRecord {
        let descriptor = WorkDescriptor {
            owner_id: owner.to_owned(),
            class_internal_name: "net/example/Thing".to_owned(),
            method_name: method.to_owned(),
            method_descriptor: "()V".to_owned(),
            ..WorkDescriptor::default()
        };
        let mut record = WorkloadRecord::new(descriptor, Fingerprint::default());
        record.dynamics.heat = heat;
        record.dynamics.last_seen_tick = seen;
        record
    }

    #[test]
    fn records_are_indexed_by_id_and_owner() {
        let mut db = WorkloadDb::new();
        let a = db.upsert(record("alpha", "tick", Heat::Hot, 1));
        let b = db.upsert(record("alpha", "move", Heat::Cold, 2));
        let c = db.upsert(record("beta", "tick", Heat::Critical, 3));

        assert_eq!(db.len(), 3);
        assert_eq!(db.by_owner("alpha"), &[a, b]);
        assert_eq!(db.by_owner("beta"), &[c]);
        assert_eq!(db.by_owner("gamma"), &[] as &[Idx]);
        assert_eq!(db.hot(), vec![a, c]);
    }

    #[test]
    fn a_removed_slot_is_reused_and_indices_stay_stable() {
        let mut db = WorkloadDb::new();
        let first = record("alpha", "tick", Heat::Cold, 1);
        let first_id = first.id;
        let a = db.upsert(first);
        let b = db.upsert(record("alpha", "move", Heat::Cold, 2));
        db.remove(first_id);
        assert!(db.at(a).is_none());
        assert_eq!(db.by_owner("alpha"), &[b]);
        let c = db.upsert(record("beta", "tick", Heat::Cold, 3));
        assert_eq!(c, a, "emplacement reutilise");
        assert!(db.at(b).is_some(), "les autres indices n'ont pas bouge");
    }

    #[test]
    fn the_history_is_recorded_per_workload() {
        let mut db = WorkloadDb::new();
        let known = record("alpha", "tick", Heat::Cold, 1);
        let id = known.id;
        db.upsert(known);
        let event = HistoryEvent {
            tick: 7,
            kind: HistoryKind::HeatChanged {
                from: Heat::Cold,
                to: Heat::Warm,
            },
        };
        assert!(db.record(id, event));
        assert!(!db.record(WorkId(1), event), "inconnue");
        assert_eq!(db.get(id).unwrap().history.len(), 1);
    }

    /// R-360, forme paresseuse : une unite de travail redecouverte avec un autre
    /// bytecode perd ses mesures ; avec le meme, elle les garde.
    #[test]
    fn observing_a_changed_workload_drops_its_measurements() {
        let mut db = WorkloadDb::new();
        let mut known = record("alpha", "tick", Heat::Hot, 50);
        known.fingerprint.bytecode_hash = 1;
        let id = known.id;
        db.upsert(known.clone());

        let same = WorkloadRecord::new(known.descriptor.clone(), known.fingerprint);
        db.observe(same);
        assert_eq!(
            db.get(id).unwrap().dynamics.heat,
            Heat::Hot,
            "mesures gardees"
        );

        let mut changed = WorkloadRecord::new(known.descriptor.clone(), known.fingerprint);
        changed.fingerprint.bytecode_hash = 2;
        db.observe(changed);
        assert_eq!(
            db.get(id).unwrap().dynamics.heat,
            Heat::Cold,
            "mesures perdues"
        );
        assert_eq!(db.len(), 1);
    }

    /// T-183 : sous budget, la base evince les plus froides, puis les moins recemment
    /// vues, et jamais plus que necessaire.
    #[test]
    fn t183_eviction_under_budget_removes_the_coldest_first() {
        let mut db = WorkloadDb::new();
        let hot = record("alpha", "hot", Heat::Hot, 1);
        let warm = record("alpha", "warm", Heat::Warm, 1);
        let old_cold = record("alpha", "old", Heat::Cold, 1);
        let recent_cold = record("alpha", "recent", Heat::Cold, 99);
        let ids = [hot.id, warm.id, old_cold.id, recent_cold.id];
        for r in [hot, warm, old_cold, recent_cold] {
            db.upsert(r);
        }

        assert_eq!(db.evict(10, usize::MAX), 0, "deja sous le budget");
        assert_eq!(db.evict(2, usize::MAX), 2);
        assert!(
            db.get(ids[2]).is_none() && db.get(ids[3]).is_none(),
            "les deux froides"
        );
        assert!(db.get(ids[0]).is_some() && db.get(ids[1]).is_some());

        let just_under = db.estimated_bytes() - 1;
        assert_eq!(
            db.evict(10, just_under),
            1,
            "le budget en octets compte aussi"
        );
        assert!(db.get(ids[0]).is_some(), "la chaude reste");
        assert_eq!(db.evictions(), 3);
    }

    #[test]
    fn retain_matching_drops_only_known_changes() {
        let mut db = WorkloadDb::new();
        let changed = record("alpha", "changed", Heat::Cold, 1);
        let unknown = record("alpha", "unknown", Heat::Cold, 1);
        let same = record("alpha", "same", Heat::Cold, 1);
        let (changed_id, unknown_id, same_id) = (changed.id, unknown.id, same.id);
        for r in [changed, unknown, same] {
            db.upsert(r);
        }
        let removed = db.retain_matching(|id| {
            if id == changed_id {
                Some(Fingerprint {
                    bytecode_hash: 9,
                    ..Fingerprint::default()
                })
            } else if id == same_id {
                Some(Fingerprint::default())
            } else {
                None
            }
        });
        assert_eq!(removed, 1);
        assert!(db.get(changed_id).is_none());
        assert!(
            db.get(unknown_id).is_some(),
            "inconnue : gardee, sera verifiee a l'observation"
        );
        assert!(db.get(same_id).is_some());
    }
}
