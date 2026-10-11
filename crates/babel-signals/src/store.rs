//! Écriture et relecture Parquet des signaux.

use crate::SignalOutput;
use arrow::array::{ArrayRef, BooleanArray, Float32Array, StringArray, UInt16Array, UInt32Array};
use arrow::datatypes::{DataType, Schema, SchemaRef};
use babel_core::SignalKind;
use babel_ingest::Corpus;
use babel_ingest::store::{StoreError, TableWriter, col_f32, col_str, col_u16, field, read_table};
use std::path::Path;
use std::sync::Arc;

/// Signaux par verset, tels que l'étape de score les relit.
#[derive(Debug, Clone, PartialEq)]
pub struct VerseSignals {
    pub omit: Vec<f32>,
    pub len: Vec<f32>,
    pub intra: Vec<f32>,
    pub n_eligible: Vec<u16>,
    pub intra_languages: Vec<u16>,
}

impl VerseSignals {
    pub fn from_output(output: &SignalOutput) -> Self {
        Self {
            omit: output.omit.clone(),
            len: output.len.clone(),
            intra: output.intra.clone(),
            n_eligible: output.n_eligible.clone(),
            intra_languages: output.intra_languages.clone(),
        }
    }

    /// Valeurs d'un signal de la phase 1.
    pub fn signal(&self, kind: SignalKind) -> Option<&[f32]> {
        match kind {
            SignalKind::Omit => Some(&self.omit),
            SignalKind::Len => Some(&self.len),
            SignalKind::Intra => Some(&self.intra),
            SignalKind::Align | SignalKind::Sem => None,
        }
    }
}

fn nullable_f32(values: impl Iterator<Item = f32>) -> ArrayRef {
    Arc::new(Float32Array::from_iter(
        values.map(|v| v.is_finite().then_some(v)),
    ))
}

fn signals_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        field("verse_id", DataType::UInt16, false),
        field("signal", DataType::Utf8, false),
        field("value", DataType::Float32, true),
        field("ci_low", DataType::Float32, true),
        field("ci_high", DataType::Float32, true),
    ]))
}

/// Écrit `signals.parquet` au format long : verse_id, signal, valeur,
/// IC bas, IC haut. Les intervalles (bootstrap, phase 2) sont nuls.
pub fn write_signals(signals: &VerseSignals, path: &Path) -> Result<(), StoreError> {
    let schema = signals_schema();
    let mut writer = TableWriter::create(path, Arc::clone(&schema))?;
    for kind in SignalKind::PHASE1 {
        let Some(values) = signals.signal(kind) else {
            continue;
        };
        let n = values.len();
        let no_interval = || nullable_f32(std::iter::repeat_n(f32::NAN, n));
        writer.write(
            &schema,
            vec![
                Arc::new(UInt16Array::from_iter_values((0..n).map(|v| v as u16))),
                Arc::new(StringArray::from_iter_values(std::iter::repeat_n(
                    kind.code(),
                    n,
                ))),
                nullable_f32(values.iter().copied()),
                no_interval(),
                no_interval(),
            ],
        )?;
    }
    writer.finish()
}

fn verse_stats_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        field("verse_id", DataType::UInt16, false),
        field("n_eligible", DataType::UInt16, false),
        field("intra_languages", DataType::UInt16, false),
    ]))
}

/// Écrit `verse_stats.parquet` : nombre de traductions éligibles et de
/// langues contributrices par verset.
pub fn write_verse_stats(signals: &VerseSignals, path: &Path) -> Result<(), StoreError> {
    let schema = verse_stats_schema();
    let mut writer = TableWriter::create(path, Arc::clone(&schema))?;
    let n = signals.n_eligible.len();
    writer.write(
        &schema,
        vec![
            Arc::new(UInt16Array::from_iter_values((0..n).map(|v| v as u16))),
            Arc::new(UInt16Array::from_iter_values(
                signals.n_eligible.iter().copied(),
            )),
            Arc::new(UInt16Array::from_iter_values(
                signals.intra_languages.iter().copied(),
            )),
        ],
    )?;
    writer.finish()
}

/// Relit les signaux écrits par [`write_signals`] et [`write_verse_stats`].
pub fn read_signals(
    signals_path: &Path,
    stats_path: &Path,
    n_verses: usize,
) -> Result<VerseSignals, StoreError> {
    let mut out = VerseSignals {
        omit: vec![f32::NAN; n_verses],
        len: vec![f32::NAN; n_verses],
        intra: vec![f32::NAN; n_verses],
        n_eligible: vec![0; n_verses],
        intra_languages: vec![0; n_verses],
    };
    let out_of_range = |path: &Path, v: usize| {
        StoreError::new(path, format!("verset {v} hors de la versification"))
    };

    for batch in read_table(signals_path)? {
        let verse = col_u16(&batch, "verse_id", signals_path)?;
        let signal = col_str(&batch, "signal", signals_path)?;
        let value = col_f32(&batch, "value", signals_path)?;
        for i in 0..batch.num_rows() {
            let v = usize::from(verse.value(i));
            let target = match SignalKind::from_code(signal.value(i)) {
                Some(SignalKind::Omit) => &mut out.omit,
                Some(SignalKind::Len) => &mut out.len,
                Some(SignalKind::Intra) => &mut out.intra,
                _ => continue,
            };
            let slot = target
                .get_mut(v)
                .ok_or_else(|| out_of_range(signals_path, v))?;
            if !arrow::array::Array::is_null(value, i) {
                *slot = value.value(i);
            }
        }
    }
    for batch in read_table(stats_path)? {
        let verse = col_u16(&batch, "verse_id", stats_path)?;
        let n_eligible = col_u16(&batch, "n_eligible", stats_path)?;
        let languages = col_u16(&batch, "intra_languages", stats_path)?;
        for i in 0..batch.num_rows() {
            let v = usize::from(verse.value(i));
            if v >= n_verses {
                return Err(out_of_range(stats_path, v));
            }
            out.n_eligible[v] = n_eligible.value(i);
            out.intra_languages[v] = languages.value(i);
        }
    }
    Ok(out)
}

