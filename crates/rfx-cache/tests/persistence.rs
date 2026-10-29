//! Persistance de la base (C-09, C-48) : T-181, T-182, T-184, T-500, T-501, T-502.
//!
//! Le disque est remplace par un systeme de fichiers en memoire qui note le fil de
//! chaque operation, peut ralentir lectures et ecritures et simuler un disque plein. La
//! rotation et l'ecriture atomique, elles, sont verifiees sur le vrai disque, dans un
//! repertoire temporaire supprime a la fin.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use rfx_cache::file::{self, DecodeError};
use rfx_cache::service::THREAD_NAME;
use rfx_cache::store::backup_path;
use rfx_cache::{
    load, AnalysisUpdate, DbConfig, DbService, DynamicsUpdate, Executor, FileInfo, FileSystem,
    Fork, HistoryKind, LoadOutcome, LoadState, Refused, RotationPolicy, StdFileSystem, SyncBatch,
    Work, WorkloadDb, WorkloadRecord,
};
use rfx_model::{Fingerprint, Heat, WorkDescriptor, WorkId};

const GUARD: Duration = Duration::from_secs(30);

/// Systeme de fichiers en memoire, instrumente.
#[derive(Default)]
struct MemoryFs {
    files: Mutex<HashMap<PathBuf, Vec<u8>>>,
    threads: Mutex<Vec<String>>,
    read_delay: Duration,
    write_delay: Duration,
    write_attempts: AtomicUsize,
    full: AtomicBool,
    locked: AtomicBool,
}

impl MemoryFs {
    fn note(&self) {
        let name = std::thread::current().name().unwrap_or("?").to_owned();
        self.threads.lock().unwrap().push(name);
    }

    fn contains(&self, path: &Path) -> bool {
        self.files.lock().unwrap().contains_key(path)
    }

    fn put(&self, path: &Path, bytes: Vec<u8>) {
        self.files.lock().unwrap().insert(path.to_path_buf(), bytes);
    }

    fn bytes(&self, path: &Path) -> Vec<u8> {
        self.files.lock().unwrap()[path].clone()
    }

    fn decoded(&self) -> Vec<WorkloadRecord> {
        file::decode(&self.bytes(&path())).unwrap().records
    }
}

impl FileSystem for MemoryFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.note();
        std::thread::sleep(self.read_delay);
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.note();
        self.write_attempts.fetch_add(1, Ordering::Relaxed);
        std::thread::sleep(self.write_delay);
        if self.full.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "disque plein (simule)",
            ));
        }
        self.put(path, bytes.to_vec());
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.note();
        let mut files = self.files.lock().unwrap();
        let bytes = files
            .remove(from)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        files.insert(to.to_path_buf(), bytes);
        Ok(())
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        self.note();
        if self.locked.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fichier verrouille (simule)",
            ));
        }
        self.files
            .lock()
            .unwrap()
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<FileInfo>> {
        self.note();
        Ok(self
            .files
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path.parent() == Some(dir))
            .map(|(path, bytes)| FileInfo {
                path: path.clone(),
                len: bytes.len() as u64,
                modified: SystemTime::now(),
            })
            .collect())
    }
}

fn environment() -> Fingerprint {
    Fingerprint {
        schema_version: 1,
        rfx_version: [0, 1, 0],
        mc_version: 0x1201,
        forge_version: 0x4704,
        mod_set_hash: 99,
        jvm_hash: 17,
        hw_class_hash: 3,
        config_hash: 5,
        ..Fingerprint::default()
    }
}

fn record(n: u64) -> WorkloadRecord {
    let descriptor = WorkDescriptor {
        owner_id: format!("mod{}", n % 7),
        class_internal_name: "net/example/Thing".to_owned(),
        method_name: format!("method{n}"),
        method_descriptor: "()V".to_owned(),
        ..WorkDescriptor::default()
    };
    let fingerprint = Fingerprint {
        bytecode_hash: n + 1,
        ..environment()
    };
    let mut record = WorkloadRecord::new(descriptor, fingerprint);
    record.dynamics.heat = Heat::Warm;
    record.dynamics.total_calls = n;
    record
}

fn records(count: u64) -> Vec<WorkloadRecord> {
    (0..count).map(record).collect()
}

fn path() -> PathBuf {
    PathBuf::from("cache").join("workload.db")
}

fn stored(fs: &MemoryFs, records: &[WorkloadRecord]) {
    fs.put(&path(), file::encode(&environment(), records).unwrap());
}

