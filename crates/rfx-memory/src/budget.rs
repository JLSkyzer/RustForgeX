//! Budget memoire natif (R-533).
//!
//! Composant : C-31. Cahier des charges : PARTIE 5.28 et PARTIE 12.2.
//!
//! Le plafond n'est pas indicatif : au-dela, une allocation non critique est
//! **refusee**. Un runtime d'optimisation qui epuiserait la memoire de la machine
//! aurait exactement l'effet inverse de celui qu'on lui demande.

use std::collections::BTreeMap;

/// Categorie d'allocation, pour la ventilation dans les diagnostics (R-534).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pool {
    /// Tampons de profilage (IF-03).
    Probes,
    /// Structures de longue duree du runtime.
    Runtime,
    /// Caches, evictables en priorite au depassement.
    Cache,
}

impl Pool {
    /// Libelle utilise comme etiquette de metrique (`rfx.mem.native_bytes{pool}`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Probes => "probes",
            Self::Runtime => "runtime",
            Self::Cache => "cache",
        }
    }

    /// Indique si le pool peut etre vide pour recuperer de la memoire.
    ///
    /// Les caches sont reconstructibles : ils partent les premiers. Les tampons de
    /// profilage et les structures du runtime ne le sont pas.
    #[must_use]
    pub fn is_evictable(self) -> bool {
        matches!(self, Self::Cache)
    }
}

/// Refus d'allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetError {
    /// Octets demandes.
    pub requested: u64,
    /// Octets deja alloues au moment du refus.
    pub in_use: u64,
    /// Plafond en vigueur.
    pub limit: u64,
}

impl core::fmt::Display for BudgetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "budget memoire natif depasse : {} octets demandes, {} deja alloues, plafond {}",
            self.requested, self.in_use, self.limit
        )
    }
}

impl std::error::Error for BudgetError {}

/// Suivi du budget memoire natif.
///
/// Compte ce que le runtime alloue lui-meme. Il ne pretend pas mesurer le RSS du
/// processus : la comparaison des deux, qui revele les fuites (R-534), est faite par
/// les diagnostics, pas ici.
#[derive(Debug)]
pub struct MemoryBudget {
    limit: u64,
    per_pool: BTreeMap<Pool, u64>,
    peak: u64,
    refusals: u64,
}

impl MemoryBudget {
    /// Construit un budget avec un plafond exprime en mebioctets
    /// (`memory.max_native_mb`).
    #[must_use]
    pub fn new(max_mb: u32) -> Self {
        Self {
            limit: u64::from(max_mb) * 1024 * 1024,
            per_pool: BTreeMap::new(),
            peak: 0,
            refusals: 0,
        }
    }

    /// Reserve des octets pour un pool.
    ///
    /// # Erreurs
    ///
    /// Renvoie [`BudgetError`] si la reservation ferait depasser le plafond. Rien
    /// n'est alors reserve : un refus laisse le budget exactement dans l'etat ou il
    /// etait.
    pub fn reserve(&mut self, pool: Pool, bytes: u64) -> Result<(), BudgetError> {
        let in_use = self.in_use();
        if in_use.saturating_add(bytes) > self.limit {
            self.refusals = self.refusals.saturating_add(1);
            return Err(BudgetError {
                requested: bytes,
                in_use,
                limit: self.limit,
            });
        }
        *self.per_pool.entry(pool).or_insert(0) += bytes;
        self.peak = self.peak.max(self.in_use());
        Ok(())
    }

    /// Libere des octets d'un pool.
    ///
    /// Liberer plus que ce qui est reserve ramene le pool a zero plutot que de le
    /// faire deborder : un compteur negatif n'aurait aucun sens et masquerait le
    /// defaut d'origine.
    pub fn release(&mut self, pool: Pool, bytes: u64) {
        if let Some(current) = self.per_pool.get_mut(&pool) {
            *current = current.saturating_sub(bytes);
        }
    }

    /// Vide un pool entier et rend les octets liberes.
    pub fn release_all(&mut self, pool: Pool) -> u64 {
        self.per_pool.insert(pool, 0).unwrap_or(0)
    }

    /// Libere tous les pools evictables et rend le total recupere.
    ///
    /// C'est la premiere reaction au depassement : purger les caches avant de degrader
    /// quoi que ce soit (PARTIE 12.2).
    pub fn evict_reclaimable(&mut self) -> u64 {
        let evictable: Vec<Pool> = self
            .per_pool
            .keys()
            .copied()
            .filter(|p| p.is_evictable())
            .collect();
        evictable.into_iter().map(|p| self.release_all(p)).sum()
    }

    /// Total actuellement reserve, tous pools confondus.
    #[must_use]
    pub fn in_use(&self) -> u64 {
        self.per_pool.values().sum()
    }

    /// Octets reserves par un pool.
    #[must_use]
    pub fn in_use_by(&self, pool: Pool) -> u64 {
        self.per_pool.get(&pool).copied().unwrap_or(0)
    }

