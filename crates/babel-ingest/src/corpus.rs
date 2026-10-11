//! Le corpus en mémoire : la versification et les traductions ingérées.

use crate::meta::{
    MetadataTables, TextBase, TranslationMeta, UNKNOWN, dominant_script, infer_text_base,
    split_stem,
};
use crate::parse::{ParseError, ParsedText, parse_translation};
use babel_core::config::Ingest;
use babel_core::versification::VersificationError;
use babel_core::{TranslationId, VerseStatus, Versification};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("lecture de {path} impossible : {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} : {source}")]
    Versification {
        path: PathBuf,
        source: VersificationError,
    },
    #[error("aucun fichier de traduction dans {0} (lancer `babel fetch` d'abord)")]
    NoTranslations(PathBuf),
    #[error("plus de {0} traductions : un TranslationId (u16) ne suffit plus")]
    TooManyTranslations(usize),
    #[error("Parquet {path} : {message}")]
    Store { path: PathBuf, message: String },
}

/// Une traduction ingérée : ses métadonnées et son texte verset par verset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    pub meta: TranslationMeta,
    pub text: ParsedText,
}

impl Translation {
    pub fn status(&self, verse: usize) -> VerseStatus {
        self.text
            .status
            .get(verse)
            .copied()
            .unwrap_or(VerseStatus::Absent)
    }
}

/// Fichier écarté à l'ingestion, avec la raison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    pub stem: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Corpus {
    pub versification: Versification,
    pub translations: Vec<Translation>,
}

impl Corpus {
    pub fn n_verses(&self) -> usize {
        self.versification.len()
    }

    /// Nombre de langues distinctes (codes ISO 639-3).
    pub fn n_languages(&self) -> usize {
        let mut languages: Vec<&str> = self
            .translations
            .iter()
            .map(|t| t.meta.language.as_str())
            .collect();
        languages.sort_unstable();
        languages.dedup();
        languages.len()
    }

    /// Nombre de couples (traduction, verset) qui portent un texte.
    pub fn n_texts(&self) -> usize {
        self.translations
            .iter()
            .map(|t| t.text.status.iter().filter(|s| s.has_own_text()).count())
            .sum()
    }

    /// Couverture de chaque livre par chaque traduction : part des versets
    /// présents ou fusionnés. Indexé `[traduction][livre]`.
    pub fn coverage(&self) -> Vec<Vec<f32>> {
        self.translations
            .par_iter()
            .map(|t| coverage_by_book(&t.text, &self.versification))
            .collect()
    }
}

/// Couverture par livre d'une traduction.
pub fn coverage_by_book(text: &ParsedText, versification: &Versification) -> Vec<f32> {
    versification
        .books()
        .iter()
        .map(|book| {
            let covered = text.status[book.range()]
                .iter()
                .filter(|s| s.is_covered())
                .count();
            covered as f32 / book.len.max(1) as f32
        })
        .collect()
}

/// Résultat brut de la lecture du dossier eBible.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub corpus: Corpus,
    pub rejected: Vec<Rejected>,
}

/// Lit `vref.txt` dans le dossier `metadata/` d'eBible.
pub fn read_versification(ebible: &Path) -> Result<Versification, IngestError> {
    let path = ebible.join("metadata").join("vref.txt");
    let text = std::fs::read_to_string(&path).map_err(|source| IngestError::Io {
        path: path.clone(),
        source,
    })?;
    Versification::parse(&text).map_err(|source| IngestError::Versification { path, source })
}

/// Liste triée des fichiers de traduction (`corpus/*.txt`).
pub fn list_translation_files(ebible: &Path) -> Result<Vec<PathBuf>, IngestError> {
    let dir = ebible.join("corpus");
    let entries = std::fs::read_dir(&dir).map_err(|source| IngestError::Io {
        path: dir.clone(),
        source,
    })?;
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(IngestError::NoTranslations(dir));
    }
    Ok(files)
}

/// Ingère un clone d'eBible : `metadata/vref.txt` et `corpus/*.txt`.
///
/// Les fichiers dont le nombre de lignes diffère de `vref.txt` sont écartés
/// et listés dans `rejected`. Le nombre de traductions, de langues et de
/// versets est recalculé à chaque appel.
pub fn load_ebible(ebible: &Path, config: &Ingest) -> Result<Loaded, IngestError> {
    let versification = read_versification(ebible)?;
    let files = list_translation_files(ebible)?;
    let tables = MetadataTables::load(&ebible.join("metadata"));
    let diagnostic = diagnostic_verses(&versification, &config.diagnostic_verses);

    let parsed: Vec<Result<(String, ParsedText, String), Rejected>> = files
        .par_iter()
        .map(|path| {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let reject = |reason: String| Rejected {
                stem: stem.clone(),
                reason,
            };
            let bytes =
                std::fs::read(path).map_err(|e| reject(format!("lecture impossible : {e}")))?;
            let fingerprint = blake3::hash(&bytes).to_hex().to_string();
            let content = String::from_utf8(bytes)
                .map_err(|_| reject("le fichier n'est pas en UTF-8".into()))?;
            let text =
                parse_translation(&content, versification.len(), config.non_text_letter_share)
                    .map_err(|e: ParseError| reject(e.to_string()))?;
            if text.covered() == 0 {
                return Err(reject("aucun verset".into()));
            }
            Ok((stem.clone(), text, fingerprint))
        })
        .collect();

    let mut translations = Vec::new();
    let mut rejected = Vec::new();
    for item in parsed {
        match item {
            Ok((stem, text, fingerprint)) => {
                let id = u16::try_from(translations.len())
                    .map_err(|_| IngestError::TooManyTranslations(usize::from(u16::MAX)))?;
                let meta = build_meta(
                    TranslationId(id),
                    &stem,
                    &text,
                    fingerprint,
                    &tables,
                    &versification,
                    &diagnostic,
                    config,
                );
                translations.push(Translation { meta, text });
            }
            Err(r) => rejected.push(r),
        }
    }
    if translations.is_empty() {
        return Err(IngestError::NoTranslations(ebible.join("corpus")));
    }
    Ok(Loaded {
        corpus: Corpus {
            versification,
            translations,
        },
        rejected,
    })
}

