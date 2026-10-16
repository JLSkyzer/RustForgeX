//! Tests du graphe de taches (C-16) : T-250, T-251, T-253.

use rfx_model::WorkId;
use rfx_taskgraph::{
    CancelToken, Deadline, GraphError, Priority, TaskGraph, TaskId, TaskKind, TaskPayload, TaskSpec,
};

/// Generateur pseudo-aleatoire deterministe : un test qui echoue doit echouer pareil.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

fn spec(priority: Priority, deadline: Deadline) -> TaskSpec {
    TaskSpec {
        work: WorkId(1),
        kind: TaskKind::Analysis,
        priority,
        deadline,
        estimated_ns: 1_000,
        payload: TaskPayload::Unit(0),
        cancel: CancelToken::new(),
    }
}

fn task(graph: &mut TaskGraph) -> TaskId {
    graph.add_task(spec(Priority::Normal, Deadline::None))
}

/// Reference naive : `from` atteint-il `to` en suivant les dependances ?
fn reaches(deps: &[Vec<usize>], from: usize, to: usize) -> bool {
    let mut seen = vec![false; deps.len()];
    let mut stack = vec![from];
    while let Some(node) = stack.pop() {
        if node == to {
            return true;
        }
        if !std::mem::replace(&mut seen[node], true) {
            stack.extend(&deps[node]);
        }
    }
    false
}

/// T-250 : les trois formes de cycle sont refusees, comptees, et laissent le graphe
/// intact.
#[test]
fn t250_every_cycle_is_refused_and_the_graph_is_left_intact() {
    let mut graph = TaskGraph::new();
    let a = task(&mut graph);
    let b = task(&mut graph);
    let c = task(&mut graph);
    graph.add_dependency(b, a).unwrap();
    graph.add_dependency(c, b).unwrap();

    let self_loop = graph.add_dependency(a, a);
    let two_cycle = graph.add_dependency(a, b);
    let three_cycle = graph.add_dependency(a, c);

    for refused in [self_loop, two_cycle, three_cycle] {
        let error = refused.unwrap_err();
        assert!(matches!(error, GraphError::Cycle { .. }));
        assert_eq!(error.code(), Some(2301));
    }
    assert_eq!(graph.stats().cycles_rejected, 3);
    assert!(
        graph.task(a).unwrap().deps.is_empty(),
        "aucune arete refusee n'est gardee"
    );
    assert_eq!(graph.dependents(c), &[] as &[TaskId]);
    assert_eq!(graph.depth(), 3);
}

/// T-250 : sur des milliers d'aretes tirees au hasard, dans les deux sens, le graphe
/// refuse exactement celles que la reference naive designe comme fermant un cycle, et
/// reste ordonnable jusqu'au bout.
#[test]
fn t250_random_edges_are_refused_exactly_when_they_close_a_cycle() {
    for seed in 1..=20u64 {
        let mut random = XorShift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let count = 60;
        let mut graph = TaskGraph::new();
        let ids: Vec<TaskId> = (0..count).map(|_| task(&mut graph)).collect();
        let mut reference: Vec<Vec<usize>> = vec![Vec::new(); count];
        for _ in 0..2_000 {
            let (from, to) = (random.below(count), random.below(count));
            let closes_cycle = from == to || reaches(&reference, to, from);
            let result = graph.add_dependency(ids[from], ids[to]);
            assert_eq!(
                result.is_err(),
                closes_cycle,
                "graine {seed}, arete {from} -> {to}"
            );
            if result.is_ok() && !reference[from].contains(&to) {
                reference[from].push(to);
            }
        }
        let ordered: usize = graph.waves().iter().map(Vec::len).sum();
        assert_eq!(
            ordered, count,
            "graine {seed} : toute tache reste ordonnable"
        );
    }
}

#[test]
fn an_unknown_task_is_refused_without_a_normative_code() {
    let mut graph = TaskGraph::new();
    let a = task(&mut graph);
    let mut other = TaskGraph::new();
    task(&mut other);
    let foreign = task(&mut other);
    let error = graph.add_dependency(a, foreign).unwrap_err();
    assert_eq!(error, GraphError::UnknownTask(foreign));
    assert_eq!(error.code(), None);
}

