//! Analyse intraprocedurale de C-11 sur le bytecode reel (ADR-055).
//!
//! Cahier des charges : PARTIE 5.11, FM-20. Maturite : `STABLE`.
//!
//! Analyse chaque methode avec du code du corpus, sous le budget donne. Sert trois
//! fins : verifier qu'aucune methode compilee et acceptee par la JVM n'est jugee
//! incoherente (ce serait une erreur de l'interpreteur), mesurer le travail par
//! methode pour fixer les budgets sur des chiffres plutot qu'au juge, et decrire ce que
//! les effets intraproceduraux savent dire, avec la hierarchie complete du corpus : la
//! repartition des precisions et des domaines, et les ancres absentes.

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use rfx_analyzer::dataflow::Source;
use rfx_analyzer::dataflow::{
    analyze, intraprocedural_effects, workload_effects, DataflowLimits, DomainTable, Unanalysable,
    CLASS_ANCHORS, INTERFACE_ANCHORS,
};
use rfx_analyzer::hierarchy::{ClassIndex, Skeleton};
use rfx_analyzer::{CallGraph, EdgeKind, MethodRef};
use rfx_bytecode::InvokeKind;

use crate::callgraph_acceptance::build_like_the_game;
use rfx_bytecode::ClassFile;
use rfx_model::{Domains, Precision};
use serde::Serialize;

use crate::callgraph_cost::Corpus;

/// Exemples gardes par categorie.
const SAMPLES: usize = 10;

/// Quantiles d'une serie.
#[derive(Debug, Default, Serialize)]
pub struct Quantiles {
    /// Mediane.
    pub p50: f64,
    /// 99e centile.
    pub p99: f64,
    /// 999e millile.
    pub p999: f64,
    /// Maximum.
    pub max: f64,
}

impl Quantiles {
    fn of(mut values: Vec<f64>) -> Self {
        if values.is_empty() {
            return Self::default();
        }
        values.sort_by(f64::total_cmp);
        let at = |q: f64| {
            let index = ((values.len() - 1) as f64 * q).round() as usize;
            values[index]
        };
        Self {
            p50: at(0.5),
            p99: at(0.99),
            p999: at(0.999),
            max: values[values.len() - 1],
        }
    }
}

/// Resultat sur un corpus.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    /// Classes du corpus.
    pub classes: u64,
    /// Classes illisibles.
    pub unreadable_classes: u64,
    /// Methodes avec du code.
    pub methods: u64,
    /// Methodes analysees.
    pub analysed: u64,
    /// Methodes avec `jsr`/`ret` ou illisibles.
    pub bytecode: u64,
    /// Methodes jugees incoherentes : erreur de l'interpreteur sur du code accepte par
    /// la JVM.
    pub incoherent: u64,
    /// Methodes hors budget.
    pub budget: u64,
    /// Methodes dont une union a depasse la borne de sources.
    pub widened_methods: u64,
    /// Blocs de base par methode.
    pub blocks: Quantiles,
    /// Emplacements d'etat par methode.
    pub cells: Quantiles,
    /// Plus grand nombre de passages dans un meme bloc, par methode.
    pub max_block_visits: Quantiles,
    /// Duree par methode, en microsecondes : decodage, analyse et effets.
    pub micros: Quantiles,
    /// Precision des ecritures : `EXACT`, `OVER_APPROX`, `UNKNOWN`.
    pub write_precision: BTreeMap<String, u64>,
    /// La meme, pour les seules methodes sans appel.
    pub write_precision_without_calls: BTreeMap<String, u64>,
    /// Methodes dont les ecritures touchent chaque domaine.
    pub write_domains: BTreeMap<String, u64>,
    /// Ancres de la table des domaines absentes de la hierarchie du corpus.
    pub missing_anchors: Vec<String>,
    /// Duree totale, en millisecondes.
    pub total_ms: f64,
    /// Exemples par categorie : `classe.methode descripteur : detail`.
    pub samples: BTreeMap<String, Vec<String>>,
}

impl Report {
    fn sample(&mut self, category: &str, text: String) {
        let list = self.samples.entry(category.to_owned()).or_default();
        if list.len() < SAMPLES {
            list.push(text);
        }
    }
}

