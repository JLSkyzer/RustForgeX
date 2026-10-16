//! C-17 : le pool de workers a vol de travail.
//!
//! Cahier des charges : PARTIE 5.17.1 a 5.17.5, 5.17.8. Exigences : R-420 (jamais plus
//! de workers que de coeurs logiques), R-423 (`BACKGROUND` ne retarde jamais `CRITICAL`),
//! R-524 (une panic n'empoisonne pas le pool). Code d'erreur : `E-2401`. Tests : T-260,
//! T-261, T-266. Maturite : `STABLE`.
//!
//! # Ou un worker cherche du travail
//!
//! Dans cet ordre, a chaque tour :
//!
//! 1. la file d'injection `CRITICAL`, toujours la premiere ;
//! 2. sa propre file, en LIFO (localite de cache) ;
//! 3. les files d'injection `HIGH` puis `NORMAL` ;
//! 4. la file d'un autre worker, en FIFO (vol) ;
//! 5. la file `LOW` : il est libre, il peut s'en charger ;
//! 6. la file `BACKGROUND`, seulement hors fenetre critique (R-423).
//!
//! Apres deux tours sans travail il cede le processeur, apres huit il s'endort
//! (PARTIE 5.17.3).
//!
//! # S'endormir sans perdre un reveil
//!
//! Le soumetteur incremente une epoque, puis regarde s'il y a des dormeurs ; le worker
//! se declare dormeur, puis regarde si l'epoque a change depuis sa derniere recherche.
//! Les deux sequences sont en ordre sequentiellement coherent : au moins l'un des deux
//! voit l'ecriture de l'autre. Le soumetteur ne prend donc le verrou que s'il y a un
//! dormeur, et une soumission ordinaire n'en prend aucun.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use rfx_taskgraph::{CancelToken, Priority};

use crate::config::SchedulerConfig;
use crate::task::{GroupState, RejectReason, Task, TaskContext, TaskGroup, TaskOutcome};

/// Ce qui accompagne une tache soumise.
#[derive(Debug, Clone)]
pub struct JobOptions {
    /// Priorite.
    pub priority: Priority,
    /// Jeton d'annulation.
    pub cancel: CancelToken,
    /// Echeance, si la tache en a une.
    pub deadline: Option<Instant>,
}

impl JobOptions {
    /// Options d'une tache de priorite donnee, sans echeance, avec son propre jeton.
    #[must_use]
    pub fn new(priority: Priority) -> Self {
        Self {
            priority,
            cancel: CancelToken::new(),
            deadline: None,
        }
    }
}

/// `E-2401` : soumission refusee par contre-pression. Mineure : le travail concerne
/// execute son chemin Java pour ce tick (PARTIE 5.17.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backpressure {
    /// Priorite de la soumission refusee.
    pub priority: Priority,
    /// Taches en attente au moment du refus.
    pub queued: usize,
}

impl Backpressure {
    /// Numero normatif de l'annexe A.2.
    #[must_use]
    pub fn code(self) -> u16 {
        2401
    }
}

/// Compteurs de l'ordonnanceur (`rfx.sched.*`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SchedulerStats {
    /// Taches executees, quelle qu'en soit l'issue.
    pub executed: u64,
    /// Taches obtenues par vol.
    pub steals: u64,
    /// Soumissions refusees par contre-pression.
    pub rejected_submissions: u64,
    /// Taches qui ont panique.
    pub panics: u64,
    /// Endormissements de workers.
    pub parks: u64,
    /// Taches terminees apres leur echeance, ou abandonnees pour cela
    /// (`rfx.sched.deadline_misses`).
    pub deadline_misses: u64,
    /// Sous-taches executees en ligne, la profondeur maximale etant atteinte.
    pub inline_spawns: u64,
    /// Workers actifs (`rfx.sched.workers_active`).
    pub active_workers: usize,
    /// `true` si la sur-souscription persistante a fait passer en execution serie
    /// (FM-25).
    pub serial_fallback: bool,
    /// Taches `CRITICAL` executees par le fil qui attend, faute de worker (FM-24).
    pub famine_escalations: u64,
}

