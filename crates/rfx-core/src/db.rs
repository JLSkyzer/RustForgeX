//! C-09 cote runtime : ouvrir la base des unites de travail, lui confier ce que le
//! profileur decouvre et mesure, la fermer a l'arret.
//!
//! Cahier des charges : PARTIE 5.9 (R-360 a R-362), PARTIE 5.46 (R-690), DM-05.
//! Decision : ADR-049. Maturite : `STABLE`.
//!
//! # Rien de lourd sous le verrou
//!
//! Tout appel au runtime passe par le verrou que le tick partage. La base vit donc sur
//! son propre fil (`RFX-IO`) ; ici, on ne fait que lui confier des lots :
//!
//! - les descripteurs des unites de travail enregistrees depuis le lot precedent ;
//! - les dynamiques que le profileur a modifiees depuis le lot precedent, collectees par
//!   tranches de [`SYNC_SLICE`] entrees. L'appelant rend le verrou entre deux tranches :
//!   le tick n'attend jamais plus d'une tranche, quelle que soit la taille de la base.
//!
//! # Avant l'ouverture
//!
//! Java ouvre la base quand il a calcule l'environnement — sur un fil a lui, car hacher
//! les jars prend du temps. Les unites enregistrees avant sont gardees en attente, au
//! plus `max_workloads`, puis confiees au premier lot.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rfx_cache::{
    AnalysisUpdate, DbConfig, DbService, DynamicsUpdate, Executor, StdFileSystem, SyncBatch,
    WorkloadRecord,
};
use rfx_model::{
    DbOpen, DbStatus, Fingerprint, HardwareClass, PersistedAnalysis, RuntimeConfig, WorkDescriptor,
    WorkId,
};
use rfx_profiler::WorkloadStore;
use xxhash_rust::xxh3::xxh3_64;

/// Nom du fichier de la base, dans le repertoire fourni par Java (PARTIE 4.18).
pub const FILE_NAME: &str = "workload.db";

/// Entrees du profileur parcourues par tranche de collecte.
pub const SYNC_SLICE: u32 = 512;

/// Attente au plus de l'ecriture finale a l'arret.
pub const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// Empreinte de l'environnement courant (DM-05, R-360).
///
/// Les chaines venues de Java sont hachees ici ; la liste des mods est triee avant,
/// pour que l'ordre de leur chargement ne change rien. La configuration entiere entre
/// dans l'empreinte : changer une option invalide la base, ce qui est plus prudent que
/// de deviner lesquelles comptent (UNKNOWN = CONSERVATIVE).
///
/// Les champs propres a une unite de travail (`owner_mod_hash`, `bytecode_hash`,
/// `deps_bytecode_hash`) restent nuls : rien ne les calcule encore. Une mise a jour de
/// mod change sa version, donc `mod_set_hash`, et invalide la base entiere.
#[must_use]
pub fn environment(open: &DbOpen, config: &RuntimeConfig, hardware: &HardwareClass) -> Fingerprint {
    let mut mods = open.mods.clone();
    mods.sort_unstable();
    mods.dedup();
    let config_hash = rfx_model::to_cbor(config).map_or(0, |bytes| xxh3_64(&bytes));
    Fingerprint {
        schema_version: u16::try_from(rfx_model::MODEL_SCHEMA_VERSION).unwrap_or(u16::MAX),
        rfx_version: rfx_version(),
        mc_version: xxh3_64(open.minecraft.as_bytes()),
        forge_version: xxh3_64(open.forge.as_bytes()),
        mod_set_hash: xxh3_64(mods.join("\n").as_bytes()),
        jvm_hash: xxh3_64(open.java.as_bytes()),
        hw_class_hash: hardware.class_hash(),
        config_hash,
        ..Fingerprint::default()
    }
}

/// Version du paquet, `[majeure, mineure, correctif]`.
fn rfx_version() -> [u8; 3] {
    let mut parts = env!("CARGO_PKG_VERSION")
        .split('.')
        .map(|part| part.parse::<u8>().unwrap_or(u8::MAX));
    [
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    ]
}

