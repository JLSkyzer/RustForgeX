//! C-24 : pipeline de promotion d'une strategie (SM-03).
//!
//! Cahier des charges : PARTIE 5.24 (R-490, R-491, R-492), PARTIE 7.3 (SM-03). Tests :
//! T-330, T-331, T-334. Maturite : `STABLE`.
//!
//! `CANDIDATE -> SHADOW -> CANARY -> ACTIVE`. Une strategie n'est active qu'apres
//! [`SHADOW_RUNS`] executions fantomes toutes conformes et un gain estime confirme, puis
//! [`CANARY_TICKS`] ticks de canary sans divergence ni depassement de delai, et un gain
//! **mesure** positif (R-492). Une divergence, meme unique, bannit la strategie pour
//! l'empreinte courante (R-490) ; un changement d'empreinte la rehabilite.

use rfx_model::TransformationLevel;

use crate::oracle::Mismatch;

/// Executions fantomes conformes exigees avant le canary (PARTIE 5.24).
pub const SHADOW_RUNS: u32 = 64;

/// Part des invocations confiees a la strategie pendant le canary, en pour cent.
pub const CANARY_PCT: u64 = 10;

/// Duree du canary, en ticks.
pub const CANARY_TICKS: u64 = 300;

/// Pourquoi une strategie est bannie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BanCause {
    /// Une execution fantome ou canary a diverge (R-490).
    Mismatch(Mismatch),
    /// Le canary a depasse son delai.
    Timeout,
    /// Le canary s'acheve sans gain mesure positif (R-492).
    NoMeasuredGain,
    /// Une anomalie en production (etat `REGRESSED`).
    Regressed,
}

/// Etat d'une strategie (SM-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Proposee, pas encore essayee.
    Candidate,
    /// En execution fantome : executions conformes jusqu'ici.
    Shadow {
        /// Executions conformes.
        matched: u32,
    },
    /// Active sur une part du trafic depuis ce tick.
    Canary {
        /// Tick d'entree en canary.
        since: u64,
    },
    /// Pleinement active.
    Active,
    /// Bannie pour l'empreinte courante.
    Banned(BanCause),
}

/// Refus d'entrer en validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// L'analyse de surete (C-14) ne l'autorise pas.
    Unsafe,
    /// Effets de bord non isolables : seule une promotion jusqu'a TL2 est possible
    /// (R-491, I-05).
    NotIsolable,
    /// La strategie n'est pas a l'etat `CANDIDATE`.
    NotCandidate,
}

/// Une strategie en cours de validation, pour une empreinte de code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pipeline {
    stage: Stage,
    fingerprint: u64,
}

/// Brassage de 64 bits (`splitmix64`), pour choisir le trafic de canary.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

impl Pipeline {
    /// Une strategie candidate, pour le code d'empreinte `fingerprint`.
    #[must_use]
    pub fn new(fingerprint: u64) -> Self {
        Self {
            stage: Stage::Candidate,
            fingerprint,
        }
    }

