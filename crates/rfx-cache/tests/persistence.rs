//! Persistance de la base (C-09, C-48) : T-181, T-182, T-184, T-500, T-501, T-502.
//!
//! Le disque est remplace par un systeme de fichiers en memoire qui note le fil de
//! chaque operation, peut ralentir les ecritures et simuler un disque plein. La rotation
//! et l'ecriture atomique, elles, sont verifiees sur le vrai disque, dans un repertoire
//! temporaire supprime a la fin.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use rfx_cache::file::{self, DecodeError};
use rfx_cache::store::backup_path;
use rfx_cache::writer::THREAD_NAME;
use rfx_cache::{
    load, FileInfo, FileSystem, LoadOutcome, PersistenceWriter, Refused, RotationPolicy,
    StdFileSystem, WorkloadDb, WorkloadRecord,
};
use rfx_model::{Fingerprint, Heat, WorkDescriptor, WorkId};

const GUARD: Duration = Duration::from_secs(30);

/// Systeme de fichiers en memoire, instrumente.
#[derive(Default)]
struct MemoryFs {
    files: Mutex<HashMap<PathBuf, Vec<u8>>>,
    threads: Mutex<Vec<String>>,
    write_delay: Duration,
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
}

impl FileSystem for MemoryFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.note();
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.note();
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

/// T-184, R-361 : confier une ecriture rend la main aussitot, meme quand le disque est
/// lent ; l'ecriture a lieu plus tard, et relit a l'identique (T-180 bout a bout).
#[test]
fn t184_persisting_never_waits_for_the_disk() {
    let fs = Arc::new(MemoryFs {
        write_delay: Duration::from_millis(300),
        ..MemoryFs::default()
    });
    let writer = PersistenceWriter::start(fs.clone()).unwrap();
    let db = WorkloadDb::from_records(records(2_000));

    writer
        .persist(path(), environment(), db.snapshot())
        .unwrap();
    assert!(
        !fs.contains(&path()),
        "l'appel est revenu avant l'ecriture : il ne l'a pas attendue"
    );

    assert!(writer.flush(GUARD));
    let decoded = file::decode(&fs.bytes(&path())).unwrap();
    assert_eq!(decoded.records.len(), 2_000);
    let reread = WorkloadDb::from_records(decoded.records);
    for record in db.iter() {
        assert_eq!(reread.get(record.id), Some(record));
    }
    assert_eq!(writer.stats().writes, 1);
}

/// Une file pleine refuse au lieu de faire attendre l'appelant.
#[test]
fn a_full_queue_refuses_instead_of_blocking() {
    let fs = Arc::new(MemoryFs {
        write_delay: Duration::from_millis(200),
        ..MemoryFs::default()
    });
    let writer = PersistenceWriter::start(fs).unwrap();
    let refused = (0..20)
        .filter(|_| writer.persist(path(), environment(), records(1)) == Err(Refused::QueueFull))
        .count();
    assert!(refused > 0, "vingt demandes contre une file de quatre");
    assert!(writer.stats().refused >= refused as u64);
    assert!(writer.flush(GUARD));
}

/// T-500, R-690 : toutes les operations sur le disque ont lieu sur le fil `RFX-IO`,
/// aucune sur le fil qui confie l'ecriture.
#[test]
fn t500_no_disk_operation_runs_on_the_calling_thread() {
    let fs = Arc::new(MemoryFs::default());
    let writer = PersistenceWriter::start(fs.clone()).unwrap();
    for _ in 0..3 {
        writer.persist(path(), environment(), records(50)).unwrap();
        assert!(writer.flush(GUARD));
    }
    writer
        .rotate(PathBuf::from("cache"), RotationPolicy::default())
        .unwrap();
    assert!(writer.flush(GUARD));

    let threads = fs.threads.lock().unwrap().clone();
    assert!(threads.len() >= 4, "operations notees : {threads:?}");
    assert!(
        threads.iter().all(|name| name == THREAD_NAME),
        "operations hors du fil d'ecriture : {threads:?}"
    );
}

/// T-502, R-692, FM-18 : un disque plein desactive la persistance ; les demandes
/// suivantes sont refusees, et la base en memoire reste utilisable.
#[test]
fn t502_a_full_disk_disables_persistence_and_nothing_else() {
    let fs = Arc::new(MemoryFs::default());
    fs.full.store(true, Ordering::Release);
    let writer = PersistenceWriter::start(fs.clone()).unwrap();
    let mut db = WorkloadDb::from_records(records(10));

    writer
        .persist(path(), environment(), db.snapshot())
        .unwrap();
    assert!(writer.flush(GUARD));
    let stats = writer.stats();
    assert!(stats.disabled);
    assert_eq!(stats.write_failures, 1);
    assert!(writer.last_error().unwrap().contains("plein"));

    assert_eq!(
        writer.persist(path(), environment(), db.snapshot()),
        Err(Refused::Disabled)
    );
    db.upsert(record(99));
    assert_eq!(db.len(), 11, "la base continue en memoire");
    assert!(!fs.contains(&path()));
}

/// Un echec de rotation est compte mais ne desactive pas l'ecriture de la base : un
/// journal verrouille par un autre programme ne doit pas arreter la persistance.
#[test]
fn a_rotation_failure_does_not_disable_persistence() {
    let fs = Arc::new(MemoryFs::default());
    fs.put(&PathBuf::from("diag").join("old.log"), vec![0; 10]);
    fs.locked.store(true, Ordering::Release);
    let writer = PersistenceWriter::start(fs.clone()).unwrap();
    let full = RotationPolicy {
        max_total_bytes: 0,
        ..RotationPolicy::default()
    };
    writer.rotate(PathBuf::from("diag"), full).unwrap();
    assert!(writer.flush(GUARD));
    let stats = writer.stats();
    assert_eq!(stats.rotation_failures, 1);
    assert!(!stats.disabled);

    writer.persist(path(), environment(), records(3)).unwrap();
    assert!(writer.flush(GUARD));
    assert_eq!(writer.stats().writes, 1);
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

    let writer = PersistenceWriter::start(Arc::new(StdFileSystem)).unwrap();

    // L'age seul : la taille est largement respectee, seul le trop vieux part.
    let by_age = RotationPolicy {
        max_age: 7 * day,
        max_total_bytes: u64::MAX,
    };
    writer.rotate(dir.0.clone(), by_age).unwrap();
    assert!(writer.flush(GUARD));
    assert!(!too_old.exists(), "trop vieux");
    assert!(oldest.exists() && middle.exists() && newest.exists());
    assert_eq!(writer.stats().rotated, 1);

    // Puis la taille : les plus anciens partent jusqu'a repasser sous le plafond.
    let by_size = RotationPolicy {
        max_total_bytes: 2_500,
        ..by_age
    };
    writer.rotate(dir.0.clone(), by_size).unwrap();
    assert!(writer.flush(GUARD));
    assert!(!oldest.exists(), "le plus ancien, pour la taille");
    assert!(middle.exists() && newest.exists());
    assert_eq!(writer.stats().rotated, 2);
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
