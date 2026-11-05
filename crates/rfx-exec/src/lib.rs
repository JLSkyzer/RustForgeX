//! Execution sure : effets differes et detection de conflits.
//!
//! Cahier des charges : PARTIE 5.21 (C-21), PARTIE 5.23 (C-23). Decision : ADR-073.
//! Maturite : `STABLE` pour ces deux composants ; rien ne les applique encore au jeu
//! (C-22, accord de Killian requis pour la premiere transformation reelle).
//!
//! Un worker ne touche jamais l'etat autoritatif : il ecrit ses effets dans un
//! [`buffer::CommandBuffer`], que le commit (C-22) validera par
//! [`conflict::ConflictDetector`] puis appliquera, dans un ordre total et deterministe.
//! `rfx-exec` depend de `rfx-model` et de rien d'autre (INV-13).

pub mod buffer;
pub mod conflict;

/// Version de l'etat autoritatif : le numero du tick au moment d'une capture ou d'une
/// ecriture (PARTIE 5.23).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Epoch(pub u64);

#[cfg(test)]
mod tests {
    /// T-245 (INV-12, R-550) : le code de `rfx-exec` ne porte aucune chaine. Aucun nom de
    /// mod ne peut donc y conditionner quoi que ce soit.
    #[test]
    fn t245_the_execution_code_names_no_mod() {
        for (file, source) in [
            ("lib.rs", include_str!("lib.rs")),
            ("buffer.rs", include_str!("buffer.rs")),
            ("conflict.rs", include_str!("conflict.rs")),
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
