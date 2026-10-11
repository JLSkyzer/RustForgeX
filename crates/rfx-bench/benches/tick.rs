//! Fenetre de tick (C-01, IF-02) : ce que coute au natif la tenue d'un tick.
//!
//! Cahier des charges : PARTIE 5.32, R-561. Mesure utilisee par T-401 (ADR-034) comme
//! majorant de la part native de la telemetrie : les compteurs de `TickMetrics`
//! (`ticks`, `unbalanced`, `invalid_transitions`, `hook_budget_exceeded`,
//! `total_window_ns`) sont tenus dans ces appels.
//!
//! Le cycle mesure reproduit ce que fait `rfx-ffi` a chaque tick, sans la traversee
//! JNI : six operations (`begin`, quatre phases, `end`), chacune chronometree pour
//! R-707 (`Instant::now` avant, `elapsed` apres, puis `record_hook`). La machine a
//! etats SM-04 et le chronometrage R-707 sont compris : ce n'est pas de la
//! telemetrie, mais les separer demanderait de mesurer moins que ce qui tourne.
// `criterion_group!` engendre une fonction publique que la macro ne documente pas, et
// qu'on ne peut pas documenter depuis ici. La derogation s'arrete a ce fichier : la
// cible de benchmark n'expose aucune API, elle n'a rien a documenter pour autrui.
#![allow(missing_docs)]

use std::time::Instant;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rfx_core::{TickPhase, TickWindow};
use rfx_model::Side;

/// Une operation de tick telle que `timed_tick_op` l'execute dans `rfx-ffi`.
fn timed(window: &mut TickWindow, op: impl FnOnce(&mut TickWindow) -> bool) -> bool {
    let started = Instant::now();
    let accepted = op(window);
    window.record_hook(started.elapsed()) && accepted
}

fn bench_cycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("tick_window");
    let mut window = TickWindow::default();
    let mut tick = 0_u64;
    group.bench_function("cycle", |b| {
        b.iter(|| {
            tick += 1;
            let mut ok = timed(&mut window, |w| {
                w.begin(black_box(tick), Side::Server, Instant::now());
                true
            });
            for phase in [
                TickPhase::Pre,
                TickPhase::Vanilla,
                TickPhase::Drain,
                TickPhase::Post,
            ] {
                ok &= timed(&mut window, |w| w.phase(black_box(phase)));
            }
            ok &= timed(&mut window, |w| w.end(Instant::now()));
            black_box(ok)
        });
    });
    group.finish();
    black_box(window.metrics());
}

criterion_group!(benches, bench_cycle);
criterion_main!(benches);