    /// Plafond en vigueur, en octets.
    #[must_use]
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Plus haut total atteint depuis le demarrage (`rfx.mem.arena_peak`).
    #[must_use]
    pub fn peak(&self) -> u64 {
        self.peak
    }

    /// Nombre de reservations refusees (`rfx.mem.alloc_failures`).
    #[must_use]
    pub fn refusals(&self) -> u64 {
        self.refusals
    }

    /// Part du plafond consommee, dans `[0, 1]`.
    #[must_use]
    pub fn usage_ratio(&self) -> f64 {
        if self.limit == 0 {
            return 1.0;
        }
        (self.in_use() as f64) / (self.limit as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T-371 : le budget est respecte.
    #[test]
    fn the_budget_is_enforced() {
        let mut budget = MemoryBudget::new(1); // 1 Mio
        assert_eq!(budget.limit(), 1024 * 1024);

        assert!(budget.reserve(Pool::Probes, 512 * 1024).is_ok());
        assert!(budget.reserve(Pool::Runtime, 512 * 1024).is_ok());
        assert_eq!(budget.in_use(), 1024 * 1024);

        let refusal = budget.reserve(Pool::Cache, 1).unwrap_err();
        assert_eq!(refusal.limit, 1024 * 1024);
        assert_eq!(refusal.requested, 1);
        assert_eq!(budget.refusals(), 1);
    }

    #[test]
    fn a_refusal_leaves_the_budget_untouched() {
        let mut budget = MemoryBudget::new(1);
        budget
            .reserve(Pool::Probes, 1024)
            .expect("reservation initiale");

        let before = budget.in_use();
        assert!(budget.reserve(Pool::Cache, 10 * 1024 * 1024).is_err());
        assert_eq!(budget.in_use(), before, "un refus ne reserve rien");
    }

    #[test]
    fn releasing_frees_the_budget() {
        let mut budget = MemoryBudget::new(1);
        budget
            .reserve(Pool::Probes, 1024 * 1024)
            .expect("reservation");
        assert!(budget.reserve(Pool::Cache, 1).is_err());

        budget.release(Pool::Probes, 512 * 1024);
        assert!(budget.reserve(Pool::Cache, 512 * 1024).is_ok());
    }

    #[test]
    fn releasing_more_than_reserved_clamps_to_zero() {
        let mut budget = MemoryBudget::new(1);
        budget.reserve(Pool::Probes, 1024).expect("reservation");
        budget.release(Pool::Probes, 999_999);
        assert_eq!(budget.in_use_by(Pool::Probes), 0);
        assert_eq!(budget.in_use(), 0);
    }

    /// PARTIE 12.2 : les caches partent avant tout le reste.
    #[test]
    fn only_caches_are_evicted() {
        let mut budget = MemoryBudget::new(4);
        budget.reserve(Pool::Probes, 1024).expect("sondes");
        budget.reserve(Pool::Runtime, 2048).expect("runtime");
        budget.reserve(Pool::Cache, 4096).expect("cache");

        let reclaimed = budget.evict_reclaimable();

        assert_eq!(reclaimed, 4096);
        assert_eq!(budget.in_use_by(Pool::Cache), 0);
        assert_eq!(budget.in_use_by(Pool::Probes), 1024, "les sondes restent");
        assert_eq!(budget.in_use_by(Pool::Runtime), 2048, "le runtime reste");
    }

    #[test]
    fn the_peak_records_the_highest_total() {
        let mut budget = MemoryBudget::new(4);
        budget.reserve(Pool::Probes, 3000).expect("reservation");
        assert_eq!(budget.peak(), 3000);

        budget.release(Pool::Probes, 3000);
        assert_eq!(budget.in_use(), 0);
        assert_eq!(budget.peak(), 3000, "le pic ne redescend pas");
    }

    #[test]
    fn the_usage_ratio_reflects_the_load() {
        let mut budget = MemoryBudget::new(1);
        assert!((budget.usage_ratio() - 0.0).abs() < f64::EPSILON);

        budget
            .reserve(Pool::Probes, 512 * 1024)
            .expect("reservation");
        assert!((budget.usage_ratio() - 0.5).abs() < 0.001);
    }

    #[test]
    fn a_zero_budget_refuses_everything() {
        let mut budget = MemoryBudget::new(0);
        assert!(budget.reserve(Pool::Probes, 1).is_err());
        assert!((budget.usage_ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn pool_labels_are_stable() {
        assert_eq!(Pool::Probes.label(), "probes");
        assert_eq!(Pool::Runtime.label(), "runtime");
        assert_eq!(Pool::Cache.label(), "cache");
        assert!(Pool::Cache.is_evictable());
        assert!(!Pool::Probes.is_evictable());
        assert!(!Pool::Runtime.is_evictable());
    }
}
