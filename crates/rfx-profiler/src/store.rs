//! Table des unites de travail observees.
//!
//! Composant : C-05. Cahier des charges : PARTIE 5.5. Exigences : R-321 (eviction
//! LRU des `COLD` au-dela du plafond, jamais de croissance non bornee).
//! Test : T-143. Maturite : `STABLE`.
//!
//! La version persistante et indexee de cette table est C-09, au jalon M2. Ici, tout
//! tient en memoire : c'est ce dont C-05 a besoin pour agreger, rien de plus.
//!
//! # Ce qu'une eviction peut et ne peut pas faire
//!
//! Un identifiant de sonde est grave dans le bytecode des methodes instrumentees : il
//! ne peut jamais etre reattribue tant que la classe est chargee. Evincer une unite de
//! travail signifie donc **eteindre sa sonde et oublier ses mesures**, pas liberer son
//! identifiant. C'est suffisant : une sonde eteinte ne coute qu'un appel statique et
//! une lecture de tableau (ADR-016).

use std::collections::HashMap;

use rfx_model::{Ewma, Heat, WorkId, WorkloadDynamics};

use crate::level::ProbeLevel;

/// Une unite de travail suivie.
#[derive(Debug, Clone)]
pub struct WorkloadEntry {
    /// Identifiant stable (DM-01).
    pub work_id: WorkId,
    /// Mesures dynamiques (DM-04).
    pub dynamics: WorkloadDynamics,
    /// Niveau de sonde courant.
    pub level: ProbeLevel,
    /// Appels observes pendant le tick courant, remis a zero a chaque cloture.
    pub calls_this_tick: u64,
    /// Temps attribue par echantillonnage pendant le tick courant, en nanosecondes.
    pub sampled_ns_this_tick: u64,
    /// Temps attribue par echantillonnage, par tick, en nanosecondes.
    ///
    /// C'est la seule mesure disponible quand aucune sonde n'est posee (R-322).
    pub sampled_ns_per_tick: Ewma,
    /// Echantillons de pile ayant designe cette unite depuis le demarrage.
    pub sampled_hits: u64,
    /// Octets alloues observes pendant le tick courant.
    pub alloc_bytes_this_tick: u64,
    /// `true` tant que l'entree porte des mesures.
    pub live: bool,
}

impl WorkloadEntry {
    /// Cree une entree vide pour une unite de travail.
    #[must_use]
    fn new(work_id: WorkId) -> Self {
        Self {
            work_id,
            dynamics: WorkloadDynamics::default(),
            // Eteinte a la creation : c'est le profiler qui arme la sonde, sous son
            // propre plafond. Une entree qui s'armerait elle-meme allumerait une sonde
            // alors que le profilage est peut-etre arrete.
            level: ProbeLevel::Off,
            calls_this_tick: 0,
            sampled_ns_this_tick: 0,
            sampled_ns_per_tick: Ewma::default(),
            sampled_hits: 0,
            alloc_bytes_this_tick: 0,
            live: true,
        }
    }

    /// Cout estime par tick, en nanosecondes.
    ///
    /// La mesure par sonde prime quand elle existe : elle est directe. En son absence,
    /// le temps attribue par echantillonnage prend le relais, ce qui permet au profiler
    /// de classer les unites de travail meme sans aucune sonde posee (R-322).
    #[must_use]
    pub fn cost_ns_per_tick(&self) -> u64 {
        let probed = self.dynamics.ns_per_tick();
        if probed > 0 {
            probed
        } else {
            self.sampled_ns_per_tick.value().max(0.0) as u64
        }
    }

    /// Eteint l'entree et oublie ses mesures, sans liberer son identifiant.
    fn evict(&mut self) {
        self.dynamics = WorkloadDynamics::default();
        self.level = ProbeLevel::Off;
        self.calls_this_tick = 0;
        self.sampled_ns_this_tick = 0;
        self.sampled_ns_per_tick = Ewma::default();
        self.sampled_hits = 0;
        self.alloc_bytes_this_tick = 0;
        self.live = false;
    }
}

/// Table des unites de travail, indexee par identifiant de sonde.
#[derive(Debug)]
pub struct WorkloadStore {
    /// Index dense : la position vaut l'identifiant de sonde.
    entries: Vec<WorkloadEntry>,
    by_work_id: HashMap<WorkId, usize>,
    max_workloads: usize,
    evictions: u64,
    collisions: u64,
}

impl WorkloadStore {
    /// Construit une table bornee a `max_workloads` unites vivantes.
    #[must_use]
    pub fn new(max_workloads: usize) -> Self {
        Self {
            entries: Vec::new(),
            by_work_id: HashMap::new(),
            max_workloads: max_workloads.max(1),
            evictions: 0,
            collisions: 0,
        }
    }

