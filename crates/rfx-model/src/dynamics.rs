//! DM-04 : `WorkloadDynamics`, mesures dynamiques d'une unite de travail.
//!
//! Cahier des charges : PARTIE 4.4. Exigences : R-207 (histogrammes bornes en
//! memoire, aucune allocation dans le chemin chaud), R-208 (une classe `Bimodal`
//! interdit toute decision fondee sur la moyenne seule). Maturite : `STABLE`.
//!
//! Toutes les mesures sont agregees en fenetres glissantes, jamais en moyennes non
//! bornees : une moyenne depuis le demarrage cesse de decrire le present des qu'une
//! partie dure un peu.

use serde::{Deserialize, Serialize};

/// Moyenne mobile exponentielle.
///
/// Structure de taille fixe, mise a jour sans allocation : elle peut donc etre
/// employee dans le chemin chaud (R-320).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Ewma {
    /// Poids accorde a la valeur la plus recente, dans `]0, 1]`.
    alpha: f64,
    /// Valeur courante, ou `None` tant qu'aucun echantillon n'a ete observe.
    value: Option<f64>,
    /// Nombre d'echantillons integres.
    samples: u64,
}

impl Ewma {
    /// Construit une moyenne mobile.
    ///
    /// `alpha` est ramene dans `]0, 1]` : une valeur nulle figerait la moyenne sur son
    /// premier echantillon, une valeur superieure a 1 la ferait diverger.
    #[must_use]
    pub fn new(alpha: f64) -> Self {
        Self {
            alpha: alpha.clamp(f64::MIN_POSITIVE, 1.0),
            value: None,
            samples: 0,
        }
    }

    /// Integre un echantillon.
    pub fn update(&mut self, sample: f64) {
        self.value = Some(match self.value {
            None => sample,
            Some(current) => self.alpha * sample + (1.0 - self.alpha) * current,
        });
        self.samples = self.samples.saturating_add(1);
    }

    /// Valeur courante, ou `0.0` tant qu'aucun echantillon n'a ete observe.
    #[must_use]
    pub fn value(&self) -> f64 {
        self.value.unwrap_or(0.0)
    }

    /// Indique si au moins un echantillon a ete integre.
    #[must_use]
    pub fn has_samples(&self) -> bool {
        self.value.is_some()
    }

    /// Nombre d'echantillons integres depuis la creation.
    #[must_use]
    pub fn samples(&self) -> u64 {
        self.samples
    }
}

impl Default for Ewma {
    /// Alpha de 0,2 : environ neuf dixiemes du poids sur la dizaine d'echantillons
    /// les plus recents, ce qui suit une charge de jeu sans osciller a chaque tick.
    fn default() -> Self {
        Self::new(0.2)
    }
}

/// Nombre de compartiments de l'histogramme : un par puissance de deux d'un `u64`.
pub const HISTOGRAM_BUCKETS: usize = 64;

