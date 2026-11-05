//! Validation et retour arriere : une strategie n'est active que prouvee correcte et
//! rentable, et elle cesse de l'etre au premier doute.
//!
//! Cahier des charges : PARTIE 5.24 (C-24), PARTIE 5.26 (C-26), SM-03. Decision :
//! ADR-074. Maturite : `STABLE` pour la logique ; rien ne l'execute encore dans le jeu
//! (aucune strategie au-dela de TL2 n'est active, ADR-073).
//!
//! `rfx-validate` depend de `rfx-model` et de rien d'autre (INV-13). Tout y est pur :
//! les executions, les horloges et le jeu sont fournis par l'appelant.

pub mod oracle;
pub mod pipeline;
pub mod rollback;

#[cfg(test)]
mod tests {
    /// T-245 (INV-12, R-550) : le code de `rfx-validate` ne porte aucune chaine. Aucun nom
    /// de mod ne peut donc y conditionner quoi que ce soit.
    #[test]
    fn t245_the_validation_code_names_no_mod() {
        for (file, source) in [
            ("lib.rs", include_str!("lib.rs")),
            ("oracle.rs", include_str!("oracle.rs")),
            ("pipeline.rs", include_str!("pipeline.rs")),
            ("rollback.rs", include_str!("rollback.rs")),
        ] {
            let code = source.split("#[cfg(test)]").next().unwrap_or("");
            for line in code.lines() {
                let line = line.trim_start();
                if line.starts_with("//") {
                    continue;
                }
                assert!(!line.contains('"'), "{file} : chaine dans {line:?}");
            }
        }
    }
}
