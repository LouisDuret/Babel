//! chrF : F-mesure sur les n-grammes de caractères (Popović, 2015).
//!
//! Les espaces sont retirés avant le découpage, comme dans l'implémentation
//! de référence (sacreBLEU). Chaque n-gramme est réduit à une empreinte de
//! 64 bits ; un profil trié permet de compter les n-grammes communs par
//! fusion, sans table de hachage.

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// N-grammes de caractères d'un texte, pour les ordres 1 à `max_order`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// Empreintes triées, ordre par ordre.
    grams: Vec<u64>,
    /// `starts[k]..starts[k + 1]` délimite les n-grammes d'ordre k + 1.
    starts: Vec<usize>,
}

impl Profile {
    pub fn new(text: &str, max_order: usize) -> Self {
        let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
        let n = chars.len();
        let mut grams = Vec::with_capacity(n * max_order);
        let mut starts = Vec::with_capacity(max_order + 1);
        for order in 1..=max_order {
            let start = grams.len();
            starts.push(start);
            if n >= order {
                for window in chars.windows(order) {
                    let mut hash = FNV_OFFSET;
                    for c in window {
                        for byte in (*c as u32).to_le_bytes() {
                            hash = (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
                        }
                    }
                    grams.push(hash);
                }
            }
            grams[start..].sort_unstable();
        }
        starts.push(grams.len());
        Self { grams, starts }
    }

    pub fn max_order(&self) -> usize {
        self.starts.len() - 1
    }

    fn order(&self, k: usize) -> &[u64] {
        &self.grams[self.starts[k]..self.starts[k + 1]]
    }
}

/// Taille de l'intersection de deux multi-ensembles triés.
fn common(a: &[u64], b: &[u64]) -> usize {
    let (mut i, mut j, mut count) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                count += 1;
                i += 1;
                j += 1;
            }
        }
    }
    count
}

fn f_beta(precision: f32, recall: f32, beta: f32) -> f32 {
    let b2 = beta * beta;
    let denominator = b2 * precision + recall;
    if denominator <= 0.0 {
        0.0
    } else {
        (1.0 + b2) * precision * recall / denominator
    }
}

/// Précision et rappel moyens sur les ordres où les deux textes ont des
/// n-grammes. `None` si aucun ordre n'est exploitable.
fn precision_recall(hypothesis: &Profile, reference: &Profile) -> Option<(f32, f32)> {
    let orders = hypothesis.max_order().min(reference.max_order());
    let (mut precision, mut recall, mut effective) = (0.0_f32, 0.0_f32, 0_u32);
    for k in 0..orders {
        let (h, r) = (hypothesis.order(k), reference.order(k));
        if h.is_empty() || r.is_empty() {
            continue;
        }
        let matches = common(h, r) as f32;
        precision += matches / h.len() as f32;
        recall += matches / r.len() as f32;
        effective += 1;
    }
    (effective > 0).then(|| (precision / effective as f32, recall / effective as f32))
}

/// chrF d'une hypothèse contre une référence, dans [0, 1].
pub fn chrf(hypothesis: &Profile, reference: &Profile, beta: f32) -> f32 {
    match precision_recall(hypothesis, reference) {
        Some((precision, recall)) => f_beta(precision, recall, beta),
        None => 0.0,
    }
}

/// chrF symétrique : moyenne des deux sens. Entre deux traductions, aucune
/// n'est la référence de l'autre.
pub fn chrf_symmetric(a: &Profile, b: &Profile, beta: f32) -> f32 {
    match precision_recall(a, b) {
        Some((p, r)) => (f_beta(p, r, beta) + f_beta(r, p, beta)) / 2.0,
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const ORDER: usize = 6;
    const BETA: f32 = 2.0;

    fn sym(a: &str, b: &str) -> f32 {
        chrf_symmetric(&Profile::new(a, ORDER), &Profile::new(b, ORDER), BETA)
    }

    #[test]
    fn valeur_calculee_a_la_main() {
        // « abc » contre « abd », ordres 1 et 2, β = 1.
        // Unigrammes : 2 communs sur 3 ; bigrammes : 1 commun sur 2.
        // Précision = rappel = (2/3 + 1/2) / 2 = 7/12.
        let (a, b) = (Profile::new("abc", 2), Profile::new("abd", 2));
        assert!((chrf(&a, &b, 1.0) - 7.0 / 12.0).abs() < 1e-6);
    }

    #[test]
    fn beta_favorise_le_rappel() {
        // L'hypothèse courte a une précision de 1 et un rappel faible.
        let (short, long) = (Profile::new("ab", 1), Profile::new("abcdefgh", 1));
        let recall_weighted = chrf(&short, &long, 2.0);
        let balanced = chrf(&short, &long, 1.0);
        assert!(recall_weighted < balanced);
        // Précision 1, rappel 1/4, β = 2 : 5 × (1/4) / (4 + 1/4) = 5/17.
        assert!((recall_weighted - 5.0 / 17.0).abs() < 1e-6);
    }

    #[test]
    fn identiques_et_disjoints() {
        assert_eq!(sym("Au commencement", "Au commencement"), 1.0);
        assert_eq!(sym("aaaa", "bbbb"), 0.0);
        assert_eq!(sym("", "abc"), 0.0);
    }

    #[test]
    fn les_espaces_ne_comptent_pas() {
        assert_eq!(sym("au commencement", "aucommencement"), 1.0);
    }

    #[test]
    fn plus_proche_plus_haut() {
        let reference = "Au commencement, Dieu créa les cieux et la terre.";
        let close = "Au commencement Dieu créa le ciel et la terre.";
        let far = "La terre était informe et vide.";
        assert!(sym(reference, close) > sym(reference, far));
        assert!(sym(reference, close) > 0.6);
    }

    proptest! {
        #[test]
        fn symetrique_et_borne(a in "[a-e ]{0,30}", b in "[a-e ]{0,30}") {
            let (ab, ba) = (sym(&a, &b), sym(&b, &a));
            prop_assert!((ab - ba).abs() < 1e-6);
            prop_assert!((0.0..=1.0).contains(&ab));
        }

        #[test]
        fn un_texte_egale_lui_meme(a in "[a-z]{1,40}") {
            prop_assert_eq!(sym(&a, &a), 1.0);
        }
    }
}