    /// Enregistre une unite de travail et rend son identifiant de sonde.
    ///
    /// Un meme `WorkId` rend toujours le meme identifiant : c'est ce qui rend les
    /// mesures comparables d'un lancement a l'autre (R-200).
    ///
    /// # Erreurs
    ///
    /// Renvoie `None` si le plafond est atteint et qu'aucune unite `COLD` ne peut
    /// etre evincee pour faire place.
    pub fn register(&mut self, work_id: WorkId) -> Option<u32> {
        if let Some(&index) = self.by_work_id.get(&work_id) {
            // L'entree existe : si elle avait ete evincee, elle reprend du service.
            let entry = &mut self.entries[index];
            if !entry.live {
                entry.live = true;
            }
            return u32::try_from(index).ok();
        }

        if self.live_count() >= self.max_workloads && self.evict_coldest().is_none() {
            return None;
        }

        let index = self.entries.len();
        self.entries.push(WorkloadEntry::new(work_id));
        self.by_work_id.insert(work_id, index);
        u32::try_from(index).ok()
    }

    /// Signale une collision d'identifiant (R-202, `E-2101`).
    ///
    /// Deux descripteurs differents ayant produit le meme `WorkId`, les deux unites
    /// sont eteintes : mesurer l'une pour l'autre serait pire que ne rien mesurer.
    pub fn report_collision(&mut self, work_id: WorkId) {
        self.collisions = self.collisions.saturating_add(1);
        if let Some(&index) = self.by_work_id.get(&work_id) {
            self.entries[index].evict();
        }
    }

    /// Entree d'un identifiant de sonde.
    #[must_use]
    pub fn get(&self, probe_id: u32) -> Option<&WorkloadEntry> {
        self.entries.get(probe_id as usize).filter(|e| e.live)
    }

    /// Entree modifiable d'un identifiant de sonde.
    pub fn get_mut(&mut self, probe_id: u32) -> Option<&mut WorkloadEntry> {
        self.entries.get_mut(probe_id as usize).filter(|e| e.live)
    }

    /// Entree d'un `WorkId`.
    #[must_use]
    pub fn by_work_id(&self, work_id: WorkId) -> Option<&WorkloadEntry> {
        self.by_work_id
            .get(&work_id)
            .and_then(|&index| self.entries.get(index))
            .filter(|e| e.live)
    }

    /// Parcourt les unites vivantes.
    pub fn iter(&self) -> impl Iterator<Item = &WorkloadEntry> {
        self.entries.iter().filter(|e| e.live)
    }