/// Crochet appele avec l'issue d'une tache, quelle qu'elle soit : succes, annulation
/// avant demarrage, echeance depassee ou panic. C'est par lui que l'execution d'un graphe
/// libere les dependants, avant que la tache ne soit comptee terminee dans son groupe.
pub(crate) type OnDone = Box<dyn FnOnce(TaskOutcome) + Send>;

pub(crate) struct Job {
    pub(crate) task: Box<dyn Task>,
    pub(crate) group: Arc<GroupState>,
    pub(crate) cancel: CancelToken,
    pub(crate) deadline: Option<Instant>,
    pub(crate) priority: Priority,
    /// Profondeur d'imbrication (PARTIE 5.17.9).
    pub(crate) depth: u8,
    pub(crate) on_done: Option<OnDone>,
}

/// Rang de la file d'injection d'une priorite : la plus urgente d'abord.
fn lane(priority: Priority) -> usize {
    match priority {
        Priority::Critical => 0,
        Priority::High => 1,
        Priority::Normal => 2,
        Priority::Low => 3,
        Priority::Background => 4,
    }
}

const CRITICAL: usize = 0;
const HIGH: usize = 1;
const NORMAL: usize = 2;
const LOW: usize = 3;
const BACKGROUND: usize = 4;

/// Attente au-dela de laquelle le fil qui attend execute lui-meme le travail `CRITICAL`
/// encore en file (FM-24).
const FAMINE_GRACE: Duration = Duration::from_millis(1);

/// Releves sur-souscrits consecutifs, a un seul worker, avant l'execution serie (FM-25).
const SERIAL_FALLBACK_STREAK: u32 = 8;

/// Delai maximal d'un endormissement : un filet, au cas ou un reveil se perdrait malgre
/// tout. Aucun ne devrait l'etre.
const PARK_TIMEOUT: Duration = Duration::from_millis(10);

pub(crate) struct Shared {
    lanes: [Injector<Job>; 5],
    stealers: Vec<Stealer<Job>>,
    queued: AtomicUsize,
    critical_window: AtomicBool,
    shutdown: AtomicBool,
    epoch: AtomicU64,
    sleepers: AtomicUsize,
    sleep_lock: Mutex<()>,
    wake: Condvar,
    executed: AtomicU64,
    steals: AtomicU64,
    rejected: AtomicU64,
    panics: AtomicU64,
    parks: AtomicU64,
    deadline_misses: AtomicU64,
    inline_spawns: AtomicU64,
    max_nesting: u8,
    /// Workers demarres.
    workers: usize,
    /// Workers autorises a prendre du travail ; les autres dorment (R-421).
    active: AtomicUsize,
    /// Releves consecutifs de sur-souscription avec un seul worker actif.
    oversubscribed_streak: AtomicU32,
    /// FM-25 : execution serie sur le fil qui soumet.
    serial_fallback: AtomicBool,
    famine_escalations: AtomicU64,
}

impl Shared {
    fn notify(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        if self.sleepers.load(Ordering::SeqCst) > 0 {
            let _guard = self
                .sleep_lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if self.active.load(Ordering::Acquire) < self.workers {
                // Un reveil unique pourrait echoir a un worker inactif, qui se
                // rendormirait : le travail attendrait le delai de garde.
                self.wake.notify_all();
            } else {
                self.wake.notify_one();
            }
        }
    }

    fn take_from_lane(&self, lane: usize) -> Option<Job> {
        loop {
            match self.lanes[lane].steal() {
                Steal::Success(job) => {
                    self.queued.fetch_sub(1, Ordering::Relaxed);
                    return Some(job);
                }
                Steal::Empty => return None,
                Steal::Retry => {}
            }
        }
    }

