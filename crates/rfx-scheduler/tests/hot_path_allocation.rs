//! T-273 : aucune allocation systeme dans le chemin chaud d'un worker (R-433).
//!
//! Composant : C-18. Cahier des charges : PARTIE 5.18 (« verifie par allocateur
//! instrumente »).
//!
//! Le test s'installe comme allocateur global, dans son propre binaire : un allocateur
//! global l'est pour tout le processus. Il ne compte que les allocations des fils
//! « armes », et seule une tache arme le fil qui l'execute. Le fil du test, qui soumet
//! et alloue donc les taches elles-memes, n'est jamais arme : c'est le chemin du worker
//! qui est juge — sa boucle, la prise des taches, l'arene, la fin de tache — et non la
//! soumission, que T-267 mesure a part.
//!
//! Une fois arme, le worker le reste entre deux taches : ce qu'il alloue en cherchant
//! du travail est compte aussi.

// `GlobalAlloc` est un trait `unsafe` : l'implementer exige d'y deroger. La derogation
// s'arrete a ce fichier de test, qui ne fait que compter avant de deleguer.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rfx_scheduler::{
    JobOptions, RejectReason, Scheduler, SchedulerConfig, TaskContext, TaskGroup, TaskOutcome,
};
use rfx_taskgraph::Priority;

struct CountingAllocator;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

/// `try_with` : pendant la destruction d'un fil, la variable peut ne plus etre
/// accessible, et un allocateur ne doit jamais paniquer.
fn note_allocation() {
    if ARMED.try_with(Cell::get).unwrap_or(false) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY : toutes les operations sont deleguees telles quelles a l'allocateur systeme,
// avec le meme `Layout` et le meme pointeur. Le comptage ne touche qu'un compteur
// atomique et une cellule du fil courant, sans modifier la memoire rendue.
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
static GLOBAL: CountingAllocator = CountingAllocator;

const GUARD: Duration = Duration::from_secs(120);

/// Une tache ordinaire : elle arme son fil, puis travaille dans son arene.
fn arena_task(cx: &mut TaskContext<'_>) -> TaskOutcome {
    ARMED.with(|armed| armed.set(true));
    let Ok(values) = cx.arena().try_alloc_slice_fill_copy(256, 7_u32) else {
        return TaskOutcome::Rejected(RejectReason::BudgetExceeded);
    };
    for (index, value) in values.iter_mut().enumerate() {
        *value = value.wrapping_mul(index as u32 + 1);
    }
    if values[255] == 7 * 256 {
        TaskOutcome::Success
    } else {
        TaskOutcome::Invalid
    }
}

fn run_batch(scheduler: &Scheduler, tasks: usize) {
    let group = TaskGroup::new();
    for _ in 0..tasks {
        scheduler
            .submit(&group, JobOptions::new(Priority::High), arena_task)
            .unwrap();
    }
    assert!(group.wait(GUARD));
    assert_eq!(group.tally().success, tasks as u64);
}

#[test]
fn t273_the_worker_hot_path_allocates_nothing_from_the_system() {
    let mut config = SchedulerConfig::for_machine(2);
    config.workers = 1;
    let scheduler = Scheduler::start(config).unwrap();

    // Echauffement : l'arene du worker, sa reserve et ses variables de fil sont creees
    // ici, une fois pour toutes.
    run_batch(&scheduler, 1_000);
    ALLOCATIONS.store(0, Ordering::SeqCst);

    run_batch(&scheduler, 10_000);

    assert_eq!(
        ALLOCATIONS.load(Ordering::SeqCst),
        0,
        "R-433 : le worker a demande de la memoire au systeme en regime etabli"
    );
}
