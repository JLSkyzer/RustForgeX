//! C-10 : dependances de donnees entre unites de travail (ADR-056).
//!
//! Cahier des charges : PARTIE 5.10, R-370, R-372, FM-19 ; tests T-191 a T-193.
//! Maturite : `EXPERIMENTAL` — l'ordre des flux d'evenements (C-06) et les regions
//! (C-11) manquent, et rien ne planifie encore les unites d'apres ces dependances.
//!
//! Pour chaque unite `W`, parmi les unites supposees du meme tick (toutes, faute de
//! mieux) :
//!
//! - RAW : `X` ecrit un domaine que `W` lit — `X` produit ce que `W` consomme ;
//! - WAW : `X` et `W` ecrivent un meme domaine — conflit ;
//! - WAR : `X` lit un domaine que `W` ecrit ;
//! - ressource : `X` et `W` prennent des verrous, d'identite inconnue ;
//! - une unite aux acces `UNKNOWN` depend de tout le tick (etape 5 de la fiche).
//!
//! L'intersection se fait au niveau des domaines seulement (R-370, premiere etape) : le
//! domaine `Unknown` croise tout (R-211). Les paires ne sont jamais enumerees : un
//! ensemble de bits par domaine donne, pour chaque unite, ses dependances par union.

use rfx_model::{AccessSet, Domain, Precision, WorkId};

/// Producteurs RAW listes au plus par unite ; au-dela, elle depend de tout le tick.
pub const MAX_DEPS_PER_WORKLOAD: usize = 256;

/// Domaines connus : tous sauf `Unknown`, traite a part.
const KNOWN_DOMAINS: u32 = Domain::Unknown as u32;

/// Ce que C-10 sait d'une unite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Workload {
    /// Identifiant.
    pub id: WorkId,
    /// Lectures (C-11).
    pub reads: AccessSet,
    /// Ecritures (C-11).
    pub writes: AccessSet,
    /// Un verrou est pris sur un objet non local (C-11).
    pub locks: bool,
}

/// Pourquoi une unite depend de tout le tick (R-372).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WholeTick {
    /// Ses lectures ou ses ecritures sont inconnues.
    UnknownAccess,
    /// Plus de [`MAX_DEPS_PER_WORKLOAD`] producteurs.
    TooManyProducers,
}

/// Dependances d'une unite.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkloadDeps {
    /// Producteurs RAW, tries ; `None` si l'unite depend de tout le tick.
    pub producers: Option<Vec<WorkId>>,
    /// Motif d'une dependance a tout le tick.
    pub whole_tick: Option<WholeTick>,
    /// Unites dont elle lit ce qu'elles ecrivent.
    pub raw: u32,
    /// Unites qui ecrivent ce qu'elle ecrit.
    pub waw: u32,
    /// Unites qui lisent ce qu'elle ecrit.
    pub war: u32,
    /// Unites qui prennent aussi des verrous.
    pub resource: u32,
}

/// Dependances de toutes les unites, dans l'ordre de l'entree.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dependencies {
    /// Une entree par unite.
    pub per_workload: Vec<WorkloadDeps>,
    /// Dependances, vues de chaque unite : autres unites dont elle depend, sans
    /// doublon (`rfx.dep.edges`).
    pub edges: u64,
    /// Conflits WAW, vus de chaque unite (`rfx.dep.conflicts`).
    pub conflicts: u64,
    /// Dependances ajoutees par surete : tout le tick, verrous d'identite inconnue
    /// (`rfx.dep.conservative_edges`).
    pub conservative_edges: u64,
}

/// Ensemble de bits sur les unites.
#[derive(Clone)]
struct Bits(Vec<u64>);

impl Bits {
    fn new(n: usize) -> Self {
        Self(vec![0; n.div_ceil(64)])
    }

    fn set(&mut self, i: usize) {
        self.0[i / 64] |= 1 << (i % 64);
    }

    fn clear(&mut self, i: usize) {
        self.0[i / 64] &= !(1 << (i % 64));
    }

    fn union(&mut self, other: &Self) {
        for (a, b) in self.0.iter_mut().zip(&other.0) {
            *a |= b;
        }
    }

    fn count(&self) -> u32 {
        self.0.iter().map(|w| w.count_ones()).sum()
    }

    fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().enumerate().flat_map(|(w, bits)| {
            (0..64)
                .filter(move |b| bits & (1 << b) != 0)
                .map(move |b| w * 64 + b)
        })
    }
}

fn unknown(set: &AccessSet) -> bool {
    set.precision == Precision::Unknown || set.domains.contains(Domain::Unknown)
}

/// Domaines connus d'un ensemble, par indice de bit.
fn domains(set: &AccessSet) -> impl Iterator<Item = usize> {
    let bits = set.domains.0;
    (0..KNOWN_DOMAINS)
        .filter(move |d| bits & (1 << d) != 0)
        .map(|d| d as usize)
}

