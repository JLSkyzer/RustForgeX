//! T-264 : parallelisme imbrique profond, sans interblocage (PARTIE 5.17.9).
//!
//! Un interblocage ne doit pas bloquer la suite de tests : chaque scenario tourne dans
//! un fil a part, attendu avec un delai. Si le delai expire, l'ordonnanceur est
//! volontairement abandonne (ses workers sont bloques, l'arreter attendrait sans fin) et
//! le test echoue.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use rfx_scheduler::{
    JobOptions, Scheduler, SchedulerConfig, Task, TaskContext, TaskGroup, TaskOutcome,
};
use rfx_taskgraph::Priority;

const DEADLOCK_GUARD: Duration = Duration::from_secs(60);

/// Somme de `start..end`, par moities lancees en sous-taches jusqu'au grain de 64.
struct SumTask {
    start: u64,
    end: u64,
    total: Arc<AtomicU64>,
}

impl Task for SumTask {
    fn run(&mut self, cx: &mut TaskContext<'_>) -> TaskOutcome {
        if self.end - self.start <= 64 {
            let sum: u64 = (self.start..self.end).sum();
            self.total.fetch_add(sum, Ordering::Relaxed);
            return TaskOutcome::Success;
        }
        let middle = self.start + (self.end - self.start) / 2;
        let children = TaskGroup::new();
        for (start, end) in [(self.start, middle), (middle, self.end)] {
            let total = Arc::clone(&self.total);
            cx.spawn(&children, SumTask { start, end, total });
        }
        cx.join(&children);
        if children.tally().success == 2 {
            TaskOutcome::Success
        } else {
            TaskOutcome::Invalid
        }
    }
}

fn config(workers: usize, force_serial: bool) -> SchedulerConfig {
    let mut config = SchedulerConfig::for_machine(16);
    config.workers = workers;
    config.force_serial = force_serial;
    config
}

/// Fait tourner `scenario` dans un fil a part ; echoue au lieu de bloquer.
fn within_guard<R: Send + 'static>(scenario: impl FnOnce() -> R + Send + 'static) -> R {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(scenario());
    });
    receiver
        .recv_timeout(DEADLOCK_GUARD)
        .expect("interblocage : le scenario n'a pas termine")
}

/// T-264 : huit arbres de sous-taches de profondeur 10, bien au-dela de la limite de 4,
/// donnent la somme exacte avec 1, 2 et 4 workers et en `FORCE_SERIAL`. Avec un seul
/// worker, la tache qui attend ses enfants doit les executer elle-meme.
#[test]
fn t264_deep_nested_parallelism_never_deadlocks() {
    let count: u64 = 64 << 13;
    let expected = count * (count - 1) / 2;
    for (workers, force_serial) in [(1, false), (2, false), (4, false), (4, true)] {
        let (sum, inline_spawns) = within_guard(move || {
            let scheduler = Scheduler::start(config(workers, force_serial)).unwrap();
            let total = Arc::new(AtomicU64::new(0));
            let group = TaskGroup::new();
            for slice in 0..8u64 {
                let (start, end) = (slice * count / 8, (slice + 1) * count / 8);
                let total = Arc::clone(&total);
                scheduler
                    .submit(
                        &group,
                        JobOptions::new(Priority::Normal),
                        SumTask { start, end, total },
                    )
                    .unwrap();
            }
            if !group.wait(DEADLOCK_GUARD) {
                // Workers bloques : les attendre a l'arret bloquerait ce fil aussi.
                std::mem::forget(scheduler);
                return (u64::MAX, 0);
            }
            assert_eq!(group.tally().success, 8);
            (
                total.load(Ordering::Relaxed),
                scheduler.stats().inline_spawns,
            )
        });
        assert_eq!(sum, expected, "{workers} workers, serie {force_serial}");
        assert!(
            inline_spawns > 0,
            "la limite d'imbrication n'a jamais servi"
        );
    }
}

/// Une sous-tache herite du jeton de sa mere : annuler la mere annule tout l'arbre.
#[test]
fn subtasks_inherit_the_cancellation_of_their_parent() {
    let outcome = within_guard(|| {
        let scheduler = Scheduler::start(config(2, false)).unwrap();
        let group = TaskGroup::new();
        let options = JobOptions::new(Priority::High);
        let cancel = options.cancel.clone();
        let children = TaskGroup::new();
        let seen = children.clone();
        scheduler
            .submit(&group, options, move |cx: &mut TaskContext<'_>| {
                cx.cancel.cancel();
                for _ in 0..4 {
                    cx.spawn(&seen, |_: &mut TaskContext<'_>| TaskOutcome::Success);
                }
                cx.join(&seen);
                TaskOutcome::Success
            })
            .unwrap();
        assert!(group.wait(DEADLOCK_GUARD));
        assert!(cancel.is_cancelled());
        children.tally()
    });
    assert_eq!(
        outcome.rejected, 4,
        "aucune sous-tache ne demarre une fois la mere annulee"
    );
    assert_eq!(outcome.success, 0);
}