/// Serialisation des compartiments sous forme de sequence de longueur fixe.
mod buckets_serde {
    use super::HISTOGRAM_BUCKETS;
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S: Serializer>(
        buckets: &[u64; HISTOGRAM_BUCKETS],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        buckets.as_slice().serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[u64; HISTOGRAM_BUCKETS], D::Error> {
        let values = Vec::<u64>::deserialize(deserializer)?;
        <[u64; HISTOGRAM_BUCKETS]>::try_from(values.as_slice())
            .map_err(|_| D::Error::invalid_length(values.len(), &"64 compartiments d'histogramme"))
    }
}

/// Histogramme a compartiments logarithmiques, de taille fixe.
///
/// Le compartiment d'une valeur est le rang de son bit de poids fort : la resolution
/// est donc relative, fine sur les petites durees et grossiere sur les grandes, ce qui
/// correspond a la lecture qu'on en fait. Aucune allocation, aucune croissance : la
/// structure occupe la meme place quelle que soit la charge (R-207).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Histogram {
    // `serde` ne derive pas au-dela de trente-deux elements : les compartiments
    // transitent donc par une sequence, convertie de part et d'autre.
    #[serde(with = "buckets_serde")]
    buckets: [u64; HISTOGRAM_BUCKETS],
    count: u64,
    min: u64,
    max: u64,
    sum: u128,
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for Histogram {
    fn eq(&self, other: &Self) -> bool {
        self.count == other.count
            && self.min == other.min
            && self.max == other.max
            && self.sum == other.sum
            && self.buckets == other.buckets
    }
}

impl Eq for Histogram {}

impl Histogram {
    /// Construit un histogramme vide.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buckets: [0; HISTOGRAM_BUCKETS],
            count: 0,
            min: u64::MAX,
            max: 0,
            sum: 0,
        }
    }

    /// Compartiment d'une valeur : le rang de son bit de poids fort.
    #[must_use]
    fn bucket_of(value: u64) -> usize {
        if value == 0 {
            0
        } else {
            (u64::BITS - 1 - value.leading_zeros()) as usize
        }
    }

    /// Enregistre une observation.
    pub fn record(&mut self, value: u64) {
        self.buckets[Self::bucket_of(value)] += 1;
        self.count = self.count.saturating_add(1);
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        self.sum = self.sum.saturating_add(u128::from(value));
    }

    /// Nombre d'observations.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Plus petite valeur observee, ou `0` si l'histogramme est vide.
    #[must_use]
    pub fn min(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            self.min
        }
    }

    /// Plus grande valeur observee.
    #[must_use]
    pub fn max(&self) -> u64 {
        self.max
    }

    /// Moyenne des valeurs observees.
    #[must_use]
    pub fn mean(&self) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        // La somme est un u128 : la conversion perd de la precision au-dela de 2^53,
        // ce qui reste tres au-dela de toute duree de tick exprimee en nanosecondes.
        (self.sum as f64) / (self.count as f64)
    }

    /// Quantile approche, borne inferieure du compartiment atteint.
    ///
    /// La valeur rendue est la borne basse du compartiment : elle sous-estime le
    /// quantile reel d'au plus un facteur deux. Cette approximation est assumee — un
    /// quantile exact demanderait de conserver les echantillons, donc une memoire non
    /// bornee, ce qu'interdit R-207.
    #[must_use]
    pub fn quantile(&self, q: f64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let q = q.clamp(0.0, 1.0);
        // Rang du premier echantillon atteignant le quantile, en base 1.
        let target = ((self.count as f64) * q).ceil().max(1.0) as u64;

        let mut seen = 0_u64;
        for (rank, &occurrences) in self.buckets.iter().enumerate() {
            seen += occurrences;
            if seen >= target {
                return if rank == 0 { 0 } else { 1_u64 << rank };
            }
        }
        self.max
    }

    /// Mediane approchee.
    #[must_use]
    pub fn p50(&self) -> u64 {
        self.quantile(0.50)
    }

    /// Quantile 95 approche.
    #[must_use]
    pub fn p95(&self) -> u64 {
        self.quantile(0.95)
    }

    /// Quantile 99 approche.
    #[must_use]
    pub fn p99(&self) -> u64 {
        self.quantile(0.99)
    }

    /// Remet l'histogramme a zero, sans desallouer.
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

/// Classe de cout d'une unite de travail (PARTIE 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub enum Heat {
    /// Moins de 50 microsecondes par tick.
    #[default]
    Cold,
    /// Entre 50 et 250 microsecondes par tick.
    Warm,
    /// Entre 250 microsecondes et 1 milliseconde par tick.
    Hot,
    /// Plus d'une milliseconde par tick.
    Critical,
}

impl Heat {
    /// Seuil haut de `Cold`, en nanosecondes par tick.
    pub const COLD_MAX_NS: u64 = 50_000;
    /// Seuil haut de `Warm`, en nanosecondes par tick.
    pub const WARM_MAX_NS: u64 = 250_000;
    /// Seuil haut de `Hot`, en nanosecondes par tick.
    pub const HOT_MAX_NS: u64 = 1_000_000;

