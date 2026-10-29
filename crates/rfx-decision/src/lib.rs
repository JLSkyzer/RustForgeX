//! Decision : ce qu'il est permis de faire d'une unite de travail.
//!
//! Cahier des charges : PARTIE 5.14 (C-14), PARTIE 5.31 (C-33), puis PARTIE 5.15 (C-15).
//! Decisions : ADR-066, ADR-067.
//! Maturite : `STABLE` pour C-14 et le moteur de C-33.
//!
//! Tout y est fonction pure des faits d'analyse et de mesure : ni horloge, ni etat
//! global, ni nom de mod (INV-12, R-413). `rfx-decision` depend de `rfx-model` et de rien
//! d'autre (INV-13) ; c'est `rfx-core` qui lui fournit les faits.

pub mod compat;
pub mod risk;

pub use compat::{
    verdict, CompatInputs, CompatVerdict, DetectedProperties, Quarantine, QuarantineReason,
    Selector, Source, UserRule, Verdict,
};
pub use risk::{assess, Blast, RiskAssessment, RiskInputs, RiskReason};
