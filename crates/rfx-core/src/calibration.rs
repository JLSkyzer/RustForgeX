//! Etalonnage A/A de la chaine de mesure sur de vraies mesures du jeu (ADR-069,
//! decision 4).
//!
//! Composant : C-15 (Decision Engine), chaine de mesure de R-410.
//! Maturite : `STABLE` pour l'etalonnage ; aucune strategie n'est essayee (M3, option A).
//!
//! Deux bras identiques : chaque tick releve d'une unite est affecte a la reference ou a
//! l'essai par [`rfx_decision::experiment::assign`], sans que rien ne change dans le
//! jeu. Toute conclusion « gain » est donc fausse par construction. Le taux de ces faux
//! gains, mesure sur la charge reelle (derive, rafales, ramasse-miettes), dit si la
//! chaine peut etre crue le jour ou un bras differera vraiment.
//!
//! Tout se passe hors du tick, sur le fil d'analyse : le tick ne fait que relever.

use std::collections::HashMap;

use rfx_decision::experiment::{self, Arm, Sample};
use rfx_model::WorkId;
use rfx_profiler::trace::TickTrace;

/// Part des ticks affectes au bras d'essai pendant l'etalonnage, en pour cent.
///
/// Plus large que l'exploration de `balanced` (2 %) : l'essai A/A ne change rien au
/// jeu, et une part plus large donne plus d'essais par session. Elle n'a d'effet que
/// sur l'ecart entre un tick d'essai et sa reference, que borne [`AA_MAX_GAP`].
pub const AA_TRIAL_PCT: u32 = 10;

/// Ticks d'essai par comparaison.
pub const AA_TRIALS_PER_RUN: usize = 100;

/// Ecart maximal entre un tick d'essai et sa reference, en ticks.
pub const AA_MAX_GAP: u64 = 20;

/// Seuil du test des signes.
pub const AA_ALPHA: f64 = 0.05;

/// Ticks de reference gardes au plus par unite ; au-dela, la plus ancienne moitie part.
/// Dix fois ce qu'une comparaison consomme en moyenne a [`AA_TRIAL_PCT`].
const MAX_REFERENCE: usize = 8_192;

/// Ticks en attente d'une unite.
#[derive(Debug, Default)]
struct Series {
    reference: Vec<Sample>,
    trial: Vec<Sample>,
}

impl Series {
    /// Compare les deux bras, rend `true` si la chaine conclut a un gain, puis repart
    /// de zero : une comparaison ne reutilise jamais les ticks d'une autre.
    fn conclude(&mut self) -> bool {
        // Les ticks arrivent dans l'ordre ; le tri garantit l'hypothese de
        // l'appariement meme si un releve venait a etre rejoue.
        self.reference.sort_by_key(|s| s.tick);
        let ratios = experiment::paired_ratios(&self.reference, &self.trial, AA_MAX_GAP);
        self.reference.clear();
        self.trial.clear();
        experiment::conclude(&ratios, AA_ALPHA).is_some()
    }
}

/// Etat de l'etalonnage.
#[derive(Debug, Default)]
pub struct AaCalibration {
    series: HashMap<WorkId, Series>,
    runs: u64,
    false_gains: u64,
    ticks: u64,
}

impl AaCalibration {
    /// Integre les ticks releves d'une unite ; conclut une comparaison des que le bras
    /// d'essai en a assez.
    pub fn feed(&mut self, work_id: WorkId, ticks: &[TickTrace]) {
        let series = self.series.entry(work_id).or_default();
        for trace in ticks.iter().filter(|t| t.timed_exits > 0) {
            #[allow(clippy::cast_precision_loss)] // des durees bien en deca de 2^52 ns
            let sample = Sample {
                tick: trace.tick,
                ns_per_item: trace.timed_ns as f64 / trace.timed_exits as f64,
            };
            self.ticks = self.ticks.saturating_add(1);
            match experiment::assign(trace.tick, work_id.0, AA_TRIAL_PCT) {
                Arm::Reference => {
                    series.reference.push(sample);
                    if series.reference.len() > MAX_REFERENCE {
                        series.reference.drain(..MAX_REFERENCE / 2);
                    }
                }
                Arm::Trial => {
                    series.trial.push(sample);
                    if series.trial.len() >= AA_TRIALS_PER_RUN {
                        self.runs = self.runs.saturating_add(1);
                        if series.conclude() {
                            self.false_gains = self.false_gains.saturating_add(1);
                        }
                    }
                }
            }
        }
    }

