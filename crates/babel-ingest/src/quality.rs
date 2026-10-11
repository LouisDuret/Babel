//! Contrôle qualité du corpus : couverture par livre, décalages de
//! versification, doublons, lignes non textuelles, écritures sans police.
//!
//! Le résultat est un rapport Markdown et une liste d'exclusions proposées ;
//! rien n'est exclu automatiquement.

use crate::corpus::{Corpus, Rejected, text_base_counts};
use babel_core::config::Ingest;
use babel_core::stats::{median, pearson};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Écritures couvertes par les polices livrées avec le viewer (polices par
/// défaut d'egui dans le prototype).
pub const SCRIPTS_WITH_FONT: &[&str] = &["Latin", "Greek", "Cyrillic"];

/// Nombre minimal de traductions pour estimer la longueur de consensus
/// d'un verset.
const CONSENSUS_MIN_TRANSLATIONS: usize = 3;

/// Chapitre dont le profil de longueurs suit mieux le consensus décalé.
#[derive(Debug, Clone, PartialEq)]
pub struct ShiftedChapter {
    pub book: usize,
    pub chapter: usize,
    /// +1 : le verset v ressemble au consensus de v+1 ; −1 : à celui de v−1.
    pub shift: i8,
    /// Corrélation avec le consensus aligné.
    pub aligned: f32,
    /// Corrélation avec le consensus décalé.
    pub shifted: f32,
}

/// Deux traductions d'une même langue au texte (presque) identique.
#[derive(Debug, Clone, PartialEq)]
pub struct Duplicate {
    pub a: usize,
    pub b: usize,
    /// Part des versets communs dont le texte est identique.
    pub identical_share: f32,
    pub common: usize,
    /// Fichiers identiques octet pour octet.
    pub exact: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranslationQuality {
    /// Part de tous les versets couverts par la traduction.
    pub coverage: f32,
    /// Nombre de livres couverts au-dessus du seuil d'éligibilité.
    pub eligible_books: usize,
    pub shifted: Vec<ShiftedChapter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptCount {
    pub script: String,
    pub translations: usize,
    pub has_font: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QualityReport {
    pub rejected: Vec<Rejected>,
    pub translations: Vec<TranslationQuality>,
    /// Par livre : (traductions éligibles, traductions partielles).
    pub books: Vec<(usize, usize)>,
    pub duplicates: Vec<Duplicate>,
    pub scripts: Vec<ScriptCount>,
}

/// Log-longueur de chaque verset qui a un texte propre ; NaN ailleurs.
/// Indexé `[traduction][verset]`.
pub fn log_lengths(corpus: &Corpus) -> Vec<Vec<f32>> {
    corpus
        .translations
        .par_iter()
        .map(|t| {
            t.text
                .status
                .iter()
                .zip(&t.text.lengths)
                .map(|(s, l)| {
                    if s.has_own_text() && *l > 0 {
                        f32::from(*l).ln()
                    } else {
                        f32::NAN
                    }
                })
                .collect()
        })
        .collect()
}

/// Consensus de longueur en une passe : `a[t]` est la médiane des
/// log-longueurs de la traduction (sa verbosité), `b[v]` la médiane sur les
/// traductions de la log-longueur centrée du verset. `b[v]` est NaN si trop
/// peu de traductions portent le verset.
pub fn length_consensus(x: &[Vec<f32>]) -> (Vec<f32>, Vec<f32>) {
    let a: Vec<f32> = x
        .par_iter()
        .map(|row| {
            let mut finite: Vec<f32> = row.iter().copied().filter(|v| v.is_finite()).collect();
            median(&mut finite).unwrap_or(f32::NAN)
        })
        .collect();
    let n_verses = x.first().map_or(0, Vec::len);
    let b: Vec<f32> = (0..n_verses)
        .into_par_iter()
        .map(|v| {
            let mut column: Vec<f32> = x
                .iter()
                .zip(&a)
                .map(|(row, at)| row[v] - at)
                .filter(|d| d.is_finite())
                .collect();
            if column.len() < CONSENSUS_MIN_TRANSLATIONS {
                return f32::NAN;
            }
            median(&mut column).unwrap_or(f32::NAN)
        })
        .collect();
    (a, b)
}

/// Corrélation entre le profil d'une traduction sur `range` et le consensus
/// décalé de `shift` versets, sans sortir du chapitre.
fn shifted_correlation(
    row: &[f32],
    consensus: &[f32],
    range: std::ops::Range<usize>,
    shift: isize,
    min_verses: usize,
) -> Option<f32> {
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    for v in range.clone() {
        let w = v as isize + shift;
        if w < range.start as isize || w >= range.end as isize {
            continue;
        }
        let (x, y) = (row[v], consensus[w as usize]);
        if x.is_finite() && y.is_finite() {
            xs.push(x);
            ys.push(y);
        }
    }
    if xs.len() < min_verses {
        return None;
    }
    pearson(&xs, &ys)
}

/// Détecte, pour chaque traduction, les chapitres décalés d'un verset.
pub fn detect_shifts(corpus: &Corpus, config: &Ingest) -> Vec<Vec<ShiftedChapter>> {
    let x = log_lengths(corpus);
    let (_, consensus) = length_consensus(&x);
    let books = corpus.versification.books();
    x.par_iter()
        .map(|row| {
            let mut flagged = Vec::new();
            for (book_index, book) in books.iter().enumerate() {
                for (chapter_index, chapter) in book.chapters.iter().enumerate() {
                    let corr = |shift| {
                        shifted_correlation(
                            row,
                            &consensus,
                            chapter.range(),
                            shift,
                            config.shift_min_verses,
                        )
                    };
                    let Some(aligned) = corr(0) else { continue };
                    let best = [(1_i8, corr(1)), (-1_i8, corr(-1))]
                        .into_iter()
                        .filter_map(|(s, r)| r.map(|r| (s, r)))
                        .max_by(|a, b| a.1.total_cmp(&b.1));
                    if let Some((shift, shifted)) = best
                        && shifted - aligned > config.shift_margin
                    {
                        flagged.push(ShiftedChapter {
                            book: book_index,
                            chapter: chapter_index,
                            shift,
                            aligned,
                            shifted,
                        });
                    }
                }
            }
            flagged
        })
        .collect()
}

/// Cherche les traductions en double à l'intérieur de chaque langue.
pub fn find_duplicates(corpus: &Corpus, identical_share: f32) -> Vec<Duplicate> {
    let mut by_language: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, t) in corpus.translations.iter().enumerate() {
        by_language
            .entry(t.meta.language.as_str())
            .or_default()
            .push(i);
    }
    let pairs: Vec<(usize, usize)> = by_language
        .values()
        .flat_map(|members| {
            members
                .iter()
                .enumerate()
                .flat_map(move |(k, a)| members[k + 1..].iter().map(move |b| (*a, *b)))
        })
        .collect();
    pairs
        .par_iter()
        .filter_map(|(a, b)| {
            let (ta, tb) = (&corpus.translations[*a], &corpus.translations[*b]);
            let exact = ta.meta.fingerprint == tb.meta.fingerprint;
            let (mut common, mut identical) = (0_usize, 0_usize);
            for v in 0..corpus.n_verses() {
                if ta.status(v).has_own_text() && tb.status(v).has_own_text() {
                    common += 1;
                    if ta.text.text(v) == tb.text.text(v) {
                        identical += 1;
                    }
                }
            }
            let share = if common == 0 {
                0.0
            } else {
                identical as f32 / common as f32
            };
            (exact || (common > 0 && share >= identical_share)).then_some(Duplicate {
                a: *a,
                b: *b,
                identical_share: share,
                common,
                exact,
            })
        })
        .collect()
}

/// Évalue la qualité du corpus ingéré.
pub fn assess(corpus: &Corpus, rejected: &[Rejected], config: &Ingest) -> QualityReport {
    let coverage = corpus.coverage();
    let shifts = detect_shifts(corpus, config);
    let n_books = corpus.versification.books().len();
    let n_verses = corpus.n_verses().max(1);

    let translations = corpus
        .translations
        .iter()
        .zip(&coverage)
        .zip(shifts)
        .map(|((t, cov), shifted)| TranslationQuality {
            coverage: t.text.covered() as f32 / n_verses as f32,
            eligible_books: cov.iter().filter(|c| **c >= config.coverage_min).count(),
            shifted,
        })
        .collect();

    let books = (0..n_books)
        .map(|b| {
            let eligible = coverage
                .iter()
                .filter(|c| c[b] >= config.coverage_min)
                .count();
            let partial = coverage
                .iter()
                .filter(|c| c[b] > 0.0 && c[b] < config.coverage_min)
                .count();
            (eligible, partial)
        })
        .collect();

    let mut script_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for t in &corpus.translations {
        *script_counts.entry(t.meta.script.as_str()).or_default() += 1;
    }
    let mut scripts: Vec<ScriptCount> = script_counts
        .into_iter()
        .map(|(script, translations)| ScriptCount {
            script: script.to_string(),
            translations,
            has_font: SCRIPTS_WITH_FONT.contains(&script),
        })
        .collect();
    scripts.sort_by(|a, b| {
        b.translations
            .cmp(&a.translations)
            .then(a.script.cmp(&b.script))
    });

    QualityReport {
        rejected: rejected.to_vec(),
        translations,
        books,
        duplicates: find_duplicates(corpus, config.duplicate_identical_share),
        scripts,
    }
}

/// Nombre maximal de lignes dans les tableaux détaillés du rapport.
const REPORT_TABLE_ROWS: usize = 40;

/// Rapport qualité au format Markdown.
pub fn render_markdown(report: &QualityReport, corpus: &Corpus, config: &Ingest) -> String {
    let mut out = String::new();
    let v = &corpus.versification;
    let stem = |i: usize| corpus.translations[i].meta.stem.as_str();

    let _ = writeln!(out, "# Rapport qualité de l'ingestion\n");
    let _ = writeln!(out, "## Résumé\n");
    let _ = writeln!(
        out,
        "- Versets (lignes de vref.txt) : {}",
        corpus.n_verses()
    );
    let _ = writeln!(out, "- Livres : {}", v.books().len());
    let _ = writeln!(
        out,
        "- Traductions ingérées : {}",
        corpus.translations.len()
    );
    let _ = writeln!(out, "- Langues : {}", corpus.n_languages());
    let _ = writeln!(
        out,
        "- Couples (traduction, verset) avec texte : {}",
        corpus.n_texts()
    );
    let _ = writeln!(out, "- Fichiers écartés : {}", report.rejected.len());
    let _ = writeln!(
        out,
        "- Seuil d'éligibilité par livre : {:.0} % de couverture\n",
        config.coverage_min * 100.0
    );

    let _ = writeln!(out, "## Fichiers écartés\n");
    if report.rejected.is_empty() {
        let _ = writeln!(
            out,
            "Aucun : chaque traduction a exactement autant de lignes que vref.txt.\n"
        );
    } else {
        let _ = writeln!(out, "| Fichier | Raison |\n|---|---|");
        for r in &report.rejected {
            let _ = writeln!(out, "| {} | {} |", r.stem, r.reason);
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "## Couverture par livre\n");
    let _ = writeln!(
        out,
        "Traductions éligibles (couverture ≥ seuil) et partielles (couverture non nulle sous le seuil). \
         La matrice complète est dans `coverage.tsv`.\n"
    );
    let _ = writeln!(
        out,
        "| Livre | Versets | Éligibles | Partielles |\n|---|---:|---:|---:|"
    );
    for (book, (eligible, partial)) in v.books().iter().zip(&report.books) {
        let _ = writeln!(
            out,
            "| {} ({}) | {} | {} | {} |",
            book.name_fr, book.code, book.len, eligible, partial
        );
    }
    let none: Vec<&str> = report
        .translations
        .iter()
        .enumerate()
        .filter(|(_, q)| q.eligible_books == 0)
        .map(|(i, _)| stem(i))
        .collect();
    let _ = writeln!(
        out,
        "\nTraductions sans aucun livre éligible : {}{}\n",
        none.len(),
        if none.is_empty() {
            String::new()
        } else {
            format!(" ({})", none.join(", "))
        }
    );

    let _ = writeln!(out, "## Décalages de versification\n");
    let _ = writeln!(
        out,
        "Un chapitre est signalé quand le profil de longueurs de la traduction corrèle mieux avec le \
         consensus décalé d'un verset qu'avec le consensus aligné (écart > {}, au moins {} versets).\n",
        config.shift_margin, config.shift_min_verses
    );
    let mut shifted: Vec<(usize, &TranslationQuality)> = report
        .translations
        .iter()
        .enumerate()
        .filter(|(_, q)| !q.shifted.is_empty())
        .collect();
    shifted.sort_by(|a, b| {
        b.1.shifted
            .len()
            .cmp(&a.1.shifted.len())
            .then(a.0.cmp(&b.0))
    });
    let total_chapters: usize = shifted.iter().map(|(_, q)| q.shifted.len()).sum();
    let _ = writeln!(
        out,
        "{} chapitres signalés dans {} traductions.\n",
        total_chapters,
        shifted.len()
    );
    if !shifted.is_empty() {
        let _ = writeln!(
            out,
            "| Traduction | Chapitres signalés | Exemples |\n|---|---:|---|"
        );
        for (i, q) in shifted.iter().take(REPORT_TABLE_ROWS) {
            let examples: Vec<String> = q
                .shifted
                .iter()
                .take(4)
                .map(|s| {
                    let book = &v.books()[s.book];
                    format!(
                        "{} {} ({:+}, r {:.2} → {:.2})",
                        book.code, book.chapters[s.chapter].number, s.shift, s.aligned, s.shifted
                    )
                })
                .collect();
            let _ = writeln!(
                out,
                "| {} | {} | {} |",
                stem(*i),
                q.shifted.len(),
                examples.join(" ; ")
            );
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "## Traductions en double\n");
    if report.duplicates.is_empty() {
        let _ = writeln!(out, "Aucun doublon détecté.\n");
    } else {
        let _ = writeln!(
            out,
            "| A | B | Versets communs | Part identique | Fichiers identiques |\n|---|---|---:|---:|---|"
        );
        for d in &report.duplicates {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {:.1} % | {} |",
                stem(d.a),
                stem(d.b),
                d.common,
                d.identical_share * 100.0,
                if d.exact { "oui" } else { "non" }
            );
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "## Lignes non textuelles\n");
    let mut noisy: Vec<(usize, u32)> = corpus
        .translations
        .iter()
        .enumerate()
        .filter(|(_, t)| t.meta.non_text_lines > 0)
        .map(|(i, t)| (i, t.meta.non_text_lines))
        .collect();
    noisy.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    if noisy.is_empty() {
        let _ = writeln!(out, "Aucune.\n");
    } else {
        let _ = writeln!(
            out,
            "Lignes avec balisage, ou dont moins de {:.0} % des caractères sont des lettres : {} traductions concernées.\n",
            config.non_text_letter_share * 100.0,
            noisy.len()
        );
        let _ = writeln!(out, "| Traduction | Lignes |\n|---|---:|");
        for (i, n) in noisy.iter().take(REPORT_TABLE_ROWS) {
            let _ = writeln!(out, "| {} | {} |", stem(*i), n);
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "## Écritures et polices\n");
    let _ = writeln!(out, "| Écriture | Traductions | Police |\n|---|---:|---|");
    for s in &report.scripts {
        let _ = writeln!(
            out,
            "| {} | {} | {} |",
            s.script,
            s.translations,
            if s.has_font {
                "intégrée"
            } else {
                "**manquante**"
            }
        );
    }
    let missing: Vec<&str> = report
        .scripts
        .iter()
        .filter(|s| !s.has_font)
        .map(|s| s.script.as_str())
        .collect();
    let _ = writeln!(
        out,
        "\nÉcritures sans police : {}\n",
        if missing.is_empty() {
            "aucune".to_string()
        } else {
            missing.join(", ")
        }
    );

    let _ = writeln!(out, "## Base textuelle du NT (inférée)\n");
    let _ = writeln!(out, "| Base | Traductions |\n|---|---:|");
    for (base, count) in text_base_counts(corpus) {
        let _ = writeln!(out, "| {} | {} |", base.label_fr(), count);
    }
    out
}

/// Exclusions proposées, au format de `config/exclusions.toml`. À relire et
/// à recopier à la main : rien n'est appliqué automatiquement.
pub fn suggested_exclusions(report: &QualityReport, corpus: &Corpus, config: &Ingest) -> String {
    let mut out = String::from(
        "# Exclusions proposées par `babel ingest`. Recopier dans config/exclusions.toml\n\
         # celles qui sont retenues.\n",
    );
    let stem = |i: usize| corpus.translations[i].meta.stem.as_str();
    for d in &report.duplicates {
        let _ = write!(
            out,
            "\n[[exclude]]\ntranslation = \"{}\"\nreason = \"Doublon de {} ({:.1} % de versets identiques).\"\n",
            stem(d.b),
            stem(d.a),
            d.identical_share * 100.0
        );
    }
    for (i, q) in report.translations.iter().enumerate() {
        if q.shifted.len() >= config.shift_chapters_alert {
            let _ = write!(
                out,
                "\n[[exclude]]\ntranslation = \"{}\"\nreason = \"Versification décalée : {} chapitres signalés.\"\n",
                stem(i),
                q.shifted.len()
            );
        }
    }
    out
}

/// Matrice de couverture au format TSV : une ligne par traduction, une
/// colonne par livre.
pub fn coverage_tsv(corpus: &Corpus) -> String {
    let mut out = String::from("translation");
    for book in corpus.versification.books() {
        let _ = write!(out, "\t{}", book.code);
    }
    out.push('\n');
    for (t, cov) in corpus.translations.iter().zip(corpus.coverage()) {
        out.push_str(&t.meta.stem);
        for c in cov {
            let _ = write!(out, "\t{c:.3}");
        }
        out.push('\n');
    }
    out
}
