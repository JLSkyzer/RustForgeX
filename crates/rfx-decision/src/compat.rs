//! C-33 : Compatibility Engine — ce que l'utilisateur, les incidents et les proprietes
//! detectees permettent de faire d'une unite de travail.
//!
//! Cahier des charges : PARTIE 5.31, PARTIE 28.2. Decision : ADR-067. Tests : T-390,
//! T-391, T-392. Maturite : `STABLE` pour le moteur ; ses sources reelles (configuration,
//! quarantaines persistees, proprietes de l'analyse) se branchent dans `rfx-core`.
//!
//! Le moteur ne connait aucun nom de mod (R-550, INV-12) : il compare les noms qu'un
//! utilisateur ecrit dans `[overrides]` a ceux du descripteur, rien de plus.

use rfx_model::{Fingerprint, TransformationLevel, WorkDescriptor};

/// Verdict de compatibilite, du moins au plus restrictif (ADR-067, decision 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    /// Aucune restriction.
    Allow,
    /// Parallelisme autorise, offload interdit.
    ForceParallel,
    /// Batching autorise, offload interdit.
    ForceBatch,
    /// Pas de parallelisme.
    ForceSerial,
    /// Execution Java uniquement.
    ForceJava,
    /// RF-X n'observe ni ne transforme l'unite.
    Disable,
}

impl Verdict {
    /// Niveau de transformation maximal ; `None` pour `Disable`, qui n'autorise pas
    /// meme l'observation.
    #[must_use]
    pub fn max_level(self) -> Option<TransformationLevel> {
        match self {
            Self::Allow => Some(TransformationLevel::Tl6),
            Self::ForceParallel => Some(TransformationLevel::Tl3),
            Self::ForceBatch | Self::ForceSerial => Some(TransformationLevel::Tl2),
            Self::ForceJava => Some(TransformationLevel::Tl0),
            Self::Disable => None,
        }
    }

    /// Nom normatif (PARTIE 5.31).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Allow => "ALLOW",
            Self::ForceParallel => "FORCE_PARALLEL",
            Self::ForceBatch => "FORCE_BATCH",
            Self::ForceSerial => "FORCE_SERIAL",
            Self::ForceJava => "FORCE_JAVA",
            Self::Disable => "DISABLE",
        }
    }
}

/// Source d'un verdict, de la plus prioritaire a la moins prioritaire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// Configuration explicite de l'utilisateur (`[overrides]`).
    User,
    /// Quarantaine automatique issue d'un incident.
    Quarantine,
    /// Proprietes detectees.
    Detected,
    /// Regles derivees apprises (C-44).
    Learned,
    /// Aucune source ne s'exprime.
    Default,
}

/// Motif d'une quarantaine (PARTIE 5.31, source 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuarantineReason {
    /// Un retour arriere a eu lieu.
    Rollback,
    /// Une panic native a ete confinee.
    Panic,
    /// Une execution comparee a diverge.
    Mismatch,
}

/// Quarantaine datee, motivee, liee a une empreinte (R-551).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quarantine {
    /// Verdict impose.
    pub verdict: Verdict,
    /// Motif.
    pub reason: QuarantineReason,
    /// Date de l'incident, en secondes depuis l'epoque Unix.
    pub since_unix_secs: u64,
    /// Empreinte complete au moment de l'incident.
    pub fingerprint: Fingerprint,
}

impl Quarantine {
    /// `true` tant que l'empreinte est celle de l'incident ; tout changement, de
    /// l'environnement ou de l'unite, la leve (R-551).
    #[must_use]
    pub fn is_active(&self, current: &Fingerprint) -> bool {
        self.fingerprint == *current
    }
}

/// Selecteur de `[overrides]` : `modid`, ou `modid:paquet.Classe#methode`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Selector {
    owner: String,
    method: Option<(String, String)>,
}

impl Selector {
    /// Lit un selecteur ; `None` s'il est vide ou mal forme (la configuration le
    /// signale et l'ignore).
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (owner, rest) = match text.split_once(':') {
            Some((owner, rest)) => (owner, Some(rest)),
            None => (text, None),
        };
        if owner.is_empty() || owner.contains('#') {
            return None;
        }
        let method = match rest {
            None => None,
            Some(rest) => {
                let (class, method) = rest.split_once('#')?;
                if class.is_empty() || method.is_empty() || method.contains('#') {
                    return None;
                }
                Some((class.to_owned(), method.to_owned()))
            }
        };
        Some(Self {
            owner: owner.to_owned(),
            method,
        })
    }

    /// `true` si le selecteur vise cette unite.
    #[must_use]
    pub fn matches(&self, descriptor: &WorkDescriptor) -> bool {
        if descriptor.owner_id != self.owner {
            return false;
        }
        match &self.method {
            None => true,
            Some((class, method)) => {
                descriptor.method_name == *method
                    && descriptor
                        .class_internal_name
                        .chars()
                        .map(|c| if c == '/' { '.' } else { c })
                        .eq(class.chars())
            }
        }
    }
}

