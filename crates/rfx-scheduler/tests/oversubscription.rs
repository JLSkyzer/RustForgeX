//! T-265 : adaptation a la sur-souscription simulee (R-421, FM-25, ADR-045).
//!
//! La charge externe est simulee : le test appelle `report_external_load` lui-meme. Le
//! capteur reel, qui comptera les fils executables tiers, viendra se brancher sur cette
//! meme entree.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rfx_scheduler::{JobOptions, Scheduler, SchedulerConfig, TaskContext, TaskGroup, TaskOutcome};
use rfx_taskgraph::Priority;

const GUARD: Duration = Duration::from_secs(120);

/// Huit workers sur une machine de seize coeurs : seuil de sur-souscription a 24.
fn eight_workers() -> Scheduler {
    let mut config = SchedulerConfig::for_machine(16);
    config.workers = 8;
    assert_eq!(config.oversubscription_threshold, 24);
    Scheduler::start(config).unwrap()
}

fn active(scheduler: &Scheduler) -> usize {
    scheduler.stats().active_workers
}

#[test]
fn t265_active_workers_halve_under_load_and_come_back_one_by_one() {
    let scheduler = eight_workers();
    assert_eq!(active(&scheduler), 8);
    scheduler.report_external_load(24);
    assert_eq!(active(&scheduler), 8, "au seuil, pas au-dessus");

    let mut seen = Vec::new();
    for _ in 0..4 {
        scheduler.report_external_load(25);
        seen.push(active(&scheduler));
    }
    assert_eq!(seen, vec![4, 2, 1, 1]);

    let mut seen = Vec::new();
    for _ in 0..8 {
        scheduler.report_external_load(0);
        seen.push(active(&scheduler));
    }
    assert_eq!(seen, vec![2, 3, 4, 5, 6, 7, 8, 8]);
}

/// FM-25 : a un seul worker, la sur-souscription persistante fait passer en execution
/// serie sur le fil qui soumet ; elle cesse des que la charge retombe.
#[test]
fn t265_persistent_oversubscription_falls_back_to_serial_and_recovers() {
    let scheduler = eight_workers();
    for _ in 0..3 {
        scheduler.report_external_load(100);
    }
    assert_eq!(active(&scheduler), 1);
    for _ in 0..7 {
        scheduler.report_external_load(100);
    }
    assert!(
        !scheduler.stats().serial_fallback,
        "sept releves ne suffisent pas"
    );
    scheduler.report_external_load(100);
    assert!(scheduler.stats().serial_fallback);

    let caller = std::thread::current().id();
    let ran_on = Arc::new(Mutex::new(None));
    let record = Arc::clone(&ran_on);
    let group = TaskGroup::new();
    scheduler
        .submit(
            &group,
            JobOptions::new(Priority::High),
            move |_: &mut TaskContext<'_>| {
                *record.lock().unwrap() = Some(std::thread::current().id());
                TaskOutcome::Success
            },
        )
        .unwrap();
    assert_eq!(
        *ran_on.lock().unwrap(),
        Some(caller),
        "executee sur le fil qui soumet"
    );

    scheduler.report_external_load(0);
    assert!(!scheduler.stats().serial_fallback);
    assert_eq!(active(&scheduler), 2);
}

/// Un worker rendu inactif ne prend plus de travail.
#[test]
fn t265_inactive_workers_take_no_work() {
    let scheduler = eight_workers();
    for _ in 0..3 {
        scheduler.report_external_load(100);
    }
    assert_eq!(active(&scheduler), 1);
    // Laisse aux workers le temps de constater qu'ils sont inactifs.
    std::thread::sleep(Duration::from_millis(50));

    let names = Arc::new(Mutex::new(HashSet::new()));
    let group = TaskGroup::new();
    for _ in 0..200 {
        let names = Arc::clone(&names);
        scheduler
            .submit(
                &group,
                JobOptions::new(Priority::High),
                move |_: &mut TaskContext<'_>| {
                    let name = std::thread::current().name().unwrap_or("?").to_owned();
                    names.lock().unwrap().insert(name);
                    TaskOutcome::Success
                },
            )
            .unwrap();
    }
    assert!(group.wait(GUARD));
    let names = names.lock().unwrap();
    assert_eq!(names.len(), 1, "un seul worker actif, vus : {names:?}");
    assert!(names.contains("rfx-worker-0"));
}

/// Pendant que la charge varie au hasard, aucune tache n'est perdue ni executee deux
/// fois, y compris aux passages en execution serie.
#[test]
fn t265_no_task_is_lost_while_the_load_changes() {
    let scheduler = Arc::new(eight_workers());
    let runs: Arc<Vec<AtomicU8>> = Arc::new((0..200_000).map(|_| AtomicU8::new(0)).collect());
    let group = TaskGroup::new();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reporter = {
        let (scheduler, stop) = (Arc::clone(&scheduler), Arc::clone(&stop));
        std::thread::spawn(move || {
            let mut state = 0x9E37_79B9_7F4A_7C15u64;
            while !stop.load(Ordering::Acquire) {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                scheduler.report_external_load(if state.is_multiple_of(3) { 0 } else { 100 });
                std::thread::sleep(Duration::from_micros(200));
            }
        })
    };
    for slot in 0..runs.len() {
        let runs = Arc::clone(&runs);
        scheduler
            .submit(
                &group,
                JobOptions::new(Priority::High),
                move |_: &mut TaskContext<'_>| {
                    runs[slot].fetch_add(1, Ordering::Relaxed);
                    TaskOutcome::Success
                },
            )
            .unwrap();
    }
    stop.store(true, Ordering::Release);
    reporter.join().unwrap();
    for _ in 0..8 {
        scheduler.report_external_load(0);
    }
    assert!(group.wait(GUARD));
    let wrong = runs
        .iter()
        .filter(|count| count.load(Ordering::Relaxed) != 1)
        .count();
    assert_eq!(wrong, 0, "taches executees zero ou plusieurs fois");
}
