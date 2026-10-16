//! Tests du pool de workers (C-17) : T-260, T-261, T-266, R-420 a R-423, R-524, E-2401.
//!
//! Aucun test ne mesure une duree : ils jugent un resultat, et passent de la meme facon
//! sur une machine chargee. Les seules bornes de temps sont des delais de garde larges,
//! qui ne servent qu'a ne pas bloquer la suite si un defaut empeche une tache de finir.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rfx_scheduler::{JobOptions, Scheduler, SchedulerConfig, TaskContext, TaskGroup, TaskOutcome};
use rfx_taskgraph::Priority;

/// Delai de garde : jamais atteint quand tout va bien.
const GUARD: Duration = Duration::from_secs(120);

fn config(workers: usize) -> SchedulerConfig {
    let mut config = SchedulerConfig::for_machine(workers.max(1));
    config.workers = workers;
    config
}

fn ok(_: &mut TaskContext<'_>) -> TaskOutcome {
    TaskOutcome::Success
}

/// Soumet en reessayant tant que la contre-pression refuse : le soumetteur attend que
/// les workers aient vide une partie de la file.
fn submit_retrying<F>(scheduler: &Scheduler, group: &TaskGroup, priority: Priority, task: F)
where
    F: FnMut(&mut TaskContext<'_>) -> TaskOutcome + Send + Clone + 'static,
{
    while scheduler
        .submit(group, JobOptions::new(priority), task.clone())
        .is_err()
    {
        std::thread::yield_now();
    }
}

/// Une tache qui occupe un worker jusqu'a ce qu'on la libere.
struct Gate {
    started: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
}

impl Gate {
    fn hold(scheduler: &Scheduler, group: &TaskGroup) -> Self {
        let gate = Gate {
            started: Arc::new(AtomicBool::new(false)),
            release: Arc::new(AtomicBool::new(false)),
        };
        let (started, release) = (Arc::clone(&gate.started), Arc::clone(&gate.release));
        scheduler
            .submit(
                group,
                JobOptions::new(Priority::Critical),
                move |_: &mut TaskContext<'_>| {
                    started.store(true, Ordering::Release);
                    while !release.load(Ordering::Acquire) {
                        std::thread::yield_now();
                    }
                    TaskOutcome::Success
                },
            )
            .unwrap();
        while !gate.started.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        gate
    }

    fn open(&self) {
        self.release.store(true, Ordering::Release);
    }
}

/// La porte se rouvre quand elle est detruite, y compris pendant le deroulement d'une
/// assertion qui echoue. Sans cela, l'arret de l'ordonnanceur attend un worker que plus
/// personne ne libere, et un test qui devrait echouer se bloque.
impl Drop for Gate {
    fn drop(&mut self) {
        self.open();
    }
}

/// T-260 : dix millions de taches, aucune perdue, aucune executee deux fois au total.
#[test]
fn t260_ten_million_tasks_complete_under_stress() {
    static SUM: AtomicU64 = AtomicU64::new(0);
    let scheduler = Scheduler::start(config(4)).unwrap();
    let group = TaskGroup::new();
    let count: u64 = 10_000_000;
    for _ in 0..count {
        submit_retrying(
            &scheduler,
            &group,
            Priority::Normal,
            |_: &mut TaskContext<'_>| {
                SUM.fetch_add(1, Ordering::Relaxed);
                TaskOutcome::Success
            },
        );
    }
    assert!(group.wait(GUARD), "le groupe n'a pas termine");
    assert_eq!(SUM.load(Ordering::Relaxed), count);
    assert_eq!(group.tally().success, count);
    assert_eq!(scheduler.stats().executed, count);
    assert_eq!(scheduler.queue_depth(), 0);
}

/// T-261 : soumises depuis quatre fils a la fois, chaque tache s'execute exactement une
/// fois.
#[test]
fn t261_every_task_runs_exactly_once() {
    let per_thread = 250_000usize;
    let submitters = 4;
    let runs: Arc<Vec<AtomicU8>> = Arc::new(
        (0..per_thread * submitters)
            .map(|_| AtomicU8::new(0))
            .collect(),
    );
    let scheduler = Arc::new(Scheduler::start(config(4)).unwrap());
    let group = TaskGroup::new();
    std::thread::scope(|scope| {
        for submitter in 0..submitters {
            let (scheduler, group, runs) = (Arc::clone(&scheduler), group.clone(), &runs);
            scope.spawn(move || {
                for offset in 0..per_thread {
                    let slot = submitter * per_thread + offset;
                    let runs = Arc::clone(runs);
                    submit_retrying(
                        &scheduler,
                        &group,
                        Priority::High,
                        move |_: &mut TaskContext<'_>| {
                            runs[slot].fetch_add(1, Ordering::Relaxed);
                            TaskOutcome::Success
                        },
                    );
                }
            });
        }
    });
    assert!(group.wait(GUARD));
    let wrong: Vec<usize> = runs
        .iter()
        .enumerate()
        .filter(|(_, count)| count.load(Ordering::Relaxed) != 1)
        .map(|(slot, _)| slot)
        .take(5)
        .collect();
    assert!(
        wrong.is_empty(),
        "taches executees zero ou plusieurs fois : {wrong:?}"
    );
}

