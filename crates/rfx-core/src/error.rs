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
pub enum Severity {
    /// Degradation locale, le systeme continue.
    Minor,
    /// Fonctionnalite perdue ou sous-systeme desactive.
    Major,
    /// Le runtime refuse de s'activer ou s'arrete.
    Critical,
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
    /// `E-1201` : cout du profilage au-dessus du budget. Mineure, la profondeur
    /// de sondage descend d'un cran (PARTIE 5.5).
    ProfilerOverBudget,
    /// `E-2101` : collision de `WorkId`. Majeure, les deux unites de travail sont
    /// exclues de toute mesure et de tout offload (R-202).
    WorkIdCollision,
    /// `E-3001` : panic Rust capturee a la frontiere FFI. Majeure.
    PanicCaught,
    /// `E-3002` : depassement du budget memoire natif. Majeure.
    MemoryBudgetExceeded,
    /// `E-3004` : invariant viole. Critique, mene a `HALTED` avec dump.
    InvariantViolated,
    /// Argument invalide fourni par l'appelant Java : pointeur nul, longueur
    /// aberrante, tampon de sortie trop petit, handle inconnu.
    ///
    /// Ce code ne figure pas en annexe A.2 : il ne decrit pas une defaillance du
    /// runtime mais un usage incorrect de l'ABI, toujours d'origine interne au
    /// projet, et il est traite comme un defaut de programmation (PARTIE 19.2).
    InvalidArgument,
}

impl ErrorCode {
    /// Numero normatif du code, sans le prefixe `E-`.
    ///
    /// `InvalidArgument` n'ayant pas de numero en annexe A.2, il emprunte le
    /// numero reserve `1000`, non attribue par le cahier des charges.
    #[must_use]
    pub fn number(self) -> i32 {
        match self {
            Self::InvalidArgument => 1000,
            Self::AbiIncompatible => 1002,
            Self::DoubleInit => 1004,
            Self::ProfilerOverBudget => 1201,
            Self::WorkIdCollision => 2101,
            Self::PanicCaught => 3001,
            Self::MemoryBudgetExceeded => 3002,
            Self::InvariantViolated => 3004,
        }
    }

    /// Valeur renvoyee a travers la frontiere FFI : le numero, negatif (R-701).
    #[must_use]
    pub fn ffi_code(self) -> i32 {
        -self.number()
    }

    /// Severite du code, telle que definie en annexe A.2.
    #[must_use]
    pub fn severity(self) -> Severity {
        match self {
            Self::InvalidArgument | Self::ProfilerOverBudget => Severity::Minor,
            Self::DoubleInit
            | Self::WorkIdCollision
            | Self::PanicCaught
            | Self::MemoryBudgetExceeded => Severity::Major,
            Self::AbiIncompatible | Self::InvariantViolated => Severity::Critical,
        }
    }

    /// Description courte, identique au libelle de l'annexe A.2.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::InvalidArgument => "argument invalide a la frontiere FFI",
            Self::AbiIncompatible => "version d'ABI incompatible",
            Self::DoubleInit => "double initialisation du runtime",
            Self::ProfilerOverBudget => "overhead du profiler au-dessus du budget",
            Self::WorkIdCollision => "collision de WorkId",
            Self::PanicCaught => "panic Rust capturee a la frontiere FFI",
            Self::MemoryBudgetExceeded => "depassement du budget memoire natif",
            Self::InvariantViolated => "invariant viole",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E-{} ({})", self.number(), self.description())
    }
}

/// Valeur de succes renvoyee par les fonctions exportees (R-701).
pub const OK: i32 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [ErrorCode; 8] = [
        ErrorCode::InvalidArgument,
        ErrorCode::AbiIncompatible,
        ErrorCode::DoubleInit,
        ErrorCode::ProfilerOverBudget,
        ErrorCode::WorkIdCollision,
        ErrorCode::PanicCaught,
        ErrorCode::MemoryBudgetExceeded,
        ErrorCode::InvariantViolated,
    ];

    /// T-008 : tous les codes d'erreur sont documentes et **uniques**.
    #[test]
    fn error_numbers_are_unique() {
        let mut numbers: Vec<i32> = ALL.iter().map(|c| c.number()).collect();
        numbers.sort_unstable();
        let before = numbers.len();
        numbers.dedup();
        assert_eq!(
            numbers.len(),
            before,
            "deux codes d'erreur partagent un numero"
        );
    }

    #[test]
    fn ffi_codes_are_negative_and_distinct_from_success() {
        for c in ALL {
            assert!(c.ffi_code() < 0, "{c} doit se transmettre en negatif");
            assert_ne!(c.ffi_code(), OK);
        }
    }

    #[test]
    fn numbers_match_appendix_a2() {
        assert_eq!(ErrorCode::AbiIncompatible.number(), 1002);
        assert_eq!(ErrorCode::DoubleInit.number(), 1004);
        assert_eq!(ErrorCode::PanicCaught.number(), 3001);
        assert_eq!(ErrorCode::MemoryBudgetExceeded.number(), 3002);
        assert_eq!(ErrorCode::InvariantViolated.number(), 3004);
    }

    #[test]
    fn severities_match_appendix_a2() {
        assert_eq!(ErrorCode::AbiIncompatible.severity(), Severity::Critical);
        assert_eq!(ErrorCode::InvariantViolated.severity(), Severity::Critical);
        assert_eq!(ErrorCode::DoubleInit.severity(), Severity::Major);
        assert_eq!(ErrorCode::PanicCaught.severity(), Severity::Major);
    }
}
