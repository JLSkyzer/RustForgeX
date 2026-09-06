//! T-142 : aucune allocation dans le chemin chaud du profiler (R-320).
//!
//! Composant : C-05. Cahier des charges : PARTIE 5.5.
//!
//! Le test s'installe comme allocateur global et compte les allocations. Il vit dans
//! son propre binaire de test : un allocateur global l'est pour tout le processus, et
//! l'installer depuis un binaire partage avec d'autres tests fausserait les leurs.
//!
//! Relire le code ne suffirait pas a prouver l'absence d'allocation : une allocation
//! peut se cacher dans n'importe quelle methode appelee indirectement. Seule la mesure
//! le prouve.
//!
//! # Pourquoi le comptage est par fil
//!
//! Un allocateur global l'est pour tout le PROCESSUS. Compter dans des statiques
//! globales revient donc a compter aussi ce qu'allouent les autres fils — a commencer
//! par le harnais de test lui-meme, qui capture la sortie sur le fil principal pendant
//! que le test s'execute sur un fil dedie.
//!
//! Ce n'est pas theorique : sous Linux, ce test a rendu tantot 0 tantot 4 allocations
//! sur du code Rust rigoureusement identique, d'un commit a l'autre. Il ne mesurait
//! pas le chemin chaud, il mesurait le processus.
//!
//! R-320 porte sur ce que fait LE CHEMIN CHAUD, pas sur ce que fait la machine autour.
//! Le comptage est donc par fil. Les cellules sont initialisees en `const` : une
//! variable de fil ainsi declaree n'a ni destructeur ni initialisation paresseuse, et
//! ne peut donc pas allouer depuis l'allocateur qui l'interroge.

// `GlobalAlloc` est un trait `unsafe` : l'implementer exige d'y deroger. La derogation
// s'arrete a ce fichier de test, qui ne fait que compter avant de deleguer.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use rfx_memory::probe_buffer::{ProbeRecord, RecordKind};
use rfx_model::WorkId;
use rfx_profiler::{Profiler, ProfilerConfig, TickCost};

/// Allocateur qui compte, pour le fil courant seulement, tant qu'il est arme.
struct CountingAllocator;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

/// Enregistre une allocation du fil courant, si le comptage y est arme.
///
/// `try_with` plutot que `with` : pendant la destruction d'un fil, une variable de fil
/// peut ne plus etre accessible. Un allocateur qui paniquerait la rendrait le processus
/// inutilisable, alors qu'il n'y a rien a compter a ce moment-la.
fn note_allocation() {
    let armed = ARMED.try_with(Cell::get).unwrap_or(false);
    if armed {
        ALLOCATIONS
            .try_with(|count| count.set(count.get().saturating_add(1)))
            .ok();
    }
}

// SAFETY : toutes les operations sont deleguees telles quelles a l'allocateur
// systeme, avec le meme `Layout` et le meme pointeur. Le comptage ne touche que des
// cellules du fil courant et ne modifie ni le pointeur rendu ni la memoire pointee.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_allocation();
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_allocation();
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Execute `body` en comptant les allocations qu'il declenche sur ce fil.
fn count_allocations<F: FnOnce()>(body: F) -> usize {
    ALLOCATIONS.with(|count| count.set(0));
    ARMED.with(|armed| armed.set(true));
    body();
    ARMED.with(|armed| armed.set(false));
    ALLOCATIONS.with(Cell::get)
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

/// Le compteur voit ce qu'il est cense voir.
///
/// Sans cette verification, une erreur dans le comptage par fil rendrait le test
/// principal vide : il constaterait zero allocation parce qu'il n'en compte aucune,
/// et non parce que le chemin chaud n'alloue rien. Un test qui ne peut plus echouer
/// est pire qu'un test absent, puisqu'il rassure.
#[test]
fn the_counter_actually_counts() {
    let seen = count_allocations(|| {
        let noise: Vec<u8> = Vec::with_capacity(4_096);
        std::hint::black_box(&noise);
    });

    assert!(seen > 0, "le compteur n'a vu aucune allocation deliberee");
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
