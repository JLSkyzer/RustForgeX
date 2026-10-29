//! Decision : ce qu'il est permis de faire d'une unite de travail.
//!
//! Cahier des charges : PARTIE 5.14 (C-14), PARTIE 5.31 (C-33), PARTIE 5.15 (C-15).
//! Decisions : ADR-066, ADR-067, ADR-068.
//! Maturite : `STABLE` pour C-14, le moteur de C-33 et C-15 au perimetre de M3.
//!
//! Tout y est fonction pure des faits d'analyse et de mesure : ni horloge, ni etat
//! global, ni nom de mod (INV-12, R-413). `rfx-decision` depend de `rfx-model` et de rien
//! d'autre (INV-13) ; c'est `rfx-core` qui lui fournit les faits.

pub mod compat;
pub mod decision;
pub mod risk;

pub use compat::{
    verdict, CompatInputs, CompatVerdict, DetectedProperties, Quarantine, QuarantineReason,
    Selector, Source, UserRule, Verdict,
};
pub use decision::{
    decide, feasibility, stabilize, Decision, DecisionInputs, Held, ModePolicy, Rejected, Scored,
    Stability,
};
pub use risk::{assess, Blast, RiskAssessment, RiskInputs, RiskReason};
