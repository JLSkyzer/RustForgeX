//! T-252 : la fusion et le decoupage n'alterent pas le resultat (C-16).
//!
//! Un programme tire au hasard — des travaux unitaires et des lots, chacun lisant le
//! resultat de travaux precedents — est execute vague par vague par un executant qui
//! **refuse** toute tache dont une entree n'est pas prete, et tout element execute deux
//! fois. Le meme programme, fusionne puis decoupe, doit produire exactement les memes
//! valeurs.

use std::collections::HashMap;

use rfx_model::WorkId;
use rfx_taskgraph::{
    CancelToken, Deadline, Priority, TaskGraph, TaskId, TaskKind, TaskPayload, TaskSpec,
};

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn range(&mut self, low: u64, high: u64) -> u64 {
        low + self.next() % (high - low)
    }
}

/// Un travail du programme ; sa poignee est son rang.
struct Job {
    work: u64,
    /// `None` pour un travail unitaire, la taille du lot sinon.
    batch: Option<u32>,
    inputs: Vec<usize>,
    estimated_ns: u64,
}

fn program(seed: u64) -> Vec<Job> {
    let mut random = XorShift(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
    let mut jobs: Vec<Job> = Vec::new();
    for index in 0..60usize {
        let batch = (random.range(0, 10) < 3).then(|| random.range(20, 200) as u32);
        let mut inputs = Vec::new();
        if index > 0 && random.range(0, 10) < 7 {
            inputs.push(index - 1);
        }
        for _ in 0..random.range(0, 3) {
            if index > 0 {
                let input = random.range(0, index as u64) as usize;
                if !inputs.contains(&input) {
                    inputs.push(input);
                }
            }
        }
        let estimated_ns = if batch.is_some() {
            random.range(50_000, 200_000)
        } else {
            random.range(1_000, 10_000)
        };
        // Des blocs de travaux consecutifs du meme travail : c'est ce qui se fusionne.
        jobs.push(Job {
            work: index as u64 / 5,
            batch,
            inputs,
            estimated_ns,
        });
    }
    jobs
}

fn build(jobs: &[Job]) -> TaskGraph {
    let mut tokens: HashMap<u64, CancelToken> = HashMap::new();
    let mut graph = TaskGraph::new();
    let ids: Vec<TaskId> = jobs
        .iter()
        .enumerate()
        .map(|(handle, job)| {
            graph.add_task(TaskSpec {
                work: WorkId(job.work),
                kind: TaskKind::Analysis,
                priority: Priority::Normal,
                deadline: Deadline::None,
                estimated_ns: job.estimated_ns,
                payload: match job.batch {
                    None => TaskPayload::Unit(handle as u64),
                    Some(len) => TaskPayload::Batch {
                        handle: handle as u64,
                        start: 0,
                        len,
                    },
                },
                cancel: tokens.entry(job.work).or_default().clone(),
            })
        })
        .collect();
    for (index, job) in jobs.iter().enumerate() {
        for &input in &job.inputs {
            graph.add_dependency(ids[index], ids[input]).unwrap();
        }
    }
    graph
}

/// Executant de test : refuse une entree non prete et une double execution.
struct World<'a> {
    jobs: &'a [Job],
    units: HashMap<u64, u64>,
    elements: HashMap<(u64, u32), u64>,
    elements_done: HashMap<u64, u32>,
}

impl World<'_> {
    fn ready(&self, handle: usize) -> bool {
        match self.jobs[handle].batch {
            None => self.units.contains_key(&(handle as u64)),
            Some(len) => self.elements_done.get(&(handle as u64)) == Some(&len),
        }
    }

    fn input_sum(&self, handle: u64) -> u64 {
        let job = &self.jobs[handle as usize];
        let mut sum = 0u64;
        for &input in &job.inputs {
            assert!(
                self.ready(input),
                "{handle} lancee avant son entree {input}"
            );
            sum = sum.wrapping_add(match self.jobs[input].batch {
                None => self.units[&(input as u64)],
                Some(len) => (0..len).fold(0, |acc: u64, element| {
                    acc.wrapping_add(self.elements[&(input as u64, element)])
                }),
            });
        }
        sum
    }

    fn run(&mut self, payload: &TaskPayload) {
        match payload {
            TaskPayload::Unit(handle) => {
                let value = mix(*handle, self.input_sum(*handle));
                assert!(
                    self.units.insert(*handle, value).is_none(),
                    "{handle} deux fois"
                );
            }
            TaskPayload::Batch { handle, start, len } => {
                let inputs = self.input_sum(*handle);
                for element in *start..start + len {
                    let value = mix(handle * 1_000 + u64::from(element), inputs);
                    let first = self.elements.insert((*handle, element), value).is_none();
                    assert!(first, "element {element} de {handle} deux fois");
                    *self.elements_done.entry(*handle).or_default() += 1;
                }
            }
            TaskPayload::Sequence(parts) => {
                for part in parts {
                    self.run(part);
                }
            }
        }
    }
}

fn mix(seed: u64, input: u64) -> u64 {
    (seed ^ 0x9E37_79B9_7F4A_7C15)
        .wrapping_mul(0xBF58_476D_1CE4_E5B9)
        .wrapping_add(input.rotate_left(17))
}