    /// Oublie les ticks en attente des unites qui ne sont plus relevees.
    pub fn retain(&mut self, traced: &[WorkId]) {
        self.series.retain(|id, _| traced.contains(id));
    }

    /// Comparaisons conclues depuis le demarrage.
    #[must_use]
    pub fn runs(&self) -> u64 {
        self.runs
    }

    /// Comparaisons qui ont conclu a un gain : toutes fausses, les bras etant identiques.
    #[must_use]
    pub fn false_gains(&self) -> u64 {
        self.false_gains
    }

    /// Ticks integres depuis le demarrage.
    #[must_use]
    pub fn ticks(&self) -> u64 {
        self.ticks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cout constant : tous les rapports valent 1, aucun gain ne peut etre conclu.
    fn flat(from: u64, count: u64) -> Vec<TickTrace> {
        (from..from + count)
            .map(|tick| TickTrace {
                tick,
                timed_ns: 1_000,
                timed_exits: 2,
            })
            .collect()
    }

    #[test]
    fn a_run_is_concluded_once_enough_trial_ticks_arrived() {
        let mut aa = AaCalibration::default();
        // ~10 % d'essai : 2 000 ticks en donnent environ 200, soit au moins une comparaison.
        aa.feed(WorkId(7), &flat(0, 2_000));
        assert!(aa.runs() >= 1, "{} comparaisons", aa.runs());
        assert_eq!(aa.false_gains(), 0, "cout constant : aucun gain");
        assert_eq!(aa.ticks(), 2_000);
    }

    #[test]
    fn ticks_without_a_timed_exit_are_ignored() {
        let mut aa = AaCalibration::default();
        let silent: Vec<TickTrace> = (0..5_000)
            .map(|tick| TickTrace {
                tick,
                timed_ns: 0,
                timed_exits: 0,
            })
            .collect();
        aa.feed(WorkId(1), &silent);
        assert_eq!((aa.runs(), aa.ticks()), (0, 0));
    }

    #[test]
    fn a_systematic_trial_advantage_would_be_counted_as_a_false_gain() {
        // Un bras d'essai plus rapide par construction : ce que l'etalonnage doit
        // signaler s'il arrivait sur de vraies mesures.
        let mut aa = AaCalibration::default();
        let ticks: Vec<TickTrace> = (0..20_000)
            .map(|tick| {
                let trial = experiment::assign(tick, 3, AA_TRIAL_PCT) == Arm::Trial;
                TickTrace {
                    tick,
                    timed_ns: if trial { 500 } else { 1_000 },
                    timed_exits: 1,
                }
            })
            .collect();
        aa.feed(WorkId(3), &ticks);
        assert!(aa.runs() >= 10);
        assert_eq!(aa.false_gains(), aa.runs());
    }

    #[test]
    fn units_no_longer_traced_are_forgotten() {
        let mut aa = AaCalibration::default();
        aa.feed(WorkId(1), &flat(0, 50));
        aa.feed(WorkId(2), &flat(0, 50));
        aa.retain(&[WorkId(2)]);
        assert_eq!(aa.series.len(), 1);
        assert!(aa.series.contains_key(&WorkId(2)));
    }

    #[test]
    fn the_reference_stays_bounded() {
        let mut aa = AaCalibration::default();
        // Un releve qui n'aurait que des references (l'affectation ne le permet pas,
        // mais la borne ne doit pas en dependre).
        let reference_only: Vec<TickTrace> = (0..100_000)
            .filter(|t| experiment::assign(*t, 5, AA_TRIAL_PCT) == Arm::Reference)
            .take(MAX_REFERENCE + 10)
            .map(|tick| TickTrace {
                tick,
                timed_ns: 1,
                timed_exits: 1,
            })
            .collect();
        aa.feed(WorkId(5), &reference_only);
        assert!(aa.series[&WorkId(5)].reference.len() <= MAX_REFERENCE);
    }
}