/// Analyse chaque methode du corpus sous `limits`.
#[must_use]
pub fn run(corpus: &Corpus, limits: &DataflowLimits) -> Report {
    let mut report = Report::default();
    let mut index = ClassIndex::new();
    for bytes in corpus.classes.values() {
        if let Ok(class) = ClassFile::parse(bytes) {
            index.declare(
                &class.name,
                Skeleton {
                    super_name: class.super_name,
                    access: class.access,
                },
            );
        }
    }
    report.missing_anchors = CLASS_ANCHORS
        .iter()
        .chain(INTERFACE_ANCHORS.iter())
        .filter(|(name, _)| index.superclass(name).is_none())
        .map(|(name, _)| (*name).to_owned())
        .collect();
    let table = DomainTable::new();
    let mut names: Vec<&String> = corpus.classes.keys().collect();
    names.sort();
    let (mut blocks, mut cells, mut visits, mut micros) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let started = Instant::now();
    for name in names {
        report.classes += 1;
        let Ok(class) = ClassFile::parse(&corpus.classes[name]) else {
            report.unreadable_classes += 1;
            continue;
        };
        for method in &class.methods {
            if method.code.is_none() {
                continue;
            }
            report.methods += 1;
            let id = || format!("{}.{} {}", class.name, method.name, method.descriptor);
            let begun = Instant::now();
            let result = analyze(&class, method, limits).map(|facts| {
                let effects = intraprocedural_effects(&facts, &index, &table);
                (facts, effects)
            });
            micros.push(begun.elapsed().as_secs_f64() * 1e6);
            match result {
                Ok((facts, effects)) => {
                    report.analysed += 1;
                    let precision = precision_name(effects.writes.precision);
                    *report.write_precision.entry(precision.clone()).or_default() += 1;
                    if facts.calls.is_empty() {
                        *report
                            .write_precision_without_calls
                            .entry(precision)
                            .or_default() += 1;
                    }
                    for domain in domain_names(effects.writes.domains) {
                        *report.write_domains.entry(domain).or_default() += 1;
                    }
                    if facts.widened > 0 {
                        report.widened_methods += 1;
                    }
                    blocks.push(f64::from(facts.work.blocks));
                    cells.push(facts.work.cells as f64);
                    visits.push(f64::from(facts.work.max_block_visits));
                }
                Err(Unanalysable::Bytecode(e)) => {
                    report.bytecode += 1;
                    report.sample("bytecode", format!("{} : {e}", id()));
                }
                Err(Unanalysable::Budget) => {
                    report.budget += 1;
                    report.sample("budget", id());
                }
                Err(e) => {
                    report.incoherent += 1;
                    report.sample("incoherent", format!("{} : {e}", id()));
                }
            }
        }
    }
    report.total_ms = started.elapsed().as_secs_f64() * 1e3;
    report.blocks = Quantiles::of(blocks);
    report.cells = Quantiles::of(cells);
    report.max_block_visits = Quantiles::of(visits);
    report.micros = Quantiles::of(micros);
    report
}

/// Effets des unites de travail : une racine, son graphe d'appel construit comme en
/// jeu, et les effets composes le long de ce graphe.
#[derive(Debug, Default, Serialize)]
pub struct WorkloadReport {
    /// Racines.
    pub roots: u64,
    /// Precision des ecritures.
    pub write_precision: BTreeMap<String, u64>,
    /// Unites dont les ecritures touchent chaque domaine.
    pub write_domains: BTreeMap<String, u64>,
    /// Unites ayant au moins une cause d'inconnu, par cause.
    pub unknown_causes: BTreeMap<String, u64>,
    /// Aretes non resolues des graphes, par raison (C-07).
    pub unresolved_edges: BTreeMap<String, u64>,
    /// Sites non resolus des methodes analysees, par `raison | receveur | code` : ce
    /// qui pese, pour choisir comment reduire l'inconnu.
    pub unresolved_sites: BTreeMap<String, u64>,
    /// Methodes analysees par unite.
    pub analysed_methods: Quantiles,
    /// Duree de la composition des effets par unite, en millisecondes, analyse des
    /// methodes comprise, construction du graphe exclue.
    pub effects_ms: Quantiles,
}

