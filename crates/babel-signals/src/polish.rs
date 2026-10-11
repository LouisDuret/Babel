//! Median polish : ajustement robuste du modèle additif
//! `x[t][v] = a[t] + b[v] + résidu` par médianes itérées (Tukey).

use babel_core::stats::median;
use rayon::prelude::*;

/// Résultat du median polish. Les cases manquantes (NaN en entrée) restent
/// NaN dans les résidus.
#[derive(Debug, Clone, PartialEq)]
pub struct Polish {
    /// Effet de ligne `a[t]` : la verbosité de la traduction.
    pub row: Vec<f32>,
    /// Effet de colonne `b[v]` : la longueur propre du verset.
    pub col: Vec<f32>,
    /// Résidus, indexés `[ligne][colonne]`.
    pub residuals: Vec<Vec<f32>>,
    /// Nombre d'itérations effectuées.
    pub iterations: usize,
}

fn median_of(values: impl Iterator<Item = f32>) -> f32 {
    let mut finite: Vec<f32> = values.filter(|v| v.is_finite()).collect();
    median(&mut finite).unwrap_or(0.0)
}

/// Ajuste `x[t][v] ≈ a[t] + b[v]`. S'arrête quand la plus grande correction
/// d'une itération passe sous `tolerance`, ou après `max_iterations`.
///
/// Toutes les lignes doivent avoir la même longueur.
pub fn median_polish(x: &[Vec<f32>], max_iterations: usize, tolerance: f32) -> Polish {
    let n_cols = x.first().map_or(0, Vec::len);
    let mut residuals: Vec<Vec<f32>> = x.to_vec();
    let mut row = vec![0.0_f32; x.len()];
    let mut col = vec![0.0_f32; n_cols];
    let mut iterations = 0;

    while iterations < max_iterations {
        iterations += 1;

        let row_delta: Vec<f32> = residuals
            .par_iter()
            .map(|r| median_of(r.iter().copied()))
            .collect();
        residuals.par_iter_mut().zip(&row_delta).for_each(|(r, d)| {
            for value in r.iter_mut() {
                *value -= d;
            }
        });
        for (a, d) in row.iter_mut().zip(&row_delta) {
            *a += d;
        }

        let col_delta: Vec<f32> = (0..n_cols)
            .into_par_iter()
            .map(|v| median_of(residuals.iter().map(|r| r[v])))
            .collect();
        residuals.par_iter_mut().for_each(|r| {
            for (value, d) in r.iter_mut().zip(&col_delta) {
                *value -= d;
            }
        });
        for (b, d) in col.iter_mut().zip(&col_delta) {
            *b += d;
        }

        let largest = row_delta
            .iter()
            .chain(&col_delta)
            .fold(0.0_f32, |m, d| m.max(d.abs()));
        if largest < tolerance {
            break;
        }
    }

    Polish {
        row,
        col,
        residuals,
        iterations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITERATIONS: usize = 20;
    const TOLERANCE: f32 = 1e-5;

    fn additive(rows: &[f32], cols: &[f32]) -> Vec<Vec<f32>> {
        rows.iter()
            .map(|a| cols.iter().map(|b| a + b).collect())
            .collect()
    }

    #[test]
    fn modele_additif_exact() {
        let x = additive(&[0.0, 1.0, 2.5, -1.0], &[3.0, 4.0, 5.0, 3.5, 7.0]);
        let p = median_polish(&x, ITERATIONS, TOLERANCE);
        for r in p.residuals.iter().flatten() {
            assert!(r.abs() < 1e-4, "résidu {r}");
        }
    }

    #[test]
    fn isole_une_valeur_aberrante() {
        let mut x = additive(&[0.0, 1.0, 2.0, 3.0, 4.0], &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        x[2][3] += 10.0;
        let p = median_polish(&x, ITERATIONS, TOLERANCE);
        for (t, row) in p.residuals.iter().enumerate() {
            for (v, r) in row.iter().enumerate() {
                if (t, v) == (2, 3) {
                    assert!((r - 10.0).abs() < 1e-3, "résidu aberrant {r}");
                } else {
                    assert!(r.abs() < 1e-3, "résidu ({t}, {v}) = {r}");
                }
            }
        }
    }

    #[test]
    fn tolere_les_cases_manquantes() {
        let mut x = additive(&[0.0, 1.0, 2.0, 3.0], &[1.0, 2.0, 3.0, 4.0, 5.0]);
        x[0][1] = f32::NAN;
        x[3][4] = f32::NAN;
        let p = median_polish(&x, ITERATIONS, TOLERANCE);
        assert!(p.residuals[0][1].is_nan());
        assert!(p.residuals[3][4].is_nan());
        for r in p.residuals.iter().flatten().filter(|r| r.is_finite()) {
            assert!(r.abs() < 1e-3, "résidu {r}");
        }
    }

    #[test]
    fn matrice_vide() {
        let p = median_polish(&[], ITERATIONS, TOLERANCE);
        assert!(p.row.is_empty() && p.col.is_empty());
    }
}
