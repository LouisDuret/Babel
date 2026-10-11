//! Étapes du pipeline : ingest → signals → score → atlas.
//!
//! Chaque étape est idempotente et mise en cache par une empreinte blake3
//! de ses entrées et de sa configuration, enregistrée dans
//! `data/cache/<étape>.stamp`.

use anyhow::{Context, Result, bail};
use babel_atlas::texts::TextWriter;
use babel_atlas::{AtlasInput, AtlasTranslation, SignalInput};
use babel_core::config::Exclusions;
use babel_core::{Config, SignalKind, TranslationId};
use babel_ingest::quality;
use babel_ingest::store::{
    CorpusPaths, read_corpus, read_translation, read_translations, write_corpus,
};
use babel_ingest::{RANGE_TOKEN, load_ebible};
use babel_score::health;
use babel_signals::store::{
    VerseSignals, read_signals, read_weights, write_per_translation, write_signals,
    write_verse_stats, write_weights,
};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct Pipeline {
    pub config: Config,
    pub exclusions: Exclusions,
    /// Texte de `config/exclusions.toml`, pour l'empreinte.
    pub exclusions_text: String,
    /// Recalcule même si le cache est à jour.
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Ingest,
    Signals,
    Score,
    Atlas,
}

impl Stage {
    /// Version du calcul de l'étape. À incrémenter quand le code change
    /// le résultat à entrées égales : le cache est alors invalidé.
    fn version(self) -> u32 {
        match self {
            Stage::Ingest => 1,
            Stage::Signals => 1,
            // 2 : rang percentile = part des valeurs strictement inférieures.
            Stage::Score => 2,
            Stage::Atlas => babel_atlas::FORMAT_VERSION,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Stage::Ingest => "ingest",
            Stage::Signals => "signals",
            Stage::Score => "score",
            Stage::Atlas => "atlas",
        }
    }
}

fn spinner(message: &str) -> ProgressBar {
    let bar = ProgressBar::new_spinner();
    if let Ok(style) = ProgressStyle::with_template("{spinner} {msg} [{elapsed}]") {
        bar.set_style(style);
    }
    bar.set_message(message.to_string());
    bar.enable_steady_tick(Duration::from_millis(120));
    bar
}

