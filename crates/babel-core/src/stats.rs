//! Statistiques robustes : médiane, MAD, médiane pondérée, moyenne tronquée
//! pondérée et rangs percentiles. Fonctions pures, sans entrée-sortie.

/// Facteur qui rend le MAD comparable à un écart-type sous loi normale.
pub const MAD_TO_SIGMA: f32 = 1.4826;

/// Médiane d'une tranche. Réordonne la tranche ; `None` si elle est vide.
/// Les valeurs doivent être finies.
pub fn median(values: &mut [f32]) -> Option<f32> {
    let n = values.len();
    if n == 0 {
        return None;
    }
    let mid = n / 2;
    let (below, upper, _) = values.select_nth_unstable_by(mid, f32::total_cmp);
    let upper = *upper;
    if n % 2 == 1 {
        return Some(upper);
    }
    let lower = below
        .iter()
        .copied()
        .max_by(f32::total_cmp)
        .unwrap_or(upper);
    Some((lower + upper) / 2.0)
}

/// Écart absolu médian autour de `center`. Réordonne la tranche.
pub fn mad(values: &mut [f32], center: f32) -> Option<f32> {
    for v in values.iter_mut() {
        *v = (*v - center).abs();
    }
    median(values)
}

/// Centre et échelle robustes d'une série : (médiane, 1,4826 × MAD).
/// `None` si la série est vide. L'échelle peut être nulle.
pub fn robust_center_scale(values: &[f32]) -> Option<(f32, f32)> {
    let mut work = values.to_vec();
    let center = median(&mut work)?;
    let scale = MAD_TO_SIGMA * mad(&mut work, center)?;
    Some((center, scale))
}

/// Médiane pondérée de couples (valeur, poids). Les poids négatifs ou nuls
/// sont ignorés. Réordonne la tranche ; `None` si le poids total est nul.
pub fn weighted_median(pairs: &mut [(f32, f32)]) -> Option<f32> {
    pairs.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = pairs.iter().map(|p| f64::from(p.1.max(0.0))).sum();
    if total <= 0.0 {
        return None;
    }
    let half = total / 2.0;
    // Tolérance relative : la somme cumulée ne doit pas dépendre de l'ordre
    // d'arrivée des poids égaux.
    let eps = total * 1e-9;
    let mut cumulative = 0.0_f64;
    for (i, (value, weight)) in pairs.iter().enumerate() {
        let w = f64::from(weight.max(0.0));
        if w <= 0.0 {
            continue;
        }
        cumulative += w;
        if cumulative > half + eps {
            return Some(*value);
        }
        if (cumulative - half).abs() <= eps {
            // Exactement la moitié du poids : moyenne avec la valeur suivante.
            let next = pairs[i + 1..]
                .iter()
                .find(|p| p.1 > 0.0)
                .map_or(*value, |p| p.0);
            return Some((*value + next) / 2.0);
        }
    }
    pairs.last().map(|p| p.0)
}

/// MAD pondéré : médiane pondérée des écarts absolus à la médiane pondérée.
pub fn weighted_mad(pairs: &mut [(f32, f32)]) -> Option<f32> {
    let center = weighted_median(pairs)?;
    for p in pairs.iter_mut() {
        p.0 = (p.0 - center).abs();
    }
    weighted_median(pairs)
}

/// Moyenne pondérée tronquée : une fraction `trim` du poids total est
/// retirée à chaque extrémité (les poids de bord sont coupés au prorata).
/// Réordonne la tranche ; `None` si le poids total est nul.
pub fn weighted_trimmed_mean(pairs: &mut [(f32, f32)], trim: f32) -> Option<f32> {
    pairs.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = pairs.iter().map(|p| f64::from(p.1.max(0.0))).sum();
    if total <= 0.0 {
        return None;
    }
    let trim = f64::from(trim.clamp(0.0, 0.499));
    let low = trim * total;
    let high = (1.0 - trim) * total;
    let mut cumulative = 0.0_f64;
    let mut sum = 0.0_f64;
    let mut kept = 0.0_f64;
    for (value, weight) in pairs.iter() {
        let w = f64::from(weight.max(0.0));
        let start = cumulative;
        cumulative += w;
        let overlap = (cumulative.min(high) - start.max(low)).max(0.0);
        sum += overlap * f64::from(*value);
        kept += overlap;
    }
    (kept > 0.0).then(|| (sum / kept) as f32)
}

/// Rang percentile de chaque valeur parmi les valeurs finies : la part des
/// valeurs strictement inférieures, dans [0, 1[.
///
/// Les ex æquo partagent le même rang, celui du premier d'entre eux. Un
/// signal nul pour la plupart des versets (l'omission) leur donne ainsi le
/// rang 0, et non un rang médian. Les valeurs non finies restent NaN.
pub fn percentile_ranks(values: &[f32]) -> Vec<f32> {
    let mut order: Vec<usize> = (0..values.len())
        .filter(|i| values[*i].is_finite())
        .collect();
    order.sort_unstable_by(|a, b| values[*a].total_cmp(&values[*b]));
    let n = order.len();
    let mut ranks = vec![f32::NAN; values.len()];
    let mut i = 0;
    while i < n {
        let mut j = i + 1;
        while j < n && values[order[j]] == values[order[i]] {
            j += 1;
        }
        // Les i valeurs qui précèdent le groupe d'ex æquo i..j lui sont
        // strictement inférieures.
        for k in &order[i..j] {
            ranks[*k] = i as f32 / n as f32;
        }
        i = j;
    }
    ranks
}

