//! Signaux sans modèle : S_omit (omission), S_len (écart de longueur) et
//! S_intra (divergence intra-langue par chrF).
//!
//! Les fonctions de calcul sont pures : elles prennent un corpus en mémoire
//! et rendent des vecteurs. Les entrées-sorties sont dans [`store`].

pub mod chrf;
pub mod polish;
pub mod redundancy;
pub mod store;

use babel_core::config::Signals;
use babel_core::stats::{robust_center_scale, weighted_mad};
use babel_ingest::Corpus;
use chrf::{Profile, chrf_symmetric};
use polish::median_polish;
use rayon::prelude::*;
use redundancy::Groups;
use std::collections::BTreeMap;

/// Résultat du calcul des signaux. Les vecteurs par verset valent NaN là
/// où le signal n'est pas défini.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalOutput {
    /// Raison de l'exclusion de chaque traduction, si elle est exclue.
    pub excluded: Vec<Option<String>>,
    /// Éligibilité `[traduction][livre]` : couverture au-dessus du seuil et
    /// traduction non exclue.
    pub eligible: Vec<Vec<bool>>,
    /// Nombre de traductions éligibles par verset.
    pub n_eligible: Vec<u16>,
    /// Groupe de redondance de chaque traduction.
    pub group: Vec<u32>,
    /// Poids w(t) : chaque groupe pèse 1 au total.
    pub weight: Vec<f32>,
    /// Verbosité a(t) estimée par le median polish (log-longueur).
    pub verbosity: Vec<f32>,
    pub omit: Vec<f32>,
    pub len: Vec<f32>,
    pub intra: Vec<f32>,
    /// Résidus de longueur normalisés z(t, v), `[traduction][verset]`.
    pub len_z: Vec<Vec<f32>>,
    /// Nombre de langues qui contribuent à S_intra, par verset.
    pub intra_languages: Vec<u16>,
}

/// Éligibilité par livre : la traduction n'est pas exclue et couvre le
/// livre au moins à hauteur de `coverage_min`.
pub fn eligibility(
    corpus: &Corpus,
    excluded: &[Option<String>],
    coverage_min: f32,
) -> Vec<Vec<bool>> {
    corpus
        .coverage()
        .into_iter()
        .zip(excluded)
        .map(|(coverage, reason)| {
            coverage
                .into_iter()
                .map(|c| reason.is_none() && c >= coverage_min)
                .collect()
        })
        .collect()
}

/// Livre de chaque verset, pour indexer l'éligibilité.
fn book_of_verse(corpus: &Corpus) -> Vec<usize> {
    (0..corpus.n_verses())
        .map(|v| corpus.versification.book_of(v))
        .collect()
}

/// Groupes de traductions comparables par chrF : même langue et même
/// écriture, au moins deux membres non exclus.
pub fn language_groups(corpus: &Corpus, excluded: &[Option<String>]) -> Vec<Vec<usize>> {
    let mut by_key: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    for (i, t) in corpus.translations.iter().enumerate() {
        if excluded[i].is_none() {
            by_key
                .entry((t.meta.language.as_str(), t.meta.script.as_str()))
                .or_default()
                .push(i);
        }
    }
    by_key
        .into_values()
        .filter(|members| members.len() >= 2)
        .collect()
}

/// S_omit : part pondérée des traductions éligibles où le verset est vide
/// ou fusionné.
pub fn omission(
    corpus: &Corpus,
    eligible: &[Vec<bool>],
    weight: &[f32],
    min_translations: usize,
) -> (Vec<f32>, Vec<u16>) {
    let books = book_of_verse(corpus);
    let per_verse: Vec<(f32, u16)> = (0..corpus.n_verses())
        .into_par_iter()
        .map(|v| {
            let (mut omitted, mut total, mut count) = (0.0_f64, 0.0_f64, 0_u16);
            for (t, translation) in corpus.translations.iter().enumerate() {
                if !eligible[t][books[v]] {
                    continue;
                }
                count += 1;
                total += f64::from(weight[t]);
                if translation.status(v).counts_as_omitted() {
                    omitted += f64::from(weight[t]);
                }
            }
            let value = if usize::from(count) >= min_translations && total > 0.0 {
                (omitted / total) as f32
            } else {
                f32::NAN
            };
            (value, count)
        })
        .collect();
    per_verse.into_iter().unzip()
}