type Outcome = (HashMap<u64, u64>, HashMap<(u64, u32), u64>);

fn execute(graph: &TaskGraph, jobs: &[Job]) -> Outcome {
    let mut world = World {
        jobs,
        units: HashMap::new(),
        elements: HashMap::new(),
        elements_done: HashMap::new(),
    };
    for wave in graph.waves() {
        for id in wave {
            world.run(&graph.task(id).unwrap().payload);
        }
    }
    for handle in 0..jobs.len() {
        assert!(
            world.ready(handle),
            "le travail {handle} n'a pas ete execute en entier"
        );
    }
    (world.units, world.elements)
}

#[test]
fn t252_fusion_and_splitting_leave_the_result_unchanged() {
    let (mut fusions, mut splits) = (0, 0);
    for seed in 1..=30u64 {
        let jobs = program(seed);
        let reference = execute(&build(&jobs), &jobs);

        let mut transformed = build(&jobs);
        fusions += transformed.fuse(30_000);
        splits += transformed.split(40_000, 4, 16);
        let ordered: usize = transformed.waves().iter().map(Vec::len).sum();
        assert_eq!(
            ordered,
            transformed.len(),
            "graine {seed} : toujours acyclique"
        );

        assert_eq!(execute(&transformed, &jobs), reference, "graine {seed}");
    }
    // Un test qui ne fusionne ni ne decoupe rien ne prouverait rien.
    assert!(fusions > 0, "aucune fusion n'a eu lieu");
    assert!(splits > 0, "aucun decoupage n'a eu lieu");
}

fn unit(graph: &mut TaskGraph, work: u64, cancel: &CancelToken) -> TaskId {
    graph.add_task(TaskSpec {
        work: WorkId(work),
        kind: TaskKind::Compute,
        priority: Priority::Normal,
        deadline: Deadline::None,
        estimated_ns: 1_000,
        payload: TaskPayload::Unit(work),
        cancel: cancel.clone(),
    })
}

#[test]
fn a_task_with_two_dependents_or_another_token_is_not_fused() {
    let shared = CancelToken::new();
    let mut graph = TaskGraph::new();
    let a = unit(&mut graph, 1, &shared);
    let b = unit(&mut graph, 1, &shared);
    let c = unit(&mut graph, 1, &shared);
    graph.add_dependency(b, a).unwrap();
    graph.add_dependency(c, a).unwrap();
    let alone = unit(&mut graph, 2, &CancelToken::new());
    let other_token = unit(&mut graph, 2, &CancelToken::new());
    graph.add_dependency(other_token, alone).unwrap();

    assert_eq!(graph.fuse(1_000_000), 0);
    assert_eq!(graph.len(), 5);
}

#[test]
fn a_chain_of_three_becomes_one_task_that_runs_in_order() {
    let shared = CancelToken::new();
    let mut graph = TaskGraph::new();
    let a = unit(&mut graph, 1, &shared);
    let b = unit(&mut graph, 1, &shared);
    let c = unit(&mut graph, 1, &shared);
    graph.add_dependency(b, a).unwrap();
    graph.add_dependency(c, b).unwrap();

    assert_eq!(graph.fuse(1_000_000), 2);

    let survivor = graph.tasks().next().unwrap();
    assert_eq!(graph.len(), 1);
    assert_eq!(graph.stats().fusions, 2);
    assert_eq!(survivor.estimated_ns, 3_000);
    assert_eq!(
        survivor.payload,
        TaskPayload::Sequence(vec![TaskPayload::Unit(1); 3]),
        "une seule sequence, a plat"
    );
}

#[test]
fn a_split_batch_covers_every_element_exactly_once() {
    let mut graph = TaskGraph::new();
    let token = CancelToken::new();
    let before = unit(&mut graph, 1, &token);
    let batch = graph.add_task(TaskSpec {
        work: WorkId(7),
        kind: TaskKind::Compute,
        priority: Priority::High,
        deadline: Deadline::ThisTick,
        estimated_ns: 100_000,
        payload: TaskPayload::Batch {
            handle: 7,
            start: 10,
            len: 103,
        },
        cancel: token.clone(),
    });
    let after = unit(&mut graph, 2, &token);
    graph.add_dependency(batch, before).unwrap();
    graph.add_dependency(after, batch).unwrap();

    assert_eq!(graph.split(50_000, 4, 16), 1);
    assert_eq!(graph.stats().splits, 1);

    let mut ranges: Vec<(u32, u32)> = graph
        .tasks()
        .filter_map(|node| match node.payload {
            TaskPayload::Batch { start, len, .. } => Some((start, len)),
            _ => None,
        })
        .collect();
    ranges.sort_unstable();
    assert_eq!(ranges, vec![(10, 26), (36, 26), (62, 26), (88, 25)]);
    for node in graph.tasks().filter(|node| node.work == WorkId(7)) {
        assert_eq!(node.deps, vec![before]);
        assert!(graph.task(after).unwrap().deps.contains(&node.id));
    }
}