/// Regle utilisateur : un verdict et l'unite ou le proprietaire qu'il vise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRule {
    /// Verdict impose.
    pub verdict: Verdict,
    /// Cible.
    pub selector: Selector,
}

/// Proprietes detectees d'une unite (PARTIE 5.31, source 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DetectedProperties {
    /// Un autre transformateur de bytecode vise la classe (mixin, access transformer,
    /// coremod).
    pub foreign_transformer: bool,
    /// Un appel reflexif a ete rencontre dans le graphe.
    pub reflection: bool,
    /// Version de classe non supportee.
    pub unsupported_class_version: bool,
}

/// Ce que le moteur lit pour une unite.
#[derive(Debug, Clone, Copy)]
pub struct CompatInputs<'a> {
    /// L'unite.
    pub descriptor: &'a WorkDescriptor,
    /// Son empreinte courante.
    pub fingerprint: &'a Fingerprint,
    /// Regles de l'utilisateur.
    pub user: &'a [UserRule],
    /// Quarantaines connues pour cette unite, actives ou non.
    pub quarantines: &'a [Quarantine],
    /// Proprietes detectees.
    pub detected: DetectedProperties,
    /// Verdict appris (C-44), s'il en existe un.
    pub learned: Option<Verdict>,
}

/// Verdict rendu, et la source qui l'a decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatVerdict {
    /// Verdict.
    pub verdict: Verdict,
    /// Source qui a decide.
    pub source: Source,
}

