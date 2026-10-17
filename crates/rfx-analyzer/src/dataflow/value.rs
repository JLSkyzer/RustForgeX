//! Valeurs abstraites de l'interpretation : origine des references (ADR-055).

use rfx_bytecode::Slot;

/// Origine possible d'une reference. Un site est designe par la position de son
/// instruction dans le code : les faits de la methode disent ce qui s'y passe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// Parametre, par indice logique : le receveur d'une methode d'instance est 0.
    Param(u16),
    /// Objet ou tableau alloue a ce site (`new`, `newarray`, `anewarray`,
    /// `multianewarray`).
    New(u32),
    /// Valeur lue par le `getstatic` de ce site.
    Static(u32),
    /// Valeur lue par le `getfield` de ce site.
    Field(u32),
    /// Element lu par le `aaload` de ce site.
    Element(u32),
    /// Valeur rendue par l'appel de ce site.
    Return(u32),
    /// Exception attrapee par le gestionnaire qui commence a cette position.
    Caught(u32),
    /// Constante du reservoir : chaine, classe, handle ou type de methode.
    Constant,
    /// Origine inconnue : n'importe quel objet.
    Unknown,
}

/// Sources gardees par valeur ; au-dela, la valeur devient [`Source::Unknown`].
pub const MAX_SOURCES: usize = 6;

/// Ensemble de sources, trie, sans doublon, de taille bornee.
///
/// L'ensemble vide est la reference `null` : un acces par elle leve une exception et
/// n'atteint aucun objet. [`Source::Unknown`] absorbe toutes les autres sources.
///
/// Les emplacements au-dela de `len` valent toujours [`Source::Unknown`] : l'egalite
/// et le hachage derives ne dependent ainsi que des sources presentes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sources {
    len: u8,
    items: [Source; MAX_SOURCES],
}

impl Sources {
    /// `null`.
    pub const NULL: Self = Self {
        len: 0,
        items: [Source::Unknown; MAX_SOURCES],
    };

    /// N'importe quel objet.
    pub const UNKNOWN: Self = Self::one(Source::Unknown);

    /// Une seule source.
    #[must_use]
    pub const fn one(source: Source) -> Self {
        let mut items = [Source::Unknown; MAX_SOURCES];
        items[0] = source;
        Self { len: 1, items }
    }

    /// Sources, dans l'ordre.
    pub fn iter(&self) -> impl Iterator<Item = Source> + '_ {
        self.items[..usize::from(self.len)].iter().copied()
    }

    /// `true` pour `null`.
    #[must_use]
    pub fn is_null(&self) -> bool {
        self.len == 0
    }

    /// `true` si l'origine est inconnue.
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        self.len == 1 && self.items[0] == Source::Unknown
    }

    /// `true` si `source` en fait partie.
    #[must_use]
    pub fn contains(&self, source: Source) -> bool {
        self.iter().any(|s| s == source)
    }

    /// Union ; le second element vaut `true` si la borne a fait perdre de
    /// l'information (aucun des deux n'etait inconnu, l'union l'est).
    #[must_use]
    pub fn union(&self, other: &Self) -> (Self, bool) {
        if self.is_unknown() || other.is_unknown() {
            return (Self::UNKNOWN, false);
        }
        let mut merged: Vec<Source> = self.iter().chain(other.iter()).collect();
        merged.sort_unstable();
        merged.dedup();
        if merged.len() > MAX_SOURCES {
            return (Self::UNKNOWN, true);
        }
        let mut out = Self::NULL;
        for (slot, source) in out.items.iter_mut().zip(&merged) {
            *slot = *source;
        }
        out.len = u8::try_from(merged.len()).unwrap_or(u8::MAX);
        (out, false)
    }
}

/// Valeur d'une variable locale ou de la pile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    /// Inutilisable : non initialisee, seconde moitie d'une valeur longue, ou de
    /// natures differentes selon le chemin.
    Top,
    /// Primitive de categorie 1.
    Prim,
    /// Primitive de categorie 2.
    Wide,
    /// Reference.
    Ref(Sources),
}

impl Value {
    /// Nature, pour les operations de pile.
    #[must_use]
    pub fn slot(&self) -> Slot {
        match self {
            Self::Wide => Slot::Wide,
            Self::Ref(_) => Slot::Ref,
            Self::Top | Self::Prim => Slot::Prim,
        }
    }