    /// Classe une unite de travail d'apres son cout par tick.
    #[must_use]
    pub fn classify(ns_per_tick: u64) -> Self {
        if ns_per_tick < Self::COLD_MAX_NS {
            Self::Cold
        } else if ns_per_tick < Self::WARM_MAX_NS {
            Self::Warm
        } else if ns_per_tick < Self::HOT_MAX_NS {
            Self::Hot
        } else {
            Self::Critical
        }
    }

    /// Libelle publie dans les rapports et `/rfx top`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Cold => "COLD",
            Self::Warm => "WARM",
            Self::Hot => "HOT",
            Self::Critical => "CRITICAL",
        }
    }
}

impl core::fmt::Display for Heat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.label())
    }
}

/// Regularite du cout observe (PARTIE 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VarianceClass {
    /// Cout regulier : la moyenne decrit correctement le comportement.
    #[default]
    Stable,
    /// Cout disperse, sans mode marque.
    Noisy,
    /// Deux regimes distincts : la moyenne ne decrit aucun des deux (R-208).
    Bimodal,
}

impl VarianceClass {
    /// Indique si une decision peut se fonder sur la moyenne seule.
    ///
    /// `Bimodal` l'interdit : la decision doit alors utiliser le p95 (R-208).
    #[must_use]
    pub fn allows_mean_based_decision(self) -> bool {
        !matches!(self, Self::Bimodal)
    }

    /// Libelle publie dans les rapports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Stable => "STABLE",
            Self::Noisy => "NOISY",
            Self::Bimodal => "BIMODAL",
        }
    }
}

/// DM-04 : mesures dynamiques d'une unite de travail.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorkloadDynamics {
    /// Appels par tick.
    pub calls_per_tick: Ewma,
    /// Temps processeur par appel, en nanosecondes.
    pub cpu_ns: Histogram,
    /// Temps ecoule par appel, en nanosecondes.
    pub wall_ns: Histogram,
    /// Octets alloues par tick.
    pub alloc_bytes: Ewma,
    /// Part du temps propre dans le temps total du tick, dans `[0, 1]`.
    pub self_ns_share: f32,
    /// Dernier tick ou l'unite a ete observee.
    pub last_seen_tick: u64,
    /// Nombre total d'appels observes.
    pub total_calls: u64,
    /// Classe de cout.
    pub heat: Heat,
    /// Regularite du cout.
    pub variance_class: VarianceClass,
    /// Confiance dans l'observation, dans `[0, 1]`.
    pub observation_quality: f32,
}

impl Default for WorkloadDynamics {
    fn default() -> Self {
        Self {
            calls_per_tick: Ewma::default(),
            cpu_ns: Histogram::new(),
            wall_ns: Histogram::new(),
            alloc_bytes: Ewma::default(),
            self_ns_share: 0.0,
            last_seen_tick: 0,
            total_calls: 0,
            heat: Heat::Cold,
            variance_class: VarianceClass::Stable,
            observation_quality: 0.0,
        }
    }
}

/// Nombre d'appels au-dela duquel l'observation est jugee pleinement fiable.
const FULL_QUALITY_CALLS: f32 = 1_000.0;

impl WorkloadDynamics {
    /// Cout estime par tick, en nanosecondes : mediane par appel fois appels par tick.
    #[must_use]
    pub fn ns_per_tick(&self) -> u64 {
        let per_call = self.cpu_ns.p50() as f64;
        let calls = self.calls_per_tick.value().max(0.0);
        (per_call * calls) as u64
    }