    /// Parcourt les unites vivantes en modification.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut WorkloadEntry> {
        self.entries.iter_mut().filter(|e| e.live)
    }

    /// Nombre d'unites vivantes.
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.entries.iter().filter(|e| e.live).count()
    }

    /// Nombre d'identifiants de sonde attribues, evinces compris.
    #[must_use]
    pub fn allocated_count(&self) -> usize {
        self.entries.len()
    }

    /// Unites evincees depuis le demarrage (`rfx.db.evictions`).
    #[must_use]
    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    /// Collisions d'identifiant signalees (R-202).
    #[must_use]
    pub fn collisions(&self) -> u64 {
        self.collisions
    }

    /// Niveaux de sonde, indexes par identifiant, tels que Java doit les appliquer.
    #[must_use]
    pub fn levels(&self) -> Vec<u8> {
        self.entries.iter().map(|e| e.level.code()).collect()
    }

    /// Les `n` unites les plus couteuses, de la plus chere a la moins chere.
    ///
    /// Sert `/rfx top`. Les unites eteintes sont exclues : elles n'ont pas de mesure.
    #[must_use]
    pub fn hottest(&self, n: usize) -> Vec<&WorkloadEntry> {
        let mut live: Vec<&WorkloadEntry> = self.entries.iter().filter(|e| e.live).collect();
        live.sort_by(|a, b| {
            b.cost_ns_per_tick()
                .cmp(&a.cost_ns_per_tick())
                .then_with(|| a.work_id.cmp(&b.work_id))
        });
        live.truncate(n);
        live
    }

    /// Evince l'unite `COLD` la moins recemment vue.
    ///
    /// Ne touche jamais a une unite plus chaude que `COLD` : evincer ce qui coute cher
    /// reviendrait a se priver de la mesure qui sert justement a decider (R-321).
    ///
    /// Renvoie l'identifiant de sonde libere, ou `None` si aucune unite n'est
    /// evincable.
    pub fn evict_coldest(&mut self) -> Option<u32> {
        let victim = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.live && e.dynamics.heat == Heat::Cold)
            .min_by_key(|(_, e)| e.dynamics.last_seen_tick)
            .map(|(index, _)| index)?;

        self.entries[victim].evict();
        self.evictions = self.evictions.saturating_add(1);
        u32::try_from(victim).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work_id(n: u64) -> WorkId {
        WorkId(n)
    }

    #[test]
    fn registering_the_same_work_id_yields_the_same_probe_id() {
        let mut store = WorkloadStore::new(10);
        let first = store.register(work_id(1)).expect("premier");
        let again = store.register(work_id(1)).expect("second");
        assert_eq!(first, again);
        assert_eq!(store.live_count(), 1);
    }

    #[test]
    fn probe_ids_are_dense_and_ordered() {
        let mut store = WorkloadStore::new(10);
        assert_eq!(store.register(work_id(1)), Some(0));
        assert_eq!(store.register(work_id(2)), Some(1));
        assert_eq!(store.register(work_id(3)), Some(2));
    }

    /// T-143 / R-321 : au-dela du plafond, une unite `COLD` est evincee.
    #[test]
    fn the_coldest_workload_is_evicted_first() {
        let mut store = WorkloadStore::new(2);
        let a = store.register(work_id(1)).expect("a");
        let b = store.register(work_id(2)).expect("b");

        // `a` a ete vu plus recemment que `b`.
        store.get_mut(b).expect("b").dynamics.last_seen_tick = 10;
        store.get_mut(a).expect("a").dynamics.last_seen_tick = 20;

        let c = store.register(work_id(3)).expect("c");

        assert_eq!(store.evictions(), 1);
        assert!(store.get(b).is_none(), "la plus ancienne est evincee");
        assert!(store.get(a).is_some(), "la plus recente reste");
        assert!(store.get(c).is_some());
    }

    #[test]
    fn a_hot_workload_is_never_evicted() {
        let mut store = WorkloadStore::new(1);
        let hot = store.register(work_id(1)).expect("hot");
        store.get_mut(hot).expect("hot").dynamics.heat = Heat::Critical;

        assert_eq!(
            store.register(work_id(2)),
            None,
            "aucune place, rien d'evincable"
        );
        assert!(store.get(hot).is_some(), "l'unite couteuse est preservee");
        assert_eq!(store.evictions(), 0);
    }

    #[test]
    fn an_evicted_probe_id_is_never_reused() {
        let mut store = WorkloadStore::new(1);
        let first = store.register(work_id(1)).expect("premier");
        store
            .get_mut(first)
            .expect("premier")
            .dynamics
            .last_seen_tick = 1;

        let second = store.register(work_id(2)).expect("second");

        assert_ne!(
            first, second,
            "un identifiant grave dans le bytecode ne se recycle pas"
        );
        assert_eq!(store.allocated_count(), 2);
        assert_eq!(store.live_count(), 1);
    }

    #[test]
    fn an_evicted_workload_comes_back_if_seen_again() {
        let mut store = WorkloadStore::new(1);
        let first = store.register(work_id(1)).expect("premier");
        store.register(work_id(2)).expect("second");
        assert!(store.get(first).is_none(), "evincee");

        let again = store.register(work_id(1)).expect("retour");
        assert_eq!(again, first, "le meme identifiant lui est rendu");
        assert!(store.get(first).is_some());
    }

    #[test]
    fn eviction_turns_the_probe_off_without_freeing_it() {
        let mut store = WorkloadStore::new(1);
        let first = store.register(work_id(1)).expect("premier");
        store.register(work_id(2)).expect("second");

        let levels = store.levels();
        assert_eq!(levels.len(), 2, "les deux identifiants restent declares");
        assert_eq!(
            levels[first as usize],
            ProbeLevel::Off.code(),
            "la sonde evincee est eteinte, pas supprimee"
        );
    }

    /// R-202 : une collision eteint l'unite concernee.
    #[test]
    fn a_collision_disables_the_workload() {
        let mut store = WorkloadStore::new(10);
        let probe = store.register(work_id(1)).expect("unite");

        store.report_collision(work_id(1));

        assert_eq!(store.collisions(), 1);
        assert!(
            store.get(probe).is_none(),
            "mesurer l'une pour l'autre serait pire"
        );
    }

    #[test]
    fn the_hottest_workloads_come_first() {
        let mut store = WorkloadStore::new(10);
        for (index, cost) in [500_u64, 5_000, 50].iter().enumerate() {
            let probe = store.register(work_id(index as u64 + 1)).expect("unite");
            let entry = store.get_mut(probe).expect("unite");
            for _ in 0..64 {
                entry.dynamics.cpu_ns.record(*cost);
            }
            entry.dynamics.calls_per_tick.update(1000.0);
            entry.dynamics.total_calls = 64;
            entry.dynamics.refresh();
        }

        let top = store.hottest(2);
        assert_eq!(top.len(), 2);
        assert!(top[0].dynamics.ns_per_tick() >= top[1].dynamics.ns_per_tick());
        assert_eq!(top[0].work_id, work_id(2), "la plus couteuse d'abord");
    }

    #[test]
    fn asking_for_more_than_available_yields_what_exists() {
        let mut store = WorkloadStore::new(10);
        store.register(work_id(1));
        assert_eq!(store.hottest(50).len(), 1);
    }
}