/// Pour un cote (lecteurs ou ecrivains) : un ensemble par domaine connu, les unites qui
/// touchent tout, et l'union de toutes.
struct Side {
    by_domain: Vec<Bits>,
    universal: Bits,
    any: Bits,
}

impl Side {
    fn build(workloads: &[Workload], access: impl Fn(&Workload) -> &AccessSet) -> Self {
        let n = workloads.len();
        let mut side = Self {
            by_domain: vec![Bits::new(n); KNOWN_DOMAINS as usize],
            universal: Bits::new(n),
            any: Bits::new(n),
        };
        for (i, workload) in workloads.iter().enumerate() {
            let set = access(workload);
            if unknown(set) {
                side.universal.set(i);
                side.any.set(i);
                continue;
            }
            for d in domains(set) {
                side.by_domain[d].set(i);
                side.any.set(i);
            }
        }
        side
    }

    /// Unites de ce cote qui touchent l'un des domaines de `set` ; toutes celles qui
    /// touchent quelque chose si `set` est inconnu ; aucune si `set` est vide.
    fn touching(&self, set: &AccessSet, n: usize) -> Bits {
        if unknown(set) {
            return self.any.clone();
        }
        let mut found = Bits::new(n);
        let mut touches = false;
        for d in domains(set) {
            found.union(&self.by_domain[d]);
            touches = true;
        }
        if touches {
            found.union(&self.universal);
        }
        found
    }
}