    /// Valeur d'une nature, venue de `source` si c'est une reference.
    #[must_use]
    pub fn of(slot: Slot, source: Source) -> Self {
        match slot {
            Slot::Prim => Self::Prim,
            Slot::Wide => Self::Wide,
            Slot::Ref => Self::Ref(Sources::one(source)),
        }
    }
}

/// Etat en un point du code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Variables locales, `max_locals` emplacements.
    pub locals: Vec<Value>,
    /// Pile, une entree par valeur.
    pub stack: Vec<Value>,
}

/// Pourquoi deux etats ne se joignent pas : des piles de formes differentes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mismatch;

impl Frame {
    /// Joint `other` dans `self`. Rend `true` si `self` a change ; compte dans
    /// `widened` les unions que la borne a rendues inconnues.
    ///
    /// # Errors
    ///
    /// [`Mismatch`] si les piles different de hauteur ou de nature : le code n'est pas
    /// verifiable, aucune valeur ne peut etre choisie.
    pub fn join(&mut self, other: &Self, widened: &mut u32) -> Result<bool, Mismatch> {
        if self.stack.len() != other.stack.len() || self.locals.len() != other.locals.len() {
            return Err(Mismatch);
        }
        let mut changed = false;
        for (mine, theirs) in self.stack.iter_mut().zip(&other.stack) {
            match (*mine, theirs) {
                (Value::Ref(a), Value::Ref(b)) => {
                    let (union, lost) = a.union(b);
                    *widened += u32::from(lost);
                    changed |= union != a;
                    *mine = Value::Ref(union);
                }
                (a, b) if a == *b => {}
                _ => return Err(Mismatch),
            }
        }
        for (mine, theirs) in self.locals.iter_mut().zip(&other.locals) {
            let joined = match (*mine, theirs) {
                (Value::Ref(a), Value::Ref(b)) => {
                    let (union, lost) = a.union(b);
                    *widened += u32::from(lost);
                    Value::Ref(union)
                }
                (a, b) if a == *b => a,
                _ => Value::Top,
            };
            changed |= joined != *mine;
            *mine = joined;
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_is_sorted_bounded_and_absorbed_by_unknown() {
        let a = Sources::one(Source::Param(2));
        let b = Sources::one(Source::Param(1));
        let (ab, lost) = a.union(&b);
        assert!(!lost);
        assert_eq!(
            ab.iter().collect::<Vec<_>>(),
            [Source::Param(1), Source::Param(2)]
        );
        assert_eq!(ab.union(&a).0, ab, "idempotente");
        assert_eq!(Sources::NULL.union(&a).0, a, "null est neutre");
        assert!(ab.union(&Sources::UNKNOWN).0.is_unknown());

        let mut many = Sources::NULL;
        for i in 0..u16::try_from(MAX_SOURCES).unwrap() {
            let (next, lost) = many.union(&Sources::one(Source::Param(i)));
            assert!(!lost);
            many = next;
        }
        let (over, lost) = many.union(&Sources::one(Source::New(0)));
        assert!(over.is_unknown() && lost, "au-dela de la borne : inconnu");
    }

    #[test]
    fn join_merges_references_and_forgets_conflicting_locals() {
        let p = |i| Value::Ref(Sources::one(Source::Param(i)));
        let mut a = Frame {
            locals: vec![p(0), Value::Prim, Value::Prim],
            stack: vec![p(1)],
        };
        let b = Frame {
            locals: vec![p(2), Value::Prim, p(3)],
            stack: vec![p(4)],
        };
        let mut widened = 0;
        assert_eq!(a.join(&b, &mut widened), Ok(true));
        assert_eq!(a.locals[2], Value::Top, "int ou reference : inutilisable");
        assert!(matches!(a.locals[0], Value::Ref(s) if s.contains(Source::Param(2))));
        assert_eq!(a.join(&b, &mut widened), Ok(false), "point fixe");

        let wide = Frame {
            locals: a.locals.clone(),
            stack: vec![Value::Wide],
        };
        assert_eq!(a.join(&wide, &mut widened), Err(Mismatch));
        assert_eq!(widened, 0);
    }

    #[test]
    fn value_size_matches_the_budget_documentation() {
        // `DataflowLimits::DEFAULT` borne la memoire d'une methode en supposant cette
        // taille : la changer impose de revoir le budget et sa documentation.
        assert_eq!(core::mem::size_of::<Value>(), 52);
    }
}
