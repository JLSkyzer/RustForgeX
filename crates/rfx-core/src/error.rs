//! Codes d'erreur du runtime.
//!
//! Cahier des charges : annexe A.2. Obligation : contrat agent 4.7 — tout nouveau code
//! d'erreur est enregistre dans l'annexe A.2 avant d'etre utilise.
//!
//! Convention de la frontiere FFI (R-701) : toute fonction exportee renvoie un
//! `i32`, `0` pour le succes et la **valeur negative** du code d'erreur sinon.
//! `E-1002` se transmet donc comme `-1002`.

use core::fmt;

/// Severite d'un code d'erreur, telle que definie en annexe A.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severite {
    /// Degradation locale, le systeme continue.
    Mineure,
    /// Fonctionnalite perdue ou sous-systeme desactive.
    Majeure,
    /// Le runtime refuse de s'activer ou s'arrete.
    Critique,
}

/// Code d'erreur normatif (annexe A.2).
///
/// Seuls les codes effectivement produits par le code implemente sont declares ici.
/// Les codes des composants non encore implementes seront ajoutes au jalon qui les
/// mobilise, afin que cette enumeration reste le reflet exact de ce qui peut arriver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// `E-1002` : version d'ABI incompatible. Critique, mene a `DISABLED`.
    AbiIncompatible,
    /// `E-1004` : double initialisation du runtime. Majeure, la seconde est refusee.
    DoubleInit,
    /// `E-3001` : panic Rust capturee a la frontiere FFI. Majeure.
    PanicCapturee,
    /// `E-3002` : depassement du budget memoire natif. Majeure.
    BudgetMemoireDepasse,
    /// `E-3004` : invariant viole. Critique, mene a `HALTED` avec dump.
    InvariantViole,
    /// Argument invalide fourni par l'appelant Java : pointeur nul, longueur
    /// aberrante, tampon de sortie trop petit, handle inconnu.
    ///
    /// Ce code ne figure pas en annexe A.2 : il ne decrit pas une defaillance du
    /// runtime mais un usage incorrect de l'ABI, toujours d'origine interne au
    /// projet, et il est traite comme un defaut de programmation (PARTIE 19.2).
    ArgumentInvalide,
}

impl ErrorCode {
    /// Numero normatif du code, sans le prefixe `E-`.
    ///
    /// `ArgumentInvalide` n'ayant pas de numero en annexe A.2, il emprunte le
    /// numero reserve `1000`, non attribue par le cahier des charges.
    #[must_use]
    pub fn numero(self) -> i32 {
        match self {
            Self::ArgumentInvalide => 1000,
            Self::AbiIncompatible => 1002,
            Self::DoubleInit => 1004,
            Self::PanicCapturee => 3001,
            Self::BudgetMemoireDepasse => 3002,
            Self::InvariantViole => 3004,
        }
    }

    /// Valeur renvoyee a travers la frontiere FFI : le numero, negatif (R-701).
    #[must_use]
    pub fn code_ffi(self) -> i32 {
        -self.numero()
    }

    /// Severite du code, telle que definie en annexe A.2.
    #[must_use]
    pub fn severite(self) -> Severite {
        match self {
            Self::ArgumentInvalide => Severite::Mineure,
            Self::DoubleInit | Self::PanicCapturee | Self::BudgetMemoireDepasse => {
                Severite::Majeure
            }
            Self::AbiIncompatible | Self::InvariantViole => Severite::Critique,
        }
    }

    /// Description courte, identique au libelle de l'annexe A.2.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::ArgumentInvalide => "argument invalide a la frontiere FFI",
            Self::AbiIncompatible => "version d'ABI incompatible",
            Self::DoubleInit => "double initialisation du runtime",
            Self::PanicCapturee => "panic Rust capturee a la frontiere FFI",
            Self::BudgetMemoireDepasse => "depassement du budget memoire natif",
            Self::InvariantViole => "invariant viole",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E-{} ({})", self.numero(), self.description())
    }
}

/// Valeur de succes renvoyee par les fonctions exportees (R-701).
pub const OK: i32 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    /// T-008 : tous les codes d'erreur sont documentes et **uniques**.
    #[test]
    fn les_numeros_de_code_sont_uniques() {
        let codes = [
            ErrorCode::ArgumentInvalide,
            ErrorCode::AbiIncompatible,
            ErrorCode::DoubleInit,
            ErrorCode::PanicCapturee,
            ErrorCode::BudgetMemoireDepasse,
            ErrorCode::InvariantViole,
        ];
        let mut numeros: Vec<i32> = codes.iter().map(|c| c.numero()).collect();
        numeros.sort_unstable();
        let avant = numeros.len();
        numeros.dedup();
        assert_eq!(
            numeros.len(),
            avant,
            "deux codes d'erreur partagent un numero"
        );
    }

    #[test]
    fn les_codes_ffi_sont_negatifs_et_distincts_du_succes() {
        for c in [
            ErrorCode::ArgumentInvalide,
            ErrorCode::AbiIncompatible,
            ErrorCode::DoubleInit,
            ErrorCode::PanicCapturee,
            ErrorCode::BudgetMemoireDepasse,
            ErrorCode::InvariantViole,
        ] {
            assert!(c.code_ffi() < 0, "{c} doit se transmettre en negatif");
            assert_ne!(c.code_ffi(), OK);
        }
    }

    #[test]
    fn les_numeros_correspondent_a_l_annexe_a2() {
        assert_eq!(ErrorCode::AbiIncompatible.numero(), 1002);
        assert_eq!(ErrorCode::DoubleInit.numero(), 1004);
        assert_eq!(ErrorCode::PanicCapturee.numero(), 3001);
        assert_eq!(ErrorCode::BudgetMemoireDepasse.numero(), 3002);
        assert_eq!(ErrorCode::InvariantViole.numero(), 3004);
    }

    #[test]
    fn les_severites_correspondent_a_l_annexe_a2() {
        assert_eq!(ErrorCode::AbiIncompatible.severite(), Severite::Critique);
        assert_eq!(ErrorCode::InvariantViole.severite(), Severite::Critique);
        assert_eq!(ErrorCode::DoubleInit.severite(), Severite::Majeure);
        assert_eq!(ErrorCode::PanicCapturee.severite(), Severite::Majeure);
    }
}