fn config() -> DbConfig {
    DbConfig {
        path: path(),
        environment: environment(),
        max_entries: 100_000,
        max_bytes: usize::MAX,
    }
}

fn start(fs: &Arc<MemoryFs>) -> DbService {
    DbService::start(fs.clone(), config()).unwrap()
}

fn news(records: Vec<WorkloadRecord>) -> SyncBatch {
    SyncBatch {
        new_records: records,
        ..SyncBatch::default()
    }
}

#[test]
fn a_missing_file_is_a_first_run() {
    let fs = MemoryFs::default();
    let outcome = load(&fs, &path(), &environment(), |_| None).unwrap();
    assert!(matches!(outcome, LoadOutcome::Missing));
    assert!(outcome.into_db().is_empty());
}

/// T-181, R-360 : un autre environnement rejette le fichier entier ; dans le meme
/// environnement, seule l'entree dont le code a change est ecartee.
#[test]
fn t181_a_different_fingerprint_is_rejected() {
    let fs = MemoryFs::default();
    let written = records(20);
    stored(&fs, &written);

    let other_mods = Fingerprint {
        mod_set_hash: 100,
        ..environment()
    };
    let outcome = load(&fs, &path(), &other_mods, |_| None).unwrap();
    assert!(matches!(outcome, LoadOutcome::EnvironmentChanged));
    assert!(fs.contains(&path()), "le fichier reste, il sera remplace");

    let changed: WorkId = written[3].id;
    let outcome = load(&fs, &path(), &environment(), |id| {
        (id == changed).then_some(Fingerprint {
            bytecode_hash: 12_345,
            ..environment()
        })
    })
    .unwrap();
    let LoadOutcome::Loaded {
        db,
        rejected_entries,
    } = outcome
    else {
        panic!("le fichier devait etre relu");
    };
    assert_eq!(rejected_entries, 1);
    assert_eq!(db.len(), 19);
    assert!(db.get(changed).is_none());
    assert_eq!(
        db.get(written[4].id),
        Some(&written[4]),
        "les autres a l'identique"
    );
}

/// T-182, FM-17, R-223 : un fichier altere ou d'un schema inconnu est mis de cote en
/// `.bak`, jamais lu en partie, et la base repart vide.
#[test]
fn t182_a_corrupted_or_unknown_file_is_set_aside() {
    let fs = MemoryFs::default();
    stored(&fs, &records(10));
    let mut bytes = fs.bytes(&path());
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    fs.put(&path(), bytes);

    let outcome = load(&fs, &path(), &environment(), |_| None).unwrap();
    let LoadOutcome::SetAside { reason, backup } = outcome else {
        panic!("le fichier devait etre mis de cote");
    };
    assert_eq!(reason, DecodeError::ChecksumMismatch);
    assert_eq!(backup, backup_path(&path()));
    assert!(!fs.contains(&path()) && fs.contains(&backup));

    let mut future = file::encode(&environment(), &records(2)).unwrap();
    future[8..12].copy_from_slice(&(file::SCHEMA_VERSION + 7).to_le_bytes());
    fs.put(&path(), future);
    let outcome = load(&fs, &path(), &environment(), |_| None).unwrap();
    assert!(matches!(
        outcome,
        LoadOutcome::SetAside {
            reason: DecodeError::UnknownSchema(_),
            ..
        }
    ));
}

/// Le demarrage ne fait pas attendre la relecture : elle a lieu sur `RFX-IO`, et un
/// lot confie pendant ce temps attend son tour, sans etre perdu.
#[test]
fn the_service_loads_in_the_background_and_keeps_early_batches() {
    let fs = Arc::new(MemoryFs {
        read_delay: Duration::from_millis(300),
        ..MemoryFs::default()
    });
    stored(&fs, &records(10));
    let service = start(&fs);
    assert_eq!(
        service.stats().load,
        LoadState::Pending,
        "rendu sans attendre"
    );
    service.sync(news(vec![record(50)])).unwrap();

    assert!(service.flush(GUARD));
    let stats = service.stats();
    assert_eq!(stats.load, LoadState::Loaded);
    assert_eq!(stats.loaded_entries, 10);
    assert_eq!(stats.entries, 11, "les dix relues et la nouvelle");
    assert_eq!(fs.decoded().len(), 11);
}

