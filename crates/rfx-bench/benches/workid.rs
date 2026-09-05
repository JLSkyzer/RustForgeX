//! B-10 : coût du calcul d'empreinte `WorkId` (PARTIE 21.5).
//!
//! Composant mesuré : DM-01. Appelé une fois par méthode retenue, au chargement de sa
//! classe — jamais dans une fenêtre de tick.
// `criterion_group!` engendre une fonction publique que la macro ne documente pas, et
// qu'on ne peut pas documenter depuis ici. La derogation s'arrete a ce fichier : la
// cible de benchmark n'expose aucune API, elle n'a rien a documenter pour autrui.
#![allow(missing_docs)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rfx_model::{Side, WorkDescriptor, WorkId};

/// Descripteur de taille réaliste : une méthode de Minecraft, nom interne complet.
fn descriptor() -> WorkDescriptor {
    WorkDescriptor {
        owner_id: "minecraft".to_owned(),
        class_internal_name: "net/minecraft/world/entity/monster/Zombie".to_owned(),
        method_name: "customServerAiStep".to_owned(),
        method_descriptor: "()V".to_owned(),
        call_context_hash: 0,
        side: Side::Server,
    }
}

fn bench(c: &mut Criterion) {
    let descriptor = descriptor();
    c.bench_function("compute", |b| {
        b.iter(|| WorkId::compute(black_box(&descriptor)))
    });
}

criterion_group!(workid, bench);
criterion_main!(workid);
