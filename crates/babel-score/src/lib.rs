//! Score composite U(v) : rangs percentiles des signaux, combinés par une
//! moyenne pondérée (§ 4.3). Fonctions pures ; les entrées-sorties sont dans
//! [`store`].

pub mod health;
#[cfg(feature = "store")]
pub mod store;

use babel_core::stats::percentile_ranks;

/// Scores par verset. Les vecteurs valent NaN là où rien n'est défini.
#[derive(Debug, Clone, PartialEq)]
pub struct Scores {
    /// Rang percentile de chaque signal, dans l'ordre des signaux fournis.
    pub ranks: Vec<Vec<f32>>,
    /// Score composite U(v), dans [0, 1[.
    pub composite: Vec<f32>,
    /// Rang percentile de U(v).
    pub composite_rank: Vec<f32>,
}

/// Combine des rangs percentiles en un composite :
/// `U(v) = Σ ω_k · rang_k(v) / Σ ω_k`, la somme portant sur les signaux
/// définis pour le verset. Sans aucun signal défini, U(v) est NaN.
///
/// Les poids n'ont pas besoin de sommer à 1 : ils sont renormalisés.
pub fn composite(ranks: &[&[f32]], weights: &[f32]) -> Vec<f32> {
    let n = ranks.first().map_or(0, |r| r.len());
    (0..n)
        .map(|v| {
            let (mut sum, mut total) = (0.0_f32, 0.0_f32);
            for (rank, weight) in ranks.iter().zip(weights) {
                if rank[v].is_finite() && *weight > 0.0 {
                    sum += weight * rank[v];
                    total += weight;
                }
            }
            if total > 0.0 { sum / total } else { f32::NAN }
        })
        .collect()
}

/// Calcule les rangs de chaque signal puis le composite.
pub fn score(signals: &[&[f32]], weights: &[f32]) -> Scores {
    let ranks: Vec<Vec<f32>> = signals.iter().map(|s| percentile_ranks(s)).collect();
    let refs: Vec<&[f32]> = ranks.iter().map(Vec::as_slice).collect();
    let composite = composite(&refs, weights);
    let composite_rank = percentile_ranks(&composite);
    Scores {
        ranks,
        composite,
        composite_rank,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn composite_a_poids_egaux() {
        let a = [0.2, 0.8, f32::NAN];
        let b = [0.4, 0.4, f32::NAN];
        let u = composite(&[&a, &b], &[1.0, 1.0]);
        assert!((u[0] - 0.3).abs() < 1e-6);
        assert!((u[1] - 0.6).abs() < 1e-6);
        assert!(u[2].is_nan());
    }

    #[test]
    fn les_poids_sont_renormalises() {
        let a = [0.2];
        let b = [0.8];
        let u1 = composite(&[&a, &b], &[1.0, 3.0]);
        let u2 = composite(&[&a, &b], &[0.25, 0.75]);
        assert!((u1[0] - 0.65).abs() < 1e-6);
        assert!((u1[0] - u2[0]).abs() < 1e-6);
    }

    #[test]
    fn un_signal_absent_ne_penalise_pas() {
        // Le second signal manque pour le verset : le composite repose sur
        // le premier seul.
        let a = [0.9];
        let b = [f32::NAN];
        assert_eq!(composite(&[&a, &b], &[1.0, 1.0]), vec![0.9]);
        // Un poids nul retire le signal.
        let b = [0.1];
        assert_eq!(composite(&[&a, &b], &[1.0, 0.0]), vec![0.9]);
    }

    #[test]
    fn score_classe_les_versets() {
        // Trois versets, deux signaux qui s'accordent : le troisième verset
        // est le plus incertain.
        let omit = [0.0, 0.1, 0.5];
        let len = [1.0, 2.0, 9.0];
        let s = score(&[&omit, &len], &[1.0, 1.0]);
        assert!(s.composite[0] < s.composite[1] && s.composite[1] < s.composite[2]);
        assert_eq!(s.composite_rank, vec![0.0, 1.0 / 3.0, 2.0 / 3.0]);
    }

    proptest! {
        /// Le composite reste entre le plus petit et le plus grand rang.
        #[test]
        fn composite_borne(
            a in prop::collection::vec(0.01_f32..0.99, 1..30),
            wa in 0.1_f32..5.0,
            wb in 0.1_f32..5.0,
        ) {
            let b: Vec<f32> = a.iter().rev().copied().collect();
            let u = composite(&[&a, &b], &[wa, wb]);
            for i in 0..a.len() {
                let (low, high) = (a[i].min(b[i]), a[i].max(b[i]));
                prop_assert!(u[i] >= low - 1e-5 && u[i] <= high + 1e-5);
            }
        }
    }
}
