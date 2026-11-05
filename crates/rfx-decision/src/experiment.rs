//! Chaine de mesure d'un gain : affectation, comparaison appariee, conclusion prudente
//! (ADR-069, decision 3).
//!
//! Cahier des charges : PARTIE 5.15 (R-410), PARTIE 11.4 et 11.5, PARTIE 28.3.
//! Maturite : `STABLE` pour la chaine ; elle ne sert a aucune decision avant M4.
//!
//! Une strategie ne peut s'activer que sur un speedup mesure (R-410). Le mesurer, c'est
//! comparer, pour une meme unite, des ticks avec et sans la strategie. Trois pieges, trois
//! parades :
//!
//! - la charge du jeu bouge : on compare chaque tick d'essai au tick de reference le plus
//!   proche, jamais des fenetres lointaines (lecon d'ADR-020) ;
//! - le bruit fabrique des gains : un gain n'est retenu que si un test des signes
//!   unilateral le rend significatif ; sinon le speedup reste 1,0 ;
//! - la chaine elle-meme pourrait se tromper : elle est etalonnee par un essai A/A, deux
//!   bras identiques, qui doit conclure « aucun gain » au moins aussi souvent que le seuil
//!   le promet.
//!
//! Aucun chiffre de performance n'est produit ici : seulement des comparaisons de
//! mesures fournies par l'appelant.

/// Bras d'une comparaison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    /// Le code tel quel.
    Reference,
    /// La strategie essayee.
    Trial,
}

/// Brassage de 64 bits (`splitmix64`) : deterministe, sans etat.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Bras d'une unite pour un tick : deterministe, par tick et par unite (ADR-069). Une
/// part `exploration_pct` des ticks va a l'essai ; 0 en mode `safe`.
#[must_use]
pub fn assign(tick: u64, work_id: u64, exploration_pct: u32) -> Arm {
    if exploration_pct == 0 {
        return Arm::Reference;
    }
    if mix(tick ^ mix(work_id)) % 100 < u64::from(exploration_pct.min(100)) {
        Arm::Trial
    } else {
        Arm::Reference
    }
}

/// Une mesure d'un tick : son numero, et le cout par element traite, en nanosecondes
/// (le nombre d'elements varie d'un tick a l'autre).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Tick de la mesure.
    pub tick: u64,
    /// Nanosecondes par element traite.
    pub ns_per_item: f64,
}

/// Rapports `reference / essai` des paires formees en associant chaque tick d'essai au
/// tick de reference le plus proche, a `max_gap` ticks au plus. Un rapport superieur a 1
/// dit que l'essai est plus rapide. `reference` doit etre trie par tick.
#[must_use]
pub fn paired_ratios(reference: &[Sample], trial: &[Sample], max_gap: u64) -> Vec<f64> {
    let valid = |s: &Sample| s.ns_per_item.is_finite() && s.ns_per_item > 0.0;
    let mut ratios = Vec::new();
    for t in trial.iter().filter(|s| valid(s)) {
        let at = reference.partition_point(|r| r.tick < t.tick);
        let candidates = [at.checked_sub(1), Some(at)];
        let nearest = candidates
            .iter()
            .flatten()
            .filter_map(|&i| reference.get(i))
            .filter(|r| valid(r))
            .min_by_key(|r| r.tick.abs_diff(t.tick));
        if let Some(r) = nearest.filter(|r| r.tick.abs_diff(t.tick) <= max_gap) {
            ratios.push(r.ns_per_item / t.ns_per_item);
        }
    }
    ratios
}

/// Probabilite d'obtenir au moins `k` succes sur `n` tirages a pile ou face : la
/// p-valeur unilaterale du test des signes. Calculee en logarithmes pour rester exacte
/// aux grands `n`.
fn sign_test_p_value(k: u64, n: u64) -> f64 {
    if k == 0 {
        return 1.0;
    }
    #[allow(clippy::cast_precision_loss)] // n reste loin de 2^52
    let ln_choose = |n: u64, i: u64| -> f64 {
        (1..=i)
            .map(|j| ((n - i + j) as f64).ln() - (j as f64).ln())
            .sum()
    };
    let ln_half_n = -(n as f64) * std::f64::consts::LN_2;
    (k..=n)
        .map(|i| (ln_choose(n, i) + ln_half_n).exp())
        .sum::<f64>()
        .min(1.0)
}

