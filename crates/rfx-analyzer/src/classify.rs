//! C-08 : drapeaux de classification et confiance, fonctions pures des faits.
//!
//! Cahier des charges : PARTIE 5.8, R-351 (« le calcul des flags DOIT etre une fonction
//! pure des faits »), R-206, R-214 a R-218, P-02, T-011, T-170. Decision : ADR-054.
//! Maturite : `STABLE`.
//!
//! Un drapeau qui **ouvre** une transformation (`PURE`, `PARALLELIZABLE`, `CACHEABLE`,
//! `THREAD_SAFE`, `READ_ONLY`, `DETERMINISTIC`) n'est pose que si les faits le prouvent.
//! Un drapeau qui **ferme** (`UNKNOWN`, `WRITE`, `SIDE_EFFECT`, `STATEFUL`,
//! `MAIN_THREAD_REQUIRED`, `ORDER_SENSITIVE`) est pose des que le fait manque : le pire
//! cas (R-206). `BATCHABLE`, `RUST_CANDIDATE`, `IR_ELIGIBLE`, `CPU_INTENSIVE` et
//! `MEMORY_INTENSIVE` ne sont jamais poses ici (ADR-054, decision 2).

use rfx_model::{
    AccessSet, ClassificationFlags as F, Confidence, DependencySet, DeterminismFacts, Domain,
    Domains, Heat, Precision, Side, SideEffectSet, ThreadFacts, Tri,
};

/// Domaines dont l'ecriture garde un etat d'un appel a l'autre.
const STATE_DOMAINS: Domains = Domains(
    1 << Domain::ModStatics as u8
        | 1 << Domain::JavaStatics as u8
        | 1 << Domain::ThreadLocals as u8
        | 1 << Domain::LevelGlobals as u8
        | 1 << Domain::Registries as u8
        | 1 << Domain::Unknown as u8,
);

/// Tout ce que C-08 sait d'une unite de travail. Un champ `None` : l'analyse qui le
/// produit n'a pas tourne, ou n'existe pas encore.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// Chaleur mesuree (C-05).
    pub heat: Heat,
    /// Cote de l'unite de travail.
    pub side: Side,
    /// Qualite de l'observation dynamique, 0 a 1 (C-05).
    pub observation_quality: f32,
    /// Dependances : appels (C-07), donnees et ordre (C-10).
    pub deps: Option<DependencySet>,
    /// Un appel reflexif a ete rencontre (C-07).
    pub reflection: bool,
    /// Lectures (C-11).
    pub reads: Option<AccessSet>,
    /// Ecritures (C-11).
    pub writes: Option<AccessSet>,
    /// Effets de bord (C-11).
    pub side_effects: Option<SideEffectSet>,
    /// Faits de fil (C-12).
    pub thread: Option<ThreadFacts>,
    /// Faits de determinisme (C-13).
    pub determinism: Option<DeterminismFacts>,
}

impl Facts {
    /// `true` si les dependances sont entierement connues : graphe complet, aucun appel
    /// non resolu (R-215), donnees, ordre et verrous analyses.
    #[must_use]
    pub fn dependencies_known(&self) -> bool {
        self.deps.as_ref().is_some_and(|deps| {
            !deps.truncated
                && deps.unresolved_calls == 0
                && deps.data_deps.is_some()
                && deps.order_deps.is_some()
                && deps.external_locks.is_some()
        })
    }

    /// `true` si aucun fait d'analyse ne manque ni n'est incertain.
    #[must_use]
    pub fn complete(&self) -> bool {
        let known =
            |set: Option<&AccessSet>| set.is_some_and(|s| s.precision != Precision::Unknown);
        self.dependencies_known()
            && known(self.reads.as_ref())
            && known(self.writes.as_ref())
            && self.side_effects.is_some_and(|e| !e.unknown)
            && self.thread.is_some()
            && self.determinism.is_some()
    }
}

/// Drapeaux de classification (DM-11) : fonction pure des faits (R-351).
#[must_use]
pub fn classify(facts: &Facts) -> F {
    let unknown = !facts.complete();
    let writes_known = facts.writes.filter(|w| w.precision != Precision::Unknown);
    let effects_known = facts.side_effects.filter(|e| !e.unknown);

    let write = writes_known.is_none_or(|w| !w.domains.is_empty());
    let side_effect = effects_known.is_none_or(|e| e.any());
    let read_only = !write && !side_effect;
    let deterministic = facts
        .determinism
        .is_some_and(|d| d.deterministic == Tri::Yes);
    let order_sensitive = facts
        .determinism
        .is_none_or(|d| d.order_sensitive != Tri::No);
    let main_thread = facts
        .thread
        .is_none_or(|t| t.main_thread_required != Tri::No);
    let stateful = writes_known.is_none_or(|w| w.domains.intersects(STATE_DOMAINS));
    let pure = !unknown && read_only && deterministic;
    // R-391 : THREAD_SAFE exige PURE, aucune preuve d'isolation n'existant encore.
    let thread_safe = pure
        && facts.thread.is_some_and(|t| {
            t.main_thread_required == Tri::No
                && !t.touches_non_threadsafe_api
                && !t.uses_thread_locals
                && !t.uses_locks
                && !t.uses_synchronized
        });
    // R-390 : une unite qui ecrit, vue sur plusieurs roles de fil, est UNSAFE ; des
    // roles inconnus ne l'excluent pas (ADR-057).
    let multi_thread_writer = write
        && facts
            .thread
            .is_none_or(|t| t.observed_roles.is_none_or(|roles| roles.count() > 1));
    let parallelizable = pure && thread_safe && !main_thread && !order_sensitive;

    F::EMPTY
        .set(F::UNKNOWN, unknown)
        .set(F::WRITE, write)
        .set(F::SIDE_EFFECT, side_effect)
        .set(F::READ_ONLY, read_only)
        .set(F::DETERMINISTIC, deterministic)
        .set(F::ORDER_SENSITIVE, order_sensitive)
        .set(F::MAIN_THREAD_REQUIRED, main_thread)
        .set(F::THREAD_SAFE, thread_safe)
        .set(F::STATEFUL, stateful)
        .set(F::PURE, pure)
        .set(F::PARALLELIZABLE, parallelizable)
        .set(F::CACHEABLE, pure)
        .set(
            F::UNSAFE,
            facts.reflection
                || facts.side_effects.is_some_and(|e| e.native_call)
                || multi_thread_writer,
        )
        .set(F::HOT, facts.heat >= Heat::Hot)
        .set(F::CLIENT_ONLY, facts.side == Side::Client)
        .set(F::SERVER_ONLY, facts.side == Side::Server)
}

/// Confiance (DM-12, R-218). Statique : 1 si tous les faits sont connus et surs, 0
/// sinon — une preuve partielle n'est pas une preuve. Dynamique : la qualite de
/// l'observation (C-05). Validation : 0, faute de shadow runs (C-21).
#[must_use]
pub fn confidence(facts: &Facts) -> Confidence {
    let static_analysis = if facts.complete() { 1.0 } else { 0.0 };
    Confidence::new(static_analysis, facts.observation_quality, 0.0)
}