/// Pourquoi l'ouverture a ete refusee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenRefused {
    /// La base est deja ouverte.
    AlreadyOpen,
    /// Le repertoire est vide.
    NoDirectory,
}

/// Collecte en cours.
#[derive(Debug)]
struct Cycle {
    /// Prochain identifiant de sonde a parcourir.
    next: u32,
    /// Tick du profileur au debut de la collecte.
    until: u64,
    /// Dynamiques collectees.
    updates: Vec<DynamicsUpdate>,
}

/// Lien entre le runtime et la base.
#[derive(Debug)]
pub struct DbLink {
    enabled: bool,
    max_entries: usize,
    service: Option<DbService>,
    environment: Fingerprint,
    pending: Vec<WorkDescriptor>,
    /// Faits d'analyse a confier au prochain lot (ADR-064).
    analyses: Vec<AnalysisUpdate>,
    cycle: Option<Cycle>,
    /// Les dynamiques vues jusqu'a ce tick sont deja confiees.
    synced_through: u64,
    syncs: u64,
    handed_back: u64,
    pending_dropped: u64,
    open_failures: u64,
    /// Ou s'execute la base : l'ordonnanceur (ADR-061), a defaut un fil dedie.
    executor: Option<Arc<dyn Executor>>,
}

impl DbLink {
    /// Fait executer la base par `executor` plutot que par un fil dedie (ADR-061). A
    /// donner avant l'ouverture.
    pub fn use_executor(&mut self, executor: Arc<dyn Executor>) {
        self.executor = Some(executor);
    }

    /// Lien ferme, d'apres la configuration.
    #[must_use]
    pub fn new(config: &RuntimeConfig) -> Self {
        Self {
            enabled: config.cache_enabled,
            max_entries: config.profiler_max_workloads as usize,
            service: None,
            environment: Fingerprint::default(),
            pending: Vec::new(),
            analyses: Vec::new(),
            cycle: None,
            synced_through: 0,
            syncs: 0,
            handed_back: 0,
            pending_dropped: 0,
            open_failures: 0,
            executor: None,
        }
    }

