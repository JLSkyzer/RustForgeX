//! DM-05 : empreinte de l'environnement dans lequel une mesure ou une decision vaut.
//!
//! Cahier des charges : PARTIE 4.5 (modele et regles d'invalidation), R-209, R-210,
//! R-360. Maturite : `STABLE`.
//!
//! # Deux comparaisons
//!
//! Un fichier persiste porte l'empreinte de l'**environnement** ou il a ete ecrit ; chaque
//! entree porte en plus celle de **son unite de travail**. R-360 en tire deux regles :
//! un fichier dont l'environnement differe est rejete en entier, une entree dont l'unite
//! de travail a change est rejetee seule. [`Fingerprint::same_environment`] et
//! [`Fingerprint::same_workload`] font ces deux comparaisons, et rien d'autre.
//!
//! La table d'invalidation de la PARTIE 4.5 est plus fine : un changement de JVM, par
//! exemple, n'invalide que les mesures. Elle sert aux caches de decision (C-32). Le
//! cache des unites de travail (C-09) applique la regle stricte de R-360 : en cas de
//! doute, on remesure (UNKNOWN = CONSERVATIVE).

use serde::{Deserialize, Serialize};

/// DM-05 : empreinte.
///
/// Un champ nul signifie « non renseigne » : deux empreintes qui different par un champ
/// non renseigne different quand meme, il n'y a pas de joker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Fingerprint {
    /// Version du modele de donnees de RUSTFORGE-X.
    pub schema_version: u16,
    /// Version du runtime, `[majeure, mineure, correctif]`.
    pub rfx_version: [u8; 3],
    /// Hachage de la version de Minecraft.
    pub mc_version: u64,
    /// Hachage de la version de Forge.
    pub forge_version: u64,
    /// Hachage trie de (modid, version, debut du sha256 du jar).
    pub mod_set_hash: u64,
    /// Hachage du jar du mod proprietaire de l'unite de travail.
    pub owner_mod_hash: u64,
    /// Hachage du bytecode de la methode apres toutes les transformations (R-209).
    pub bytecode_hash: u64,
    /// Hachage combine des methodes appelees analysees.
    pub deps_bytecode_hash: u64,
    /// Fournisseur, version et options de ramasse-miettes de la JVM.
    pub jvm_hash: u64,
    /// Classe de materiel (DM-14), jamais l'identite de la machine.
    pub hw_class_hash: u64,
    /// Options de RUSTFORGE-X qui influencent les decisions.
    pub config_hash: u64,
}

impl Fingerprint {
    /// `true` si les deux empreintes decrivent le meme environnement : tout sauf les
    /// champs propres a une unite de travail.
    #[must_use]
    pub fn same_environment(&self, other: &Self) -> bool {
        self.environment() == other.environment()
    }

    /// `true` si les champs propres a l'unite de travail sont identiques : jar du mod
    /// proprietaire, bytecode de la methode, bytecode de ce qu'elle appelle.
    #[must_use]
    pub fn same_workload(&self, other: &Self) -> bool {
        (
            self.owner_mod_hash,
            self.bytecode_hash,
            self.deps_bytecode_hash,
        ) == (
            other.owner_mod_hash,
            other.bytecode_hash,
            other.deps_bytecode_hash,
        )
    }

    /// La seule partie « environnement », champs propres a l'unite de travail a zero.
    /// C'est ce que porte l'en-tete d'un fichier.
    #[must_use]
    pub fn environment(&self) -> Self {
        Self {
            owner_mod_hash: 0,
            bytecode_hash: 0,
            deps_bytecode_hash: 0,
            ..*self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Fingerprint {
        Fingerprint {
            schema_version: 1,
            rfx_version: [0, 1, 0],
            mc_version: 0x1201,
            forge_version: 0x4704,
            mod_set_hash: 11,
            owner_mod_hash: 21,
            bytecode_hash: 31,
            deps_bytecode_hash: 41,
            jvm_hash: 51,
            hw_class_hash: 61,
            config_hash: 71,
        }
    }

    #[test]
    fn the_two_comparisons_look_at_disjoint_fields() {
        let base = sample();
        let other_bytecode = Fingerprint {
            bytecode_hash: 32,
            ..base
        };
        assert!(base.same_environment(&other_bytecode));
        assert!(!base.same_workload(&other_bytecode));

        let other_mods = Fingerprint {
            mod_set_hash: 12,
            ..base
        };
        assert!(!base.same_environment(&other_mods));
        assert!(base.same_workload(&other_mods));
    }

    #[test]
    fn every_environment_field_counts() {
        let base = sample();
        let changed = [
            Fingerprint {
                schema_version: 2,
                ..base
            },
            Fingerprint {
                rfx_version: [0, 2, 0],
                ..base
            },
            Fingerprint {
                mc_version: 1,
                ..base
            },
            Fingerprint {
                forge_version: 1,
                ..base
            },
            Fingerprint {
                mod_set_hash: 1,
                ..base
            },
            Fingerprint {
                jvm_hash: 1,
                ..base
            },
            Fingerprint {
                hw_class_hash: 1,
                ..base
            },
            Fingerprint {
                config_hash: 1,
                ..base
            },
        ];
        for other in changed {
            assert!(!base.same_environment(&other), "{other:?}");
        }
    }

    #[test]
    fn the_environment_part_drops_only_workload_fields() {
        let environment = sample().environment();
        assert_eq!(environment.bytecode_hash, 0);
        assert_eq!(environment.mod_set_hash, 11);
        assert!(environment.same_environment(&sample()));
    }
}