/// Effets des unites de travail de racines `roots`.
#[must_use]
pub fn workloads(corpus: &Corpus, roots: &[MethodRef], limits: &DataflowLimits) -> WorkloadReport {
    let mut index = ClassIndex::new();
    for bytes in corpus.classes.values() {
        if let Ok(class) = ClassFile::parse(bytes) {
            index.declare(
                &class.name,
                Skeleton {
                    super_name: class.super_name,
                    access: class.access,
                },
            );
        }
    }
    let table = DomainTable::new();
    let mut report = WorkloadReport::default();
    let (mut analysed, mut millis) = (Vec::new(), Vec::new());
    for root in roots {
        report.roots += 1;
        let graph = build_like_the_game(&mut index, corpus, root);
        for edge in graph.edges() {
            if let EdgeKind::VirtualUnresolved(reason) = edge.kind {
                *report
                    .unresolved_edges
                    .entry(format!("{reason:?}"))
                    .or_default() += 1;
            }
        }
        diagnose_unresolved(&graph, &index, limits, &mut report.unresolved_sites);
        let begun = Instant::now();
        let effects = workload_effects(&graph, &index, &table, limits);
        millis.push(begun.elapsed().as_secs_f64() * 1e3);
        analysed.push(f64::from(effects.analysed_methods));
        *report
            .write_precision
            .entry(precision_name(effects.writes.precision))
            .or_default() += 1;
        for domain in domain_names(effects.writes.domains) {
            *report.write_domains.entry(domain).or_default() += 1;
        }
        let causes = effects.causes;
        for (name, present) in [
            ("unresolved_sites", causes.unresolved_sites > 0),
            ("unexplored_sites", causes.unexplored_sites > 0),
            ("external_methods", causes.external_methods > 0),
            ("unanalysable_methods", causes.unanalysable_methods > 0),
            ("truncated", causes.truncated),
            ("not_converged", causes.not_converged),
        ] {
            if present {
                *report.unknown_causes.entry(name.to_owned()).or_default() += 1;
            }
        }
    }
    report.analysed_methods = Quantiles::of(analysed);
    report.effects_ms = Quantiles::of(millis);
    report
}

/// Range chaque site non resolu d'une methode analysee selon sa raison, son receveur
/// (alloue sur place avec une classe connue, parametre, autre) et le code qui le porte.
fn diagnose_unresolved(
    graph: &CallGraph,
    index: &ClassIndex,
    limits: &DataflowLimits,
    sites: &mut BTreeMap<String, u64>,
) {
    for (id, node) in graph.nodes().iter().enumerate() {
        let Some(class) = index.class(&node.method.owner) else {
            continue;
        };
        let Some(method) = class.method(&node.method.name, &node.method.descriptor) else {
            continue;
        };
        let Ok(facts) = analyze(class, method, limits) else {
            continue;
        };
        let reasons: HashMap<u32, String> = graph
            .edges()
            .iter()
            .filter(|edge| edge.from == id)
            .filter_map(|edge| match (edge.kind, edge.offset) {
                (EdgeKind::VirtualUnresolved(reason), Some(offset)) => {
                    Some((offset, format!("{reason:?}")))
                }
                _ => None,
            })
            .collect();
        let allocated: HashMap<u32, bool> = facts
            .allocations
            .iter()
            .map(|a| (a.offset, a.class.is_some()))
            .collect();
        for call in &facts.calls {
            let Some(reason) = reasons.get(&call.offset) else {
                continue;
            };
            let receiver = match (call.kind, call.args.first()) {
                (InvokeKind::Static | InvokeKind::Dynamic, _) => "sans receveur",
                (_, Some(Some(sources))) => {
                    let all: Vec<Source> = sources.iter().collect();
                    if !all.is_empty()
                        && all.iter().all(|s| {
                            matches!(s, Source::New(site) if allocated.get(site) == Some(&true))
                        })
                    {
                        "alloue sur place, classe connue"
                    } else if all.iter().any(|s| matches!(s, Source::Param(_))) {
                        "parametre"
                    } else {
                        "autre"
                    }
                }
                _ => "autre",
            };
            let owner = &node.method.owner;
            let code = if ["java/", "javax/", "jdk/", "sun/"]
                .iter()
                .any(|p| owner.starts_with(p))
            {
                "JDK"
            } else if owner.starts_with("net/minecraftforge/") {
                "Forge"
            } else if owner.starts_with("net/minecraft/") {
                "Minecraft"
            } else {
                "autre"
            };
            *sites
                .entry(format!("{reason} | {receiver} | {code}"))
                .or_default() += 1;
        }
    }
}

fn precision_name(precision: Precision) -> String {
    format!("{precision:?}")
}

/// Noms des domaines d'un ensemble, dans l'ordre de DM-06.
fn domain_names(domains: Domains) -> Vec<String> {
    use rfx_model::Domain::{
        BlockEntityState, Capabilities, ChunkData, ChunkStatus, EntityInventory, EntityState,
        FileSystem, JavaStatics, LevelGlobals, ModStatics, NativeExternal, Network, Random,
        Registries, ThreadLocals, Time, Unknown, WorldBlocks, WorldFluids, WorldLighting,
    };
    [
        WorldBlocks,
        WorldFluids,
        WorldLighting,
        ChunkData,
        ChunkStatus,
        EntityState,
        EntityInventory,
        BlockEntityState,
        LevelGlobals,
        Registries,
        Capabilities,
        ModStatics,
        JavaStatics,
        ThreadLocals,
        Network,
        FileSystem,
        NativeExternal,
        Random,
        Time,
        Unknown,
    ]
    .into_iter()
    .filter(|domain| domains.contains(*domain))
    .map(|domain| format!("{domain:?}"))
    .collect()
}