    /// `true` une fois la base ouverte.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.service.is_some()
    }

    /// Note une unite de travail enregistree, pour le prochain lot.
    pub fn note(&mut self, descriptor: &WorkDescriptor) {
        if !self.enabled {
            return;
        }
        if self.pending.len() >= self.max_entries {
            self.pending_dropped = self.pending_dropped.saturating_add(1);
            return;
        }
        self.pending.push(descriptor.clone());
    }

    /// Note des faits d'analyse a ecrire au prochain lot (ADR-064). Au-dela de
    /// `max_workloads` en attente, les plus recents sont perdus : l'unite sera
    /// analysee de nouveau a la partie suivante.
    pub fn note_analyses(&mut self, analyses: Vec<(WorkId, PersistedAnalysis)>) {
        if !self.enabled {
            return;
        }
        for (id, analysis) in analyses {
            if self.analyses.len() >= self.max_entries {
                self.pending_dropped = self.pending_dropped.saturating_add(1);
                continue;
            }
            self.analyses.push(AnalysisUpdate {
                id,
                analysis: Some(analysis),
            });
        }
    }

    /// Analyses relues au demarrage, rendues une seule fois (ADR-064).
    pub fn take_restored(&mut self) -> Option<Vec<(WorkId, PersistedAnalysis)>> {
        self.service.as_ref().and_then(DbService::take_restored)
    }

    /// Ouvre la base : demarre le fil, qui la relit en arriere-plan. Sans effet si
    /// `cache.enabled` vaut `false`.
    ///
    /// # Errors
    ///
    /// [`OpenRefused`] si la base est deja ouverte ou si le repertoire est vide. Un fil
    /// impossible a creer n'est pas une erreur de l'appelant : la base reste fermee et
    /// l'echec est compte (`open_failures`).
    pub fn open(
        &mut self,
        open: &DbOpen,
        environment: Fingerprint,
        max_bytes: usize,
    ) -> Result<(), OpenRefused> {
        if !self.enabled {
            return Ok(());
        }
        if self.service.is_some() {
            return Err(OpenRefused::AlreadyOpen);
        }
        if open.dir.is_empty() {
            return Err(OpenRefused::NoDirectory);
        }
        let config = DbConfig {
            path: PathBuf::from(&open.dir).join(FILE_NAME),
            environment,
            max_entries: self.max_entries,
            max_bytes,
        };
        let started = match &self.executor {
            Some(executor) => Ok(DbService::start_on(
                Arc::new(StdFileSystem),
                config,
                Arc::clone(executor),
            )),
            None => DbService::start(Arc::new(StdFileSystem), config),
        };
        match started {
            Ok(service) => {
                self.environment = environment;
                self.service = Some(service);
            }
            Err(_) => self.open_failures = self.open_failures.saturating_add(1),
        }
        Ok(())
    }

    /// Avance la collecte d'une tranche. Quand elle est complete, confie le lot.
    ///
    /// Renvoie `true` si le cycle est termine (ou s'il n'y a rien a faire), `false`
    /// s'il reste des tranches : l'appelant rend le verrou puis rappelle.
    pub fn sync_step(&mut self, store: &WorkloadStore, tick: u64, max_bytes: usize) -> bool {
        self.sync_slice(store, tick, max_bytes, SYNC_SLICE)
    }

    fn sync_slice(
        &mut self,
        store: &WorkloadStore,
        tick: u64,
        max_bytes: usize,
        slice: u32,
    ) -> bool {
        let Some(service) = &self.service else {
            return true;
        };
        let cycle = self.cycle.get_or_insert_with(|| Cycle {
            next: 0,
            until: tick,
            updates: Vec::new(),
        });
        let end = u32::try_from(store.allocated_count()).unwrap_or(u32::MAX);
        let stop = cycle.next.saturating_add(slice).min(end);
        for probe_id in cycle.next..stop {
            if let Some(entry) = store.get(probe_id) {
                if entry.dynamics.last_seen_tick > self.synced_through {
                    cycle.updates.push(DynamicsUpdate {
                        id: entry.work_id,
                        dynamics: entry.dynamics,
                    });
                }
            }
        }
        cycle.next = stop;
        if stop < end {
            return false;
        }

        let Some(cycle) = self.cycle.take() else {
            return true;
        };
        let environment = self.environment;
        let batch = SyncBatch {
            new_records: self
                .pending
                .drain(..)
                .map(|descriptor| WorkloadRecord::new(descriptor, environment))
                .collect(),
            updates: cycle.updates,
            analyses: std::mem::take(&mut self.analyses),
            max_bytes: Some(max_bytes),
        };
        match service.sync(batch) {
            Ok(()) => {
                self.synced_through = cycle.until;
                self.syncs = self.syncs.saturating_add(1);
            }
            Err(rejected) => {
                // Les descripteurs reviennent en attente ; les dynamiques seront
                // collectees de nouveau, `synced_through` n'ayant pas avance.
                self.handed_back = self.handed_back.saturating_add(1);
                self.pending.extend(
                    rejected
                        .batch
                        .new_records
                        .into_iter()
                        .map(|record| record.descriptor),
                );
                self.analyses.extend(rejected.batch.analyses);
            }
        }
        true
    }

    /// Ferme la base : confie un dernier lot complet, puis attend son ecriture, le tout
    /// au plus `timeout`. A appeler hors du verrou du runtime. Renvoie `false` si le
    /// delai est depasse.
    pub fn close(
        &mut self,
        store: &WorkloadStore,
        tick: u64,
        max_bytes: usize,
        timeout: Duration,
    ) -> bool {
        if self.service.is_none() {
            return true;
        }
        let deadline = Instant::now() + timeout;
        loop {
            let refused_before = self.handed_back;
            self.cycle = None;
            self.sync_slice(store, tick, max_bytes, u32::MAX);
            if self.handed_back == refused_before || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        self.service
            .take()
            .is_none_or(|service| service.close(remaining))
    }

    /// Memoire estimee de la base, en octets.
    #[must_use]
    pub fn estimated_bytes(&self) -> u64 {
        self.service
            .as_ref()
            .map_or(0, |service| service.stats().estimated_bytes)
    }

    /// Etat publie (`rfx.db.*`).
    #[must_use]
    pub fn status(&self) -> DbStatus {
        let mut status = DbStatus {
            enabled: self.enabled,
            open: self.service.is_some(),
            open_failures: self.open_failures,
            syncs: self.syncs,
            handed_back: self.handed_back,
            pending: self.pending.len() as u64,
            pending_dropped: self.pending_dropped,
            ..DbStatus::default()
        };
        if let Some(service) = &self.service {
            let stats = service.stats();
            status.load = stats.load.code();
            status.loaded_entries = stats.loaded_entries;
            status.rejected_entries = stats.rejected_entries;
            status.entries = stats.entries;
            status.estimated_bytes = stats.estimated_bytes;
            status.evictions = stats.evictions;
            status.writes = stats.writes;
            status.write_failures = stats.write_failures;
            status.persist_ms = stats.last_persist_ms;
            status.disabled = stats.disabled;
        }
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> DbOpen {
        DbOpen {
            schema: rfx_model::MODEL_SCHEMA_VERSION,
            dir: "cache".to_owned(),
            minecraft: "1.20.1".to_owned(),
            forge: "47.4.0".to_owned(),
            java: "17.0.12 (Test)".to_owned(),
            mods: vec!["alpha@1.0@aa".to_owned(), "beta@2.0@bb".to_owned()],
        }
    }

    fn fingerprint(open: &DbOpen, config: &RuntimeConfig) -> Fingerprint {
        environment(open, config, &HardwareClass::default())
    }

    /// R-360 : l'ordre de chargement des mods ne change pas l'empreinte ; chaque entree
    /// de l'environnement, si.
    #[test]
    fn the_environment_follows_every_input_but_not_the_mod_order() {
        let config = RuntimeConfig::default();
        let base = fingerprint(&open(), &config);

        let mut reordered = open();
        reordered.mods.reverse();
        assert_eq!(fingerprint(&reordered, &config), base, "ordre des mods");

        type Change = fn(&mut DbOpen);
        let variants: [(&str, Change); 4] = [
            ("minecraft", |o| o.minecraft.push('x')),
            ("forge", |o| o.forge.push('x')),
            ("java", |o| o.java.push('x')),
            ("mods", |o| o.mods[0] = "alpha@1.1@aa".to_owned()),
        ];
        for (name, change) in variants {
            let mut changed = open();
            change(&mut changed);
            assert!(
                !fingerprint(&changed, &config).same_environment(&base),
                "{name}"
            );
        }

        let other_config = RuntimeConfig {
            cache_max_memory_mb: 64,
            ..RuntimeConfig::default()
        };
        assert!(
            !fingerprint(&open(), &other_config).same_environment(&base),
            "configuration"
        );
        let other_hardware = HardwareClass {
            physical_cores: 64,
            ..HardwareClass::default()
        };
        assert!(
            !environment(&open(), &config, &other_hardware).same_environment(&base),
            "materiel"
        );
    }

    /// Avant l'ouverture, l'attente est bornee ; base desactivee, rien n'est garde.
    #[test]
    fn pending_descriptors_are_bounded() {
        let mut link = DbLink::new(&RuntimeConfig {
            profiler_max_workloads: 2,
            ..RuntimeConfig::default()
        });
        for _ in 0..5 {
            link.note(&WorkDescriptor::default());
        }
        let status = link.status();
        assert_eq!((status.pending, status.pending_dropped), (2, 3));

        let mut disabled = DbLink::new(&RuntimeConfig {
            cache_enabled: false,
            ..RuntimeConfig::default()
        });
        disabled.note(&WorkDescriptor::default());
        assert_eq!(disabled.status().pending, 0);
        assert_eq!(disabled.open(&open(), Fingerprint::default(), 1), Ok(()));
        assert!(!disabled.is_open());
    }
}
