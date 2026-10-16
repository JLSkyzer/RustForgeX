//! Acceptance de C-09 (PARTIE 5.9) : 20 000 unites de travail tenues en moins de
//! 64 Mio.
//!
//! Le test s'installe comme allocateur global, dans son propre binaire, et compte les
//! octets vivants : la memoire mesuree est celle que la base retient vraiment, index et
//! emplacements compris, et non une estimation.
//!
//! Il confronte aussi cette mesure a `WorkloadDb::estimated_bytes`, sur laquelle repose
//! l'eviction par octets (R-362) : une estimation qui sous-compte laisserait la base
//! depasser son plafond.
//!
//! Le cas mesure est le pire : chaque unite a un historique plein et des noms longs.

// `GlobalAlloc` est un trait `unsafe` : l'implementer exige d'y deroger. La derogation
// s'arrete a ce fichier de test, qui ne fait que compter avant de deleguer.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use rfx_cache::{HistoryEvent, HistoryKind, WorkloadDb, WorkloadRecord, HISTORY_CAPACITY};
use rfx_model::{Fingerprint, Heat, WorkDescriptor};

struct CountingAllocator;

/// Octets vivants. Signe : un bloc alloue avant le debut d'une mesure peut etre libere
/// pendant.
static LIVE: AtomicIsize = AtomicIsize::new(0);

fn add(bytes: usize, sign: isize) {
    LIVE.fetch_add(sign * bytes as isize, Ordering::Relaxed);
}

// SAFETY : toutes les operations sont deleguees telles quelles a l'allocateur systeme,
// avec le meme `Layout` et le meme pointeur. Le comptage ne touche qu'un compteur
// atomique, sans modifier la memoire rendue.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            add(layout.size(), 1);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        add(layout.size(), -1);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let moved = System.realloc(ptr, layout, new_size);
        if !moved.is_null() {
            add(layout.size(), -1);
            add(new_size, 1);
        }
        moved
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

const WORKLOADS: u64 = 20_000;
const LIMIT_BYTES: usize = 64 << 20;

/// Une unite de travail au pire : noms longs, historique plein.
fn worst_case(n: u64) -> WorkloadRecord {
    let descriptor = WorkDescriptor {
        owner_id: format!("an_unusually_long_mod_identifier_{:03}", n % 300),
        class_internal_name: format!(
            "com/example/somemod/content/machines/processing/advanced/AdvancedMachineBlockEntity{n}"
        ),
        method_name: format!("lambda$serverTickProcessingRecipes${n}"),
        method_descriptor: "(Lnet/minecraft/world/level/Level;Lnet/minecraft/core/BlockPos;Lnet/minecraft/world/level/block/state/BlockState;)V".to_owned(),
        call_context_hash: n,
        ..WorkDescriptor::default()
    };
    let fingerprint = Fingerprint {
        bytecode_hash: n,
        ..Fingerprint::default()
    };
    let mut record = WorkloadRecord::new(descriptor, fingerprint);
    record.dynamics.heat = Heat::Warm;
    record.dynamics.last_seen_tick = n;
    for tick in 0..HISTORY_CAPACITY as u64 + 4 {
        record.history.push(HistoryEvent {
            tick,
            kind: HistoryKind::HeatChanged {
                from: Heat::Cold,
                to: Heat::Warm,
            },
        });
    }
    record
}

/// Une unite de travail ordinaire : noms courts, historique vide.
fn typical(n: u64) -> WorkloadRecord {
    let descriptor = WorkDescriptor {
        owner_id: format!("mod{}", n % 50),
        class_internal_name: format!("net/minecraft/world/entity/Mob{n}"),
        method_name: "tick".to_owned(),
        method_descriptor: "()V".to_owned(),
        ..WorkDescriptor::default()
    };
    WorkloadRecord::new(descriptor, Fingerprint::default())
}

/// Construit la base, et rend les octets mesures et estimes.
fn measure(make: fn(u64) -> WorkloadRecord) -> (usize, usize) {
    let before = LIVE.load(Ordering::Relaxed);
    let db = WorkloadDb::from_records((0..WORKLOADS).map(make));
    let after = LIVE.load(Ordering::Relaxed);
    assert_eq!(db.len(), WORKLOADS as usize);
    let measured = usize::try_from(after - before).unwrap();
    let estimated = db.estimated_bytes();
    println!(
        "{WORKLOADS} unites : {measured} octets mesures ({:.1} Mio, {} par unite), \
         {estimated} estimes",
        measured as f64 / f64::from(1 << 20),
        measured / WORKLOADS as usize
    );
    (measured, estimated)
}

/// L'eviction par octets (R-362) ne vaut que ce que vaut l'estimation. Elle reproduit
/// les allocations au lieu de les deviner : a 1 % pres. Un ecart plus grand signale un
/// terme oublie, ou une table de hachage de `std` qui a change de disposition.
fn assert_estimate_close(measured: usize, estimated: usize) {
    assert!(
        estimated.abs_diff(measured) * 100 <= measured,
        "estimation {estimated} trop loin de la mesure {measured}"
    );
}

/// Un seul test : le compteur est global au processus, et deux tests lances en parallele
/// compteraient chacun les allocations de l'autre.
#[test]
fn twenty_thousand_workloads_fit_in_64_mib() {
    let (measured, estimated) = measure(worst_case);
    assert!(
        measured < LIMIT_BYTES,
        "{measured} octets pour {WORKLOADS} unites : plafond de 64 Mio depasse"
    );
    assert_estimate_close(measured, estimated);

    // Une base ordinaire : l'estimation doit suivre aussi.
    let (measured, estimated) = measure(typical);
    assert_estimate_close(measured, estimated);
}
