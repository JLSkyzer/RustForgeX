//! Tests du contexte d'execution des taches (C-18) : T-270, T-271.
//!
//! T-272 (respect de l'annulation) est porte par T-263 dans `tests/pool.rs` : une tache
//! en cours s'arrete a son point de controle quand son jeton est annule.

use std::time::Duration;

use rfx_scheduler::{
    JobOptions, RejectReason, Scheduler, SchedulerConfig, TaskContext, TaskGroup, TaskOutcome,
};
use rfx_taskgraph::Priority;

const GUARD: Duration = Duration::from_secs(120);

/// Un seul worker, et une arene de 4 Kio plafonnee a 1 Mio.
fn one_worker() -> Scheduler {
    let mut config = SchedulerConfig::for_machine(2);
    config.workers = 1;
    config.arena_initial_bytes = 1 << 12;
    config.arena_max_bytes = 1 << 20;
    Scheduler::start(config).unwrap()
}

/// Remplit 600 Kio : une fois tient sous le plafond de 1 Mio, et la remise a zero garde
/// le bloc pour la tache suivante ; deux fois sans remise a zero le depasseraient.
fn fill_most_of_the_arena(cx: &TaskContext<'_>) -> bool {
    cx.arena()
        .try_alloc_slice_fill_copy(600 << 10, 0xAB_u8)
        .is_ok()
}

/// T-270 : une panic est contenue, le worker continue, et l'arene de la tache qui a
/// panique est quand meme remise a zero.
#[test]
fn t270_a_panic_is_contained_and_the_arena_is_still_reset() {
    let scheduler = one_worker();
    let group = TaskGroup::new();
    scheduler
        .submit(
            &group,
            JobOptions::new(Priority::Normal),
            |cx: &mut TaskContext<'_>| -> TaskOutcome {
                assert!(fill_most_of_the_arena(cx));
                panic!("panic volontaire : arene remplie puis abandonnee")
            },
        )
        .unwrap();
    for _ in 0..3 {
        scheduler
            .submit(
                &group,
                JobOptions::new(Priority::Normal),
                |cx: &mut TaskContext<'_>| {
                    if fill_most_of_the_arena(cx) {
                        TaskOutcome::Success
                    } else {
                        TaskOutcome::Rejected(RejectReason::BudgetExceeded)
                    }
                },
            )
            .unwrap();
    }

    assert!(group.wait(GUARD));
    let tally = group.tally();
    assert_eq!(tally.panicked, 1);
    assert_eq!(
        tally.success, 3,
        "l'arene a ete rendue vide malgre la panic"
    );
}

/// T-271 : une allocation au-dela du plafond est refusee proprement ; la tache rend
/// `Rejected(BudgetExceeded)` et le worker sert la suivante.
#[test]
fn t271_an_arena_overflow_is_a_clean_rejection() {
    let scheduler = one_worker();
    let group = TaskGroup::new();
    scheduler
        .submit(
            &group,
            JobOptions::new(Priority::Normal),
            |cx: &mut TaskContext<'_>| match cx.arena().try_alloc_slice_fill_copy(4 << 20, 0_u8) {
                Ok(_) => TaskOutcome::Success,
                Err(full) => {
                    assert_eq!(full.limit, 1 << 20);
                    TaskOutcome::Rejected(RejectReason::BudgetExceeded)
                }
            },
        )
        .unwrap();
    scheduler
        .submit(
            &group,
            JobOptions::new(Priority::Normal),
            |cx: &mut TaskContext<'_>| {
                let value = cx.arena().try_alloc(41_u64).map(|v| *v + 1);
                if value == Ok(42) {
                    TaskOutcome::Success
                } else {
                    TaskOutcome::Invalid
                }
            },
        )
        .unwrap();

    assert!(group.wait(GUARD));
    let tally = group.tally();
    assert_eq!(tally.rejected, 1, "{tally:?}");
    assert_eq!(tally.success, 1, "{tally:?}");
    assert!(scheduler.stats().arena_peak_bytes > 0);
}