/// Conclusion d'une comparaison : le speedup median si l'essai est significativement
/// plus rapide au seuil `alpha` (test des signes unilateral, egalites ecartees), sinon
/// `None` — le speedup reste alors 1,0 (R-410).
#[must_use]
pub fn conclude(ratios: &[f64], alpha: f64) -> Option<f64> {
    let faster = ratios.iter().filter(|r| **r > 1.0).count() as u64;
    let slower = ratios.iter().filter(|r| **r < 1.0).count() as u64;
    let n = faster + slower;
    if n == 0 || sign_test_p_value(faster, n) > alpha {
        return None;
    }
    let mut sorted: Vec<f64> = ratios.iter().copied().filter(|r| r.is_finite()).collect();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    (median > 1.0).then_some(median)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generateur deterministe pour les simulations : ce ne sont pas des mesures.
    struct Noise(u64);

    impl Noise {
        fn unit(&mut self) -> f64 {
            self.0 = mix(self.0);
            #[allow(clippy::cast_precision_loss)]
            let value = (self.0 >> 11) as f64 / (1_u64 << 53) as f64;
            value
        }
    }

    /// Un essai simule : `ticks` ticks, cout de base qui derive lentement (charge du
    /// jeu) plus un bruit de +/- 30 %, l'essai divise le cout par `speedup`.
    fn simulate(seed: u64, ticks: u64, pct: u32, speedup: f64) -> (Vec<Sample>, Vec<Sample>) {
        let mut noise = Noise(seed);
        let (mut reference, mut trial) = (Vec::new(), Vec::new());
        for tick in 0..ticks {
            #[allow(clippy::cast_precision_loss)]
            let drift = 1_000.0 + 400.0 * ((tick as f64) / 500.0).sin();
            let cost = drift * (0.7 + 0.6 * noise.unit());
            match assign(tick, seed, pct) {
                Arm::Reference => reference.push(Sample {
                    tick,
                    ns_per_item: cost,
                }),
                Arm::Trial => trial.push(Sample {
                    tick,
                    ns_per_item: cost / speedup,
                }),
            }
        }
        (reference, trial)
    }

    #[test]
    fn assignment_is_deterministic_and_follows_the_exploration_share() {
        assert_eq!(assign(5, 9, 2), assign(5, 9, 2));
        assert_eq!(
            assign(5, 9, 0),
            Arm::Reference,
            "mode safe : jamais d'essai"
        );
        let trials = (0..100_000_u64)
            .filter(|t| assign(*t, 42, 5) == Arm::Trial)
            .count() as u64;
        assert!(
            (4_500..5_500).contains(&trials),
            "{trials} sur 100 000 a 5 %"
        );
        assert!((0..1_000).all(|t| assign(t, 1, 100) == Arm::Trial));
    }

    #[test]
    fn pairs_take_the_nearest_reference_within_the_gap() {
        let r = [
            Sample {
                tick: 10,
                ns_per_item: 100.0,
            },
            Sample {
                tick: 20,
                ns_per_item: 200.0,
            },
        ];
        let t = [
            Sample {
                tick: 12,
                ns_per_item: 50.0,
            },
            Sample {
                tick: 19,
                ns_per_item: 100.0,
            },
            Sample {
                tick: 90,
                ns_per_item: 10.0,
            },
            Sample {
                tick: 15,
                ns_per_item: f64::NAN,
            },
        ];
        assert_eq!(paired_ratios(&r, &t, 5), vec![2.0, 2.0]);
    }

    #[test]
    fn the_sign_test_matches_known_values() {
        assert!((sign_test_p_value(1, 1) - 0.5).abs() < 1e-12);
        assert!((sign_test_p_value(10, 10) - 1.0 / 1024.0).abs() < 1e-12);
        assert!((sign_test_p_value(5, 10) - 638.0 / 1024.0).abs() < 1e-12);
        assert!((sign_test_p_value(0, 10) - 1.0).abs() < 1e-12);
    }

    /// Etalonnage A/A : deux bras identiques. La chaine conclut a un gain dans au plus
    /// la proportion que le seuil promet (avec une marge d'echantillonnage), jamais plus.
    #[test]
    fn an_a_a_run_does_not_invent_gains() {
        let alpha = 0.05;
        let runs: u64 = 400;
        let false_gains = (0..runs)
            .filter(|seed| {
                let (r, t) = simulate(*seed, 4_000, 5, 1.0);
                conclude(&paired_ratios(&r, &t, 20), alpha).is_some()
            })
            .count() as u64;
        // Attendu au plus 5 % ; 9 % laisse la marge d'un echantillon de 400.
        assert!(
            false_gains <= runs * 9 / 100,
            "{false_gains} faux gains sur {runs}"
        );
    }

    /// La chaine voit un vrai gain malgre la derive et le bruit, et l'estime.
    #[test]
    fn a_real_gain_is_seen_and_estimated() {
        let mut found = 0;
        for seed in 0..50 {
            let (r, t) = simulate(1_000 + seed, 4_000, 5, 1.3);
            if let Some(speedup) = conclude(&paired_ratios(&r, &t, 20), 0.05) {
                assert!((1.1..1.6).contains(&speedup), "{speedup}");
                found += 1;
            }
        }
        assert!(found >= 45, "{found} sur 50");
    }

    #[test]
    fn nothing_is_concluded_without_data_or_on_a_slowdown() {
        assert_eq!(conclude(&[], 0.05), None);
        let (r, t) = simulate(7, 4_000, 5, 0.8);
        assert_eq!(conclude(&paired_ratios(&r, &t, 20), 0.05), None);
    }
}