fn results(force_serial: bool) -> Vec<u64> {
    let mut config = config(4);
    config.force_serial = force_serial;
    let scheduler = Scheduler::start(config).unwrap();
    let slots: Arc<Vec<AtomicU64>> = Arc::new((0..10_000).map(|_| AtomicU64::new(0)).collect());
    let group = TaskGroup::new();
    for index in 0..slots.len() {
        let slots = Arc::clone(&slots);
        submit_retrying(
            &scheduler,
            &group,
            Priority::Normal,
            move |_: &mut TaskContext<'_>| {
                let value = (index as u64)
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .rotate_left(7);
                slots[index].store(value, Ordering::Relaxed);
                TaskOutcome::Success
            },
        );
    }
    assert!(group.wait(GUARD));
    slots
        .iter()
        .map(|slot| slot.load(Ordering::Relaxed))
        .collect()
}

/// T-266 : `FORCE_SERIAL` produit les memes resultats, sans aucun worker.
#[test]
fn t266_force_serial_produces_identical_results() {
    let mut serial = config(4);
    serial.force_serial = true;
    assert_eq!(Scheduler::start(serial).unwrap().workers(), 0);
    assert_eq!(results(true), results(false));
}

/// R-524 : une panic rend `Panicked` et le worker continue de servir.
#[test]
fn a_panicking_task_does_not_poison_the_pool() {
    let scheduler = Scheduler::start(config(1)).unwrap();
    let group = TaskGroup::new();
    scheduler
        .submit(
            &group,
            JobOptions::new(Priority::Normal),
            |_: &mut TaskContext<'_>| -> TaskOutcome {
                panic!("panic volontaire : confinement d'une tache")
            },
        )
        .unwrap();
    for _ in 0..100 {
        scheduler
            .submit(&group, JobOptions::new(Priority::Normal), ok)
            .unwrap();
    }
    assert!(group.wait(GUARD));
    assert_eq!(group.tally().panicked, 1);
    assert_eq!(group.tally().success, 100);
    assert_eq!(scheduler.stats().panics, 1);
}

/// PARTIE 5.17.4 : un worker libre prend `CRITICAL`, puis `HIGH`, `NORMAL`, `LOW`.
#[test]
fn a_free_worker_takes_the_most_urgent_task_first() {
    let scheduler = Scheduler::start(config(1)).unwrap();
    let group = TaskGroup::new();
    let gate = Gate::hold(&scheduler, &group);
    let order: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    for priority in [
        Priority::Low,
        Priority::Normal,
        Priority::High,
        Priority::Critical,
    ] {
        let order = Arc::clone(&order);
        scheduler
            .submit(
                &group,
                JobOptions::new(priority),
                move |_: &mut TaskContext<'_>| {
                    order.lock().unwrap().push(priority);
                    TaskOutcome::Success
                },
            )
            .unwrap();
    }
    gate.open();
    assert!(group.wait(GUARD));
    assert_eq!(
        *order.lock().unwrap(),
        vec![
            Priority::Critical,
            Priority::High,
            Priority::Normal,
            Priority::Low
        ]
    );
}

/// R-423 : aucune tache `BACKGROUND` ne demarre pendant la fenetre critique.
#[test]
fn background_tasks_wait_for_the_critical_window_to_close() {
    let scheduler = Scheduler::start(config(2)).unwrap();
    scheduler.set_critical_window(true);
    let group = TaskGroup::new();
    let ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&ran);
    scheduler
        .submit(
            &group,
            JobOptions::new(Priority::Background),
            move |_: &mut TaskContext<'_>| {
                flag.store(true, Ordering::Release);
                TaskOutcome::Success
            },
        )
        .unwrap();
    assert!(!group.wait(Duration::from_millis(100)));
    assert!(
        !ran.load(Ordering::Acquire),
        "demarree pendant la fenetre critique"
    );

    scheduler.set_critical_window(false);
    assert!(group.wait(GUARD));
    assert!(ran.load(Ordering::Acquire));
}