/// T-251 : chaque tache vient apres toutes ses dependances, et une vague va de la plus
/// urgente a la moins urgente.
#[test]
fn t251_waves_respect_every_dependency_and_put_urgent_tasks_first() {
    let priorities = [
        Priority::Background,
        Priority::Low,
        Priority::Normal,
        Priority::High,
        Priority::Critical,
    ];
    for seed in 1..=20u64 {
        let mut random = XorShift(seed.wrapping_mul(0xD1B5_4A32_D192_ED03));
        let mut graph = TaskGraph::new();
        let ids: Vec<TaskId> = (0..80)
            .map(|_| graph.add_task(spec(priorities[random.below(5)], Deadline::None)))
            .collect();
        for _ in 0..300 {
            let (from, to) = (random.below(80), random.below(80));
            let _ = graph.add_dependency(ids[from], ids[to]);
        }
        let waves = graph.waves();
        let mut wave_of = vec![usize::MAX; 80];
        for (level, wave) in waves.iter().enumerate() {
            for id in wave {
                wave_of[id.index()] = level;
            }
            let urgency: Vec<Priority> = wave
                .iter()
                .map(|id| graph.task(*id).unwrap().priority)
                .collect();
            assert!(
                urgency.windows(2).all(|pair| pair[0] >= pair[1]),
                "graine {seed}"
            );
        }
        for node in graph.tasks() {
            for dep in &node.deps {
                assert!(
                    wave_of[dep.index()] < wave_of[node.id.index()],
                    "graine {seed}"
                );
            }
        }
        assert_eq!(graph.depth(), waves.len());
    }
}

/// T-253 : a la fenetre de drain, une tache `THIS_TICK` non terminee est annulee, ce qui
/// en depend aussi, et leur travail retombe sur Java. Le reste n'est pas touche.
#[test]
fn t253_unfinished_this_tick_tasks_and_their_dependents_are_cancelled_at_drain() {
    let mut graph = TaskGraph::new();
    let with_work = |work: u64, deadline: Deadline| TaskSpec {
        work: WorkId(work),
        ..spec(Priority::High, deadline)
    };
    let late = graph.add_task(with_work(10, Deadline::ThisTick));
    let after_late = graph.add_task(with_work(11, Deadline::Ticks(2)));
    let on_time = graph.add_task(with_work(12, Deadline::ThisTick));
    let after_on_time = graph.add_task(with_work(13, Deadline::Ticks(1)));
    let unrelated = graph.add_task(with_work(10, Deadline::None));
    let late_twin = graph.add_task(with_work(10, Deadline::ThisTick));
    graph.add_dependency(after_late, late).unwrap();
    graph.add_dependency(after_on_time, on_time).unwrap();

    let report = graph.cancel_at_drain(|id| id == on_time);

    assert_eq!(report.cancelled, vec![late, after_late, late_twin]);
    assert_eq!(
        report.fallback,
        vec![WorkId(10), WorkId(11)],
        "sans doublon"
    );
    for id in [late, after_late, late_twin] {
        assert!(graph.task(id).unwrap().cancel.is_cancelled());
    }
    for id in [on_time, after_on_time, unrelated] {
        assert!(
            !graph.task(id).unwrap().cancel.is_cancelled(),
            "{id:?} n'est pas touchee"
        );
    }
}

/// Construction naturelle (aretes descendantes) : aucun parcours, et le resultat est le
/// meme qu'avec le parcours.
#[test]
fn descending_edges_never_close_a_cycle() {
    let mut graph = TaskGraph::with_capacity(500);
    let ids: Vec<TaskId> = (0..500).map(|_| task(&mut graph)).collect();
    for (index, &id) in ids.iter().enumerate().skip(1) {
        graph.add_dependency(id, ids[index - 1]).unwrap();
        graph.add_dependency(id, ids[index / 2]).unwrap();
    }
    assert_eq!(graph.stats().cycles_rejected, 0);
    assert_eq!(graph.waves().iter().map(Vec::len).sum::<usize>(), 500);
    assert!(
        graph.add_dependency(ids[0], ids[499]).is_err(),
        "l'arete montante est jugee"
    );
}
