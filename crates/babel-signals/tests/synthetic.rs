//! Chaque signal retrouve la divergence injectée à la main dans le
//! mini-corpus synthétique, et les propriétés de la phase 1 tiennent.

mod common;

use babel_signals::store::{VerseSignals, read_signals, write_signals, write_verse_stats};
use babel_signals::{SignalOutput, compute, eligibility, intra_divergence, language_groups};
use common::{INTRA_VERSE, LENGTH_VERSE, N_VERSES, OMITTED_VERSE, argmax, config};
use proptest::prelude::*;

const SEED: u64 = 7;

fn signals(corpus: &babel_ingest::Corpus) -> SignalOutput {
    let cfg = config();
    let excluded = vec![None; corpus.translations.len()];
    compute(corpus, excluded, cfg.ingest.coverage_min, &cfg.signals)
}

#[test]
fn s_omit_retrouve_le_verset_omis() {
    let out = signals(&common::corpus(SEED));
    assert_eq!(argmax(&out.omit), OMITTED_VERSE);
    // Quatre traductions indépendantes sur neuf n'ont pas le verset.
    assert!((out.omit[OMITTED_VERSE] - 4.0 / 9.0).abs() < 1e-6);
    for (v, value) in out.omit.iter().enumerate() {
        if v != OMITTED_VERSE {
            assert_eq!(*value, 0.0, "verset {v}");
        }
    }
    assert!(out.n_eligible.iter().all(|n| *n == 9));
}

#[test]
fn s_len_retrouve_le_verset_de_longueur_variable() {
    let out = signals(&common::corpus(SEED));
    assert_eq!(argmax(&out.len), LENGTH_VERSE);
    let mut others: Vec<f32> = out
        .len
        .iter()
        .enumerate()
        .filter(|(v, _)| *v != LENGTH_VERSE)
        .map(|(_, x)| *x)
        .collect();
    others.sort_by(f32::total_cmp);
    // Le verset injecté domine nettement le reste de la distribution.
    assert!(out.len[LENGTH_VERSE] > 2.0 * others[others.len() - 1]);
}

#[test]
fn s_intra_retrouve_le_verset_divergent() {
    let out = signals(&common::corpus(SEED));
    assert_eq!(argmax(&out.intra), INTRA_VERSE);
    for (v, n) in out.intra_languages.iter().enumerate() {
        // Au verset omis, la langue « aaa » n'a plus aucune traduction.
        assert_eq!(*n, if v == OMITTED_VERSE { 2 } else { 3 }, "verset {v}");
    }
    // Les traductions d'une langue se ressemblent sans être redondantes :
    // chacune forme son propre groupe et pèse 1.
    assert!(out.weight.iter().all(|w| *w == 1.0));
}

#[test]
fn les_copies_sont_regroupees() {
    // Deux copies conformes de la première traduction : elles forment un
    // groupe de trois avec l'originale, qui pèse 1 au total.
    let mut texts = common::translations(SEED);
    let original = texts[0].1.clone();
    texts.push(("aaa-copie1".into(), original.clone()));
    texts.push(("aaa-copie2".into(), original));
    let out = signals(&common::build(&texts));

    let third = 1.0 / 3.0;
    assert_eq!(out.weight[0], third);
    assert_eq!(out.weight[9], third);
    assert_eq!(out.weight[10], third);
    assert_eq!(out.group[9], out.group[0]);
    assert!(out.weight[1..9].iter().all(|w| *w == 1.0));
    // Les copies ne font pas monter la part d'omission : 4 voix sur 9.
    assert!((out.omit[OMITTED_VERSE] - 4.0 / 9.0).abs() < 1e-6);
}

#[test]
fn une_traduction_exclue_ne_compte_pas() {
    let corpus = common::corpus(SEED);
    let cfg = config();
    let mut excluded = vec![None; corpus.translations.len()];
    excluded[0] = Some("test".to_string());
    let out = compute(&corpus, excluded, cfg.ingest.coverage_min, &cfg.signals);
    assert!(out.n_eligible.iter().all(|n| *n == 8));
    // Trois des huit traductions restantes omettent le verset.
    assert!((out.omit[OMITTED_VERSE] - 3.0 / 8.0).abs() < 1e-6);
    assert!(out.len_z[0].iter().all(|z| z.is_nan()));
}

#[test]
fn trop_peu_de_traductions_pas_de_signal() {
    let texts = common::translations(SEED);
    let out = signals(&common::build(&texts[..4]));
    assert!(out.omit.iter().all(|v| v.is_nan()));
    assert!(out.len.iter().all(|v| v.is_nan()));
    assert!(out.intra.iter().all(|v| v.is_nan()));
}

#[test]
fn traductions_identiques_dispersion_nulle() {
    // Neuf langues, un même texte : aucune omission, aucun écart de longueur.
    let text = &common::translations(SEED)[4].1;
    let texts: Vec<(String, String)> = (0..9)
        .map(|i| (format!("l{i:02}-t"), text.clone()))
        .collect();
    let out = signals(&common::build(&texts));
    assert!(out.omit.iter().all(|v| *v == 0.0));
    assert!(out.len.iter().all(|v| *v == 0.0));

    // Dans une même langue, des traductions identiques ont une distance
    // intra-langue nulle partout, donc un signal nul.
    let texts: Vec<(String, String)> = (0..9)
        .map(|i| (format!("aaa-t{i}"), text.clone()))
        .collect();
    let corpus = common::build(&texts);
    let cfg = config();
    let excluded = vec![None; 9];
    let eligible = eligibility(&corpus, &excluded, cfg.ingest.coverage_min);
    let groups = language_groups(&corpus, &excluded);
    let (intra, _) = intra_divergence(&corpus, &eligible, &groups, &[1.0; 9], &cfg.signals);
    assert!(intra.iter().all(|v| *v == 0.0));
}

#[test]
fn aller_retour_parquet() {
    let out = signals(&common::corpus(SEED));
    let signals = VerseSignals::from_output(&out);
    let dir = std::env::temp_dir().join(format!("babel-test-signals-{}", std::process::id()));
    let (a, b) = (dir.join("signals.parquet"), dir.join("verse_stats.parquet"));
    write_signals(&signals, &a).expect("écriture des signaux");
    write_verse_stats(&signals, &b).expect("écriture des statistiques");
    let back = read_signals(&a, &b, N_VERSES).expect("relecture");
    std::fs::remove_dir_all(&dir).expect("nettoyage");
    assert_eq!(back, signals);
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (x.is_nan() && y.is_nan()) || (x - y).abs() < 1e-4)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    /// L'ordre des traductions ne change rien aux signaux.
    #[test]
    fn l_ordre_des_traductions_ne_change_rien(seed in any::<u64>(), shuffle in any::<u64>()) {
        let texts = common::translations(seed);
        let mut permuted = texts.clone();
        let mut state = shuffle;
        for i in (1..permuted.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            permuted.swap(i, (state >> 33) as usize % (i + 1));
        }
        let a = signals(&common::build(&texts));
        let b = signals(&common::build(&permuted));
        prop_assert!(close(&a.omit, &b.omit));
        prop_assert!(close(&a.len, &b.len));
        prop_assert!(close(&a.intra, &b.intra));
        prop_assert_eq!(a.n_eligible, b.n_eligible);
    }
}
