//! T-142 : aucune allocation dans le chemin chaud du profiler (R-320).
//!
//! Composant : C-05. Cahier des charges : PARTIE 5.5.
//!
//! Le test s'installe comme allocateur global et compte les allocations. Il vit dans
//! son propre binaire de test, et ce binaire ne contient qu'un seul test : un
//! allocateur global est global au processus, et deux tests executes en parallele
//! compteraient chacun les allocations de l'autre.
//!
//! Relire le code ne suffirait pas a prouver l'absence d'allocation : une allocation
//! peut se cacher dans n'importe quelle methode appelee indirectement. Seule la mesure
//! le prouve.

// `GlobalAlloc` est un trait `unsafe` : l'implementer exige d'y deroger. La derogation
// s'arrete a ce fichier de test, qui ne fait que compter avant de deleguer.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use rfx_memory::probe_buffer::{ProbeRecord, RecordKind};
use rfx_model::WorkId;
use rfx_profiler::{Profiler, ProfilerConfig, TickCost};

/// Allocateur qui compte les allocations pendant qu'il est arme.
struct CountingAllocator;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ARMED: AtomicBool = AtomicBool::new(false);

// SAFETY : toutes les operations sont deleguees telles quelles a l'allocateur
// systeme, avec le meme `Layout` et le meme pointeur. Le comptage n'ajoute qu'un
// increment atomique et ne modifie ni le pointeur rendu ni la memoire pointee.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Execute `body` en comptant les allocations qu'il declenche.
fn count_allocations<F: FnOnce()>(body: F) -> usize {
    ALLOCATIONS.store(0, Ordering::Relaxed);
    ARMED.store(true, Ordering::Relaxed);
    body();
    ARMED.store(false, Ordering::Relaxed);
    ALLOCATIONS.load(Ordering::Relaxed)
}

fn record(probe_id: u32, kind: RecordKind, value: u64) -> ProbeRecord {
    ProbeRecord {
        probe_id,
        flags: 0,
        context_hash16: 0,
        timestamp_ns: 0,
        value,
        kind: kind.to_byte(),
    }
}

#[test]
fn the_hot_path_never_allocates() {
    let mut profiler = Profiler::new(ProfilerConfig::default());
    profiler.start();

    // L'enregistrement des unites de travail alloue — c'est son role, il a lieu au
    // chargement des classes, pas pendant le jeu. Il est fait hors du comptage.
    let probes: Vec<u32> = (1..=64_u64)
        .map(|n| profiler.register(WorkId(n)).expect("sonde"))
        .collect();

    // Un tick de rodage : la table des niveaux est prise une fois, ce qui alloue.
    profiler.end_tick(cheap_tick());
    let _ = profiler.take_levels();

    let allocations = count_allocations(|| {
        for _ in 0..100 {
            for &probe in &probes {
                profiler.ingest(&record(probe, RecordKind::Exit, 4_096));
                profiler.ingest(&record(probe, RecordKind::Enter, 0));
                profiler.ingest(&record(probe, RecordKind::Alloc, 128));
                profiler.ingest(&record(probe, RecordKind::Sample, 1_000));
            }
            profiler.end_tick(cheap_tick());
        }
        // Un identifiant inconnu est refuse, et le refus n'alloue pas davantage.
        for _ in 0..1_000 {
            profiler.ingest(&record(999_999, RecordKind::Exit, 1));
        }
    });

    assert_eq!(
        allocations, 0,
        "R-320 : le chemin chaud du profiler ne doit rien allouer ({allocations} constatees)"
    );
}

fn cheap_tick() -> TickCost {
    TickCost {
        profiling_ns: 10_000,
        tick_ns: 10_000_000,
        period_ns: 50_000_000,
    }
}