/// Une unite de travail deja connue garde les dynamiques relues tant que le profileur
/// n'en rapporte pas de nouvelles ; un changement de classe de cout entre a l'historique.
#[test]
fn known_workloads_keep_their_dynamics_until_updated() {
    let fs = Arc::new(MemoryFs::default());
    let written = records(3);
    stored(&fs, &written);
    let service = start(&fs);

    // Redecouverte au lancement : descripteur neuf, sans mesure.
    let fresh: Vec<WorkloadRecord> = written
        .iter()
        .map(|r| WorkloadRecord::new(r.descriptor.clone(), r.fingerprint))
        .collect();
    service.sync(news(fresh)).unwrap();
    assert!(service.flush(GUARD));
    assert_eq!(fs.decoded(), written, "les mesures relues restent");

    let mut hot = written[1].dynamics;
    hot.heat = Heat::Hot;
    hot.last_seen_tick = 777;
    service
        .sync(SyncBatch {
            updates: vec![DynamicsUpdate {
                id: written[1].id,
                dynamics: hot,
            }],
            ..SyncBatch::default()
        })
        .unwrap();
    assert!(service.flush(GUARD));
    let reread = WorkloadDb::from_records(fs.decoded());
    let updated = reread.get(written[1].id).unwrap();
    assert_eq!(updated.dynamics, hot);
    let event = updated.history.iter().last().unwrap();
    assert_eq!(event.tick, 777);
    assert_eq!(
        event.kind,
        HistoryKind::HeatChanged {
            from: Heat::Warm,
            to: Heat::Hot
        }
    );
    assert_eq!(reread.get(written[0].id), Some(&written[0]));
}

/// R-360 : une unite de travail dont le code a change remplace l'entree relue.
#[test]
fn a_rediscovered_workload_with_new_code_replaces_the_loaded_entry() {
    let fs = Arc::new(MemoryFs::default());
    let written = records(2);
    stored(&fs, &written);
    let service = start(&fs);
    let changed = WorkloadRecord::new(
        written[0].descriptor.clone(),
        Fingerprint {
            bytecode_hash: 4_242,
            ..environment()
        },
    );
    service.sync(news(vec![changed.clone()])).unwrap();
    assert!(service.flush(GUARD));
    let reread = WorkloadDb::from_records(fs.decoded());
    assert_eq!(
        reread.get(changed.id),
        Some(&changed),
        "mesures repartent de zero"
    );
}

/// T-184, R-361 : confier un lot rend la main aussitot, meme quand le disque est lent ;
/// l'ecriture a lieu plus tard, et relit a l'identique (T-180 bout a bout).
#[test]
fn t184_syncing_never_waits_for_the_disk() {
    let fs = Arc::new(MemoryFs {
        write_delay: Duration::from_millis(300),
        ..MemoryFs::default()
    });
    let service = start(&fs);
    let written = records(2_000);

    service.sync(news(written.clone())).unwrap();
    assert!(
        !fs.contains(&path()),
        "l'appel est revenu avant l'ecriture : il ne l'a pas attendue"
    );

    assert!(service.flush(GUARD));
    let reread = WorkloadDb::from_records(fs.decoded());
    assert_eq!(reread.len(), 2_000);
    for record in &written {
        assert_eq!(reread.get(record.id), Some(record));
    }
    assert_eq!(service.stats().writes, 1);
}

/// Un lot vide n'ecrit rien : le fichier ne change que si la base a change.
#[test]
fn nothing_is_written_when_nothing_changed() {
    let fs = Arc::new(MemoryFs::default());
    let service = start(&fs);
    service.sync(SyncBatch::default()).unwrap();
    assert!(service.flush(GUARD));
    assert_eq!(service.stats().writes, 0);
    assert_eq!(fs.write_attempts.load(Ordering::Relaxed), 0);
}

/// Une file pleine refuse au lieu de faire attendre, et rend le lot intact.
#[test]
fn a_full_queue_hands_the_batch_back() {
    let fs = Arc::new(MemoryFs {
        write_delay: Duration::from_millis(200),
        ..MemoryFs::default()
    });
    let service = start(&fs);
    let mut handed_back = 0;
    for n in 0..20 {
        let batch = news(vec![record(n)]);
        if let Err(rejected) = service.sync(batch.clone()) {
            assert_eq!(rejected.reason, Refused::QueueFull);
            assert_eq!(rejected.batch, batch, "rendu intact");
            handed_back += 1;
        }
    }
    assert!(handed_back > 0, "vingt lots contre une file de quatre");
    assert_eq!(service.stats().refused, handed_back);
    assert!(service.flush(GUARD));
}

