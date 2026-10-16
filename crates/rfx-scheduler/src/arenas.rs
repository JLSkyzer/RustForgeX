//! C-18 : reserve d'arenes de taches, propre a chaque fil.
//!
//! Cahier des charges : PARTIE 5.18 (`TaskContext::arena`, R-433), PARTIE 5.28 (une
//! arene par worker, remise a zero apres la tache). Decision : ADR-047. Maturite :
//! `STABLE`.
//!
//! Chaque execution de tache emprunte une arene a la reserve du fil qui l'execute, et
//! la rend remise a zero. Une tache imbriquee (`join`, execution en ligne) en emprunte
//! une autre : deux taches en cours sur un meme fil n'ecrivent jamais dans la meme
//! arene. En regime etabli, la reserve tient une arene par niveau d'imbrication et
//! aucune n'est plus creee : rien n'est demande au systeme (R-433, T-273).

use std::cell::RefCell;

use rfx_memory::Arena;

thread_local! {
    static RESERVE: RefCell<Vec<Arena>> = const { RefCell::new(Vec::new()) };
}

/// Prete une arene a `f`, puis la remet a zero et la rend a la reserve du fil.
///
/// `f` ne doit pas laisser s'echapper de panic : l'arene ne serait pas rendue. Les
/// taches sont executees sous `catch_unwind`, a l'interieur de `f`.
///
/// Renvoie le resultat de `f` et la plus grande reservation de l'arene.
pub(crate) fn with_arena<R>(
    initial: usize,
    limit: usize,
    f: impl FnOnce(&Arena) -> R,
) -> (R, usize) {
    let lent = RESERVE.with(|reserve| reserve.borrow_mut().pop());
    // Une arene d'un autre plafond vient d'un autre ordonnanceur : elle est remplacee.
    let mut arena = lent
        .filter(|arena| arena.limit() == limit)
        .unwrap_or_else(|| Arena::new(initial, limit));
    let result = f(&arena);
    let peak = arena.peak_bytes();
    arena.reset();
    RESERVE.with(|reserve| reserve.borrow_mut().push(arena));
    (result, peak)
}