/// Index des versets diagnostiques (§ 4.4) présents dans la versification,
/// avec le livre de chacun.
pub fn diagnostic_verses(
    versification: &Versification,
    references: &[String],
) -> Vec<(usize, usize)> {
    references
        .iter()
        .filter_map(|reference| {
            let (code, rest) = reference.split_once(' ')?;
            let (chapter, verse) = rest.split_once(':')?;
            let id = versification.find_usfm(code, chapter.parse().ok()?, verse.parse().ok()?)?;
            Some((id.index(), versification.book_of(id.index())))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_meta(
    id: TranslationId,
    stem: &str,
    text: &ParsedText,
    fingerprint: String,
    tables: &MetadataTables,
    versification: &Versification,
    diagnostic: &[(usize, usize)],
    config: &Ingest,
) -> TranslationMeta {
    let (language, _) = split_stem(stem);
    let described = tables.describe(stem);
    let coverage = coverage_by_book(text, versification);

    // Seuls les versets diagnostiques des livres couverts renseignent sur
    // la base textuelle.
    let statuses: Vec<VerseStatus> = diagnostic
        .iter()
        .filter(|(_, book)| coverage[*book] >= config.coverage_min)
        .map(|(verse, _)| text.status[*verse])
        .collect();
    let text_base = infer_text_base(
        &statuses,
        diagnostic.len(),
        config.textbase_received_min,
        config.textbase_critical_max,
    );

    let script = dominant_script(text.full_text())
        .map_or(UNKNOWN.to_string(), |s| s.full_name().to_string());

    TranslationMeta {
        id,
        stem: stem.to_string(),
        language: language.to_string(),
        language_name: described
            .language_name
            .unwrap_or_else(|| language.to_string()),
        title: described.title.unwrap_or_else(|| stem.to_string()),
        script,
        family: described.family.unwrap_or_else(|| UNKNOWN.to_string()),
        license: described.license.unwrap_or_else(|| UNKNOWN.to_string()),
        redistributable: described.redistributable.unwrap_or(false),
        year: described.year,
        text_base,
        fingerprint,
        non_text_lines: text.non_text_lines,
    }
}

/// Construit un corpus en mémoire à partir de textes, pour les tests et les
/// jeux d'essai. `translations` associe un nom de fichier (`fra-a`) à son
/// contenu, une ligne par verset.
pub fn corpus_from_texts(
    vref: &str,
    translations: &[(&str, &str)],
    config: &Ingest,
) -> Result<Loaded, IngestError> {
    let path = PathBuf::from("vref.txt");
    let versification =
        Versification::parse(vref).map_err(|source| IngestError::Versification { path, source })?;
    let diagnostic = diagnostic_verses(&versification, &config.diagnostic_verses);
    let tables = MetadataTables::default();
    let mut out = Vec::new();
    let mut rejected = Vec::new();
    for (stem, content) in translations {
        match parse_translation(content, versification.len(), config.non_text_letter_share) {
            Ok(text) => {
                let id = TranslationId(out.len() as u16);
                let fingerprint = blake3::hash(content.as_bytes()).to_hex().to_string();
                let meta = build_meta(
                    id,
                    stem,
                    &text,
                    fingerprint,
                    &tables,
                    &versification,
                    &diagnostic,
                    config,
                );
                out.push(Translation { meta, text });
            }
            Err(e) => rejected.push(Rejected {
                stem: (*stem).to_string(),
                reason: e.to_string(),
            }),
        }
    }
    Ok(Loaded {
        corpus: Corpus {
            versification,
            translations: out,
        },
        rejected,
    })
}

/// Résumé des bases textuelles du corpus : (base, nombre de traductions).
pub fn text_base_counts(corpus: &Corpus) -> Vec<(TextBase, usize)> {
    TextBase::ALL
        .into_iter()
        .map(|base| {
            (
                base,
                corpus
                    .translations
                    .iter()
                    .filter(|t| t.meta.text_base == base)
                    .count(),
            )
        })
        .collect()
}
