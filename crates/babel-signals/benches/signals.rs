//! Mesures de référence pour chrF et le median polish.
//!
//! Lancer avec `cargo bench -p babel-signals`. Les chiffres relevés sur la
//! machine de développement sont notés dans DECISIONS.md.

use babel_signals::chrf::{Profile, chrf_symmetric};
use babel_signals::polish::median_polish;
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

const ORDER: usize = 6;
const BETA: f32 = 2.0;
const A: &str = "Car Dieu a tant aimé le monde qu'il a donné son Fils unique, afin que quiconque \
                 croit en lui ne périsse point, mais qu'il ait la vie éternelle.";
const B: &str = "Car Dieu a tellement aimé le monde qu'il a donné son Fils, son unique, pour que \
                 tous ceux qui placent leur foi en lui échappent à la perdition et qu'ils aient \
                 la vie éternelle.";

fn chrf(c: &mut Criterion) {
    c.bench_function("chrf/profil d'un verset", |b| {
        b.iter(|| Profile::new(black_box(A), ORDER));
    });
    let (a, b_profile) = (Profile::new(A, ORDER), Profile::new(B, ORDER));
    c.bench_function("chrf/paire de profils", |b| {
        b.iter(|| chrf_symmetric(black_box(&a), black_box(&b_profile), BETA));
    });
}

/// Matrice 200 traductions × 5 000 versets, avec un bruit déterministe et
/// un dixième de cases manquantes.
fn matrix() -> Vec<Vec<f32>> {
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1_u64 << 24) as f32
    };
    (0..200)
        .map(|t| {
            (0..5000)
                .map(|v| {
                    let noise = next();
                    if noise < 0.1 {
                        f32::NAN
                    } else {
                        t as f32 * 0.01 + (v % 97) as f32 * 0.02 + noise * 0.3
                    }
                })
                .collect()
        })
        .collect()
}

fn polish(c: &mut Criterion) {
    let x = matrix();
    c.bench_function("median polish/200 x 5000", |b| {
        b.iter(|| median_polish(black_box(&x), 20, 0.0005));
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20);
    targets = chrf, polish
}
criterion_main!(benches);