/// R-362 : la base reste sous son plafond d'entrees ; les plus froides partent.
#[test]
fn the_base_stays_under_its_entry_cap() {
    let fs = Arc::new(MemoryFs::default());
    let service = DbService::start(
        fs.clone(),
        DbConfig {
            max_entries: 5,
            ..config()
        },
    )
    .unwrap();
    service.sync(news(records(12))).unwrap();
    assert!(service.flush(GUARD));
    let stats = service.stats();
    assert_eq!((stats.entries, stats.evictions), (5, 7));
    assert_eq!(fs.decoded().len(), 5);

    // Un plafond d'octets venu du budget memoire s'applique au lot suivant.
    service
        .sync(SyncBatch {
            max_bytes: Some(1),
            ..SyncBatch::default()
        })
        .unwrap();
    assert!(service.flush(GUARD));
    assert_eq!(service.stats().entries, 0);
}

/// T-500, R-690 : toutes les operations sur le disque — relecture comprise — ont lieu
/// sur le fil `RFX-IO`, aucune sur le fil qui confie les lots.
#[test]
fn t500_no_disk_operation_runs_on_the_calling_thread() {
    let fs = Arc::new(MemoryFs::default());
    stored(&fs, &records(5));
    let service = start(&fs);
    for n in 0..3 {
        service.sync(news(vec![record(100 + n)])).unwrap();
        assert!(service.flush(GUARD));
    }
    service
        .rotate(PathBuf::from("diag"), RotationPolicy::default())
        .unwrap();
    assert!(service.flush(GUARD));

    let threads = fs.threads.lock().unwrap().clone();
    assert!(threads.len() >= 5, "operations notees : {threads:?}");
    assert!(
        threads.iter().all(|name| name == THREAD_NAME),
        "operations hors du fil d'ecriture : {threads:?}"
    );
}

/// T-502, R-692, FM-18 : un disque plein desactive l'ecriture ; la base continue en
/// memoire, et le disque n'est plus sollicite.
#[test]
fn t502_a_full_disk_disables_writing_and_nothing_else() {
    let fs = Arc::new(MemoryFs::default());
    fs.full.store(true, Ordering::Release);
    let service = start(&fs);

    service.sync(news(records(10))).unwrap();
    assert!(service.flush(GUARD));
    let stats = service.stats();
    assert!(stats.disabled);
    assert_eq!(stats.write_failures, 1);
    assert!(service.last_error().unwrap().contains("plein"));

    service.sync(news(vec![record(99)])).unwrap();
    assert!(service.flush(GUARD));
    assert_eq!(service.stats().entries, 11, "la base continue en memoire");
    assert_eq!(
        fs.write_attempts.load(Ordering::Relaxed),
        1,
        "plus aucune ecriture tentee"
    );
    assert!(!fs.contains(&path()));
}

/// Un echec de rotation est compte mais ne desactive pas l'ecriture de la base : un
/// journal verrouille par un autre programme ne doit pas arreter la persistance.
#[test]
fn a_rotation_failure_does_not_disable_persistence() {
    let fs = Arc::new(MemoryFs::default());
    fs.put(&PathBuf::from("diag").join("old.log"), vec![0; 10]);
    fs.locked.store(true, Ordering::Release);
    let service = start(&fs);
    let full = RotationPolicy {
        max_total_bytes: 0,
        ..RotationPolicy::default()
    };
    service.rotate(PathBuf::from("diag"), full).unwrap();
    assert!(service.flush(GUARD));
    let stats = service.stats();
    assert_eq!(stats.rotation_failures, 1);
    assert!(!stats.disabled);

    service.sync(news(records(3))).unwrap();
    assert!(service.flush(GUARD));
    assert_eq!(service.stats().writes, 1);
    assert!(fs.contains(&path()));
}

/// Repertoire temporaire, supprime a la fin du test, meme s'il echoue.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("rfx-cache-{name}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn file_aged(dir: &Path, name: &str, len: usize, age: Duration) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, vec![0_u8; len]).unwrap();
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(SystemTime::now() - age).unwrap();
    path
}