/// Log-longueur des versets éligibles qui ont un texte propre ; NaN ailleurs.
pub fn eligible_log_lengths(corpus: &Corpus, eligible: &[Vec<bool>]) -> Vec<Vec<f32>> {
    let books = book_of_verse(corpus);
    corpus
        .translations
        .par_iter()
        .zip(eligible)
        .map(|(t, elig)| {
            (0..corpus.n_verses())
                .map(|v| {
                    let length = t.text.lengths[v];
                    if elig[books[v]] && t.status(v).has_own_text() && length > 0 {
                        f32::from(length).ln()
                    } else {
                        f32::NAN
                    }
                })
                .collect()
        })
        .collect()
}

/// Normalisation robuste par traduction (§ 4.2) :
/// `z = (d − médiane) / (1,4826 × MAD)`, médiane et MAD pris sur les
/// versets de la traduction. Une échelle nulle (plus de la moitié des
/// mesures égales) donne z = 0 : la traduction ne s'écarte nulle part.
pub fn robust_z(values: &[f32]) -> Vec<f32> {
    let finite: Vec<f32> = values.iter().copied().filter(|v| v.is_finite()).collect();
    let Some((center, scale)) = robust_center_scale(&finite) else {
        return vec![f32::NAN; values.len()];
    };
    values
        .iter()
        .map(|v| {
            if !v.is_finite() {
                f32::NAN
            } else if scale > 0.0 {
                (v - center) / scale
            } else {
                0.0
            }
        })
        .collect()
}

/// S_len : MAD pondéré, sur les traductions, des résidus de longueur
/// normalisés par traduction.
pub fn length_dispersion(len_z: &[Vec<f32>], weight: &[f32], min_translations: usize) -> Vec<f32> {
    let n_verses = len_z.first().map_or(0, Vec::len);
    (0..n_verses)
        .into_par_iter()
        .map(|v| {
            let mut pairs: Vec<(f32, f32)> = len_z
                .iter()
                .zip(weight)
                .filter(|(z, _)| z[v].is_finite())
                .map(|(z, w)| (z[v], *w))
                .collect();
            if pairs.len() < min_translations {
                return f32::NAN;
            }
            weighted_mad(&mut pairs).unwrap_or(f32::NAN)
        })
        .collect()
}

/// Membres d'un groupe de langue présents et éligibles pour le verset.
fn present_members(
    corpus: &Corpus,
    eligible: &[Vec<bool>],
    members: &[usize],
    verse: usize,
    book: usize,
) -> Vec<usize> {
    members
        .iter()
        .copied()
        .filter(|t| eligible[*t][book] && corpus.translations[*t].status(verse).has_own_text())
        .collect()
}

/// chrF moyen de chaque paire de traductions d'un même groupe de langue,
/// mesuré sur un verset tous les `stride`. Renvoie (a, b, chrF moyen).
pub fn pairwise_similarity(
    corpus: &Corpus,
    eligible: &[Vec<bool>],
    groups: &[Vec<usize>],
    config: &Signals,
) -> Vec<(usize, usize, f32)> {
    let books = book_of_verse(corpus);
    let sampled: Vec<usize> = (0..corpus.n_verses())
        .step_by(config.redundancy_stride)
        .collect();
    groups
        .par_iter()
        .flat_map_iter(|members| {
            let n = members.len();
            let mut sum = vec![0.0_f64; n * n];
            let mut count = vec![0_u32; n * n];
            for v in &sampled {
                let profiles: Vec<Option<Profile>> = members
                    .iter()
                    .map(|t| {
                        let translation = &corpus.translations[*t];
                        (eligible[*t][books[*v]] && translation.status(*v).has_own_text())
                            .then(|| Profile::new(translation.text.text(*v), config.chrf_max_order))
                    })
                    .collect();
                for i in 0..n {
                    for j in i + 1..n {
                        if let (Some(a), Some(b)) = (&profiles[i], &profiles[j]) {
                            sum[i * n + j] += f64::from(chrf_symmetric(a, b, config.chrf_beta));
                            count[i * n + j] += 1;
                        }
                    }
                }
            }
            let mut pairs = Vec::new();
            for i in 0..n {
                for j in i + 1..n {
                    let k = i * n + j;
                    if count[k] > 0 {
                        pairs.push((
                            members[i],
                            members[j],
                            (sum[k] / f64::from(count[k])) as f32,
                        ));
                    }
                }
            }
            pairs
        })
        .collect()
}

