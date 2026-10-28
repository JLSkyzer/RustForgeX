//! C-17 cote runtime : les travaux internes sur l'ordonnanceur (ADR-061).
//!
//! Cahier des charges : PARTIE 5.17 (5.17.4 : `BACKGROUND` pour l'analyse et la
//! persistance), PARTIE 5.46 (R-690). Maturite : `STABLE`.
//!
//! [`SchedulerExecutor`] realise l'executeur de la base (`rfx_cache::Executor`) sur
//! l'ordonnanceur : chaque travail est une tache `BACKGROUND` soumise en detache —
//! jamais executee sur le fil qui la confie, meme en serie —, et ses sous-travaux sont
//! des sous-taches, que l'attente aide a executer.
//!
//! L'executeur ne garde qu'une reference faible : l'ordonnanceur appartient au runtime,
//! et sa derniere reference ne doit jamais etre lachee par une tache, sur un worker qui
//! tenterait alors de se rejoindre lui-meme.

use std::sync::{Arc, Mutex, PoisonError, Weak};

use rfx_cache::{Executor, Fork, Work};
use rfx_scheduler::{JobOptions, Scheduler, TaskContext, TaskGroup, TaskOutcome};
use rfx_taskgraph::Priority;

/// Executeur des travaux de la base sur l'ordonnanceur.
#[derive(Debug)]
pub struct SchedulerExecutor {
    scheduler: Weak<Scheduler>,
}

impl SchedulerExecutor {
    /// Executeur sur `scheduler`.
    #[must_use]
    pub fn new(scheduler: &Arc<Scheduler>) -> Self {
        Self {
            scheduler: Arc::downgrade(scheduler),
        }
    }
}

impl Executor for SchedulerExecutor {
    fn execute(&self, work: Work) -> Result<(), Work> {
        let Some(scheduler) = self.scheduler.upgrade() else {
            return Err(work);
        };
        // La tache prend le travail ; une soumission refusee le rend.
        let slot = Arc::new(Mutex::new(Some(work)));
        let taken = Arc::clone(&slot);
        let submitted = scheduler.submit_detached(
            &TaskGroup::new(),
            JobOptions::new(Priority::Background),
            move |ctx: &mut TaskContext<'_>| {
                let work = taken.lock().unwrap_or_else(PoisonError::into_inner).take();
                if let Some(work) = work {
                    let fork = ContextFork {
                        ctx,
                        group: TaskGroup::new(),
                    };
                    work(&fork);
                }
                TaskOutcome::Success
            },
        );
        match submitted {
            Ok(()) => Ok(()),
            Err(_) => slot
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                .map_or(Ok(()), Err),
        }
    }
}

/// Sous-travaux d'une tache : des sous-taches de l'ordonnanceur.
struct ContextFork<'a, 'b> {
    ctx: &'a TaskContext<'b>,
    group: TaskGroup,
}

impl Fork for ContextFork<'_, '_> {
    fn spawn(&self, work: Box<dyn FnOnce() + Send>) {
        let mut once = Some(work);
        self.ctx.spawn(&self.group, move |_: &mut TaskContext<'_>| {
            if let Some(work) = once.take() {
                work();
            }
            TaskOutcome::Success
        });
    }

    fn join(&self) {
        self.ctx.join(&self.group);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use rfx_scheduler::SchedulerConfig;

    fn scheduler(serial: bool) -> Arc<Scheduler> {
        let mut config = SchedulerConfig::for_machine(4);
        config.workers = 2;
        config.force_serial = serial;
        Arc::new(Scheduler::start(config).expect("demarrage"))
    }

    /// Un travail et ses sous-travaux s'executent sur les workers, jamais sur le fil qui
    /// confie ; l'attente des sous-travaux les voit tous finis.
    #[test]
    fn work_and_subwork_run_on_the_workers() {
        let scheduler = scheduler(false);
        let executor = SchedulerExecutor::new(&scheduler);
        let caller = std::thread::current().id();
        let done = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = std::sync::mpsc::channel();
        let counted = Arc::clone(&done);
        let work: Work = Box::new(move |fork: &dyn Fork| {
            assert_ne!(std::thread::current().id(), caller);
            for _ in 0..8 {
                let counted = Arc::clone(&counted);
                fork.spawn(Box::new(move || {
                    counted.fetch_add(1, Ordering::Relaxed);
                }));
            }
            fork.join();
            sender.send(counted.load(Ordering::Relaxed)).unwrap();
        });
        assert!(executor.execute(work).is_ok());
        assert_eq!(receiver.recv_timeout(Duration::from_secs(30)).unwrap(), 8);
        assert_eq!(done.load(Ordering::Relaxed), 8);
    }

    /// En serie, le travail est rendu intact au lieu d'etre execute en ligne ; un
    /// ordonnanceur detruit refuse aussi.
    #[test]
    fn a_serial_or_stopped_scheduler_hands_the_work_back() {
        let serial = scheduler(true);
        let executor = SchedulerExecutor::new(&serial);
        let ran = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&ran);
        let back = executor
            .execute(Box::new(move |_: &dyn Fork| {
                counted.fetch_add(1, Ordering::Relaxed);
            }))
            .expect_err("refuse en serie");
        assert_eq!(ran.load(Ordering::Relaxed), 0, "jamais en ligne");
        back(&rfx_cache::InlineFork);
        assert_eq!(ran.load(Ordering::Relaxed), 1, "rendu intact");

        let stopped = SchedulerExecutor::new(&scheduler(false));
        assert!(stopped.execute(Box::new(|_: &dyn Fork| {})).is_err());
    }
}
