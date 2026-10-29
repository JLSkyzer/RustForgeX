//! Decision : ce qu'il est permis de faire d'une unite de travail.
//!
//! Cahier des charges : PARTIE 5.14 (C-14), puis PARTIE 5.15 (C-15). Decision : ADR-066.
//! Maturite : `STABLE` pour C-14.
//!
//! Tout y est fonction pure des faits d'analyse et de mesure : ni horloge, ni etat
//! global, ni nom de mod (INV-12, R-413). `rfx-decision` depend de `rfx-model` et de rien
//! d'autre (INV-13) ; c'est `rfx-core` qui lui fournit les faits.

pub mod risk;

pub use risk::{assess, Blast, RiskAssessment, RiskInputs, RiskReason};