/// Verdict de compatibilite d'une unite (ADR-067). Fonction pure.
#[must_use]
pub fn verdict(inputs: &CompatInputs<'_>) -> CompatVerdict {
    let decided = |verdict, source| CompatVerdict { verdict, source };

    let user = inputs
        .user
        .iter()
        .filter(|rule| rule.selector.matches(inputs.descriptor))
        .map(|rule| rule.verdict)
        .max();
    if let Some(verdict) = user {
        return decided(verdict, Source::User);
    }

    let quarantined = inputs
        .quarantines
        .iter()
        .filter(|q| q.is_active(inputs.fingerprint))
        .map(|q| q.verdict)
        .max();
    if let Some(verdict) = quarantined {
        return decided(verdict, Source::Quarantine);
    }

    let d = inputs.detected;
    let detected = [
        (d.foreign_transformer, Verdict::ForceSerial),
        (d.reflection, Verdict::ForceSerial),
        (d.unsupported_class_version, Verdict::ForceJava),
    ]
    .into_iter()
    .filter_map(|(present, verdict)| present.then_some(verdict))
    .max();
    if let Some(verdict) = detected {
        return decided(verdict, Source::Detected);
    }

    if let Some(verdict) = inputs.learned {
        return decided(verdict, Source::Learned);
    }
    decided(Verdict::Allow, Source::Default)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Verdict; 6] = [
        Verdict::Allow,
        Verdict::ForceParallel,
        Verdict::ForceBatch,
        Verdict::ForceSerial,
        Verdict::ForceJava,
        Verdict::Disable,
    ];

    fn unit() -> WorkDescriptor {
        WorkDescriptor {
            owner_id: "examplemod".to_owned(),
            class_internal_name: "net/example/Machine".to_owned(),
            method_name: "tick".to_owned(),
            method_descriptor: "()V".to_owned(),
            ..WorkDescriptor::default()
        }
    }

    fn fingerprint(mod_set: u64) -> Fingerprint {
        Fingerprint {
            mod_set_hash: mod_set,
            ..Fingerprint::default()
        }
    }

    fn quarantine(verdict: Verdict, fp: Fingerprint) -> Quarantine {
        Quarantine {
            verdict,
            reason: QuarantineReason::Panic,
            since_unix_secs: 1_793_000_000,
            fingerprint: fp,
        }
    }

    fn rule(verdict: Verdict, selector: &str) -> UserRule {
        UserRule {
            verdict,
            selector: Selector::parse(selector).expect("selecteur"),
        }
    }

    #[test]
    fn verdicts_are_ordered_by_the_level_they_allow() {
        assert!(ALL.windows(2).all(|w| w[0] < w[1]));
        let levels: Vec<_> = ALL.iter().map(|v| v.max_level()).collect();
        for pair in levels.windows(2) {
            match (pair[0], pair[1]) {
                (Some(a), Some(b)) => assert!(a >= b),
                (Some(_), None) => {}
                other => panic!("ordre des niveaux : {other:?}"),
            }
        }
        assert_eq!(
            Verdict::ForceJava.max_level(),
            Some(TransformationLevel::Tl0)
        );
        assert_eq!(Verdict::Disable.max_level(), None);
    }

    /// T-390 : la source la plus prioritaire qui s'exprime decide, quel que soit le
    /// verdict des sources moins prioritaires ; dans une source, le plus restrictif.
    #[test]
    fn t390_the_highest_priority_source_decides() {
        let unit = unit();
        let fp = fingerprint(1);
        let detected = DetectedProperties {
            foreign_transformer: true,
            ..DetectedProperties::default()
        };
        for user_verdict in ALL {
            for quarantine_verdict in ALL {
                for learned in ALL {
                    let user = [rule(user_verdict, "examplemod")];
                    let quarantines = [quarantine(quarantine_verdict, fp)];
                    let full = CompatInputs {
                        descriptor: &unit,
                        fingerprint: &fp,
                        user: &user,
                        quarantines: &quarantines,
                        detected,
                        learned: Some(learned),
                    };
                    assert_eq!(
                        verdict(&full),
                        CompatVerdict {
                            verdict: user_verdict,
                            source: Source::User
                        }
                    );
                    let no_user = CompatInputs { user: &[], ..full };
                    assert_eq!(verdict(&no_user).source, Source::Quarantine);
                    assert_eq!(verdict(&no_user).verdict, quarantine_verdict);
                    let detected_only = CompatInputs {
                        quarantines: &[],
                        ..no_user
                    };
                    assert_eq!(
                        verdict(&detected_only),
                        CompatVerdict {
                            verdict: Verdict::ForceSerial,
                            source: Source::Detected
                        }
                    );
                    let learned_only = CompatInputs {
                        detected: DetectedProperties::default(),
                        ..detected_only
                    };
                    assert_eq!(
                        verdict(&learned_only),
                        CompatVerdict {
                            verdict: learned,
                            source: Source::Learned
                        }
                    );
                    let nothing = CompatInputs {
                        learned: None,
                        ..learned_only
                    };
                    assert_eq!(
                        verdict(&nothing),
                        CompatVerdict {
                            verdict: Verdict::Allow,
                            source: Source::Default
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn within_a_source_the_most_restrictive_wins() {
        let unit = unit();
        let fp = fingerprint(1);
        let user = [
            rule(Verdict::ForceSerial, "examplemod"),
            rule(Verdict::ForceJava, "examplemod:net.example.Machine#tick"),
            rule(Verdict::Disable, "othermod"),
        ];
        let inputs = CompatInputs {
            descriptor: &unit,
            fingerprint: &fp,
            user: &user,
            quarantines: &[],
            detected: DetectedProperties::default(),
            learned: None,
        };
        assert_eq!(verdict(&inputs).verdict, Verdict::ForceJava);

        let detected = CompatInputs {
            user: &[],
            detected: DetectedProperties {
                foreign_transformer: true,
                reflection: true,
                unsupported_class_version: true,
            },
            ..inputs
        };
        assert_eq!(verdict(&detected).verdict, Verdict::ForceJava);
    }

    /// ADR-067, decision 4 : chaque propriete detectee, seule, rend son verdict.
    #[test]
    fn each_detected_property_alone_has_its_verdict() {
        let unit = unit();
        let fp = fingerprint(1);
        let cases = [
            (
                DetectedProperties {
                    foreign_transformer: true,
                    ..DetectedProperties::default()
                },
                Verdict::ForceSerial,
            ),
            (
                DetectedProperties {
                    reflection: true,
                    ..DetectedProperties::default()
                },
                Verdict::ForceSerial,
            ),
            (
                DetectedProperties {
                    unsupported_class_version: true,
                    ..DetectedProperties::default()
                },
                Verdict::ForceJava,
            ),
        ];
        for (detected, expected) in cases {
            let inputs = CompatInputs {
                descriptor: &unit,
                fingerprint: &fp,
                user: &[],
                quarantines: &[],
                detected,
                learned: None,
            };
            assert_eq!(
                verdict(&inputs),
                CompatVerdict {
                    verdict: expected,
                    source: Source::Detected
                },
                "{detected:?}"
            );
        }
    }

    /// R-552 : un transformateur concurrent impose au moins `FORCE_SERIAL`.
    #[test]
    fn a_foreign_transformer_forces_at_least_serial() {
        let unit = unit();
        let fp = fingerprint(1);
        let inputs = CompatInputs {
            descriptor: &unit,
            fingerprint: &fp,
            user: &[],
            quarantines: &[],
            detected: DetectedProperties {
                foreign_transformer: true,
                ..DetectedProperties::default()
            },
            learned: Some(Verdict::Allow),
        };
        let v = verdict(&inputs).verdict;
        assert!(v >= Verdict::ForceSerial, "{v:?}");
        assert!(v.max_level().unwrap() <= TransformationLevel::Tl2);
    }

    /// T-391 : une quarantaine est levee des que l'empreinte change, de l'environnement
    /// ou de l'unite ; elle tient tant qu'elle est identique.
    #[test]
    fn t391_a_quarantine_is_lifted_when_the_fingerprint_changes() {
        let unit = unit();
        let at_incident = fingerprint(1);
        let quarantines = [quarantine(Verdict::ForceJava, at_incident)];
        let same = CompatInputs {
            descriptor: &unit,
            fingerprint: &at_incident,
            user: &[],
            quarantines: &quarantines,
            detected: DetectedProperties::default(),
            learned: None,
        };
        assert_eq!(
            verdict(&same),
            CompatVerdict {
                verdict: Verdict::ForceJava,
                source: Source::Quarantine
            }
        );

        let new_mods = fingerprint(2);
        let changed_environment = CompatInputs {
            fingerprint: &new_mods,
            ..same
        };
        assert_eq!(verdict(&changed_environment).source, Source::Default);

        let new_code = Fingerprint {
            bytecode_hash: 9,
            ..at_incident
        };
        let changed_unit = CompatInputs {
            fingerprint: &new_code,
            ..same
        };
        assert_eq!(verdict(&changed_unit).source, Source::Default);
    }

    #[test]
    fn selectors_target_an_owner_or_a_method() {
        let unit = unit();
        assert!(Selector::parse("examplemod").unwrap().matches(&unit));
        assert!(Selector::parse(" examplemod ").unwrap().matches(&unit));
        assert!(Selector::parse("examplemod:net.example.Machine#tick")
            .unwrap()
            .matches(&unit));
        assert!(!Selector::parse("examplemod:net.example.Machine#other")
            .unwrap()
            .matches(&unit));
        assert!(!Selector::parse("examplemod:net.example.Other#tick")
            .unwrap()
            .matches(&unit));
        assert!(!Selector::parse("othermod").unwrap().matches(&unit));
        for malformed in [
            "",
            ":net.example.Machine#tick",
            "examplemod:",
            "examplemod:net.example.Machine",
            "examplemod:#tick",
            "examplemod:net.example.Machine#",
            "examplemod:a#b#c",
            "example#mod",
        ] {
            assert_eq!(Selector::parse(malformed), None, "{malformed:?}");
        }
    }

    /// T-392 : le code du moteur ne porte aucune chaine hors des noms normatifs. Un nom
    /// de mod ne peut donc pas y conditionner une decision (R-550, INV-12).
    #[test]
    fn t392_the_engine_names_no_mod() {
        const ALLOWED: [&str; 16] = [
            "ALLOW",
            "FORCE_PARALLEL",
            "FORCE_BATCH",
            "FORCE_SERIAL",
            "FORCE_JAVA",
            "DISABLE",
            "R1",
            "R2",
            "R3",
            "R4",
            "R5",
            "R6",
            "R7",
            "R8",
            "R9",
            "R10",
        ];
        for (file, source) in [
            ("lib.rs", include_str!("lib.rs")),
            ("risk.rs", include_str!("risk.rs")),
            ("compat.rs", include_str!("compat.rs")),
            ("decision.rs", include_str!("decision.rs")),
        ] {
            let code = source.split("#[cfg(test)]").next().unwrap_or("");
            for line in code.lines() {
                let line = line.trim_start();
                if line.starts_with("//") {
                    continue;
                }
                let mut parts = line.split('"');
                parts.next();
                while let Some(literal) = parts.next() {
                    assert!(
                        ALLOWED.contains(&literal),
                        "{file} : chaine non autorisee {literal:?} dans {line:?}"
                    );
                    parts.next();
                }
            }
        }
    }
}
