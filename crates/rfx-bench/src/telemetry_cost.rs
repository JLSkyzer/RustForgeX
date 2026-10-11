//! T-401 : jugement du cout de la telemetrie contre R-561 (« moins de 0,2 % du MSPT »).
//!
//! Cahier des charges : PARTIE 5.32. Methode : ADR-034.
//!
//! Chaque execution du serveur de banc rend, mesures en place, la part Java de la
//! telemetrie par tick et le MSPT moyen. La part native vient d'un micro-benchmark
//! (`tick_window/cycle`) : la borne haute de son intervalle de confiance est ajoutee a
//! chaque execution. Le verdict porte sur le **pire** rapport des executions, pas sur
//! la mediane : l'exigence dit « inferieur a », sans moyenne.

use serde::{Deserialize, Serialize};

use crate::macro_bench::{aggregate_values, Aggregate};

/// Seuil de R-561, en pourcent du MSPT.
pub const THRESHOLD_PCT: f64 = 0.2;

/// Executions exigees par la PARTIE 21.3 pour qu'une mesure soit publiable.
pub const MIN_RUNS: usize = 5;

/// Statistiques d'une serie, telles que `TelemetryCost` les ecrit.
#[derive(Debug, Deserialize)]
pub struct SeriesStats {
    /// Moyenne, en nanosecondes.
    #[serde(default)]
    pub mean: f64,
}

/// Une execution, telle que `TelemetryCost` l'ecrit.
#[derive(Debug, Deserialize)]
pub struct RunFile {
    /// Etiquette de l'execution.
    pub label: String,
    /// RUSTFORGE-X etait-il actif ? Sans lui, il n'y a rien a mesurer.
    pub rfx_active: bool,
    /// Le journal de tick etait-il emis ? Sinon son cout est celui d'un appel ignore.
    pub trace_debug_enabled: bool,
    /// Ticks mesures.
    pub ticks: u64,
    /// MSPT du serveur, en nanosecondes.
    pub mspt_ns: SeriesStats,
    /// Cout a vide du chronometre, en nanosecondes.
    pub bracket_ns: SeriesStats,
    /// Part Java de la telemetrie, par tick, en nanosecondes.
    pub java_telemetry_ns_per_tick: f64,
}

/// Verdict de T-401.
#[derive(Debug, Serialize)]
pub struct Verdict {
    /// Executions retenues.
    pub runs: usize,
    /// Part native retenue, en nanosecondes par tick (borne haute de l'IC).
    pub native_ns_per_tick: f64,
    /// Part Java par tick, inter-executions.
    pub java_ns_per_tick: Aggregate,
    /// Cout a vide du chronometre, compris dans la part Java : ce que pese l'instrument.
    pub bracket_ns: Aggregate,
    /// MSPT moyen par execution, en millisecondes.
    pub mspt_ms: Aggregate,
    /// Rapport telemetrie / MSPT, en pourcent, inter-executions.
    pub ratio_pct: Aggregate,
    /// Pire rapport observe : c'est lui qui est juge.
    pub worst_ratio_pct: f64,
    /// Seuil de R-561.
    pub threshold_pct: f64,
    /// Methodologie respectee : assez d'executions, RUSTFORGE-X actif dans chacune.
    pub methodology_compliant: bool,
    /// R-561 tenue, sur une campagne conforme.
    pub conforms: bool,
    /// Ce que le lecteur doit savoir.
    pub notes: Vec<String>,
}

/// Juge une campagne.
#[must_use]
pub fn judge(runs: &[RunFile], native_ns_per_tick: f64) -> Verdict {
    let mut notes = Vec::new();
    let usable: Vec<&RunFile> = runs
        .iter()
        .filter(|r| {
            let keep = r.rfx_active && r.ticks > 0 && r.mspt_ns.mean > 0.0;
            if !keep {
                notes.push(format!(
                    "{} ecartee : RUSTFORGE-X inactif ou aucun tick",
                    r.label
                ));
            }
            keep
        })
        .collect();
    if usable.iter().any(|r| !r.trace_debug_enabled) {
        notes.push(
            "journal de tick non emis dans au moins une execution : son cout y est celui \
             d'un appel filtre"
                .to_owned(),
        );
    }

    let ratios: Vec<f64> = usable
        .iter()
        .map(|r| 100.0 * (r.java_telemetry_ns_per_tick + native_ns_per_tick) / r.mspt_ns.mean)
        .collect();
    let java: Vec<f64> = usable
        .iter()
        .map(|r| r.java_telemetry_ns_per_tick)
        .collect();
    let mspt: Vec<f64> = usable.iter().map(|r| r.mspt_ns.mean / 1e6).collect();
    let bracket: Vec<f64> = usable.iter().map(|r| r.bracket_ns.mean).collect();
    let worst = ratios.iter().copied().fold(f64::NAN, f64::max);

    let methodology_compliant = usable.len() >= MIN_RUNS;
    if !methodology_compliant {
        notes.push(format!(
            "{} execution(s) exploitable(s), {MIN_RUNS} exigees : non publiable",
            usable.len()
        ));
    }

    Verdict {
        runs: usable.len(),
        native_ns_per_tick,
        java_ns_per_tick: aggregate_values(&java),
        bracket_ns: aggregate_values(&bracket),
        mspt_ms: aggregate_values(&mspt),
        ratio_pct: aggregate_values(&ratios),
        worst_ratio_pct: worst,
        threshold_pct: THRESHOLD_PCT,
        methodology_compliant,
        conforms: methodology_compliant && worst < THRESHOLD_PCT,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(java_ns: f64, mspt_ns: f64, active: bool) -> RunFile {
        RunFile {
            label: "r".to_owned(),
            rfx_active: active,
            trace_debug_enabled: true,
            ticks: 6000,
            mspt_ns: SeriesStats { mean: mspt_ns },
            bracket_ns: SeriesStats { mean: 30.0 },
            java_telemetry_ns_per_tick: java_ns,
        }
    }

    #[test]
    fn the_worst_run_decides() {
        // 1 000 ns sur 1 ms = 0,1 % ; la derniere execution, a 0,5 ms, monte a 0,2 %.
        let mut runs: Vec<RunFile> = (0..4).map(|_| run(900.0, 1e6, true)).collect();
        runs.push(run(900.0, 5e5, true));
        let verdict = judge(&runs, 100.0);
        assert!(verdict.methodology_compliant);
        assert!((verdict.worst_ratio_pct - 0.2).abs() < 1e-9);
        assert!(!verdict.conforms, "0,2 % n'est pas inferieur a 0,2 %");
    }

    #[test]
    fn a_cheap_telemetry_conforms() {
        let runs: Vec<RunFile> = (0..5).map(|_| run(500.0, 6.5e6, true)).collect();
        let verdict = judge(&runs, 435.0);
        assert!(verdict.conforms);
        assert!(verdict.worst_ratio_pct < 0.02);
    }

    #[test]
    fn too_few_runs_never_conform() {
        let runs: Vec<RunFile> = (0..4).map(|_| run(1.0, 6.5e6, true)).collect();
        let verdict = judge(&runs, 1.0);
        assert!(!verdict.methodology_compliant);
        assert!(!verdict.conforms);
    }

    #[test]
    fn inactive_runs_are_set_aside() {
        let mut runs: Vec<RunFile> = (0..5).map(|_| run(1.0, 6.5e6, true)).collect();
        runs.push(run(0.0, 6.5e6, false));
        let verdict = judge(&runs, 1.0);
        assert_eq!(verdict.runs, 5);
        assert!(verdict.notes.iter().any(|n| n.contains("ecartee")));
    }
}