/// PARTIE 5.17.5 : `LOW` refusee au-dela du seuil haut, `NORMAL` au-dela de la limite
/// dure, `HIGH` et `CRITICAL` jamais (`E-2401`).
#[test]
fn backpressure_refuses_by_priority() {
    let mut config = config(1);
    config.high_watermark = 4;
    config.hard_limit = 8;
    let scheduler = Scheduler::start(config).unwrap();
    let group = TaskGroup::new();
    let gate = Gate::hold(&scheduler, &group);
    let submit = |priority| scheduler.submit(&group, JobOptions::new(priority), ok);

    for _ in 0..4 {
        submit(Priority::Normal).unwrap();
    }
    let refused = submit(Priority::Low).unwrap_err();
    assert_eq!(
        (refused.priority, refused.queued, refused.code()),
        (Priority::Low, 4, 2401)
    );
    assert!(submit(Priority::Background).is_err());
    for _ in 0..4 {
        submit(Priority::Normal).unwrap();
    }
    assert!(submit(Priority::Normal).is_err(), "limite dure atteinte");
    submit(Priority::High).unwrap();
    submit(Priority::Critical).unwrap();
    assert_eq!(scheduler.stats().rejected_submissions, 3);

    gate.open();
    assert!(group.wait(GUARD));
    assert_eq!(
        group.tally().success,
        11,
        "la porte, huit NORMAL, un HIGH, un CRITICAL"
    );
}

/// Une tache annulee ou en retard avant de demarrer n'est pas executee.
#[test]
fn a_cancelled_or_late_task_is_not_run() {
    let scheduler = Scheduler::start(config(1)).unwrap();
    let group = TaskGroup::new();
    let ran = Arc::new(AtomicU64::new(0));

    let cancelled = JobOptions::new(Priority::Normal);
    cancelled.cancel.cancel();
    let late = JobOptions {
        deadline: Some(Instant::now()),
        ..JobOptions::new(Priority::Normal)
    };
    for options in [cancelled, late] {
        let ran = Arc::clone(&ran);
        scheduler
            .submit(&group, options, move |_: &mut TaskContext<'_>| {
                ran.fetch_add(1, Ordering::Relaxed);
                TaskOutcome::Success
            })
            .unwrap();
    }
    assert!(group.wait(GUARD));
    assert_eq!(ran.load(Ordering::Relaxed), 0);
    assert_eq!(group.tally().rejected, 1);
    assert_eq!(group.tally().timeout, 1);
}

/// INV-08 : attendre un groupe ne bloque jamais au-dela du delai donne.
#[test]
fn waiting_never_blocks_beyond_the_timeout() {
    let scheduler = Scheduler::start(config(1)).unwrap();
    let group = TaskGroup::new();
    let gate = Gate::hold(&scheduler, &group);
    let started = Instant::now();
    assert!(!group.wait(Duration::from_millis(20)));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "attente non bornee"
    );
    gate.open();
    assert!(group.wait(GUARD));
}

/// Annule un jeton a sa destruction : une assertion qui echoue ne laisse pas une tache
/// tourner indefiniment pendant que l'arret du pool l'attend.
struct CancelOnDrop(rfx_taskgraph::CancelToken);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// T-263 : une tache en cours s'arrete a son point de controle quand on annule son
/// jeton depuis un autre fil (PARTIE 5.17.7, R-434).
#[test]
fn t263_a_running_task_stops_when_its_token_is_cancelled() {
    let scheduler = Scheduler::start(config(1)).unwrap();
    let group = TaskGroup::new();
    let options = JobOptions::new(Priority::High);
    let guard = CancelOnDrop(options.cancel.clone());
    let started = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&started);
    scheduler
        .submit(&group, options, move |cx: &mut TaskContext<'_>| {
            flag.store(true, Ordering::Release);
            while !cx.should_stop() {
                std::thread::yield_now();
            }
            TaskOutcome::Rejected(rfx_scheduler::RejectReason::Cancelled)
        })
        .unwrap();
    while !started.load(Ordering::Acquire) {
        std::thread::yield_now();
    }
    assert!(
        !group.wait(Duration::from_millis(20)),
        "la tache tourne encore"
    );

    guard.0.cancel();

    assert!(group.wait(GUARD), "la tache ne s'est pas arretee");
    assert_eq!(group.tally().rejected, 1);
}

/// `rfx.sched.deadline_misses` compte la tache qui depasse son echeance en cours
/// d'execution, et celle dont l'echeance etait deja passee au demarrage.
#[test]
fn deadline_misses_are_counted() {
    let scheduler = Scheduler::start(config(1)).unwrap();
    let group = TaskGroup::new();
    let running_late = JobOptions {
        deadline: Some(Instant::now() + Duration::from_millis(20)),
        ..JobOptions::new(Priority::High)
    };
    let _guard = CancelOnDrop(running_late.cancel.clone());
    scheduler
        .submit(&group, running_late, |cx: &mut TaskContext<'_>| {
            while !cx.should_stop() {
                std::thread::yield_now();
            }
            TaskOutcome::Timeout
        })
        .unwrap();
    let already_late = JobOptions {
        deadline: Some(Instant::now()),
        ..JobOptions::new(Priority::High)
    };
    scheduler.submit(&group, already_late, ok).unwrap();
    scheduler
        .submit(&group, JobOptions::new(Priority::High), ok)
        .unwrap();

    assert!(group.wait(GUARD));
    assert_eq!(group.tally().timeout, 2);
    assert_eq!(group.tally().success, 1);
    assert_eq!(scheduler.stats().deadline_misses, 2);
}
