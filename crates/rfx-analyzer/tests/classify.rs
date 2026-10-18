//! T-170 : la fonction des drapeaux est pure, et sa table de verite est complete sur
//! l'abstraction des faits ; T-011 : ses resultats respectent les regles de coherence
//! (C-08, ADR-054).
//!
//! Chaque fait prend un petit nombre d'etats qui couvrent tous les cas que la fonction
//! distingue : absent, incertain, connu et favorable, connu et defavorable. Toutes les
//! combinaisons sont parcourues (315 000), et les drapeaux attendus sont recalcules ici a
//! partir des seules etiquettes, sans reutiliser les predicats du code teste.

use rfx_analyzer::{classify, confidence, Facts};
use rfx_model::{
    AccessSet, ClassificationFlags as F, DependencySet, DeterminismFacts, Domain, Domains, Heat,
    Precision, Side, SideEffectSet, ThreadFacts, ThreadRole, ThreadRoles, Tri,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Deps {
    Absent,
    Clean,
    Truncated,
    Unresolved,
    DataUnanalysed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reads {
    Absent,
    Imprecise,
    Exact,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Writes {
    Absent,
    Imprecise,
    None,
    World,
    Statics,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Effects {
    Absent,
    Unanalysable,
    None,
    MutatesWorld,
    Native,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Thread {
    Absent,
    MainYes,
    MainUnknown,
    FreeSafe,
    FreeThreadLocals,
    /// Vue sur le seul fil autoritatif.
    MainYesSeenOnce,
    /// Vue sur deux roles de fil.
    MainYesSeenTwice,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Det {
    Absent,
    YesUnordered,
    Unknown,
    YesOrderUnknown,
    No,
}

/// Une combinaison d'etats.
#[derive(Clone, Copy, Debug)]
struct Case {
    deps: Deps,
    reads: Reads,
    writes: Writes,
    effects: Effects,
    thread: Thread,
    det: Det,
    reflection: bool,
    heat: Heat,
    side: Side,
}

/// Drapeaux que C-08 ne pose jamais (ADR-054, decision 2).
const NEVER: [F; 5] = [
    F::BATCHABLE,
    F::RUST_CANDIDATE,
    F::IR_ELIGIBLE,
    F::CPU_INTENSIVE,
    F::MEMORY_INTENSIVE,
];

fn access(domains: Domains, precision: Precision) -> AccessSet {
    AccessSet { domains, precision }
}

fn facts(c: Case) -> Facts {
    let clean = DependencySet {
        data_deps: Some(Vec::new()),
        order_deps: Some(Vec::new()),
        external_locks: Some(Vec::new()),
        ..DependencySet::default()
    };
    Facts {
        heat: c.heat,
        side: c.side,
        observation_quality: 0.5,
        deps: match c.deps {
            Deps::Absent => None,
            Deps::Clean => Some(clean),
            Deps::Truncated => Some(DependencySet {
                truncated: true,
                ..clean
            }),
            Deps::Unresolved => Some(DependencySet {
                unresolved_calls: 2,
                ..clean
            }),
            Deps::DataUnanalysed => Some(DependencySet {
                data_deps: None,
                ..clean
            }),
        },
        reflection: c.reflection,
        reads: match c.reads {
            Reads::Absent => None,
            Reads::Imprecise => Some(access(Domains::EMPTY, Precision::Unknown)),
            Reads::Exact => Some(access(
                Domains::EMPTY.with(Domain::WorldBlocks),
                Precision::Exact,
            )),
        },
        writes: match c.writes {
            Writes::Absent => None,
            Writes::Imprecise => Some(access(Domains::EMPTY, Precision::Unknown)),
            Writes::None => Some(access(Domains::EMPTY, Precision::Exact)),
            Writes::World => Some(access(
                Domains::EMPTY.with(Domain::WorldBlocks),
                Precision::OverApprox,
            )),
            Writes::Statics => Some(access(
                Domains::EMPTY.with(Domain::ModStatics),
                Precision::Exact,
            )),
        },
        side_effects: match c.effects {
            Effects::Absent => None,
            Effects::Unanalysable => Some(SideEffectSet {
                unknown: true,
                ..SideEffectSet::default()
            }),
            Effects::None => Some(SideEffectSet::default()),
            Effects::MutatesWorld => Some(SideEffectSet {
                mutates_world: true,
                ..SideEffectSet::default()
            }),
            Effects::Native => Some(SideEffectSet {
                native_call: true,
                ..SideEffectSet::default()
            }),
        },
        thread: match c.thread {
            Thread::Absent => None,
            Thread::MainYes => Some(ThreadFacts {
                main_thread_required: Tri::Yes,
                ..ThreadFacts::default()
            }),
            Thread::MainUnknown => Some(ThreadFacts::default()),
            Thread::FreeSafe => Some(ThreadFacts {
                main_thread_required: Tri::No,
                ..ThreadFacts::default()
            }),
            Thread::FreeThreadLocals => Some(ThreadFacts {
                main_thread_required: Tri::No,
                uses_thread_locals: true,
                ..ThreadFacts::default()
            }),
            Thread::MainYesSeenOnce => Some(ThreadFacts {
                main_thread_required: Tri::Yes,
                observed_roles: Some(ThreadRoles::EMPTY.with(ThreadRole::Authoritative)),
                ..ThreadFacts::default()
            }),
            Thread::MainYesSeenTwice => Some(ThreadFacts {
                main_thread_required: Tri::Yes,
                observed_roles: Some(
                    ThreadRoles::EMPTY
                        .with(ThreadRole::Authoritative)
                        .with(ThreadRole::Netty),
                ),
                ..ThreadFacts::default()
            }),
        },
        determinism: match c.det {
            Det::Absent => None,
            Det::YesUnordered => Some(DeterminismFacts {
                deterministic: Tri::Yes,
                order_sensitive: Tri::No,
                ..DeterminismFacts::default()
            }),
            Det::Unknown => Some(DeterminismFacts {
                order_sensitive: Tri::No,
                ..DeterminismFacts::default()
            }),
            Det::YesOrderUnknown => Some(DeterminismFacts {
                deterministic: Tri::Yes,
                ..DeterminismFacts::default()
            }),
            Det::No => Some(DeterminismFacts {
                deterministic: Tri::No,
                order_sensitive: Tri::No,
                ..DeterminismFacts::default()
            }),
        },
    }
}

/// Les drapeaux attendus, d'apres les seules etiquettes (ADR-054, tableau).
fn expected(c: Case) -> F {
    let complete = c.deps == Deps::Clean
        && c.reads == Reads::Exact
        && !matches!(c.writes, Writes::Absent | Writes::Imprecise)
        && !matches!(c.effects, Effects::Absent | Effects::Unanalysable)
        && c.thread != Thread::Absent
        && c.det != Det::Absent;
    let read_only = c.writes == Writes::None && c.effects == Effects::None;
    let deterministic = matches!(c.det, Det::YesUnordered | Det::YesOrderUnknown);
    let order_sensitive = !matches!(c.det, Det::YesUnordered | Det::Unknown | Det::No);
    let pure = complete && read_only && deterministic;
    // R-391 : THREAD_SAFE exige PURE.
    let thread_safe = c.thread == Thread::FreeSafe && pure;
    // R-390 : ecrire en etant vu (ou pas vu) sur plusieurs roles de fil.
    let multi_thread_writer = c.writes != Writes::None && c.thread != Thread::MainYesSeenOnce;
    F::EMPTY
        .set(F::UNKNOWN, !complete)
        .set(F::WRITE, c.writes != Writes::None)
        .set(F::SIDE_EFFECT, c.effects != Effects::None)
        .set(F::READ_ONLY, read_only)
        .set(F::DETERMINISTIC, deterministic)
        .set(F::ORDER_SENSITIVE, order_sensitive)
        .set(
            F::MAIN_THREAD_REQUIRED,
            !matches!(c.thread, Thread::FreeSafe | Thread::FreeThreadLocals),
        )
        .set(F::THREAD_SAFE, thread_safe)
        .set(
            F::STATEFUL,
            matches!(
                c.writes,
                Writes::Absent | Writes::Imprecise | Writes::Statics
            ),
        )
        .set(F::PURE, pure)
        .set(F::PARALLELIZABLE, pure && thread_safe && !order_sensitive)
        .set(F::CACHEABLE, pure)
        .set(
            F::UNSAFE,
            c.reflection || c.effects == Effects::Native || multi_thread_writer,
        )
        .set(F::HOT, matches!(c.heat, Heat::Hot | Heat::Critical))
        .set(F::CLIENT_ONLY, c.side == Side::Client)
        .set(F::SERVER_ONLY, c.side == Side::Server)
}

/// Toutes les combinaisons d'etats.
fn all_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for deps in [
        Deps::Absent,
        Deps::Clean,
        Deps::Truncated,
        Deps::Unresolved,
        Deps::DataUnanalysed,
    ] {
        for reads in [Reads::Absent, Reads::Imprecise, Reads::Exact] {
            for writes in [
                Writes::Absent,
                Writes::Imprecise,
                Writes::None,
                Writes::World,
                Writes::Statics,
            ] {
                for effects in [
                    Effects::Absent,
                    Effects::Unanalysable,
                    Effects::None,
                    Effects::MutatesWorld,
                    Effects::Native,
                ] {
                    for thread in [
                        Thread::Absent,
                        Thread::MainYes,
                        Thread::MainUnknown,
                        Thread::FreeSafe,
                        Thread::FreeThreadLocals,
                        Thread::MainYesSeenOnce,
                        Thread::MainYesSeenTwice,
                    ] {
                        for det in [
                            Det::Absent,
                            Det::YesUnordered,
                            Det::Unknown,
                            Det::YesOrderUnknown,
                            Det::No,
                        ] {
                            for reflection in [false, true] {
                                for heat in [Heat::Cold, Heat::Warm, Heat::Hot, Heat::Critical] {
                                    for side in [Side::Client, Side::Server, Side::Common] {
                                        cases.push(Case {
                                            deps,
                                            reads,
                                            writes,
                                            effects,
                                            thread,
                                            det,
                                            reflection,
                                            heat,
                                            side,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    cases
}

/// T-170, T-011, R-215, R-218, P-02 sur les 315 000 combinaisons.
#[test]
fn t170_the_flag_function_matches_its_complete_truth_table() {
    let cases = all_cases();
    assert_eq!(cases.len(), 315_000);
    let (mut pure_seen, mut parallel_seen) = (0_u32, 0_u32);
    for case in cases {
        let f = facts(case);
        let flags = classify(&f);
        assert_eq!(flags, expected(case), "{case:?}");
        // Pure : meme entree, meme sortie (R-351).
        assert_eq!(classify(&f.clone()), flags, "{case:?}");
        // T-011.
        assert_eq!(flags.incoherence(), None, "{case:?}");
        // R-215 : tronque ou non resolu => UNKNOWN.
        if matches!(case.deps, Deps::Truncated | Deps::Unresolved) {
            assert!(flags.contains(F::UNKNOWN), "{case:?}");
        }
        // P-02 : un fait absent ou incertain ferme tout.
        if flags.contains(F::UNKNOWN) {
            for open in [F::PURE, F::PARALLELIZABLE, F::CACHEABLE] {
                assert!(!flags.contains(open), "{case:?}");
            }
        }
        for never in NEVER {
            assert!(!flags.contains(never), "{case:?}");
        }
        // R-218 : jamais au-dela de la preuve statique.
        let c = confidence(&f);
        assert!(c.combined <= c.static_analysis, "{case:?}");
        assert_eq!(
            c.static_analysis >= 1.0,
            !flags.contains(F::UNKNOWN),
            "{case:?}"
        );
        pure_seen += u32::from(flags.contains(F::PURE));
        parallel_seen += u32::from(flags.contains(F::PARALLELIZABLE));
    }
    // La table n'est pas vide de cas permissifs : la fonction sait les poser.
    assert!(
        pure_seen > 0 && parallel_seen > 0,
        "{pure_seen} {parallel_seen}"
    );
}

/// Aujourd'hui, seules C-05 et C-07 produisent des faits : tout descripteur est
/// UNKNOWN et n'ouvre rien, quelle que soit la qualite du graphe (ADR-054).
#[test]
fn with_only_the_call_graph_everything_stays_unknown() {
    let f = Facts {
        heat: Heat::Critical,
        side: Side::Server,
        observation_quality: 1.0,
        deps: Some(DependencySet::default()),
        ..Facts::default()
    };
    let flags = classify(&f);
    for closed in [
        F::UNKNOWN,
        F::WRITE,
        F::SIDE_EFFECT,
        F::STATEFUL,
        F::MAIN_THREAD_REQUIRED,
        F::ORDER_SENSITIVE,
        F::HOT,
        F::SERVER_ONLY,
    ] {
        assert!(flags.contains(closed));
    }
    for open in [
        F::READ_ONLY,
        F::PURE,
        F::DETERMINISTIC,
        F::THREAD_SAFE,
        F::PARALLELIZABLE,
        F::CACHEABLE,
    ] {
        assert!(!flags.contains(open));
    }
    assert!(confidence(&f).combined.abs() < f32::EPSILON);
}
