//! Tests d'ingestion sur le mini-corpus synthétique de `tests/fixtures/mini`.

use babel_core::{Config, VerseStatus};
use babel_ingest::quality::{assess, render_markdown, suggested_exclusions};
use babel_ingest::store::{CorpusPaths, read_corpus, read_translation, write_corpus};
use babel_ingest::{TextBase, corpus_from_texts, load_ebible};
use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/mini")
}

fn config() -> Config {
    Config::embedded().expect("configuration du dépôt valide")
}

#[test]
fn ingere_le_mini_corpus() {
    let loaded = load_ebible(&fixture(), &config().ingest).expect("ingestion");
    let corpus = &loaded.corpus;

    assert_eq!(corpus.n_verses(), 6);
    assert_eq!(corpus.translations.len(), 2);
    assert_eq!(corpus.n_languages(), 1);
    // Chaque traduction ingérée a exactement autant de lignes que vref.txt.
    for t in &corpus.translations {
        assert_eq!(t.text.len(), corpus.n_verses());
    }

    // Le fichier trop court est écarté, avec la raison.
    assert_eq!(loaded.rejected.len(), 1);
    assert_eq!(loaded.rejected[0].stem, "eng-short");
    assert!(loaded.rejected[0].reason.contains("5 lignes au lieu des 6"));

    let a = &corpus.translations[0];
    assert_eq!(a.meta.stem, "fra-a");
    use VerseStatus::{Absent, Present, RangeContinuation, RangeHead};
    assert_eq!(
        a.text.status,
        vec![
            Present,
            Absent,
            Present,
            RangeHead,
            RangeContinuation,
            Present
        ]
    );
    // Normalisation NFC : « créa » est précomposé après ingestion.
    assert!(a.text.text(0).contains("cr\u{e9}a"));
    assert!(!a.text.text(0).contains('\u{301}'));

    assert_eq!(a.meta.language, "fra");
    assert_eq!(a.meta.language_name, "French");
    assert_eq!(a.meta.script, "Latin");
    assert_eq!(a.meta.family, "Indo-European");
    assert_eq!(a.meta.license, "Public Domain");
    assert!(a.meta.redistributable);
    assert_eq!(a.meta.year, Some(1910));
    let b = &corpus.translations[1];
    assert!(!b.meta.redistributable);
    assert_eq!(b.meta.year, Some(1987));
    // fra-a : trois versets avec un texte propre (la tête de plage est exclue).
    assert_eq!(corpus.n_texts(), 3 + 6);
}

#[test]
fn couverture_par_livre() {
    let loaded = load_ebible(&fixture(), &config().ingest).expect("ingestion");
    let coverage = loaded.corpus.coverage();
    // fra-a : Genèse 2/3 (un verset absent), Matthieu 3/3 (la plage compte).
    assert!((coverage[0][0] - 2.0 / 3.0).abs() < 1e-6);
    assert_eq!(coverage[0][1], 1.0);
    assert_eq!(coverage[1], vec![1.0, 1.0]);
}

#[test]
fn aller_retour_parquet() {
    let loaded = load_ebible(&fixture(), &config().ingest).expect("ingestion");
    let dir = std::env::temp_dir().join(format!("babel-test-ingest-{}", std::process::id()));
    let paths = CorpusPaths::in_dir(&dir);
    write_corpus(&loaded.corpus, &paths).expect("écriture");
    let back = read_corpus(&paths).expect("relecture");
    // Lecture ciblée d'une seule traduction.
    let second = read_translation(&paths.corpus, 6, babel_core::TranslationId(1)).expect("fra-b");
    let missing = read_translation(&paths.corpus, 6, babel_core::TranslationId(2));
    std::fs::remove_dir_all(&dir).expect("nettoyage");
    assert_eq!(back, loaded.corpus);
    assert_eq!(second.status, loaded.corpus.translations[1].text.status);
    assert_eq!(second.text(1), loaded.corpus.translations[1].text.text(1));
    assert!(missing.is_err());
}

#[test]
fn rapport_qualite() {
    let cfg = config();
    let loaded = load_ebible(&fixture(), &cfg.ingest).expect("ingestion");
    let report = assess(&loaded.corpus, &loaded.rejected, &cfg.ingest);
    assert_eq!(report.books, vec![(1, 1), (2, 0)]);
    assert_eq!(report.translations[0].eligible_books, 1);
    assert_eq!(report.translations[1].eligible_books, 2);
    assert!(report.duplicates.is_empty());
    assert_eq!(report.scripts.len(), 1);
    assert!(report.scripts[0].has_font);

    let md = render_markdown(&report, &loaded.corpus, &cfg.ingest);
    assert!(md.contains("Traductions ingérées : 2"));
    assert!(md.contains("| eng-short |"));
    assert!(md.contains("| Genèse (GEN) | 3 | 1 | 1 |"));
    assert!(md.contains("Écritures sans police : aucune"));
}