fn per_translation_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        field("translation_id", DataType::UInt16, false),
        field("verse_id", DataType::UInt16, false),
        field("signal", DataType::Utf8, false),
        field("z", DataType::Float32, false),
    ]))
}

/// Écrit `per_translation.parquet` : z par signal, traduction et verset.
/// En phase 1, seul le résidu de longueur a une valeur par traduction.
pub fn write_per_translation(output: &SignalOutput, path: &Path) -> Result<(), StoreError> {
    let schema = per_translation_schema();
    let mut writer = TableWriter::create(path, Arc::clone(&schema))?;
    for (t, z) in output.len_z.iter().enumerate() {
        let finite: Vec<usize> = (0..z.len()).filter(|v| z[*v].is_finite()).collect();
        if finite.is_empty() {
            continue;
        }
        let code = SignalKind::Len.code();
        writer.write(
            &schema,
            vec![
                Arc::new(UInt16Array::from_iter_values(
                    finite.iter().map(|_| t as u16),
                )),
                Arc::new(UInt16Array::from_iter_values(
                    finite.iter().map(|v| *v as u16),
                )),
                Arc::new(StringArray::from_iter_values(finite.iter().map(|_| code))),
                Arc::new(Float32Array::from_iter_values(finite.iter().map(|v| z[*v]))),
            ],
        )?;
    }
    writer.finish()
}

fn weights_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        field("translation_id", DataType::UInt16, false),
        field("stem", DataType::Utf8, false),
        field("excluded", DataType::Boolean, false),
        field("reason", DataType::Utf8, true),
        field("eligible_books", DataType::UInt16, false),
        field("group", DataType::UInt32, false),
        field("weight", DataType::Float32, false),
        field("verbosity", DataType::Float32, true),
    ]))
}

/// Écrit `weights.parquet` : exclusion, groupe de redondance, poids et
/// verbosité de chaque traduction.
pub fn write_weights(
    corpus: &Corpus,
    output: &SignalOutput,
    path: &Path,
) -> Result<(), StoreError> {
    let schema = weights_schema();
    let mut writer = TableWriter::create(path, Arc::clone(&schema))?;
    let n = corpus.translations.len();
    writer.write(
        &schema,
        vec![
            Arc::new(UInt16Array::from_iter_values((0..n).map(|t| t as u16))),
            Arc::new(StringArray::from_iter_values(
                corpus.translations.iter().map(|t| t.meta.stem.as_str()),
            )),
            Arc::new(BooleanArray::from_iter(
                output.excluded.iter().map(|r| Some(r.is_some())),
            )),
            Arc::new(StringArray::from_iter(
                output.excluded.iter().map(|r| r.as_deref()),
            )),
            Arc::new(UInt16Array::from_iter_values(
                output
                    .eligible
                    .iter()
                    .map(|e| e.iter().filter(|b| **b).count() as u16),
            )),
            Arc::new(UInt32Array::from_iter_values(output.group.iter().copied())),
            Arc::new(Float32Array::from_iter_values(
                output.weight.iter().copied(),
            )),
            nullable_f32(output.verbosity.iter().copied()),
        ],
    )?;
    writer.finish()
}

/// Poids et exclusions relus de `weights.parquet`, pour l'atlas.
#[derive(Debug, Clone, PartialEq)]
pub struct TranslationWeight {
    pub excluded: bool,
    pub eligible_books: u16,
    pub group: u32,
    pub weight: f32,
}

pub fn read_weights(path: &Path) -> Result<Vec<TranslationWeight>, StoreError> {
    use babel_ingest::store::{col_bool, col_u32};
    let mut out = Vec::new();
    for batch in read_table(path)? {
        let excluded = col_bool(&batch, "excluded", path)?;
        let eligible = col_u16(&batch, "eligible_books", path)?;
        let group = col_u32(&batch, "group", path)?;
        let weight = col_f32(&batch, "weight", path)?;
        for i in 0..batch.num_rows() {
            out.push(TranslationWeight {
                excluded: excluded.value(i),
                eligible_books: eligible.value(i),
                group: group.value(i),
                weight: weight.value(i),
            });
        }
    }
    Ok(out)
}
