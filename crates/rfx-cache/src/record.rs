//! C-09 : ce que la base retient d'une unite de travail.
//!
//! Cahier des charges : PARTIE 4.3 (DM-03), PARTIE 5.9 (historique borne a 32 entrees),
//! R-205. Decision : ADR-048. Maturite : `STABLE`.
//!
//! # Un sous-ensemble de DM-03
//!
//! DM-03 decrit un descripteur complet : identite, empreinte, dynamiques, mais aussi les
//! faits d'analyse (acces memoire, effets de bord, dependances, fils, determinisme) et la
//! gouvernance (cycle de vie, strategie). Les analyseurs qui produisent ces faits
//! (C-10, C-11, C-12) et le moteur de decision n'existent pas encore. Un champ qui n'a
//! aucun producteur serait une fiction : l'enregistrement ne porte donc que ce qui est
//! mesure ou connu. Chaque ajout changera [`crate::file::SCHEMA_VERSION`], et un fichier
//! d'un ancien schema sera mis de cote et le cache reconstruit (R-223).

use std::collections::VecDeque;

use rfx_model::{Fingerprint, Heat, WorkDescriptor, WorkId, WorkloadDynamics};
use serde::{Deserialize, Serialize};

/// Evenements gardes par unite de travail (PARTIE 5.9).
pub const HISTORY_CAPACITY: usize = 32;

/// Nature d'un evenement de l'historique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoryKind {
    /// La classe de cout a change.
    HeatChanged {
        /// Classe precedente.
        from: Heat,
        /// Nouvelle classe.
        to: Heat,
    },
    /// Le niveau de sonde a change (code de `ProbeLevel`).
    ProbeLevelChanged {
        /// Nouveau niveau.
        level: u8,
    },
}

/// Evenement date de l'historique d'une unite de travail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEvent {
    /// Tick ou l'evenement a eu lieu.
    pub tick: u64,
    /// Nature.
    pub kind: HistoryKind,
}

/// Historique borne : au-dela de [`HISTORY_CAPACITY`], le plus ancien s'efface.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    events: VecDeque<HistoryEvent>,
}

impl History {
    /// Ajoute un evenement, en effacant le plus ancien si l'historique est plein.
    pub fn push(&mut self, event: HistoryEvent) {
        if self.events.len() == HISTORY_CAPACITY {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    /// Evenements, du plus ancien au plus recent.
    pub fn iter(&self) -> impl Iterator<Item = &HistoryEvent> {
        self.events.iter()
    }

    /// Nombre d'evenements gardes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// `true` si l'historique est vide.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Unite de travail telle que la base la retient et la persiste.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkloadRecord {
    /// Identifiant stable (DM-01).
    pub id: WorkId,
    /// Identite : proprietaire, methode, contexte, cote.
    pub descriptor: WorkDescriptor,
    /// Empreinte de l'environnement et de l'unite de travail (DM-05).
    pub fingerprint: Fingerprint,
    /// Mesures dynamiques (DM-04).
    pub dynamics: WorkloadDynamics,
    /// Historique borne.
    pub history: History,
}

impl WorkloadRecord {
    /// Enregistrement sans mesure ni historique ; l'identifiant est calcule depuis le
    /// descripteur (`WORKID_V1`).
    #[must_use]
    pub fn new(descriptor: WorkDescriptor, fingerprint: Fingerprint) -> Self {
        Self {
            id: WorkId::compute(&descriptor),
            descriptor,
            fingerprint,
            dynamics: WorkloadDynamics::default(),
            history: History::default(),
        }
    }

    /// Estimation de la memoire occupee, en octets : la structure, ses chaines et son
    /// historique. Sert au plafond de la base (R-362) ; ce n'est pas une mesure.
    #[must_use]
    pub fn estimated_bytes(&self) -> usize {
        let d = &self.descriptor;
        size_of::<Self>()
            + d.owner_id.capacity()
            + d.class_internal_name.capacity()
            + d.method_name.capacity()
            + d.method_descriptor.capacity()
            + self.history.events.capacity() * size_of::<HistoryEvent>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_history_keeps_the_most_recent_events() {
        let mut history = History::default();
        for tick in 0..(HISTORY_CAPACITY as u64 + 10) {
            history.push(HistoryEvent {
                tick,
                kind: HistoryKind::ProbeLevelChanged { level: 1 },
            });
        }
        assert_eq!(history.len(), HISTORY_CAPACITY);
        assert_eq!(
            history.iter().next().unwrap().tick,
            10,
            "les dix plus anciens sont partis"
        );
    }

    #[test]
    fn a_new_record_carries_the_id_of_its_descriptor() {
        let descriptor = WorkDescriptor {
            owner_id: "minecraft".to_owned(),
            class_internal_name: "net/minecraft/world/entity/Mob".to_owned(),
            method_name: "tick".to_owned(),
            method_descriptor: "()V".to_owned(),
            ..WorkDescriptor::default()
        };
        let record = WorkloadRecord::new(descriptor.clone(), Fingerprint::default());
        assert_eq!(record.id, WorkId::compute(&descriptor));
        assert!(record.history.is_empty());
        assert!(record.estimated_bytes() > size_of::<WorkloadRecord>());
    }
}