/// T-501, R-691 : la rotation supprime d'abord ce qui a depasse l'age, puis les plus
/// anciens tant que le total depasse la taille.
#[test]
fn t501_rotation_by_age_then_by_size() {
    let dir = TempDir::new("rotation");
    let day = Duration::from_secs(24 * 3600);
    let too_old = file_aged(&dir.0, "a.log", 100, 8 * day);
    let oldest = file_aged(&dir.0, "b.log", 1_000, 3 * day);
    let middle = file_aged(&dir.0, "c.log", 1_000, 2 * day);
    let newest = file_aged(&dir.0, "d.log", 1_000, day);

    let service = DbService::start(
        Arc::new(StdFileSystem),
        DbConfig {
            path: dir.0.join("cache").join("workload.db"),
            ..config()
        },
    )
    .unwrap();

    // L'age seul : la taille est largement respectee, seul le trop vieux part.
    let by_age = RotationPolicy {
        max_age: 7 * day,
        max_total_bytes: u64::MAX,
    };
    service.rotate(dir.0.clone(), by_age).unwrap();
    assert!(service.flush(GUARD));
    assert!(!too_old.exists(), "trop vieux");
    assert!(oldest.exists() && middle.exists() && newest.exists());
    assert_eq!(service.stats().rotated, 1);

    // Puis la taille : les plus anciens partent jusqu'a repasser sous le plafond.
    let by_size = RotationPolicy {
        max_total_bytes: 2_500,
        ..by_age
    };
    service.rotate(dir.0.clone(), by_size).unwrap();
    assert!(service.flush(GUARD));
    assert!(!oldest.exists(), "le plus ancien, pour la taille");
    assert!(middle.exists() && newest.exists());
    assert_eq!(service.stats().rotated, 2);
}

/// R-224 : l'ecriture sur le vrai disque remplace le fichier en entier et ne laisse
/// aucun fichier temporaire.
#[test]
fn the_real_disk_write_is_atomic_and_leaves_no_temporary() {
    let dir = TempDir::new("atomique");
    let target = dir.0.join("cache").join("workload.db");
    let fs = StdFileSystem;
    fs.write_atomic(&target, b"premier").unwrap();
    fs.write_atomic(&target, b"second, plus long").unwrap();
    assert_eq!(fs.read(&target).unwrap(), b"second, plus long");
    let names: Vec<_> = fs
        .list(target.parent().unwrap())
        .unwrap()
        .into_iter()
        .map(|file| file.path)
        .collect();
    assert_eq!(names, vec![target.clone()], "aucun .tmp restant");
}

/// Executeur qui refuse tout, comme l'ordonnanceur en serie (ADR-061).
#[derive(Debug)]
struct Refusing;

impl Executor for Refusing {
    fn execute(&self, work: Work) -> Result<(), Work> {
        Err(work)
    }
}

/// ADR-061 : rien ne s'execute tant que l'executeur refuse ; la fermeture sert alors la
/// file sur son propre fil, et rien n'est perdu.
#[test]
fn a_refusing_executor_defers_the_work_to_closing() {
    let fs = Arc::new(MemoryFs::default());
    let service = DbService::start_on(fs.clone(), config(), Arc::new(Refusing));
    service.sync(news(records(3))).unwrap();
    assert_eq!(
        service.stats().load,
        LoadState::Pending,
        "rien n'a pu etre lance"
    );
    assert_eq!(service.stats().writes, 0);
    // Pas d'attente du delai entier : le refus se voit, et la fermeture sert la file.
    let started = std::time::Instant::now();
    assert!(service.close(GUARD));
    assert!(started.elapsed() < GUARD / 2, "{:?}", started.elapsed());
    assert_eq!(fs.decoded().len(), 3, "ecrit a la fermeture");
}

/// Executeur qui lance chaque travail sur un fil neuf : des taches de vidage peuvent se
/// chevaucher, comme sur un pool. Compte les taches lancees et terminees.
#[derive(Debug)]
struct Spawning(AtomicUsize, Arc<AtomicUsize>);

impl Executor for Spawning {
    fn execute(&self, work: Work) -> Result<(), Work> {
        self.0.fetch_add(1, Ordering::Relaxed);
        let finished = Arc::clone(&self.1);
        std::thread::spawn(move || {
            struct Serial;
            impl Fork for Serial {
                fn spawn(&self, work: Box<dyn FnOnce() + Send>) {
                    work();
                }
                fn join(&self) {}
            }
            work(&Serial);
            finished.fetch_add(1, Ordering::Release);
        });
        Ok(())
    }
}

