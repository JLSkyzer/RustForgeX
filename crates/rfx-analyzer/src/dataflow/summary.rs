//! Resume d'une methode : ce qu'un appelant doit savoir de ses effets, exprime par
//! rapport a ses parametres (ADR-055).
//!
//! Les resumes forment un treillis fini ordonne par inclusion : [`Summary::bottom`]
//! (aucun effet) en bas, [`Summary::top`] (tout est possible) en haut, [`Summary::join`]
//! pour l'union. Le point fixe interprocedural part du bas et ne fait que monter.

use rfx_model::{Domain, Domains, SideEffectSet};

use super::effects::union;

/// Ce qu'un appelant doit savoir d'une methode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Domaines lus quel que soit l'appelant.
    pub reads: Domains,
    /// Domaines ecrits quel que soit l'appelant.
    pub writes: Domains,
    /// Par parametre logique (receveur = 0) : domaines lus sur l'objet passe, a compter
    /// chez l'appelant si cet objet n'y est pas local.
    pub param_reads: Vec<Domains>,
    /// La meme chose pour les ecritures.
    pub param_writes: Vec<Domains>,
    /// Par parametre : l'objet passe s'echappe (range hors de portee de l'appelant).
    pub escaping_params: Vec<bool>,
    /// Par parametre : l'objet passe peut etre rendu.
    pub returns_params: Vec<bool>,
    /// Un objet alloue pendant l'appel, et atteignable seulement par le retour, peut
    /// etre rendu.
    pub returns_fresh: bool,
    /// Un objet partage, ou d'origine inconnue, peut etre rendu.
    pub returns_shared: bool,
    /// Effets de bord ; `mutates_world` n'y est pas tenu, il se deduit des ecritures.
    pub side_effects: SideEffectSet,
    /// Un verrou est pris sur un objet non local.
    pub locks: bool,
    /// Une base melange objets locaux et partages : surapproximation (R-212).
    pub over: bool,
}

impl Summary {
    /// Aucun effet : point de depart du point fixe.
    #[must_use]
    pub fn bottom(params: usize) -> Self {
        Self {
            reads: Domains::EMPTY,
            writes: Domains::EMPTY,
            param_reads: vec![Domains::EMPTY; params],
            param_writes: vec![Domains::EMPTY; params],
            escaping_params: vec![false; params],
            returns_params: vec![false; params],
            returns_fresh: false,
            returns_shared: false,
            side_effects: SideEffectSet::default(),
            locks: false,
            over: false,
        }
    }

    /// Tout est possible : methode inconnue, non analysable, ou appel non resolu
    /// (R-380). Lit et ecrit `Unknown`, laisse tout s'echapper, rend un objet partage.
    #[must_use]
    pub fn top(params: usize) -> Self {
        let unknown = Domains::EMPTY.with(Domain::Unknown);
        Self {
            reads: unknown,
            writes: unknown,
            param_reads: vec![unknown; params],
            param_writes: vec![unknown; params],
            escaping_params: vec![true; params],
            returns_params: vec![false; params],
            returns_fresh: false,
            returns_shared: true,
            side_effects: SideEffectSet {
                unknown: true,
                throws_observable: true,
                ..SideEffectSet::default()
            },
            locks: true,
            over: false,
        }
    }

    /// `true` si la methode a un effet inconnu.
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        self.side_effects.unknown
    }

    /// Union avec `other` ; rend `true` si `self` a change.
    pub fn join(&mut self, other: &Self) -> bool {
        let before = self.clone();
        self.reads = union(self.reads, other.reads);
        self.writes = union(self.writes, other.writes);
        join_domains(&mut self.param_reads, &other.param_reads);
        join_domains(&mut self.param_writes, &other.param_writes);
        join_flags(&mut self.escaping_params, &other.escaping_params);
        join_flags(&mut self.returns_params, &other.returns_params);
        self.returns_fresh |= other.returns_fresh;
        self.returns_shared |= other.returns_shared;
        let (mine, theirs) = (&mut self.side_effects, &other.side_effects);
        mine.mutates_world |= theirs.mutates_world;
        mine.spawns_or_removes_entity |= theirs.spawns_or_removes_entity;
        mine.sends_packets |= theirs.sends_packets;
        mine.schedules_tasks |= theirs.schedules_tasks;
        mine.fires_events |= theirs.fires_events;
        mine.io |= theirs.io;
        mine.native_call |= theirs.native_call;
        mine.throws_observable |= theirs.throws_observable;
        mine.cancels_event |= theirs.cancels_event;
        mine.unknown |= theirs.unknown;
        self.locks |= other.locks;
        self.over |= other.over;
        *self != before
    }
}

fn join_domains(mine: &mut Vec<Domains>, theirs: &[Domains]) {
    if mine.len() < theirs.len() {
        mine.resize(theirs.len(), Domains::EMPTY);
    }
    for (a, b) in mine.iter_mut().zip(theirs) {
        *a = union(*a, *b);
    }
}

fn join_flags(mine: &mut Vec<bool>, theirs: &[bool]) {
    if mine.len() < theirs.len() {
        mine.resize(theirs.len(), false);
    }
    for (a, b) in mine.iter_mut().zip(theirs) {
        *a |= *b;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_is_an_upper_bound_and_idempotent() {
        let mut a = Summary::bottom(2);
        let mut b = Summary::bottom(3);
        b.param_writes[2] = Domains::EMPTY.with(Domain::EntityState);
        b.escaping_params[1] = true;
        assert!(a.join(&b));
        assert_eq!(a.param_writes.len(), 3);
        assert!(a.escaping_params[1]);
        assert!(!a.join(&b), "idempotent");
        let top = Summary::top(3);
        assert!(a.join(&top));
        // L'union garde ce qui etait connu, et tout devient possible.
        assert!(a.is_unknown() && a.returns_shared && a.locks);
        assert!(a.escaping_params.iter().all(|e| *e));
        assert!(a.param_writes.iter().all(|d| d.contains(Domain::Unknown)));
        assert!(a.param_writes[2].contains(Domain::EntityState));
        assert!(!a.join(&top), "idempotent avec top");
    }
}