/// Vingt versets d'un seul chapitre, de longueurs très contrastées.
fn verse(i: usize, extra: &str) -> String {
    let words = 2 + (i * 7) % 11;
    format!("{}{extra}", "mot ".repeat(words))
}

#[test]
fn detecte_un_decalage_de_versification() {
    let cfg = config();
    let vref: String = (1..=20).map(|i| format!("GEN 1:{i}\n")).collect();
    let straight = |extra: &str| (0..20).map(|i| verse(i, extra) + "\n").collect::<String>();
    // Traduction décalée : la ligne v porte le texte du verset v+1.
    let shifted: String = (0..20).map(|i| verse((i + 1) % 20, "x") + "\n").collect();
    let texts = [
        ("aaa-a", straight("a")),
        ("bbb-b", straight("bb")),
        ("ccc-c", straight("ccc")),
        ("ddd-d", straight("dddd")),
        ("eee-shift", shifted),
    ];
    let refs: Vec<(&str, &str)> = texts.iter().map(|(s, t)| (*s, t.as_str())).collect();
    let loaded = corpus_from_texts(&vref, &refs, &cfg.ingest).expect("corpus");
    let report = assess(&loaded.corpus, &loaded.rejected, &cfg.ingest);

    for q in &report.translations[..4] {
        assert!(q.shifted.is_empty(), "traduction alignée signalée à tort");
    }
    let flagged = &report.translations[4].shifted;
    assert_eq!(flagged.len(), 1);
    assert_eq!(flagged[0].shift, 1);
    assert!(flagged[0].shifted > 0.9);
}

#[test]
fn detecte_les_doublons() {
    let cfg = config();
    let vref: String = (1..=20).map(|i| format!("GEN 1:{i}\n")).collect();
    let original: String = (0..20).map(|i| verse(i, "a") + "\n").collect();
    // Copie conforme sauf le premier verset.
    let copy = original.replacen("mot ", "MOT ", 1);
    let other: String = (0..20).map(|i| verse(i, "autre") + "\n").collect();
    let loaded = corpus_from_texts(
        &vref,
        &[
            ("fra-a", &original),
            ("fra-copie", &copy),
            ("fra-autre", &other),
            ("eng-a", &original),
        ],
        &cfg.ingest,
    )
    .expect("corpus");
    let report = assess(&loaded.corpus, &loaded.rejected, &cfg.ingest);

    // Seule la paire de même langue est signalée : `eng-a` est identique à
    // `fra-a` mais dans une autre langue.
    assert_eq!(report.duplicates.len(), 1);
    let d = &report.duplicates[0];
    assert_eq!((d.a, d.b), (0, 1));
    assert!(!d.exact);
    assert!((d.identical_share - 0.95).abs() < 1e-6);

    let toml = suggested_exclusions(&report, &loaded.corpus, &cfg.ingest);
    assert!(toml.contains("translation = \"fra-copie\""));
    assert!(toml.contains("Doublon de fra-a"));
}

#[test]
fn base_textuelle_inferee() {
    let cfg = config();
    // Les seize versets diagnostiques, plus un verset témoin par livre pour
    // que chaque livre soit couvert au-dessus du seuil chez tout le monde.
    let diagnostic = &cfg.ingest.diagnostic_verses;
    let mut vref = String::new();
    let mut received = String::new();
    let mut critical = String::new();
    for reference in diagnostic {
        let (book, _) = reference.split_once(' ').expect("référence « LIV c:v »");
        for k in 1..=12 {
            vref.push_str(&format!("{book} 99:{k}\n"));
            received.push_str("texte commun\n");
            critical.push_str("texte commun\n");
        }
        vref.push_str(&format!("{reference}\n"));
        received.push_str("verset du texte reçu\n");
        critical.push('\n');
    }
    let loaded = corpus_from_texts(
        &vref,
        &[("eng-tr", &received), ("eng-na", &critical)],
        &cfg.ingest,
    )
    .expect("corpus");
    assert_eq!(
        loaded.corpus.translations[0].meta.text_base,
        TextBase::Received
    );
    assert_eq!(
        loaded.corpus.translations[1].meta.text_base,
        TextBase::Critical
    );
}
