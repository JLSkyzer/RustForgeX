//! Releve tick par tick du cout de quelques unites chaudes (ADR-069, decision 3).
//!
//! Composant : C-05 (Profiler), au service de la chaine de mesure de C-15.
//! Maturite : `STABLE`.
//!
//! Les mesures glissantes du profiler (`Ewma`, histogrammes) lissent le cout d'une
//! unite : elles ne disent plus ce qu'a coute **un** tick precis. La chaine de mesure
//! d'un gain compare pourtant des ticks voisins entre eux (comparaison appariee). Ce
//! releve garde donc, pour au plus [`TRACE_SLOTS`] unites choisies hors du tick, le
//! temps mesure par sonde et le nombre de sorties chronometrees de chaque tick ou
//! l'unite a tourne.
//!
//! Le chemin du tick n'alloue jamais (INV-14) : chaque emplacement reserve sa memoire
//! au choix de l'unite, et un releve plein refuse et compte au lieu de grandir.

use rfx_model::WorkId;

/// Unites relevees au plus en meme temps.
pub const TRACE_SLOTS: usize = 4;

/// Ticks gardes par unite entre deux vidages.
///
/// A vingt ticks par seconde, 512 ticks laissent environ vingt-cinq secondes au fil
/// d'analyse pour passer vider le releve.
pub const TRACE_CAPACITY: usize = 512;

/// Cout d'une unite pendant un tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TickTrace {
    /// Tick cloture.
    pub tick: u64,
    /// Somme des durees des sorties chronometrees, en nanosecondes.
    pub timed_ns: u64,
    /// Sorties chronometrees pendant ce tick.
    pub timed_exits: u64,
}

/// Une unite relevee.
#[derive(Debug)]
struct Slot {
    work_id: WorkId,
    probe_id: u32,
    ticks: Vec<TickTrace>,
}

/// Releve borne des unites choisies.
#[derive(Debug, Default)]
pub struct TickTracer {
    slots: Vec<Slot>,
    /// Ticks perdus parce qu'un releve etait plein.
    dropped: u64,
}

impl TickTracer {
    /// Choisit les unites relevees, au plus [`TRACE_SLOTS`]. Une unite deja relevee
    /// garde ses ticks non vides ; une unite qui sort du choix perd les siens.
    ///
    /// A appeler hors du tick : c'est ici, et seulement ici, que la memoire est
    /// reservee.
    pub fn select(&mut self, chosen: &[(WorkId, u32)]) {
        let chosen = &chosen[..chosen.len().min(TRACE_SLOTS)];
        self.slots
            .retain(|slot| chosen.iter().any(|(id, _)| *id == slot.work_id));
        for &(work_id, probe_id) in chosen {
            match self.slots.iter_mut().find(|slot| slot.work_id == work_id) {
                Some(slot) => slot.probe_id = probe_id,
                None => self.slots.push(Slot {
                    work_id,
                    probe_id,
                    ticks: Vec::with_capacity(TRACE_CAPACITY),
                }),
            }
        }
    }

    /// Identifiants de sonde des unites relevees, avec leur `WorkId`.
    pub fn traced(&self) -> impl Iterator<Item = (WorkId, u32)> + '_ {
        self.slots.iter().map(|slot| (slot.work_id, slot.probe_id))
    }

    /// Ajoute le tick d'une unite relevee. Sans allocation : un releve plein refuse le
    /// tick et le compte.
    pub(crate) fn push(&mut self, probe_id: u32, work_id: WorkId, trace: TickTrace) {
        let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.probe_id == probe_id && slot.work_id == work_id)
        else {
            return;
        };
        if slot.ticks.len() < TRACE_CAPACITY {
            slot.ticks.push(trace);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    /// Rend les ticks releves de chaque unite, puis les oublie. La memoire reservee
    /// reste a l'emplacement.
    pub fn drain(&mut self, mut f: impl FnMut(WorkId, &[TickTrace])) {
        for slot in &mut self.slots {
            if !slot.ticks.is_empty() {
                f(slot.work_id, &slot.ticks);
                slot.ticks.clear();
            }
        }
    }

    /// Ticks perdus parce qu'un releve etait plein.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(tick: u64) -> TickTrace {
        TickTrace {
            tick,
            timed_ns: tick * 10,
            timed_exits: 1,
        }
    }

    fn drained(tracer: &mut TickTracer) -> Vec<(WorkId, Vec<TickTrace>)> {
        let mut out = Vec::new();
        tracer.drain(|id, ticks| out.push((id, ticks.to_vec())));
        out
    }

    #[test]
    fn only_selected_units_are_traced_and_a_drain_empties_them() {
        let mut tracer = TickTracer::default();
        tracer.select(&[(WorkId(1), 0), (WorkId(2), 1)]);
        tracer.push(0, WorkId(1), trace(5));
        tracer.push(1, WorkId(2), trace(5));
        tracer.push(2, WorkId(3), trace(5));
        // Identifiant de sonde reutilise par une autre unite apres eviction : ignore.
        tracer.push(0, WorkId(9), trace(6));
        let out = drained(&mut tracer);
        assert_eq!(
            out,
            vec![(WorkId(1), vec![trace(5)]), (WorkId(2), vec![trace(5)])]
        );
        assert!(drained(&mut tracer).is_empty(), "deja vide");
    }

    #[test]
    fn a_full_trace_refuses_without_growing() {
        let mut tracer = TickTracer::default();
        tracer.select(&[(WorkId(1), 0)]);
        for tick in 0..(TRACE_CAPACITY as u64 + 3) {
            tracer.push(0, WorkId(1), trace(tick));
        }
        assert_eq!(tracer.dropped(), 3);
        let out = drained(&mut tracer);
        assert_eq!(out[0].1.len(), TRACE_CAPACITY);
        assert_eq!(out[0].1.last(), Some(&trace(TRACE_CAPACITY as u64 - 1)));
    }

    #[test]
    fn reselection_keeps_continuing_units_and_drops_the_others() {
        let mut tracer = TickTracer::default();
        tracer.select(&[(WorkId(1), 0), (WorkId(2), 1)]);
        tracer.push(0, WorkId(1), trace(1));
        tracer.push(1, WorkId(2), trace(1));
        tracer.select(&[(WorkId(1), 0), (WorkId(3), 2)]);
        let ids: Vec<WorkId> = tracer.traced().map(|(id, _)| id).collect();
        assert_eq!(ids, vec![WorkId(1), WorkId(3)]);
        assert_eq!(drained(&mut tracer), vec![(WorkId(1), vec![trace(1)])]);
    }

    #[test]
    fn selection_is_capped() {
        let mut tracer = TickTracer::default();
        let many: Vec<(WorkId, u32)> = (0..10).map(|i| (WorkId(i), i as u32)).collect();
        tracer.select(&many);
        assert_eq!(tracer.traced().count(), TRACE_SLOTS);
    }
}
