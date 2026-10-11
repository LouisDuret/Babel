//! Mesure de référence du score composite sur la Bible entière.
//!
//! Lancer avec `cargo bench -p babel-score`. Le viewer refait ce calcul à
//! chaque mouvement d'un curseur de poids : il doit tenir en moins de 16 ms.

use babel_core::stats::percentile_ranks;
use babel_score::{composite, score};
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

/// Nombre de versets de `vref.txt`.
const VERSES: usize = 41_899;

fn signal(seed: u64) -> Vec<f32> {
    let mut state = seed;
    (0..VERSES)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 40) as f32 / (1_u64 << 24) as f32
        })
        .collect()
}

fn scoring(c: &mut Criterion) {
    let signals = [signal(1), signal(2), signal(3)];
    let refs: Vec<&[f32]> = signals.iter().map(Vec::as_slice).collect();
    let weights = [1.0, 1.0, 1.0];

    c.bench_function("score/rangs percentiles d'un signal", |b| {
        b.iter(|| percentile_ranks(black_box(&signals[0])));
    });
    let ranks: Vec<Vec<f32>> = signals.iter().map(|s| percentile_ranks(s)).collect();
    let rank_refs: Vec<&[f32]> = ranks.iter().map(Vec::as_slice).collect();
    c.bench_function("score/composite a poids variables", |b| {
        b.iter(|| composite(black_box(&rank_refs), black_box(&weights)));
    });
    c.bench_function("score/rangs et composite complets", |b| {
        b.iter(|| score(black_box(&refs), black_box(&weights)));
    });
}

criterion_group!(benches, scoring);
criterion_main!(benches);