    fn steal_from_peer(&self, thief: usize, start: usize) -> Option<Job> {
        let peers = self.stealers.len();
        for offset in 0..peers {
            let victim = (start + offset) % peers;
            if victim == thief {
                continue;
            }
            loop {
                match self.stealers[victim].steal() {
                    Steal::Success(job) => {
                        self.steals.fetch_add(1, Ordering::Relaxed);
                        return Some(job);
                    }
                    Steal::Empty => break,
                    Steal::Retry => {}
                }
            }
        }
        None
    }

    fn find_job(&self, index: usize, local: &Worker<Job>, round: usize) -> Option<Job> {
        self.take_from_lane(CRITICAL)
            .or_else(|| local.pop())
            .or_else(|| self.take_from_lane(HIGH))
            .or_else(|| self.take_from_lane(NORMAL))
            .or_else(|| self.steal_from_peer(index, round))
            .or_else(|| self.take_from_lane(LOW))
            .or_else(|| {
                if self.critical_window.load(Ordering::Acquire) {
                    None
                } else {
                    self.take_from_lane(BACKGROUND)
                }
            })
    }

    fn lanes_empty(&self) -> bool {
        self.lanes.iter().all(Injector::is_empty)
    }

    fn park(&self, seen_epoch: u64) {
        let guard = self
            .sleep_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.sleepers.fetch_add(1, Ordering::SeqCst);
        if self.epoch.load(Ordering::SeqCst) == seen_epoch && !self.shutdown.load(Ordering::SeqCst)
        {
            self.parks.fetch_add(1, Ordering::Relaxed);
            drop(
                self.wake
                    .wait_timeout(guard, PARK_TIMEOUT)
                    .unwrap_or_else(PoisonError::into_inner),
            );
        } else {
            drop(guard);
        }
        self.sleepers.fetch_sub(1, Ordering::SeqCst);
    }

    /// Execute une tache sur le fil courant ; `local` est la file du worker qui
    /// l'execute, `None` en `FORCE_SERIAL`.
    pub(crate) fn run(&self, mut job: Job, local: Option<&Worker<Job>>) -> TaskOutcome {
        let group = Arc::clone(&job.group);
        let on_done = job.on_done.take();
        let deadline = job.deadline;
        let outcome = execute(job, self, local);
        if outcome == TaskOutcome::Panicked {
            self.panics.fetch_add(1, Ordering::Relaxed);
        }
        if outcome == TaskOutcome::Timeout || deadline.is_some_and(|at| Instant::now() > at) {
            self.deadline_misses.fetch_add(1, Ordering::Relaxed);
        }
        self.executed.fetch_add(1, Ordering::Relaxed);
        if let Some(on_done) = on_done {
            on_done(outcome);
        }
        group.finish(outcome);
        outcome
    }

    /// Met une tache deja admise en file, sans contre-pression : c'est un dependant
    /// libere d'un graphe dont l'admission a eu lieu au depart.
    pub(crate) fn dispatch(&self, job: Job) {
        job.group.enter();
        self.requeue(job);
    }

