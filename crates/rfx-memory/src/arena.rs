//! C-31 : arene de taches, une par worker et par niveau d'imbrication.
//!
//! Cahier des charges : PARTIE 5.28 (« Arena par worker : bump, reset apres la tache,
//! capacite initiale 1 Mio, croissance x2 jusqu'a `arena_max` »), PARTIE 5.18 (R-433 :
//! aucune allocation systeme dans le chemin chaud ; un depassement d'arene rend
//! `Rejected(BudgetExceeded)`). Decision : ADR-047. Maturite : `STABLE`.
//!
//! # Pourquoi seules les allocations faillibles sont exposees
//!
//! L'arene repose sur `bumpalo`. Au plafond, ses allocations infaillibles (`alloc`, et
//! les collections qui s'en servent) paniquent (`oom()` : « out of memory », lu dans les
//! sources de la version 3.20.3). La panic serait rattrapee par l'ordonnanceur, mais
//! comptee comme une panic du runtime (R-523), dont trois desactivent le sous-systeme —
//! alors qu'un depassement d'arene est un refus prevu. Ce type n'expose donc que des
//! allocations qui rendent une erreur : la tache la transforme en
//! `Rejected(BudgetExceeded)`, sans panic.
//!
//! # Pourquoi seulement des types `Copy`
//!
//! Une arene ne rend jamais sa memoire objet par objet : elle ne lance aucun
//! destructeur. Un `Vec` ou un `String` alloue dedans perdrait la memoire systeme qu'il
//! detient. Restreindre aux types `Copy` interdit ce cas a la compilation.

use bumpalo::Bump;

/// Capacite initiale par defaut, en octets (PARTIE 5.28).
pub const DEFAULT_ARENA_INITIAL_BYTES: usize = 1 << 20;

/// Plafond par defaut, en octets (`arena_max`, PARTIE 5.28).
pub const DEFAULT_ARENA_MAX_BYTES: usize = 16 << 20;

/// Allocation refusee : l'arene aurait depasse son plafond.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArenaFull {
    /// Octets demandes par l'allocation refusee.
    pub requested: usize,
    /// Plafond de l'arene, en octets.
    pub limit: usize,
}

/// Arene a allocation par pointeur, remise a zero entre deux taches.
#[derive(Debug)]
pub struct Arena {
    bump: Bump,
    limit: usize,
    peak: usize,
}

impl Arena {
    /// Arene de capacite initiale `initial` octets, plafonnee a `limit` octets.
    ///
    /// # Le premier bloc doit tenir sous le plafond
    ///
    /// `bumpalo` 3.20 n'applique son plafond qu'a l'ouverture d'un nouveau bloc, et
    /// seulement si la reservation est encore **sous** le plafond : au-dessus, il le
    /// traite comme absent (`chunk_fits_under_limit` rend `true` faute de reste). Or
    /// `with_capacity` arrondit son bloc vers le haut : une capacite initiale egale au
    /// plafond le depasse des la construction, et plus rien n'est refuse. La capacite est
    /// donc reduite de moitie jusqu'a ce que le premier bloc tienne ; les blocs suivants,
    /// eux, sont filtres par `bumpalo`, et la reservation reste sous le plafond.
    #[must_use]
    pub fn new(initial: usize, limit: usize) -> Self {
        let mut capacity = initial.min(limit);
        let bump = loop {
            let bump = Bump::with_capacity(capacity);
            if bump.allocated_bytes() <= limit || capacity == 0 {
                break bump;
            }
            capacity /= 2;
        };
        bump.set_allocation_limit(Some(limit));
        Self {
            bump,
            limit,
            peak: 0,
        }
    }

    /// Plafond, en octets.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Octets reserves aupres du systeme par les blocs de l'arene.
    #[must_use]
    pub fn allocated_bytes(&self) -> usize {
        self.bump.allocated_bytes()
    }

    /// Plus grande reservation constatee, remises a zero comprises.
    #[must_use]
    pub fn peak_bytes(&self) -> usize {
        self.peak.max(self.allocated_bytes())
    }

