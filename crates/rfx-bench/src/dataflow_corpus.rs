//! Analyse intraprocedurale de C-11 sur le bytecode reel (ADR-055).
//!
//! Cahier des charges : PARTIE 5.11, FM-20. Maturite : `STABLE`.
//!
//! Analyse chaque methode avec du code du corpus, sous le budget donne. Sert deux
//! fins : verifier qu'aucune methode compilee et acceptee par la JVM n'est jugee
//! incoherente (ce serait une erreur de l'interpreteur), et mesurer le travail par
//! methode pour fixer les budgets sur des chiffres plutot qu'au juge.

use std::collections::BTreeMap;
use std::time::Instant;

use rfx_analyzer::dataflow::{analyze, DataflowLimits, Unanalysable};
use rfx_bytecode::ClassFile;
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
    /// Duree d'analyse par methode, en microsecondes, decodage compris.
    pub micros: Quantiles,
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
            let result = analyze(&class, method, limits);
            micros.push(begun.elapsed().as_secs_f64() * 1e6);
            match result {
                Ok(facts) => {
                    report.analysed += 1;
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
