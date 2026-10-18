//! C-13 : determinisme d'une unite de travail (ADR-058).
//!
//! Cahier des charges : PARTIE 5.13, DM-10, R-217, R-400, R-401 ; tests T-220, T-221.
//! Maturite : `EXPERIMENTAL` — la verification dynamique (shadow runs, C-21) manque ;
//! seule la recherche statique des sources de non-determinisme est faite.
//!
//! Les appels sont lus tels qu'ecrits dans le code analyse, resolus ou non : appeler
//! `Random.nextInt` est une source, que C-07 sache ou non quelle methode s'executera.
//! `deterministic` vaut `NO` des qu'une source definie est trouvee, `UNKNOWN` si seule
//! une partie inconnue du graphe empeche de conclure, `YES` sinon.

use rfx_bytecode::{access, CallTarget, MemberRef};
use rfx_model::{DeterminismFacts, Domain, NonDetSource, Precision, Tri};

use crate::callgraph::CallGraph;
use crate::dataflow::{DomainTable, GraphFacts, WorkloadEffects};
use crate::hierarchy::{ClassIndex, OBJECT};
use crate::thread::is_lock_api;

/// Generateurs pseudo-aleatoires sans ancre de domaine (interfaces et utilitaires).
const RANDOM_TYPES: [&str; 4] = [
    "java/util/Random",
    "net/minecraft/util/RandomSource",
    "java/util/random/RandomGenerator",
    "java/util/concurrent/ThreadLocalRandom",
];

/// Tables de hachage du JDK dont l'ordre d'iteration depend des hachages.
const HASH_COLLECTIONS: [&str; 6] = [
    "java/util/HashMap",
    "java/util/HashSet",
    "java/util/IdentityHashMap",
    "java/util/WeakHashMap",
    "java/util/concurrent/ConcurrentHashMap",
    "java/util/Hashtable",
];

/// Methodes qui parcourent une collection.
const ITERATIONS: [&str; 7] = [
    "keySet",
    "values",
    "entrySet",
    "iterator",
    "forEach",
    "spliterator",
    "stream",
];

/// Paquets d'entrees-sorties externes.
const IO_PACKAGES: [&str; 4] = [
    "java/io/",
    "java/nio/file/",
    "java/nio/channels/",
    "java/net/",
];

/// Arithmetique flottante non associative : `fadd`, `dadd` ... `frem`, `drem`.
const FLOAT_OPCODES: [u8; 10] = [0x62, 0x63, 0x66, 0x67, 0x6a, 0x6b, 0x6e, 0x6f, 0x72, 0x73];

fn bit(source: NonDetSource) -> u16 {
    1 << source as u16
}

/// Sources d'un appel ecrit.
fn call_sources(target: &MemberRef, index: &ClassIndex, domains: &DomainTable) -> u16 {
    let owner = target.owner.as_str();
    let name = target.name.as_str();
    let mut found = 0;
    let random = RANDOM_TYPES.contains(&owner)
        || (owner == "java/lang/Math" && name == "random")
        || domains
            .of_class(index, owner)
            .is_some_and(|d| d.contains(Domain::Random));
    if random {
        found |= bit(NonDetSource::SharedRandom);
    }
    let time = (owner == "java/lang/System" && matches!(name, "currentTimeMillis" | "nanoTime"))
        || (owner.starts_with("java/time/") && name == "now")
        || owner == "java/time/Clock";
    if time {
        found |= bit(NonDetSource::SystemTime);
    }
    let identity = (owner == "java/lang/System" && name == "identityHashCode")
        || (owner == OBJECT && matches!(name, "hashCode" | "toString"));
    if identity {
        found |= bit(NonDetSource::IdentityHash);
    }
    if HASH_COLLECTIONS.contains(&owner) && ITERATIONS.contains(&name) {
        found |= bit(NonDetSource::HashMapIteration);
    }
    if IO_PACKAGES.iter().any(|p| owner.starts_with(p)) {
        found |= bit(NonDetSource::ExternalIo);
    }
    if is_lock_api(target) {
        found |= bit(NonDetSource::ThreadScheduling);
    }
    found
}

/// Faits de determinisme de l'unite dont `graph` est le graphe d'appel, ses methodes
/// analysees (`analysed`) et ses effets composes (`effects`, C-11).
#[must_use]
pub fn determinism_facts(
    graph: &CallGraph,
    index: &ClassIndex,
    analysed: &GraphFacts,
    effects: &WorkloadEffects,
    domains: &DomainTable,
) -> DeterminismFacts {
    let incomplete = effects.causes.any();
    let mut sources = 0_u16;
    if incomplete {
        sources |= bit(NonDetSource::Unknown);
    }
    if graph.reflection() {
        sources |= bit(NonDetSource::Reflection);
    }
    let has = |domain: Domain| {
        effects.reads.domains.contains(domain) || effects.writes.domains.contains(domain)
    };
    if has(Domain::Random) {
        sources |= bit(NonDetSource::SharedRandom);
    }
    if has(Domain::Time) {
        sources |= bit(NonDetSource::SystemTime);
    }
    if has(Domain::FileSystem) || has(Domain::Network) {
        sources |= bit(NonDetSource::ExternalIo);
    }

    let mut float_sensitive = incomplete;
    for (node, facts) in graph.nodes().iter().zip(&analysed.facts) {
        let method = &node.method;
        // `hashCode` ou `toString` d'Object atteint par le graphe : non surcharge.
        if method.owner == OBJECT && matches!(method.name.as_str(), "hashCode" | "toString") {
            sources |= bit(NonDetSource::IdentityHash);
        }
        let found = index.class(&method.owner).and_then(|class| {
            class
                .method(&method.name, &method.descriptor)
                .map(|info| (class, info))
        });
        if let Some((class, info)) = found {
            if info.access & access::NATIVE != 0 {
                sources |= bit(NonDetSource::NativeCall);
            }
            if facts.is_some() && !float_sensitive {
                if let (Some(code), Ok(instructions)) = (&info.code, class.instructions(info)) {
                    float_sensitive = instructions.iter().any(|insn| {
                        code.get(insn.offset as usize)
                            .is_some_and(|op| FLOAT_OPCODES.contains(op))
                    });
                }
            }
        }
        for call in facts.iter().flat_map(|f| &f.calls) {
            if let CallTarget::Member(target) = &call.target {
                sources |= call_sources(target, index, domains);
            }
        }
    }

    let definite = sources & !bit(NonDetSource::Unknown);
    let deterministic = if definite != 0 {
        Tri::No
    } else if incomplete {
        Tri::Unknown
    } else {
        Tri::Yes
    };
    let writes = effects.writes;
    let side = effects.side_effects;
    let writes_known = writes.precision != Precision::Unknown;
    // Ecritures inconnues : C-11 pose `mutates_world` par prudence, ce n'est pas un effet
    // connu ; seul un ensemble d'effets sans part inconnue en fait foi.
    let order_sensitive =
        if writes_known && writes.domains.is_empty() && !side.unknown && !side.any() {
            Tri::No
        } else if (writes_known && !writes.domains.is_empty()) || (!side.unknown && side.any()) {
            Tri::Yes
        } else {
            Tri::Unknown
        };
    DeterminismFacts {
        deterministic,
        sources_of_nondeterminism: sources,
        order_sensitive,
        float_sensitive,
        verified_runs: 0,
        mismatches: 0,
    }
}