    /// Alloue `value` dans l'arene.
    ///
    /// # Errors
    ///
    /// [`ArenaFull`] si l'allocation depasserait le plafond.
    pub fn try_alloc<T: Copy>(&self, value: T) -> Result<&mut T, ArenaFull> {
        self.bump
            .try_alloc(value)
            .map_err(|_| self.full(size_of::<T>()))
    }

    /// Copie `source` dans l'arene.
    ///
    /// # Errors
    ///
    /// [`ArenaFull`] si l'allocation depasserait le plafond.
    pub fn try_alloc_slice_copy<T: Copy>(&self, source: &[T]) -> Result<&mut [T], ArenaFull> {
        self.bump
            .try_alloc_slice_copy(source)
            .map_err(|_| self.full(size_of_val(source)))
    }

    /// Alloue `len` copies de `value` dans l'arene.
    ///
    /// # Errors
    ///
    /// [`ArenaFull`] si l'allocation depasserait le plafond.
    pub fn try_alloc_slice_fill_copy<T: Copy>(
        &self,
        len: usize,
        value: T,
    ) -> Result<&mut [T], ArenaFull> {
        self.bump
            .try_alloc_slice_fill_copy(len, value)
            .map_err(|_| self.full(len.saturating_mul(size_of::<T>())))
    }

    /// Remet l'arene a zero. Le plus grand bloc est garde : la tache suivante n'aura pas
    /// a le redemander au systeme.
    pub fn reset(&mut self) {
        self.peak = self.peak.max(self.allocated_bytes());
        self.bump.reset();
    }

    fn full(&self, requested: usize) -> ArenaFull {
        ArenaFull {
            requested,
            limit: self.limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocations_live_in_the_arena() {
        let arena = Arena::new(1024, 1 << 16);
        let value = arena.try_alloc(41_u64).unwrap();
        *value += 1;
        assert_eq!(*value, 42);
        assert_eq!(
            arena.try_alloc_slice_copy(&[1_u8, 2, 3]).unwrap(),
            &[1, 2, 3]
        );
        assert_eq!(arena.try_alloc_slice_fill_copy(4, 7_u16).unwrap(), &[7; 4]);
    }

    /// Au-dela du plafond, l'allocation est refusee, sans panic ni arret du processus.
    #[test]
    fn beyond_the_limit_an_allocation_is_refused() {
        let arena = Arena::new(1024, 1 << 16);
        let refused = arena.try_alloc_slice_fill_copy(1 << 20, 0_u8).unwrap_err();
        assert_eq!(
            refused,
            ArenaFull {
                requested: 1 << 20,
                limit: 1 << 16
            }
        );
        assert!(arena.try_alloc(1_u8).is_ok(), "l'arene reste utilisable");
    }

    /// Sans remise a zero, huit fois 40 000 octets depasseraient le plafond de 128 Kio ;
    /// avec, la place se libere a chaque tour.
    #[test]
    fn a_reset_makes_room_again() {
        let mut arena = Arena::new(1 << 16, 1 << 17);
        for _ in 0..8 {
            assert!(arena.try_alloc_slice_fill_copy(40_000, 0_u8).is_ok());
            arena.reset();
        }
        assert!(arena.peak_bytes() >= 40_000);
    }

    /// Le cas qui a echoue : capacite initiale egale au plafond. Le premier bloc de
    /// `bumpalo` le depassait, et le plafond n'etait plus applique du tout.
    #[test]
    fn an_initial_capacity_equal_to_the_limit_keeps_the_limit() {
        for limit in [1 << 12, 1 << 16, 1 << 18, 1 << 20] {
            let arena = Arena::new(limit, limit);
            assert!(
                arena.allocated_bytes() <= limit,
                "premier bloc sous le plafond"
            );
            assert!(
                arena.try_alloc_slice_fill_copy(limit * 4, 0_u8).is_err(),
                "plafond de {limit} octets non applique"
            );
        }
    }

    #[test]
    fn without_reset_the_limit_is_reached() {
        let arena = Arena::new(1 << 16, 1 << 17);
        let refused = (0..8).any(|_| arena.try_alloc_slice_fill_copy(40_000, 0_u8).is_err());
        assert!(refused, "le plafond doit finir par refuser");
    }
}