    /// Recalcule chaleur, classe de variance et qualite d'observation.
    ///
    /// Appelee apres integration des mesures d'un tick, jamais dans le chemin chaud.
    pub fn refresh(&mut self) {
        self.heat = Heat::classify(self.ns_per_tick());
        self.variance_class = self.classify_variance();
        self.observation_quality = ((self.total_calls as f32) / FULL_QUALITY_CALLS).clamp(0.0, 1.0);
    }

    /// Classe la regularite du cout a partir de la dispersion observee.
    ///
    /// Le rapport p95/p50 mesure l'etalement de la distribution. Un facteur superieur
    /// a huit revele deux regimes bien separes — typiquement un chemin rapide et un
    /// chemin lent empruntes alternativement — que la moyenne ne decrirait ni l'un ni
    /// l'autre.
    fn classify_variance(&self) -> VarianceClass {
        let p50 = self.cpu_ns.p50();
        if self.cpu_ns.count() < 32 || p50 == 0 {
            // Trop peu d'echantillons pour conclure : on ne declare pas une dispersion
            // qu'on n'a pas mesuree.
            return VarianceClass::Stable;
        }
        let ratio = (self.cpu_ns.p95() as f64) / (p50 as f64);
        if ratio >= 8.0 {
            VarianceClass::Bimodal
        } else if ratio >= 3.0 {
            VarianceClass::Noisy
        } else {
            VarianceClass::Stable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ewma_starts_on_its_first_sample() {
        let mut e = Ewma::new(0.5);
        assert!(!e.has_samples());
        assert_eq!(e.value(), 0.0);

        e.update(10.0);
        assert!(e.has_samples());
        assert!((e.value() - 10.0).abs() < f64::EPSILON);

        e.update(20.0);
        assert!((e.value() - 15.0).abs() < f64::EPSILON);
        assert_eq!(e.samples(), 2);
    }

    #[test]
    fn ewma_converges_towards_a_constant_signal() {
        let mut e = Ewma::new(0.3);
        for _ in 0..200 {
            e.update(42.0);
        }
        assert!((e.value() - 42.0).abs() < 0.001, "obtenu {}", e.value());
    }

    #[test]
    fn an_aberrant_alpha_is_clamped() {
        let mut zero = Ewma::new(0.0);
        zero.update(1.0);
        zero.update(100.0);
        // Alpha ramene au plus petit positif : la moyenne bouge, sans se figer.
        assert!(zero.value() >= 1.0);

        let mut excessive = Ewma::new(5.0);
        excessive.update(1.0);
        excessive.update(100.0);
        assert!((excessive.value() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn histogram_buckets_follow_powers_of_two() {
        assert_eq!(Histogram::bucket_of(0), 0);
        assert_eq!(Histogram::bucket_of(1), 0);
        assert_eq!(Histogram::bucket_of(2), 1);
        assert_eq!(Histogram::bucket_of(3), 1);
        assert_eq!(Histogram::bucket_of(4), 2);
        assert_eq!(Histogram::bucket_of(u64::MAX), 63);
    }

    #[test]
    fn histogram_tracks_count_min_max_and_mean() {
        let mut h = Histogram::new();
        assert_eq!(h.count(), 0);
        assert_eq!(h.min(), 0);
        assert_eq!(h.p50(), 0);

        for v in [10_u64, 20, 30, 40] {
            h.record(v);
        }
        assert_eq!(h.count(), 4);
        assert_eq!(h.min(), 10);
        assert_eq!(h.max(), 40);
        assert!((h.mean() - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn quantiles_frame_the_distribution() {
        let mut h = Histogram::new();
        // Mille valeurs autour de 1000 ns, puis dix a 1 000 000 ns.
        for _ in 0..1000 {
            h.record(1_000);
        }
        for _ in 0..10 {
            h.record(1_000_000);
        }

        // La borne basse du compartiment de 1000 est 512.
        assert_eq!(h.p50(), 512);
        // Le p99 reste dans le mode principal, les valeurs extremes etant marginales.
        assert_eq!(h.p99(), 512);
        assert_eq!(h.max(), 1_000_000);
    }

    #[test]
    fn a_quantile_never_underestimates_by_more_than_a_factor_of_two() {
        let mut h = Histogram::new();
        for v in 1..=10_000_u64 {
            h.record(v);
        }
        let p50 = h.p50();
        // La mediane exacte vaut 5000 : la borne basse du compartiment est 4096.
        assert!(p50 <= 5000 && p50 * 2 > 5000, "p50 = {p50}");
    }

    #[test]
    fn heat_thresholds_match_the_specification() {
        assert_eq!(Heat::classify(0), Heat::Cold);
        assert_eq!(Heat::classify(49_999), Heat::Cold);
        assert_eq!(Heat::classify(50_000), Heat::Warm);
        assert_eq!(Heat::classify(249_999), Heat::Warm);
        assert_eq!(Heat::classify(250_000), Heat::Hot);
        assert_eq!(Heat::classify(999_999), Heat::Hot);
        assert_eq!(Heat::classify(1_000_000), Heat::Critical);
    }

    #[test]
    fn heat_is_ordered_from_cold_to_critical() {
        assert!(Heat::Cold < Heat::Warm);
        assert!(Heat::Warm < Heat::Hot);
        assert!(Heat::Hot < Heat::Critical);
    }

    /// R-208 : une distribution bimodale interdit la decision sur la moyenne.
    #[test]
    fn a_bimodal_distribution_forbids_mean_based_decisions() {
        let mut d = WorkloadDynamics::default();
        // Deux regimes nettement separes : 1 us et 1 ms.
        for _ in 0..100 {
            d.cpu_ns.record(1_000);
        }
        for _ in 0..20 {
            d.cpu_ns.record(1_000_000);
        }
        d.total_calls = 120;
        d.calls_per_tick.update(1.0);
        d.refresh();

        assert_eq!(d.variance_class, VarianceClass::Bimodal);
        assert!(!d.variance_class.allows_mean_based_decision());
    }

    #[test]
    fn a_regular_cost_is_classified_stable() {
        let mut d = WorkloadDynamics::default();
        for _ in 0..100 {
            d.cpu_ns.record(1_000);
        }
        d.total_calls = 100;
        d.calls_per_tick.update(1.0);
        d.refresh();

        assert_eq!(d.variance_class, VarianceClass::Stable);
        assert!(d.variance_class.allows_mean_based_decision());
    }

    #[test]
    fn variance_is_not_declared_without_enough_samples() {
        let mut d = WorkloadDynamics::default();
        d.cpu_ns.record(1_000);
        d.cpu_ns.record(1_000_000);
        d.total_calls = 2;
        d.refresh();

        assert_eq!(
            d.variance_class,
            VarianceClass::Stable,
            "deux echantillons ne suffisent pas a conclure a une dispersion"
        );
    }

    #[test]
    fn cost_per_tick_combines_median_and_call_rate() {
        let mut d = WorkloadDynamics::default();
        for _ in 0..100 {
            d.cpu_ns.record(1_024);
        }
        for _ in 0..50 {
            d.calls_per_tick.update(400.0);
        }
        d.total_calls = 100;
        d.refresh();

        // p50 = 1024 ns par appel, environ 400 appels par tick : au-dela de 250 us.
        assert_eq!(d.ns_per_tick(), 1_024 * 400);
        assert_eq!(d.heat, Heat::Hot);
    }

    #[test]
    fn observation_quality_grows_with_the_number_of_calls() {
        let mut d = WorkloadDynamics::default();
        d.refresh();
        assert!((d.observation_quality - 0.0).abs() < f32::EPSILON);

        d.total_calls = 500;
        d.refresh();
        assert!((d.observation_quality - 0.5).abs() < 0.01);

        d.total_calls = 100_000;
        d.refresh();
        assert!(
            (d.observation_quality - 1.0).abs() < f32::EPSILON,
            "bornee a 1"
        );
    }
}