/// Distance intra-langue d'un verset : moyenne des `1 − chrF` par paires,
/// pondérée par le produit des poids. Renvoie aussi le nombre effectif de
/// traductions indépendantes (somme des poids).
fn intra_distance(
    corpus: &Corpus,
    present: &[usize],
    weight: &[f32],
    verse: usize,
    config: &Signals,
) -> Option<(f32, f32)> {
    if present.len() < 2 {
        return None;
    }
    let profiles: Vec<Profile> = present
        .iter()
        .map(|t| {
            Profile::new(
                corpus.translations[*t].text.text(verse),
                config.chrf_max_order,
            )
        })
        .collect();
    let (mut sum, mut total) = (0.0_f64, 0.0_f64);
    for i in 0..present.len() {
        for j in i + 1..present.len() {
            let w = f64::from(weight[present[i]]) * f64::from(weight[present[j]]);
            let similarity = chrf_symmetric(&profiles[i], &profiles[j], config.chrf_beta);
            sum += w * f64::from(1.0 - similarity);
            total += w;
        }
    }
    let effective: f32 = present.iter().map(|t| weight[*t]).sum();
    (total > 0.0).then(|| ((sum / total) as f32, effective))
}

/// S_intra : pour chaque langue à au moins deux traductions, distance
/// moyenne par paires, normalisée par langue, puis moyenne pondérée sur les
/// langues. Renvoie le signal et le nombre de langues contributrices.
pub fn intra_divergence(
    corpus: &Corpus,
    eligible: &[Vec<bool>],
    groups: &[Vec<usize>],
    weight: &[f32],
    config: &Signals,
) -> (Vec<f32>, Vec<u16>) {
    let books = book_of_verse(corpus);
    let n_verses = corpus.n_verses();

    // Par langue : distance et nombre effectif de traductions, par verset.
    let per_language: Vec<(Vec<f32>, Vec<f32>)> = groups
        .iter()
        .map(|members| {
            let per_verse: Vec<(f32, f32)> = (0..n_verses)
                .into_par_iter()
                .map(|v| {
                    let present = present_members(corpus, eligible, members, v, books[v]);
                    intra_distance(corpus, &present, weight, v, config).unwrap_or((f32::NAN, 0.0))
                })
                .collect();
            per_verse.into_iter().unzip()
        })
        .collect();

    // Normalisation par langue : une langue aux traductions très libres ne
    // doit pas dominer le signal.
    let normalized: Vec<Vec<f32>> = per_language.par_iter().map(|(d, _)| robust_z(d)).collect();

    let combined: Vec<(f32, u16)> = (0..n_verses)
        .into_par_iter()
        .map(|v| {
            let (mut sum, mut total, mut languages) = (0.0_f64, 0.0_f64, 0_u16);
            for (z, (_, effective)) in normalized.iter().zip(&per_language) {
                // Une langue dont toutes les traductions sont redondantes
                // (une seule voix) n'apporte aucune information.
                let lambda = (effective[v] - 1.0).min(config.intra_language_weight_cap);
                if z[v].is_finite() && lambda > 0.0 {
                    sum += f64::from(lambda) * f64::from(z[v]);
                    total += f64::from(lambda);
                    languages += 1;
                }
            }
            if total > 0.0 {
                ((sum / total) as f32, languages)
            } else {
                (f32::NAN, 0)
            }
        })
        .collect();
    combined.into_iter().unzip()
}