/// Dependances de chaque unite de `workloads`, supposees toutes du meme tick.
#[must_use]
pub fn dependencies(workloads: &[Workload]) -> Dependencies {
    let n = workloads.len();
    let writers = Side::build(workloads, |w| &w.writes);
    let readers = Side::build(workloads, |w| &w.reads);
    let mut lockers = Bits::new(n);
    for (i, workload) in workloads.iter().enumerate() {
        if workload.locks {
            lockers.set(i);
        }
    }

    let mut result = Dependencies::default();
    let others = u32::try_from(n.saturating_sub(1)).unwrap_or(u32::MAX);
    for (i, workload) in workloads.iter().enumerate() {
        let mut raw = writers.touching(&workload.reads, n);
        let mut waw = writers.touching(&workload.writes, n);
        let mut war = readers.touching(&workload.writes, n);
        let mut resource = if workload.locks {
            lockers.clone()
        } else {
            Bits::new(n)
        };
        for set in [&mut raw, &mut waw, &mut war, &mut resource] {
            set.clear(i);
        }
        let mut deps = WorkloadDeps {
            raw: raw.count(),
            waw: waw.count(),
            war: war.count(),
            resource: resource.count(),
            ..WorkloadDeps::default()
        };
        if unknown(&workload.reads) || unknown(&workload.writes) {
            deps.whole_tick = Some(WholeTick::UnknownAccess);
            result.edges += u64::from(others);
            result.conservative_edges += u64::from(others);
        } else {
            let mut all = raw.clone();
            all.union(&waw);
            all.union(&war);
            all.union(&resource);
            result.edges += u64::from(all.count());
            result.conservative_edges += u64::from(deps.resource);
            if deps.raw as usize > MAX_DEPS_PER_WORKLOAD {
                deps.whole_tick = Some(WholeTick::TooManyProducers);
            } else {
                let mut producers: Vec<WorkId> = raw.iter().map(|j| workloads[j].id).collect();
                producers.sort_unstable();
                deps.producers = Some(producers);
            }
        }
        result.conflicts += u64::from(deps.waw);
        result.per_workload.push(deps);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfx_model::Domains;

    fn set(domains: &[Domain]) -> AccessSet {
        AccessSet {
            domains: domains.iter().fold(Domains::EMPTY, |acc, d| acc.with(*d)),
            precision: Precision::Exact,
        }
    }

    fn unknown_set() -> AccessSet {
        AccessSet {
            domains: Domains::EMPTY.with(Domain::Unknown),
            precision: Precision::Unknown,
        }
    }

    fn workload(id: u64, reads: AccessSet, writes: AccessSet) -> Workload {
        Workload {
            id: WorkId(id),
            reads,
            writes,
            locks: false,
        }
    }

    #[test]
    fn t191_a_write_read_by_another_workload_is_a_raw_dependency() {
        let deps = dependencies(&[
            workload(1, set(&[]), set(&[Domain::EntityState])),
            workload(2, set(&[Domain::EntityState]), set(&[])),
        ]);
        assert_eq!(deps.per_workload[1].producers, Some(vec![WorkId(1)]));
        assert_eq!(deps.per_workload[1].raw, 1);
        assert_eq!(deps.per_workload[0].war, 1, "1 ecrit ce que 2 lit");
        assert_eq!(
            deps.per_workload[0].producers,
            Some(vec![]),
            "1 ne lit rien"
        );
        assert_eq!(deps.edges, 2);
        assert_eq!(deps.conservative_edges, 0);
    }

    #[test]
    fn disjoint_domains_are_independent() {
        // Analogue de T-190 au niveau des domaines : les regions n'existent pas encore.
        let deps = dependencies(&[
            workload(1, set(&[Domain::ChunkData]), set(&[Domain::ChunkData])),
            workload(2, set(&[Domain::EntityState]), set(&[Domain::EntityState])),
            workload(3, set(&[Domain::JavaStatics]), set(&[Domain::ModStatics])),
        ]);
        for w in &deps.per_workload {
            assert_eq!((w.raw, w.waw, w.war, w.resource), (0, 0, 0, 0));
            assert_eq!(w.producers, Some(vec![]));
        }
        assert_eq!(deps.edges, 0);
    }

    #[test]
    fn writes_to_the_same_domain_conflict() {
        let deps = dependencies(&[
            workload(1, set(&[]), set(&[Domain::WorldBlocks])),
            workload(2, set(&[]), set(&[Domain::WorldBlocks])),
        ]);
        assert_eq!(deps.per_workload[0].waw, 1);
        assert_eq!(deps.conflicts, 2, "vu de chaque unite");
    }

    #[test]
    fn t192_unknown_access_depends_on_the_whole_tick() {
        let deps = dependencies(&[
            workload(1, unknown_set(), unknown_set()),
            workload(2, set(&[Domain::Registries]), set(&[])),
            workload(3, set(&[]), set(&[])),
            workload(4, set(&[]), set(&[Domain::EntityState])),
        ]);
        let unknown = &deps.per_workload[0];
        assert_eq!(unknown.whole_tick, Some(WholeTick::UnknownAccess));
        assert_eq!(unknown.producers, None);
        // Lire et ecrire l'inconnu, c'est lire et ecrire tout ce que les autres touchent.
        assert_eq!((unknown.raw, unknown.waw, unknown.war), (1, 1, 1));
        // L'inconnu ecrit tout : celui qui lit un domaine connu en depend.
        assert_eq!(deps.per_workload[1].producers, Some(vec![WorkId(1)]));
        // Celui qui ne lit rien n'a pas de producteur, mais l'inconnu depend de lui.
        assert_eq!(deps.per_workload[2].producers, Some(vec![]));
        assert_eq!(
            deps.conservative_edges, 3,
            "tout le tick : trois autres unites"
        );
    }

    #[test]
    fn an_over_approximation_keeps_its_domains() {
        let mut over = set(&[Domain::Random]);
        over.precision = Precision::OverApprox;
        let deps = dependencies(&[
            workload(1, set(&[]), over),
            workload(2, set(&[Domain::Random]), set(&[])),
        ]);
        assert_eq!(
            deps.per_workload[0].whole_tick, None,
            "surapprox, pas inconnu"
        );
        assert_eq!(deps.per_workload[1].producers, Some(vec![WorkId(1)]));
    }

    #[test]
    fn t193_the_producer_list_is_capped() {
        let mut workloads: Vec<Workload> = (0..=MAX_DEPS_PER_WORKLOAD as u64)
            .map(|i| workload(i + 10, set(&[]), set(&[Domain::LevelGlobals])))
            .collect();
        workloads.push(workload(1, set(&[Domain::LevelGlobals]), set(&[])));
        let deps = dependencies(&workloads);
        let reader = deps.per_workload.last().expect("lecteur");
        assert_eq!(reader.raw as usize, MAX_DEPS_PER_WORKLOAD + 1);
        assert_eq!(reader.whole_tick, Some(WholeTick::TooManyProducers));
        assert_eq!(reader.producers, None);
    }

    #[test]
    fn locks_of_unknown_identity_are_a_resource_dependency() {
        let mut a = workload(1, set(&[]), set(&[]));
        let mut b = workload(2, set(&[]), set(&[]));
        a.locks = true;
        b.locks = true;
        let deps = dependencies(&[a, b, workload(3, set(&[]), set(&[]))]);
        assert_eq!(deps.per_workload[0].resource, 1);
        assert_eq!(deps.per_workload[2].resource, 0);
        assert_eq!(deps.conservative_edges, 2);
    }

    #[test]
    fn a_workload_never_depends_on_itself() {
        let deps = dependencies(&[workload(
            1,
            set(&[Domain::EntityState]),
            set(&[Domain::EntityState]),
        )]);
        let alone = &deps.per_workload[0];
        assert_eq!((alone.raw, alone.waw, alone.war), (0, 0, 0));
        assert_eq!(alone.producers, Some(vec![]));
    }
}
