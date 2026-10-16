//! C-16 : fusion et decoupage des taches.
//!
//! Cahier des charges : PARTIE 5.16, « Algorithm » ; PARTIE 5.17.6 (grain minimal).
//! Test : T-252 (fusion et decoupage n'alterent pas le resultat). Maturite : `STABLE`.
//!
//! # Fusion
//!
//! Deux taches consecutives — `B` depend de `A`, et rien d'autre ne depend de `A` — du
//! meme travail, dont le cout cumule reste sous le seuil, deviennent une seule tache qui
//! execute `A` puis `B`. Le cahier ne demande que ces trois conditions ; deux de plus
//! sont exigees ici, par prudence :
//!
//! - **la meme nature** : fusionner un instantane et un calcul melangerait deux
//!   etapes que l'executant traite differemment ;
//! - **le meme jeton d'annulation** : la tache fusionnee n'en porte qu'un. Si `A` et
//!   `B` en avaient deux, annuler celui de `A` ne l'arreterait plus.
//!
//! La fusion ne peut pas fermer de cycle : `A` n'a qu'un dependant, `B`, et un chemin de
//! `B` vers une dependance de `A` passerait deja par un cycle avant la fusion. Elle peut
//! seulement retarder `A`, qui attend desormais aussi les autres dependances de `B`.
//!
//! # Decoupage
//!
//! Une tache dont le cout depasse le seuil et dont la charge est un lot divisible est
//! decoupee en `k = min(workers libres, taille / grain)` morceaux contigus, de tailles
//! egales a un element pres. Chaque morceau a les dependances de la tache d'origine, et
//! tout ce qui dependait d'elle depend de chacun d'eux.

use crate::graph::TaskGraph;
use crate::node::{TaskId, TaskPayload, TaskSpec};

impl TaskGraph {
    /// Fusionne les taches consecutives d'un meme travail tant que leur cout cumule reste
    /// strictement sous `threshold_ns`.
    ///
    /// Repete jusqu'a ce qu'aucune fusion ne soit plus possible : une chaine de trois
    /// taches peut devenir une seule.
    ///
    /// Renvoie le nombre de fusions effectuees.
    pub fn fuse(&mut self, threshold_ns: u64) -> usize {
        let mut fused = 0;
        let mut changed = true;
        while changed {
            changed = false;
            for index in 0..self.nodes.len() {
                let first = task_id(index);
                if let Some(second) = self.fusion_partner(first, threshold_ns) {
                    self.absorb(first, second);
                    fused += 1;
                    changed = true;
                }
            }
        }
        self.stats.fusions += fused as u64;
        fused
    }

    fn fusion_partner(&self, first: TaskId, threshold_ns: u64) -> Option<TaskId> {
        let a = self.task(first)?;
        let [second] = self.dependents[first.index()].as_slice() else {
            return None;
        };
        let b = self.node(*second);
        let fusable = a.work == b.work
            && a.kind == b.kind
            && a.cancel.same_as(&b.cancel)
            && a.estimated_ns.saturating_add(b.estimated_ns) < threshold_ns;
        fusable.then_some(*second)
    }

    /// Absorbe `first` dans `second`, qui l'executera en premier.
    fn absorb(&mut self, first: TaskId, second: TaskId) {
        let a = self.nodes[first.index()].take().expect("tache vivante");
        self.dependents[first.index()].clear();
        for &dep in &a.deps {
            let feeding = &mut self.dependents[dep.index()];
            feeding.retain(|&task| task != first);
            if !feeding.contains(&second) {
                feeding.push(second);
            }
            if second < dep {
                self.descending_only = false;
            }
        }
        let b = self.node_mut(second);
        b.deps.retain(|&dep| dep != first);
        for dep in a.deps {
            if !b.deps.contains(&dep) {
                b.deps.push(dep);
            }
        }
        b.estimated_ns = b.estimated_ns.saturating_add(a.estimated_ns);
        b.priority = b.priority.max(a.priority);
        b.deadline = b.deadline.min(a.deadline);
        let tail = std::mem::replace(&mut b.payload, TaskPayload::Sequence(Vec::new()));
        b.payload = sequence(a.payload, tail);
    }

    /// Decoupe les lots trop couteux (voir le module).
    ///
    /// Ne revisite pas les morceaux qu'il vient de creer. Un `grain_min` nul ne decoupe
    /// rien.
    ///
    /// Renvoie le nombre de taches decoupees.
    pub fn split(&mut self, threshold_ns: u64, free_workers: usize, grain_min: u32) -> usize {
        if grain_min == 0 {
            return 0;
        }
        let workers = u32::try_from(free_workers).unwrap_or(u32::MAX);
        let mut split = 0;
        for index in 0..self.nodes.len() {
            let id = task_id(index);
            let Some(node) = self.task(id) else {
                continue;
            };
            let TaskPayload::Batch { handle, start, len } = node.payload else {
                continue;
            };
            let pieces = (len / grain_min).min(workers);
            if node.estimated_ns <= threshold_ns || pieces < 2 {
                continue;
            }
            self.split_into(id, handle, start, len, pieces);
            split += 1;
        }
        self.stats.splits += split as u64;
        split
    }

    fn split_into(&mut self, id: TaskId, handle: u64, start: u32, len: u32, pieces: u32) {
        let template = self.node(id).clone();
        let dependents = self.dependents[id.index()].clone();
        let base = len / pieces;
        let extra = len % pieces;
        let mut offset = start;
        for piece in 0..pieces {
            let piece_len = base + u32::from(piece < extra);
            let payload = TaskPayload::Batch {
                handle,
                start: offset,
                len: piece_len,
            };
            let estimated_ns = share(template.estimated_ns, piece_len, len);
            offset += piece_len;
            if piece == 0 {
                let node = self.node_mut(id);
                node.payload = payload;
                node.estimated_ns = estimated_ns;
                continue;
            }
            let created = self.add_task(TaskSpec {
                work: template.work,
                kind: template.kind,
                priority: template.priority,
                deadline: template.deadline,
                estimated_ns,
                payload,
                cancel: template.cancel.clone(),
            });
            // Le morceau est cree apres ses dependances : ces aretes descendent.
            for &dep in &template.deps {
                self.node_mut(created).deps.push(dep);
                self.dependents[dep.index()].push(created);
            }
            for &dependent in &dependents {
                self.node_mut(dependent).deps.push(created);
                self.dependents[created.index()].push(dependent);
                if dependent < created {
                    self.descending_only = false;
                }
            }
        }
    }
}

fn task_id(index: usize) -> TaskId {
    TaskId(u32::try_from(index).expect("identifiant attribue par add_task"))
}

/// Part `part / whole` d'un cout, sans debordement.
fn share(cost: u64, part: u32, whole: u32) -> u64 {
    let scaled = u128::from(cost) * u128::from(part) / u128::from(whole.max(1));
    u64::try_from(scaled).unwrap_or(u64::MAX)
}

/// Enchaine deux charges, a plat : une fusion de fusions reste une seule sequence.
fn sequence(first: TaskPayload, second: TaskPayload) -> TaskPayload {
    let mut parts = Vec::new();
    for payload in [first, second] {
        match payload {
            TaskPayload::Sequence(inner) => parts.extend(inner),
            other => parts.push(other),
        }
    }
    TaskPayload::Sequence(parts)
}
