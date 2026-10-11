//! Configuration : `config/babel.toml` et `config/exclusions.toml`.
//!
//! Aucun paramètre n'a de valeur par défaut dans le code. Le fichier
//! `config/babel.toml` du dépôt est embarqué à la compilation et sert quand
//! aucun fichier n'est trouvé sur disque.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Contenu de `config/babel.toml` au moment de la compilation.
pub const EMBEDDED_CONFIG: &str = include_str!("../../../config/babel.toml");
/// Contenu de `config/exclusions.toml` au moment de la compilation.
pub const EMBEDDED_EXCLUSIONS: &str = include_str!("../../../config/exclusions.toml");

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("lecture de {path} impossible : {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("configuration invalide dans {path} : {source}")]
    Parse {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    #[error("configuration invalide : {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub paths: Paths,
    pub fetch: Fetch,
    pub ingest: Ingest,
    pub signals: Signals,
    pub score: Score,
    pub health: Health,
    pub atlas: Atlas,
    pub view: View,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Paths {
    pub data: PathBuf,
    pub exclusions: PathBuf,
}

impl Paths {
    pub fn raw_ebible(&self) -> PathBuf {
        self.data.join("raw").join("ebible")
    }
    pub fn raw_stepbible(&self) -> PathBuf {
        self.data.join("raw").join("stepbible")
    }
    pub fn corpus(&self) -> PathBuf {
        self.data.join("corpus.parquet")
    }
    pub fn translations(&self) -> PathBuf {
        self.data.join("translations.parquet")
    }
    pub fn signals(&self) -> PathBuf {
        self.data.join("signals.parquet")
    }
    pub fn verse_stats(&self) -> PathBuf {
        self.data.join("verse_stats.parquet")
    }
    pub fn per_translation(&self) -> PathBuf {
        self.data.join("per_translation.parquet")
    }
    pub fn weights(&self) -> PathBuf {
        self.data.join("weights.parquet")
    }
    pub fn scores(&self) -> PathBuf {
        self.data.join("scores.parquet")
    }
    pub fn atlas(&self) -> PathBuf {
        self.data.join("atlas.bin")
    }
    pub fn texts(&self) -> PathBuf {
        self.data.join("texts.zst")
    }
    pub fn report_dir(&self) -> PathBuf {
        self.data.join("report")
    }
    pub fn cache_dir(&self) -> PathBuf {
        self.data.join("cache")
    }
    pub fn view_state(&self) -> PathBuf {
        self.data.join("view_state.toml")
    }
}

/// Mode de sélection des traductions téléchargées.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Selection {
    /// Tout le corpus.
    All,
    /// Les langues qui ont au moins deux traductions.
    Multi,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fetch {
    pub ebible_url: String,
    pub stepbible_url: String,
    pub selection: Selection,
    pub always_include: Vec<String>,
    pub stepbible: bool,
    pub confirm_above_files: usize,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Ingest {
    pub coverage_min: f32,
    pub shift_min_verses: usize,
    pub shift_margin: f32,
    pub shift_chapters_alert: usize,
    pub duplicate_identical_share: f32,
    pub non_text_letter_share: f32,
    pub textbase_received_min: f32,
    pub textbase_critical_max: f32,
    pub diagnostic_verses: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Signals {
    pub min_translations: usize,
    pub len_polish_iterations: usize,
    pub len_polish_tolerance: f32,
    pub chrf_max_order: usize,
    pub chrf_beta: f32,
    pub redundancy_chrf: f32,
    pub redundancy_stride: usize,
    pub redundancy_len_correlation: f32,
    pub intra_language_weight_cap: f32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Score {
    pub weights: Weights,
}

/// Poids ω des signaux dans le composite.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Weights {
    pub omit: f32,
    pub len: f32,
    pub intra: f32,
}

impl Weights {
    /// Poids dans l'ordre de `SignalKind::PHASE1`.
    pub fn as_array(&self) -> [f32; 3] {
        [self.omit, self.len, self.intra]
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Health {
    pub top_share: f32,
    pub omit_verses: Vec<String>,
    pub len_verses: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Atlas {
    pub references: Vec<String>,
    pub wall_wrap: usize,
    pub wall_header_rows: usize,
    pub wall_column_gap: usize,
    pub wall_section_gap: usize,
    pub wall_shelf_gap: usize,
    pub wall_aspect: f32,
    pub wall_shelf_rows_min: usize,
    pub wall_shelf_rows_max: usize,
    pub hilbert_order: u32,
    pub text_zstd_level: i32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub cell_gap: f32,
    pub z1_px: f32,
    pub z2_px: f32,
    pub z3_px: f32,
    pub z4_px: f32,
    pub corner_radius: f32,
    pub view_transition_ms: u64,
    pub flight_min_ms: u64,
    pub flight_max_ms: u64,
    pub tooltip_delay_ms: u64,
    pub wheel_zoom: f32,
    pub zoom_inertia: f32,
    pub max_cell_px: f32,
    pub fit_margin: f32,
}

impl Config {
    /// Lit un fichier de configuration.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&text, path)
    }

    /// Lit `path` s'il existe, sinon la configuration embarquée.
    pub fn load_or_embedded(path: &Path) -> Result<Self, ConfigError> {
        if path.exists() {
            Self::load(path)
        } else {
            Self::embedded()
        }
    }

    /// Configuration du dépôt, embarquée à la compilation.
    pub fn embedded() -> Result<Self, ConfigError> {
        Self::parse(EMBEDDED_CONFIG, Path::new("config/babel.toml (embarqué)"))
    }

    fn parse(text: &str, path: &Path) -> Result<Self, ConfigError> {
        let config: Config = toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let unit = |name: &str, value: f32| {
            if (0.0..=1.0).contains(&value) {
                Ok(())
            } else {
                Err(ConfigError::Invalid(format!(
                    "{name} = {value} doit être entre 0 et 1"
                )))
            }
        };
        unit("ingest.coverage_min", self.ingest.coverage_min)?;
        unit(
            "ingest.duplicate_identical_share",
            self.ingest.duplicate_identical_share,
        )?;
        unit("signals.redundancy_chrf", self.signals.redundancy_chrf)?;
        unit("health.top_share", self.health.top_share)?;
        unit("view.cell_gap", self.view.cell_gap)?;
        if self.signals.chrf_max_order == 0 {
            return Err(ConfigError::Invalid(
                "signals.chrf_max_order doit être ≥ 1".into(),
            ));
        }
        if self.signals.redundancy_stride == 0 {
            return Err(ConfigError::Invalid(
                "signals.redundancy_stride doit être ≥ 1".into(),
            ));
        }
        if self.atlas.wall_wrap == 0 {
            return Err(ConfigError::Invalid("atlas.wall_wrap doit être ≥ 1".into()));
        }
        if self.atlas.wall_shelf_rows_min == 0
            || self.atlas.wall_shelf_rows_min > self.atlas.wall_shelf_rows_max
        {
            return Err(ConfigError::Invalid(
                "atlas.wall_shelf_rows_min doit être ≥ 1 et ≤ wall_shelf_rows_max".into(),
            ));
        }
        if !(1..=8).contains(&self.atlas.hilbert_order) {
            return Err(ConfigError::Invalid(
                "atlas.hilbert_order doit être entre 1 et 8".into(),
            ));
        }
        let w = self.score.weights.as_array();
        if w.iter().any(|x| *x < 0.0) || w.iter().sum::<f32>() <= 0.0 {
            return Err(ConfigError::Invalid(
                "score.weights : poids positifs, au moins un non nul".into(),
            ));
        }
        Ok(())
    }
}

/// Une entrée de `config/exclusions.toml`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Exclusion {
    /// Code ISO 639-3 : exclut toutes les traductions de la langue.
    pub language: Option<String>,
    /// Nom du fichier eBible sans extension : exclut une traduction.
    pub translation: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Exclusions {
    #[serde(default)]
    pub exclude: Vec<Exclusion>,
}

impl Exclusions {
    pub fn load_or_embedded(path: &Path) -> Result<Self, ConfigError> {
        let (text, origin) = if path.exists() {
            let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
                path: path.to_path_buf(),
                source,
            })?;
            (text, path.to_path_buf())
        } else {
            (
                EMBEDDED_EXCLUSIONS.to_string(),
                PathBuf::from("config/exclusions.toml (embarqué)"),
            )
        };
        Self::parse(&text, &origin)
    }

    pub fn parse(text: &str, path: &Path) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source: Box::new(source),
        })
    }

    /// Raison de l'exclusion d'une traduction, si elle est exclue.
    /// `stem` est le nom du fichier sans extension (`fra-fraLSG`).
    pub fn reason_for(&self, stem: &str, language: &str) -> Option<&str> {
        self.exclude
            .iter()
            .find(|e| {
                e.translation.as_deref() == Some(stem) || e.language.as_deref() == Some(language)
            })
            .map(|e| e.reason.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_configuration_du_depot_est_valide() {
        let config = Config::embedded().expect("config/babel.toml valide");
        assert!(config.ingest.coverage_min > 0.0);
        assert_eq!(config.ingest.diagnostic_verses.len(), 16);
    }

    #[test]
    fn les_exclusions_du_depot_sont_valides() {
        let ex = Exclusions::parse(EMBEDDED_EXCLUSIONS, Path::new("exclusions.toml"))
            .expect("config/exclusions.toml valide");
        assert!(ex.reason_for("grc-grctr", "grc").is_some());
        assert!(ex.reason_for("fra-fraLSG", "fra").is_none());
    }

    #[test]
    fn exclusion_par_traduction() {
        let ex = Exclusions::parse(
            "[[exclude]]\ntranslation = \"eng-engxyz\"\nreason = \"doublon\"\n",
            Path::new("test"),
        )
        .expect("toml valide");
        assert_eq!(ex.reason_for("eng-engxyz", "eng"), Some("doublon"));
        assert_eq!(ex.reason_for("eng-engabc", "eng"), None);
    }

    #[test]
    fn refuse_un_champ_inconnu() {
        let text = EMBEDDED_CONFIG.replace("[signals]", "[signals]\nmystere = 1");
        assert!(Config::parse(&text, Path::new("test")).is_err());
    }
}