    /// Etat.
    #[must_use]
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// `true` si la strategie est pleinement active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.stage == Stage::Active
    }

    /// Fait entrer une candidate en execution fantome.
    ///
    /// # Erreurs
    ///
    /// [`Refusal`] si l'analyse de surete refuse, si les effets ne sont pas isolables au
    /// dela de TL2 (R-491), ou si la strategie n'est pas candidate.
    pub fn begin(
        &mut self,
        safety_ok: bool,
        isolable: bool,
        level: TransformationLevel,
    ) -> Result<(), Refusal> {
        if self.stage != Stage::Candidate {
            return Err(Refusal::NotCandidate);
        }
        if !safety_ok {
            return Err(Refusal::Unsafe);
        }
        if !isolable && level > TransformationLevel::Tl2 {
            return Err(Refusal::NotIsolable);
        }
        self.stage = Stage::Shadow { matched: 0 };
        Ok(())
    }

    /// Enregistre une execution fantome. Une divergence bannit (R-490) ; au bout de
    /// [`SHADOW_RUNS`] executions conformes, le canary commence si le gain estime est
    /// confirme. Sans effet hors de l'etat `SHADOW`.
    pub fn record_shadow(&mut self, run: Result<(), Mismatch>, gain_confirmed: bool, tick: u64) {
        let Stage::Shadow { matched } = self.stage else {
            return;
        };
        match run {
            Err(mismatch) => self.stage = Stage::Banned(BanCause::Mismatch(mismatch)),
            Ok(()) => {
                let matched = matched.saturating_add(1);
                self.stage = if matched >= SHADOW_RUNS && gain_confirmed {
                    Stage::Canary { since: tick }
                } else {
                    Stage::Shadow { matched }
                };
            }
        }
    }

    /// `true` si l'invocation `invocation` est confiee a la strategie : pendant le canary,
    /// une part deterministe de [`CANARY_PCT`] % ; une fois active, toutes ; sinon aucune.
    #[must_use]
    pub fn admits(&self, invocation: u64) -> bool {
        match self.stage {
            Stage::Canary { .. } => mix(invocation ^ self.fingerprint) % 100 < CANARY_PCT,
            Stage::Active => true,
            _ => false,
        }
    }

    /// Enregistre une invocation canary : une divergence ou un depassement de delai
    /// bannissent. Sans effet hors du canary.
    pub fn record_canary(&mut self, run: Result<(), Mismatch>, timed_out: bool) {
        if !matches!(self.stage, Stage::Canary { .. }) {
            return;
        }
        if let Err(mismatch) = run {
            self.stage = Stage::Banned(BanCause::Mismatch(mismatch));
        } else if timed_out {
            self.stage = Stage::Banned(BanCause::Timeout);
        }
    }

    /// Clot le canary a `tick` s'il a dure [`CANARY_TICKS`] : active si le gain mesure
    /// est positif, bannie sinon (R-492). Une estimation ne compte pas.
    pub fn close_canary(&mut self, tick: u64, measured_gain_ns: Option<f64>) {
        let Stage::Canary { since } = self.stage else {
            return;
        };
        if tick.saturating_sub(since) < CANARY_TICKS {
            return;
        }
        self.stage = match measured_gain_ns {
            Some(gain) if gain.is_finite() && gain > 0.0 => Stage::Active,
            _ => Stage::Banned(BanCause::NoMeasuredGain),
        };
    }

    /// Anomalie d'une strategie active (`REGRESSED`) : bannie pour cette empreinte. Le
    /// retour arriere lui-meme releve de C-26.
    pub fn regress(&mut self) {
        if self.stage == Stage::Active {
            self.stage = Stage::Banned(BanCause::Regressed);
        }
    }

    /// Nouvelle empreinte du code : tout ce qui a ete valide l'a ete pour l'ancien code.
    /// La strategie redevient candidate, bannie ou non (SM-03).
    pub fn on_fingerprint(&mut self, fingerprint: u64) {
        if fingerprint != self.fingerprint {
            self.fingerprint = fingerprint;
            self.stage = Stage::Candidate;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started() -> Pipeline {
        let mut p = Pipeline::new(42);
        p.begin(true, true, TransformationLevel::Tl4).unwrap();
        p
    }

    fn through_shadow(p: &mut Pipeline) {
        for _ in 0..SHADOW_RUNS {
            p.record_shadow(Ok(()), true, 1_000);
        }
    }

    /// T-330 : une divergence injectee, meme a la derniere execution, bannit.
    #[test]
    fn an_injected_mismatch_bans() {
        let mut p = started();
        for _ in 0..SHADOW_RUNS - 1 {
            p.record_shadow(Ok(()), true, 0);
        }
        p.record_shadow(Err(Mismatch::Returned), true, 0);
        assert_eq!(
            p.stage(),
            Stage::Banned(BanCause::Mismatch(Mismatch::Returned))
        );
        p.record_shadow(Ok(()), true, 0);
        assert!(
            !p.is_active() && !p.admits(1),
            "bannie, elle n'execute plus rien"
        );

        let mut canary = started();
        through_shadow(&mut canary);
        canary.record_canary(Err(Mismatch::Commands), false);
        assert_eq!(
            canary.stage(),
            Stage::Banned(BanCause::Mismatch(Mismatch::Commands))
        );
    }

    /// T-331 : pas de promotion sans gain mesure positif.
    #[test]
    fn no_promotion_without_a_measured_gain() {
        for gain in [None, Some(0.0), Some(-5.0), Some(f64::NAN)] {
            let mut p = started();
            through_shadow(&mut p);
            p.close_canary(1_000 + CANARY_TICKS, gain);
            assert_eq!(
                p.stage(),
                Stage::Banned(BanCause::NoMeasuredGain),
                "{gain:?}"
            );
        }
        let mut p = started();
        through_shadow(&mut p);
        p.close_canary(1_000 + CANARY_TICKS - 1, Some(10.0));
        assert!(
            matches!(p.stage(), Stage::Canary { .. }),
            "canary trop court"
        );
        p.close_canary(1_000 + CANARY_TICKS, Some(10.0));
        assert!(p.is_active());
    }

    #[test]
    fn the_shadow_needs_all_runs_and_a_confirmed_estimate() {
        let mut p = started();
        for _ in 0..SHADOW_RUNS {
            p.record_shadow(Ok(()), false, 0);
        }
        assert_eq!(
            p.stage(),
            Stage::Shadow {
                matched: SHADOW_RUNS
            }
        );
        p.record_shadow(Ok(()), true, 7);
        assert_eq!(p.stage(), Stage::Canary { since: 7 });
        let mut short = started();
        for _ in 0..SHADOW_RUNS - 1 {
            short.record_shadow(Ok(()), true, 0);
        }
        assert!(matches!(short.stage(), Stage::Shadow { .. }));
    }

    /// T-334 : le canary ne recoit qu'une part bornee du trafic.
    #[test]
    fn the_canary_is_limited_in_traffic() {
        let mut p = started();
        assert!(!p.admits(0), "en fantome : aucun trafic");
        through_shadow(&mut p);
        let admitted = (0..100_000_u64).filter(|i| p.admits(*i)).count();
        assert!(
            (9_000..11_000).contains(&admitted),
            "{admitted} sur 100 000"
        );
        p.close_canary(1_000 + CANARY_TICKS, Some(1.0));
        assert!(
            (0..1_000_u64).all(|i| p.admits(i)),
            "active : tout le trafic"
        );
    }

    #[test]
    fn a_timeout_or_a_regression_bans_and_a_new_fingerprint_rehabilitates() {
        let mut p = started();
        through_shadow(&mut p);
        p.record_canary(Ok(()), true);
        assert_eq!(p.stage(), Stage::Banned(BanCause::Timeout));
        p.on_fingerprint(42);
        assert_eq!(p.stage(), Stage::Banned(BanCause::Timeout), "meme code");
        p.on_fingerprint(43);
        assert_eq!(p.stage(), Stage::Candidate);

        let mut active = started();
        through_shadow(&mut active);
        active.close_canary(2_000, Some(3.0));
        active.regress();
        assert_eq!(active.stage(), Stage::Banned(BanCause::Regressed));
    }

    #[test]
    fn entry_is_refused_when_unsafe_or_not_isolable() {
        let mut p = Pipeline::new(1);
        assert_eq!(
            p.begin(false, true, TransformationLevel::Tl2),
            Err(Refusal::Unsafe)
        );
        assert_eq!(
            p.begin(true, false, TransformationLevel::Tl3),
            Err(Refusal::NotIsolable)
        );
        assert_eq!(p.begin(true, false, TransformationLevel::Tl2), Ok(()));
        assert_eq!(
            p.begin(true, true, TransformationLevel::Tl2),
            Err(Refusal::NotCandidate)
        );
    }

    /// Acceptance de C-24 : sur des sequences d'evenements tirees au hasard, aucune
    /// strategie n'atteint `ACTIVE` sans 64 executions fantomes conformes, sans canary
    /// complet, ni sans gain mesure positif.
    #[test]
    fn no_strategy_becomes_active_without_its_proofs() {
        let mut seed = 0x1234_5678_9ABC_DEF0_u64;
        let mut next = || {
            seed = mix(seed);
            seed
        };
        let mut activations = 0;
        for _ in 0..2_000 {
            let mut p = Pipeline::new(next());
            let _ = p.begin(true, true, TransformationLevel::Tl4);
            let (mut conform, mut canary_start, mut tick) = (0_u32, None, 0_u64);
            let mut positive_close = false;
            for _ in 0..200 {
                tick += next() % 10;
                match next() % 6 {
                    0..=2 => {
                        let ok = next() % 500 != 0;
                        let before = p.stage();
                        p.record_shadow(
                            if ok { Ok(()) } else { Err(Mismatch::Events) },
                            next() % 4 != 0,
                            tick,
                        );
                        if ok && matches!(before, Stage::Shadow { .. }) {
                            conform += 1;
                        }
                        if matches!(p.stage(), Stage::Canary { .. }) && canary_start.is_none() {
                            canary_start = Some(tick);
                        }
                    }
                    3 => p.record_canary(Ok(()), next() % 50 == 0),
                    _ => {
                        let gain = match next() % 3 {
                            0 => None,
                            1 => Some(-1.0),
                            _ => Some(2.0),
                        };
                        let was_canary = matches!(p.stage(), Stage::Canary { .. });
                        p.close_canary(tick, gain);
                        if was_canary && p.is_active() {
                            positive_close = gain.is_some_and(|g| g > 0.0);
                        }
                    }
                }
                if p.is_active() {
                    assert!(conform >= SHADOW_RUNS, "{conform} executions conformes");
                    let start = canary_start.expect("passee par le canary");
                    assert!(tick - start >= CANARY_TICKS);
                    assert!(positive_close, "gain mesure positif");
                    activations += 1;
                    break;
                }
            }
        }
        assert!(activations > 0, "la propriete a ete exercee");
    }
}