/// ADR-061 : sur un executeur concurrent, les lots sont appliques dans l'ordre ou ils
/// ont ete confies.
#[test]
fn batches_are_applied_in_order_on_a_concurrent_executor() {
    let fs = Arc::new(MemoryFs::default());
    let executor = Arc::new(Spawning(AtomicUsize::new(0), Arc::new(AtomicUsize::new(0))));
    let service = DbService::start_on(fs.clone(), config(), executor.clone());
    let first = record(0);
    let id = first.id;
    service.sync(news(vec![first.clone()])).unwrap();
    for calls in 1..=40_u64 {
        let mut dynamics = first.dynamics;
        dynamics.total_calls = calls;
        let batch = SyncBatch {
            updates: vec![DynamicsUpdate { id, dynamics }],
            ..SyncBatch::default()
        };
        // File bornee : on attend son tour plutot que de perdre un lot.
        while service.sync(batch.clone()).is_err() {
            std::thread::yield_now();
        }
        // Au milieu, la file est videe et l'on attend que toute tache se soit retiree :
        // la suite en demande forcement une autre. Sans cela, une seule tache pouvait
        // servir les quarante lots quand le vidage allait assez vite, et le test ne
        // verifiait plus ce qu'il annonce.
        if calls == 20 {
            assert!(service.flush(GUARD));
            while executor.1.load(Ordering::Acquire) < executor.0.load(Ordering::Relaxed) {
                std::thread::yield_now();
            }
        }
    }
    assert!(service.flush(GUARD));
    let stored = fs.decoded();
    assert_eq!(stored.len(), 1);
    assert_eq!(
        stored[0].dynamics.total_calls, 40,
        "le dernier lot l'emporte"
    );
    assert!(
        executor.0.load(Ordering::Relaxed) >= 2,
        "plusieurs taches de vidage"
    );
}

fn analysis(stamp: u64) -> rfx_model::PersistedAnalysis {
    rfx_model::PersistedAnalysis {
        stamp,
        bytecode_hash: Some(stamp + 1),
        nodes: 4,
        resolved_calls: 3,
        unresolved_calls: 0,
        graph_truncated: false,
        reflection: false,
        recursive_root: false,
        rounds: 1,
        calls: vec!["net/example/Thing.helper()V".to_owned()],
        depth: 1,
        truncated: false,
        external_locks: Some(Vec::new()),
        reads: rfx_model::AccessSet::default(),
        writes: rfx_model::AccessSet::default(),
        side_effects: rfx_model::SideEffectSet::default(),
        locks: false,
        thread: rfx_model::ThreadFacts::default(),
        determinism: rfx_model::DeterminismFacts::default(),
        may_be_dispatched: false,
    }
}

/// ADR-064 : les faits d'analyse confies sont ecrits avec leur enregistrement, relus a
/// la partie suivante et rendus une seule fois ; ceux d'une unite inconnue de la base
/// sont ignores, et `None` efface.
#[test]
fn analyses_are_written_reloaded_and_handed_back_once() {
    let fs = Arc::new(MemoryFs::default());
    let records: Vec<WorkloadRecord> = (0..3).map(record).collect();
    let ids: Vec<WorkId> = records.iter().map(|r| r.id).collect();
    {
        let service = start(&fs);
        service.sync(news(records)).unwrap();
        service
            .sync(SyncBatch {
                analyses: vec![
                    AnalysisUpdate {
                        id: ids[0],
                        analysis: Some(analysis(10)),
                    },
                    AnalysisUpdate {
                        id: ids[1],
                        analysis: Some(analysis(20)),
                    },
                    AnalysisUpdate {
                        id: WorkId(0xdead),
                        analysis: Some(analysis(30)),
                    },
                ],
                ..SyncBatch::default()
            })
            .unwrap();
        service
            .sync(SyncBatch {
                analyses: vec![AnalysisUpdate {
                    id: ids[1],
                    analysis: None,
                }],
                ..SyncBatch::default()
            })
            .unwrap();
        assert!(service.close(GUARD));
    }
    let on_disk = fs.decoded();
    assert_eq!(on_disk.len(), 3, "l'unite inconnue n'a rien cree");
    assert_eq!(
        on_disk.iter().filter(|r| r.analysis.is_some()).count(),
        1,
        "la seconde analyse a ete effacee"
    );

    let service = start(&fs);
    assert!(service.flush(GUARD));
    let restored = service.take_restored().expect("relecture terminee");
    assert_eq!(restored, vec![(ids[0], analysis(10))]);
    assert_eq!(service.take_restored(), None, "rendues une seule fois");
}