    /// FM-24 : attend la fin de `group`, au plus jusqu'a `deadline`, sans laisser une
    /// tache `CRITICAL` en file plus de [`FAMINE_GRACE`].
    ///
    /// Passe ce delai, le fil qui attend prend lui-meme le travail `CRITICAL` restant :
    /// c'est le fil autoritatif, qui n'a rien d'autre a faire que d'attendre son tick. Il
    /// ne prend jamais une tache d'une autre priorite. L'attente cede le processeur
    /// (`yield`) au lieu de dormir : sous Windows, un sommeil d'une milliseconde peut en
    /// durer quinze, et la famine ne serait plus detectee a temps.
    ///
    /// Renvoie `true` si le groupe est vide au retour.
    pub(crate) fn wait_helping_critical(&self, group: &TaskGroup, deadline: Instant) -> bool {
        let started = Instant::now();
        loop {
            if group.pending() == 0 {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            if now.duration_since(started) >= FAMINE_GRACE {
                if let Some(job) = self.take_from_lane(CRITICAL) {
                    self.famine_escalations.fetch_add(1, Ordering::Relaxed);
                    self.run(job, None);
                    continue;
                }
            }
            std::thread::yield_now();
        }
    }

    /// Remet en file une tache deja comptee dans son groupe.
    fn requeue(&self, job: Job) {
        self.queued.fetch_add(1, Ordering::Relaxed);
        self.lanes[lane(job.priority)].push(job);
        self.notify();
    }

    fn is_active(&self, index: usize) -> bool {
        index < self.active.load(Ordering::Acquire)
    }

    /// `true` si les taches s'executent sur le fil qui soumet : `FORCE_SERIAL`, ou
    /// sur-souscription persistante (FM-25).
    pub(crate) fn runs_serial(&self) -> bool {
        self.workers == 0 || self.serial_fallback.load(Ordering::Acquire)
    }
}

/// Execute une tache, sans jamais laisser une panic remonter (R-524).
fn execute(mut job: Job, shared: &Shared, local: Option<&Worker<Job>>) -> TaskOutcome {
    if job.cancel.is_cancelled() {
        return TaskOutcome::Rejected(RejectReason::Cancelled);
    }
    if job.deadline.is_some_and(|at| Instant::now() >= at) {
        return TaskOutcome::Timeout;
    }
    let mut cx = TaskContext {
        cancel: &job.cancel,
        deadline: job.deadline,
        priority: job.priority,
        depth: job.depth,
        shared,
        local,
    };
    catch_unwind(AssertUnwindSafe(|| job.task.run(&mut cx))).unwrap_or(TaskOutcome::Panicked)
}

/// Parallelisme imbrique (PARTIE 5.17.9).
///
/// Une sous-tache rejoint la file du worker qui la lance ; un autre worker peut la voler.
/// Pour l'attendre, la tache qui l'a lancee travaille au lieu de bloquer (`join`), mais
/// seulement sur des taches **plus profondes** qu'elle, prises dans sa propre file. Deux
/// consequences :
///
/// - la pile ne croit que d'un cadre par niveau d'imbrication, jamais au gre du travail
///   en attente ;
/// - aucun interblocage : au-dela de `max_nesting`, une sous-tache s'execute en ligne,
///   donc les taches les plus profondes n'attendent personne ; et une tache qui attend
///   trouve ses enfants dans sa propre file, ou les sait en cours chez un voleur.
impl TaskContext<'_> {
    /// Lance une sous-tache dans `group`. Elle herite du jeton d'annulation, de
    /// l'echeance et de la priorite de la tache courante.
    ///
    /// Au-dela de la profondeur maximale, ou en `FORCE_SERIAL`, elle s'execute tout de
    /// suite, sur ce fil.
    pub fn spawn<T: Task + 'static>(&self, group: &TaskGroup, task: T) {
        let job = Job {
            task: Box::new(task),
            group: Arc::clone(&group.0),
            cancel: self.cancel.clone(),
            deadline: self.deadline,
            priority: self.priority,
            depth: self.depth.saturating_add(1),
            on_done: None,
        };
        group.0.enter();
        match self.local {
            Some(local) if job.depth <= self.shared.max_nesting => {
                local.push(job);
                self.shared.notify();
            }
            _ => {
                self.shared.inline_spawns.fetch_add(1, Ordering::Relaxed);
                self.shared.run(job, self.local);
            }
        }
    }

    /// Attend la fin de toutes les taches de `group`, en executant celles de sa propre
    /// file qui sont plus profondes que la tache courante.
    pub fn join(&self, group: &TaskGroup) {
        while group.pending() > 0 {
            if let Some(local) = self.local {
                match local.pop() {
                    Some(job) if job.depth > self.depth => {
                        self.shared.run(job, Some(local));
                        continue;
                    }
                    // Plus ancienne que la tache courante : elle reste en file, pour ce
                    // worker une fois libre, ou pour un voleur.
                    Some(job) => local.push(job),
                    None => {}
                }
            }
            std::thread::yield_now();
        }
    }
}