/// Seuil du nombre de versets communs en dessous duquel une corrélation de
/// résidus entre deux écritures n'est pas jugée fiable.
const MIN_COMMON_VERSES: usize = 100;

/// Paires de traductions d'une même langue écrites dans deux écritures
/// différentes (translittérations), avec la corrélation de leurs résidus de
/// longueur.
pub fn cross_script_similarity(
    corpus: &Corpus,
    excluded: &[Option<String>],
    residuals: &[Vec<f32>],
) -> Vec<(usize, usize, f32)> {
    let mut by_language: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, t) in corpus.translations.iter().enumerate() {
        if excluded[i].is_none() {
            by_language
                .entry(t.meta.language.as_str())
                .or_default()
                .push(i);
        }
    }
    let pairs: Vec<(usize, usize)> = by_language
        .values()
        .flat_map(|m| {
            m.iter()
                .enumerate()
                .flat_map(move |(k, a)| m[k + 1..].iter().map(move |b| (*a, *b)))
        })
        .filter(|(a, b)| corpus.translations[*a].meta.script != corpus.translations[*b].meta.script)
        .collect();
    pairs
        .par_iter()
        .filter_map(|(a, b)| {
            let (mut xs, mut ys) = (Vec::new(), Vec::new());
            for (x, y) in residuals[*a].iter().zip(&residuals[*b]) {
                if x.is_finite() && y.is_finite() {
                    xs.push(*x);
                    ys.push(*y);
                }
            }
            if xs.len() < MIN_COMMON_VERSES {
                return None;
            }
            babel_core::stats::pearson(&xs, &ys).map(|r| (*a, *b, r))
        })
        .collect()
}

/// Calcule les trois signaux de la phase 1.
///
/// `excluded[t]` porte la raison de l'exclusion de la traduction t, ou
/// `None`. `coverage_min` est le seuil d'éligibilité par livre.
pub fn compute(
    corpus: &Corpus,
    excluded: Vec<Option<String>>,
    coverage_min: f32,
    config: &Signals,
) -> SignalOutput {
    let n = corpus.translations.len();
    let eligible = eligibility(corpus, &excluded, coverage_min);
    let groups = language_groups(corpus, &excluded);

    // Longueurs : le median polish ne dépend pas des poids.
    let x = eligible_log_lengths(corpus, &eligible);
    let polish = median_polish(
        &x,
        config.len_polish_iterations,
        config.len_polish_tolerance,
    );
    drop(x);

    // Redondance : chrF moyen à l'intérieur d'une écriture, corrélation des
    // résidus de longueur entre écritures.
    let mut redundancy = Groups::new(n);
    for (a, b, similarity) in pairwise_similarity(corpus, &eligible, &groups, config) {
        if similarity > config.redundancy_chrf {
            redundancy.union(a, b);
        }
    }
    for (a, b, correlation) in cross_script_similarity(corpus, &excluded, &polish.residuals) {
        if correlation > config.redundancy_len_correlation {
            redundancy.union(a, b);
        }
    }
    let group = redundancy.labels();
    let weight = redundancy.weights();

    let (omit, n_eligible) = omission(corpus, &eligible, &weight, config.min_translations);

    let len_z: Vec<Vec<f32>> = polish.residuals.par_iter().map(|r| robust_z(r)).collect();
    let mut len = length_dispersion(&len_z, &weight, config.min_translations);

    let (mut intra, intra_languages) =
        intra_divergence(corpus, &eligible, &groups, &weight, config);

    // Un verset sans assez de traductions éligibles n'a aucun signal.
    for v in 0..corpus.n_verses() {
        if usize::from(n_eligible[v]) < config.min_translations {
            len[v] = f32::NAN;
            intra[v] = f32::NAN;
        }
    }

    SignalOutput {
        excluded,
        eligible,
        n_eligible,
        group,
        weight,
        verbosity: polish.row,
        omit,
        len,
        intra,
        len_z,
        intra_languages,
    }
}