/// Corrélation de Pearson entre deux séries de même longueur. `None` si
/// moins de deux points ou si une série est constante.
pub fn pearson(x: &[f32], y: &[f32]) -> Option<f32> {
    let n = x.len().min(y.len());
    if n < 2 {
        return None;
    }
    let mean = |s: &[f32]| s[..n].iter().map(|v| f64::from(*v)).sum::<f64>() / n as f64;
    let (mx, my) = (mean(x), mean(y));
    let (mut sxy, mut sxx, mut syy) = (0.0_f64, 0.0_f64, 0.0_f64);
    for i in 0..n {
        let (dx, dy) = (f64::from(x[i]) - mx, f64::from(y[i]) - my);
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    (sxx > 0.0 && syy > 0.0).then(|| (sxy / (sxx * syy).sqrt()) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn mediane_paire_et_impaire() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(median(&mut []), None);
    }

    #[test]
    fn mad_connu() {
        // Série 1..=5 : médiane 3, écarts 2 1 0 1 2, MAD 1.
        let mut v = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(mad(&mut v, 3.0), Some(1.0));
    }

    #[test]
    fn mediane_ponderee() {
        let mut p = [(1.0, 1.0), (2.0, 1.0), (10.0, 1.0)];
        assert_eq!(weighted_median(&mut p), Some(2.0));
        // Un poids dominant emporte la médiane.
        let mut p = [(1.0, 1.0), (2.0, 1.0), (10.0, 5.0)];
        assert_eq!(weighted_median(&mut p), Some(10.0));
        // Moitié exacte : moyenne des deux valeurs centrales.
        let mut p = [(1.0, 1.0), (3.0, 1.0)];
        assert_eq!(weighted_median(&mut p), Some(2.0));
        assert_eq!(weighted_median(&mut [(1.0, 0.0)]), None);
    }

    #[test]
    fn moyenne_tronquee() {
        // Dix valeurs de poids 1, 10 % retirés de chaque côté : la valeur
        // aberrante 1000 disparaît.
        let mut p: Vec<(f32, f32)> = (0..9).map(|i| (i as f32, 1.0)).collect();
        p.push((1000.0, 1.0));
        let m = weighted_trimmed_mean(&mut p, 0.1).expect("poids non nul");
        assert!((m - 4.5).abs() < 1e-5, "moyenne tronquée = {m}");
        // Sans troncature, on retrouve la moyenne pondérée.
        let mut p = [(1.0, 1.0), (3.0, 3.0)];
        assert_eq!(weighted_trimmed_mean(&mut p, 0.0), Some(2.5));
    }

    #[test]
    fn rangs_percentiles() {
        let r = percentile_ranks(&[10.0, f32::NAN, 30.0, 20.0, 20.0]);
        assert_eq!(r[0], 0.0);
        assert!(r[1].is_nan());
        assert_eq!(r[2], 0.75);
        assert_eq!(r[3], 0.25);
        assert_eq!(r[4], 0.25);
        // Une majorité d'ex æquo au minimum reste au rang 0.
        assert_eq!(
            percentile_ranks(&[0.0, 0.0, 0.0, 0.2]),
            vec![0.0, 0.0, 0.0, 0.75]
        );
    }

    #[test]
    fn pearson_connu() {
        assert_eq!(pearson(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]), Some(1.0));
        assert_eq!(pearson(&[1.0, 2.0, 3.0], &[3.0, 2.0, 1.0]), Some(-1.0));
        assert_eq!(pearson(&[1.0, 1.0, 1.0], &[3.0, 2.0, 1.0]), None);
    }

    proptest! {
        /// L'ordre des entrées ne change aucune statistique.
        #[test]
        fn insensible_a_l_ordre(
            mut pairs in prop::collection::vec((-100.0_f32..100.0, 0.1_f32..5.0), 1..40),
            seed in any::<u64>(),
        ) {
            let m1 = weighted_median(&mut pairs.clone());
            let t1 = weighted_trimmed_mean(&mut pairs.clone(), 0.1);
            // Mélange déterministe.
            let n = pairs.len();
            let mut s = seed;
            for i in (1..n).rev() {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                pairs.swap(i, (s >> 33) as usize % (i + 1));
            }
            let m2 = weighted_median(&mut pairs.clone());
            let t2 = weighted_trimmed_mean(&mut pairs, 0.1);
            prop_assert_eq!(m1, m2);
            let (t1, t2) = (t1.unwrap_or(f32::NAN), t2.unwrap_or(f32::NAN));
            prop_assert!((t1 - t2).abs() < 1e-3, "{} != {}", t1, t2);
        }

        /// Les rangs sont dans [0, 1[ et respectent l'ordre des valeurs.
        #[test]
        fn rangs_monotones(values in prop::collection::vec(-1e3_f32..1e3, 1..60)) {
            let r = percentile_ranks(&values);
            for i in 0..values.len() {
                prop_assert!(r[i] >= 0.0 && r[i] < 1.0);
                for j in 0..values.len() {
                    if values[i] < values[j] { prop_assert!(r[i] < r[j]); }
                    if values[i] == values[j] { prop_assert_eq!(r[i], r[j]); }
                }
            }
        }
    }
}