fn worker_loop(index: usize, local: &Worker<Job>, shared: &Shared) {
    let mut idle_rounds = 0u32;
    let mut round = index;
    loop {
        let seen_epoch = shared.epoch.load(Ordering::SeqCst);
        round = round.wrapping_add(1);
        let active = shared.is_active(index);
        if let Some(job) = active
            .then(|| shared.find_job(index, local, round))
            .flatten()
        {
            idle_rounds = 0;
            if shared.is_active(index) {
                shared.run(job, Some(local));
            } else {
                // Rendu inactif entre-temps : la tache revient a un worker actif.
                shared.requeue(job);
            }
            continue;
        }
        if shared.shutdown.load(Ordering::SeqCst) {
            if shared.lanes_empty() {
                return;
            }
            continue;
        }
        if !active {
            shared.park(seen_epoch);
            continue;
        }
        idle_rounds += 1;
        match idle_rounds {
            0..=2 => std::hint::spin_loop(),
            3..=8 => std::thread::yield_now(),
            _ => {
                shared.park(seen_epoch);
                idle_rounds = 0;
            }
        }
    }
}

/// Ordonnanceur a vol de travail (C-17).
pub struct Scheduler {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
    config: SchedulerConfig,
}

impl std::fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scheduler")
            .field("workers", &self.threads.len())
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Scheduler {
    /// Demarre les workers.
    ///
    /// En `FORCE_SERIAL`, aucun worker n'est demarre : tout s'execute sur le fil qui
    /// soumet.
    ///
    /// # Errors
    ///
    /// L'erreur du systeme si un fil ne peut pas etre cree ; les fils deja crees sont
    /// alors arretes.
    pub fn start(config: SchedulerConfig) -> std::io::Result<Self> {
        let workers = if config.force_serial {
            0
        } else {
            config.effective_workers()
        };
        let locals: Vec<Worker<Job>> = (0..workers).map(|_| Worker::new_lifo()).collect();
        let shared = Arc::new(Shared {
            lanes: Default::default(),
            stealers: locals.iter().map(Worker::stealer).collect(),
            queued: AtomicUsize::new(0),
            critical_window: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            epoch: AtomicU64::new(0),
            sleepers: AtomicUsize::new(0),
            sleep_lock: Mutex::new(()),
            wake: Condvar::new(),
            executed: AtomicU64::new(0),
            steals: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            panics: AtomicU64::new(0),
            parks: AtomicU64::new(0),
            deadline_misses: AtomicU64::new(0),
            inline_spawns: AtomicU64::new(0),
            max_nesting: config.max_nesting,
            workers,
            active: AtomicUsize::new(workers),
            oversubscribed_streak: AtomicU32::new(0),
            serial_fallback: AtomicBool::new(false),
            famine_escalations: AtomicU64::new(0),
        });
        let mut scheduler = Self {
            shared,
            threads: Vec::with_capacity(workers),
            config,
        };
        for (index, local) in locals.into_iter().enumerate() {
            let shared = Arc::clone(&scheduler.shared);
            let spawned = std::thread::Builder::new()
                .name(format!("rfx-worker-{index}"))
                .spawn(move || worker_loop(index, &local, &shared));
            match spawned {
                Ok(handle) => scheduler.threads.push(handle),
                Err(error) => {
                    scheduler.stop();
                    return Err(error);
                }
            }
        }
        Ok(scheduler)
    }

    /// Nombre de workers demarres.
    #[must_use]
    pub fn workers(&self) -> usize {
        self.threads.len()
    }

    /// Soumet une tache.
    ///
    /// # Errors
    ///
    /// [`Backpressure`] (`E-2401`) si la file depasse le seuil de la priorite : `LOW` et
    /// `BACKGROUND` au-dela de `high_watermark`, `NORMAL` au-dela de `hard_limit`.
    /// `HIGH` et `CRITICAL` ne sont jamais refusees.
    pub fn submit<T: Task + 'static>(
        &self,
        group: &TaskGroup,
        options: JobOptions,
        task: T,
    ) -> Result<(), Backpressure> {
        let queued = self.shared.queued.load(Ordering::Relaxed);
        let refused = match options.priority {
            Priority::Low | Priority::Background => queued >= self.config.high_watermark,
            Priority::Normal => queued >= self.config.hard_limit,
            Priority::High | Priority::Critical => false,
        };
        if refused {
            self.shared.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(Backpressure {
                priority: options.priority,
                queued,
            });
        }
        let job = Job {
            task: Box::new(task),
            group: Arc::clone(&group.0),
            cancel: options.cancel,
            deadline: options.deadline,
            priority: options.priority,
            depth: 0,
            on_done: None,
        };
        if self.shared.runs_serial() {
            // FORCE_SERIAL, ou sur-souscription persistante : sur le fil qui soumet.
            group.0.enter();
            self.shared.run(job, None);
            return Ok(());
        }
        self.shared.dispatch(job);
        Ok(())
    }

    /// Etat partage avec les workers, pour l'execution d'un graphe.
    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// Releve de la charge externe : nombre de fils executables qui ne sont pas des
    /// workers du pool (R-421, ADR-045).
    ///
    /// Au-dessus du seuil, les workers actifs sont divises par deux, sans descendre sous
    /// un ; a un seul worker, huit releves sur-souscrits de suite font passer en
    /// execution serie (FM-25). Sous le seuil, l'execution serie cesse et un worker de
    /// plus est reactive. A appeler depuis un seul fil, a cadence reguliere.
    pub fn report_external_load(&self, runnable_threads: usize) {
        let shared = &self.shared;
        if shared.workers == 0 {
            return;
        }
        let active = shared.active.load(Ordering::Acquire);
        if runnable_threads > self.config.oversubscription_threshold {
            if active > 1 {
                shared.active.store((active / 2).max(1), Ordering::Release);
                shared.oversubscribed_streak.store(0, Ordering::Relaxed);
            } else if shared.oversubscribed_streak.fetch_add(1, Ordering::Relaxed) + 1
                >= SERIAL_FALLBACK_STREAK
            {
                shared.serial_fallback.store(true, Ordering::Release);
            }
            return;
        }
        shared.oversubscribed_streak.store(0, Ordering::Relaxed);
        shared.serial_fallback.store(false, Ordering::Release);
        if active < shared.workers {
            shared.active.store(active + 1, Ordering::Release);
            shared.notify();
        }
    }

    /// Ouvre ou ferme la fenetre critique : pendant qu'elle est ouverte, aucune tache
    /// `BACKGROUND` ne demarre (R-423).
    pub fn set_critical_window(&self, open: bool) {
        self.shared.critical_window.store(open, Ordering::Release);
        if !open {
            self.shared.notify();
        }
    }

    /// Taches en attente dans les files d'injection (`rfx.sched.queue_depth`).
    #[must_use]
    pub fn queue_depth(&self) -> usize {
        self.shared.queued.load(Ordering::Relaxed)
    }

    /// Compteurs.
    #[must_use]
    pub fn stats(&self) -> SchedulerStats {
        let read = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        SchedulerStats {
            executed: read(&self.shared.executed),
            steals: read(&self.shared.steals),
            rejected_submissions: read(&self.shared.rejected),
            panics: read(&self.shared.panics),
            parks: read(&self.shared.parks),
            deadline_misses: read(&self.shared.deadline_misses),
            inline_spawns: read(&self.shared.inline_spawns),
            active_workers: self.shared.active.load(Ordering::Acquire),
            serial_fallback: self.shared.serial_fallback.load(Ordering::Acquire),
            famine_escalations: read(&self.shared.famine_escalations),
        }
    }

    /// Arrete l'ordonnanceur apres avoir execute tout ce qui a ete soumis.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
        {
            let _guard = self
                .shared
                .sleep_lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            self.shared.wake.notify_all();
        }
        for handle in self.threads.drain(..) {
            // Un worker ne panique pas : les taches sont executees sous `catch_unwind`.
            let _ = handle.join();
        }
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.stop();
    }
}