fn fingerprint(parts: &[&str]) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in parts {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

/// Empreinte du contenu des fichiers bruts : vref.txt, métadonnées et
/// traductions.
fn raw_fingerprint(ebible: &Path) -> Result<String> {
    let mut files = babel_ingest::corpus::list_translation_files(ebible)?;
    let metadata = ebible.join("metadata");
    for name in [
        "vref.txt",
        "translations.csv",
        "lang_details.tsv",
        "licences.tsv",
    ] {
        let path = metadata.join(name);
        if path.exists() {
            files.push(path);
        }
    }
    let mut hashes: Vec<(String, String)> = files
        .par_iter()
        .map(|path| {
            let bytes =
                std::fs::read(path).with_context(|| format!("lecture de {}", path.display()))?;
            Ok((
                path.to_string_lossy().into_owned(),
                blake3::hash(&bytes).to_hex().to_string(),
            ))
        })
        .collect::<Result<_>>()?;
    hashes.sort();
    let flat: Vec<&str> = hashes
        .iter()
        .flat_map(|(p, h)| [p.as_str(), h.as_str()])
        .collect();
    Ok(fingerprint(&flat))
}

impl Pipeline {
    fn stamp_path(&self, stage: Stage) -> PathBuf {
        self.config
            .paths
            .cache_dir()
            .join(format!("{}.stamp", stage.name()))
    }

    fn read_stamp(&self, stage: Stage) -> Option<String> {
        std::fs::read_to_string(self.stamp_path(stage))
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn write_stamp(&self, stage: Stage, value: &str) -> Result<()> {
        let path = self.stamp_path(stage);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, value).with_context(|| format!("écriture de {}", path.display()))
    }

    /// L'étape est à jour : même empreinte et sorties présentes.
    fn up_to_date(&self, stage: Stage, expected: &str, outputs: &[PathBuf]) -> bool {
        !self.force
            && self.read_stamp(stage).as_deref() == Some(expected)
            && outputs.iter().all(|p| p.exists())
    }

    /// Empreinte de l'étape précédente, ou une erreur qui dit quoi lancer.
    fn upstream(&self, stage: Stage) -> Result<String> {
        self.read_stamp(stage).with_context(|| {
            format!(
                "l'étape « {0} » n'a pas été lancée : `babel {0}`",
                stage.name()
            )
        })
    }

    fn corpus_paths(&self) -> CorpusPaths {
        CorpusPaths::in_dir(&self.config.paths.data)
    }

    /// eBible → corpus.parquet, translations.parquet, verses.parquet et
    /// rapport qualité.
    pub fn ingest(&self) -> Result<()> {
        let cfg = &self.config;
        let raw = cfg.paths.raw_ebible();
        if !raw.join("corpus").exists() {
            bail!(
                "{} est absent : lancer `babel fetch` d'abord",
                raw.join("corpus").display()
            );
        }
        let stamp = fingerprint(&[
            "ingest",
            &Stage::Ingest.version().to_string(),
            &format!("{:?}", cfg.ingest),
            &raw_fingerprint(&raw)?,
        ]);
        let paths = self.corpus_paths();
        let report_dir = cfg.paths.report_dir();
        let outputs = [
            paths.corpus.clone(),
            paths.translations.clone(),
            paths.verses.clone(),
            report_dir.join("quality.md"),
        ];
        if self.up_to_date(Stage::Ingest, &stamp, &outputs) {
            println!("ingest   : à jour (cache)");
            return Ok(());
        }

        let start = Instant::now();
        let bar = spinner("ingest   : lecture des traductions");
        let loaded = load_ebible(&raw, &cfg.ingest)?;
        bar.set_message("ingest   : contrôle qualité");
        let report = quality::assess(&loaded.corpus, &loaded.rejected, &cfg.ingest);
        bar.set_message("ingest   : écriture Parquet");
        write_corpus(&loaded.corpus, &paths)?;
        std::fs::create_dir_all(&report_dir)?;
        std::fs::write(
            report_dir.join("quality.md"),
            quality::render_markdown(&report, &loaded.corpus, &cfg.ingest),
        )?;
        std::fs::write(
            report_dir.join("coverage.tsv"),
            quality::coverage_tsv(&loaded.corpus),
        )?;
        std::fs::write(
            report_dir.join("exclusions.suggested.toml"),
            quality::suggested_exclusions(&report, &loaded.corpus, &cfg.ingest),
        )?;
        bar.finish_and_clear();

        let corpus = &loaded.corpus;
        let shifted: usize = report.translations.iter().map(|q| q.shifted.len()).sum();
        println!(
            "ingest   : {} traductions, {} langues, {} versets, {} textes ({:.1} s)",
            corpus.translations.len(),
            corpus.n_languages(),
            corpus.n_verses(),
            corpus.n_texts(),
            start.elapsed().as_secs_f32()
        );
        println!(
            "           {} fichiers écartés, {} doublons, {} chapitres décalés → {}",
            report.rejected.len(),
            report.duplicates.len(),
            shifted,
            report_dir.join("quality.md").display()
        );
        self.write_stamp(Stage::Ingest, &stamp)
    }

    /// corpus.parquet → signals.parquet, verse_stats.parquet,
    /// per_translation.parquet, weights.parquet.
    pub fn signals(&self) -> Result<()> {
        let cfg = &self.config;
        let stamp = fingerprint(&[
            "signals",
            &Stage::Signals.version().to_string(),
            &format!("{:?}", cfg.signals),
            &cfg.ingest.coverage_min.to_string(),
            &self.exclusions_text,
            &self.upstream(Stage::Ingest)?,
        ]);
        let outputs = [
            cfg.paths.signals(),
            cfg.paths.verse_stats(),
            cfg.paths.per_translation(),
            cfg.paths.weights(),
        ];
        if self.up_to_date(Stage::Signals, &stamp, &outputs) {
            println!("signals  : à jour (cache)");
            return Ok(());
        }

        let start = Instant::now();
        let bar = spinner("signals  : lecture du corpus");
        let corpus = read_corpus(&self.corpus_paths())?;
        let excluded: Vec<Option<String>> = corpus
            .translations
            .iter()
            .map(|t| {
                self.exclusions
                    .reason_for(&t.meta.stem, &t.meta.language)
                    .map(str::to_string)
            })
            .collect();
        let n_excluded = excluded.iter().filter(|e| e.is_some()).count();
        bar.set_message("signals  : S_omit, S_len, S_intra");
        let output =
            babel_signals::compute(&corpus, excluded, cfg.ingest.coverage_min, &cfg.signals);
        bar.set_message("signals  : écriture Parquet");
        let verse_signals = VerseSignals::from_output(&output);
        write_signals(&verse_signals, &cfg.paths.signals())?;
        write_verse_stats(&verse_signals, &cfg.paths.verse_stats())?;
        write_per_translation(&output, &cfg.paths.per_translation())?;
        write_weights(&corpus, &output, &cfg.paths.weights())?;
        bar.finish_and_clear();

        let defined = |values: &[f32]| values.iter().filter(|v| v.is_finite()).count();
        let groups: std::collections::BTreeSet<u32> = output
            .group
            .iter()
            .zip(&output.excluded)
            .filter(|(_, e)| e.is_none())
            .map(|(g, _)| *g)
            .collect();
        println!(
            "signals  : {} traductions exclues, {} groupes indépendants ({:.1} s)",
            n_excluded,
            groups.len(),
            start.elapsed().as_secs_f32()
        );
        println!(
            "           versets avec signal : omit {}, len {}, intra {}",
            defined(&output.omit),
            defined(&output.len),
            defined(&output.intra)
        );
        self.write_stamp(Stage::Signals, &stamp)
    }

    /// signals.parquet → scores.parquet et tests de santé.
    pub fn score(&self) -> Result<()> {
        let cfg = &self.config;
        let stamp = fingerprint(&[
            "score",
            &Stage::Score.version().to_string(),
            &format!("{:?}", cfg.score),
            &format!("{:?}", cfg.health),
            &self.upstream(Stage::Signals)?,
        ]);
        let health_path = cfg.paths.report_dir().join("health.md");
        let outputs = [cfg.paths.scores(), health_path.clone()];
        if self.up_to_date(Stage::Score, &stamp, &outputs) {
            println!("score    : à jour (cache)");
            return Ok(());
        }

        let start = Instant::now();
        let versification = babel_ingest::store::read_versification(&self.corpus_paths().verses)?;
        let signals = read_signals(
            &cfg.paths.signals(),
            &cfg.paths.verse_stats(),
            versification.len(),
        )?;
        let scores = babel_score::score(
            &[&signals.omit, &signals.len, &signals.intra],
            &cfg.score.weights.as_array(),
        );
        babel_score::store::write_scores(&versification, &signals, &scores, &cfg.paths.scores())?;

        let checks = health::run(&versification, &signals.omit, &signals.len, &cfg.health);
        std::fs::create_dir_all(cfg.paths.report_dir())?;
        std::fs::write(&health_path, health::render_markdown(&checks, &cfg.health))?;

        let scored = scores.composite.iter().filter(|u| u.is_finite()).count();
        println!(
            "score    : composite U(v) pour {} versets sur {} ({:.1} s)",
            scored,
            versification.len(),
            start.elapsed().as_secs_f32()
        );
        for check in &checks {
            let verdict = match &check.outcome {
                health::Outcome::Passed => "réussi".to_string(),
                health::Outcome::Failed => "ÉCHEC".to_string(),
                health::Outcome::NotEvaluated(why) => format!("non évalué ({why})"),
            };
            let rank = check.rank.map_or(String::new(), |r| {
                format!(" rang {r}/{} dans le NT,", check.population)
            });
            println!(
                "           santé S_{} {} :{} {}",
                check.signal.code(),
                check.reference,
                rank,
                verdict
            );
        }
        self.write_stamp(Stage::Score, &stamp)
    }

    /// Tout → atlas.bin et texts.zst pour le viewer.
    pub fn atlas(&self) -> Result<()> {
        let cfg = &self.config;
        let stamp = fingerprint(&[
            "atlas",
            &Stage::Atlas.version().to_string(),
            &format!("{:?}", cfg.atlas),
            &self.upstream(Stage::Score)?,
        ]);
        let outputs = [cfg.paths.atlas(), cfg.paths.texts()];
        if self.up_to_date(Stage::Atlas, &stamp, &outputs) {
            println!("atlas    : à jour (cache)");
            return Ok(());
        }

        let start = Instant::now();
        let paths = self.corpus_paths();
        let versification = babel_ingest::store::read_versification(&paths.verses)?;
        let n = versification.len();
        let metas = read_translations(&paths.translations)?;
        let weights = read_weights(&cfg.paths.weights())?;
        if weights.len() != metas.len() {
            bail!("weights.parquet ne correspond pas au corpus : relancer `babel signals`");
        }
        let signals = read_signals(&cfg.paths.signals(), &cfg.paths.verse_stats(), n)?;
        let scores = babel_score::store::read_scores(&cfg.paths.scores(), n)?;

        // Textes des traductions de référence, un bloc par livre.
        let mut writer = TextWriter::create(&cfg.paths.texts(), cfg.atlas.text_zstd_level)?;
        let mut references = Vec::new();
        for stem in &cfg.atlas.references {
            let Some(index) = metas.iter().position(|m| &m.stem == stem) else {
                println!("atlas    : traduction de référence « {stem} » absente du corpus");
                continue;
            };
            let text = read_translation(&paths.corpus, n, TranslationId(index as u16))?;
            for (book_index, book) in versification.books().iter().enumerate() {
                let verses: Vec<&str> = book
                    .range()
                    .map(|v| {
                        if text.status[v] == babel_core::VerseStatus::RangeContinuation {
                            RANGE_TOKEN
                        } else {
                            text.text(v)
                        }
                    })
                    .collect();
                if verses.iter().any(|t| !t.is_empty()) {
                    writer.push(index as u16, book_index as u16, &verses)?;
                }
            }
            references.push(index as u16);
        }
        let text_blocks = writer.finish()?;

        let weight_array = cfg.score.weights.as_array();
        let signal_inputs = SignalKind::PHASE1
            .iter()
            .zip(&scores.ranks)
            .zip(weight_array)
            .filter_map(|((kind, rank), weight)| {
                Some(SignalInput {
                    code: kind.code().to_string(),
                    label: kind.label_fr().to_string(),
                    raw: signals.signal(*kind)?.to_vec(),
                    rank: rank.clone(),
                    weight,
                })
            })
            .collect();
        let translations = metas
            .iter()
            .zip(&weights)
            .map(|(m, w)| AtlasTranslation {
                stem: m.stem.clone(),
                language: m.language.clone(),
                language_name: m.language_name.clone(),
                title: m.title.clone(),
                script: m.script.clone(),
                family: m.family.clone(),
                license: m.license.clone(),
                redistributable: m.redistributable,
                year: m.year.unwrap_or(0),
                text_base: m.text_base.code().to_string(),
                excluded: w.excluded,
                eligible_books: w.eligible_books,
                weight: w.weight,
            })
            .collect();

        let atlas = babel_atlas::build(
            AtlasInput {
                versification: &versification,
                fingerprint: stamp.clone(),
                signals: signal_inputs,
                n_eligible: signals.n_eligible.clone(),
                translations,
                references,
                text_blocks,
            },
            &cfg.atlas,
        )?;
        babel_atlas::write(&atlas, &cfg.paths.atlas())?;

        let size =
            |p: &Path| std::fs::metadata(p).map_or(0, |m| m.len()) as f64 / (1024.0 * 1024.0);
        println!(
            "atlas    : {} ({:.1} Mo) et {} ({:.1} Mo), mur de {} × {} cellules ({:.1} s)",
            cfg.paths.atlas().display(),
            size(&cfg.paths.atlas()),
            cfg.paths.texts().display(),
            size(&cfg.paths.texts()),
            atlas.wall_size[0],
            atlas.wall_size[1],
            start.elapsed().as_secs_f32()
        );
        self.write_stamp(Stage::Atlas, &stamp)
    }

    /// Toutes les étapes, avec cache.
    pub fn run(&self) -> Result<()> {
        self.ingest()?;
        self.signals()?;
        self.score()?;
        self.atlas()
    }
}
