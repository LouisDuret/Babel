//! Écriture et relecture de `scores.parquet` : une ligne par verset, avec
//! les signaux bruts, leurs rangs et le composite.

use crate::Scores;
use arrow::array::{Array, ArrayRef, Float32Array, StringArray, UInt16Array};
use arrow::datatypes::{DataType, Schema, SchemaRef};
use babel_core::{SignalKind, VerseId, Versification};
use babel_ingest::store::{StoreError, TableWriter, col_f32, col_u16, field, read_table};
use babel_signals::store::VerseSignals;
use std::path::Path;
use std::sync::Arc;

fn nullable_f32(values: &[f32]) -> ArrayRef {
    Arc::new(Float32Array::from_iter(
        values.iter().map(|v| v.is_finite().then_some(*v)),
    ))
}

fn schema() -> SchemaRef {
    let mut fields = vec![
        field("verse_id", DataType::UInt16, false),
        field("ref", DataType::Utf8, false),
        field("testament", DataType::Utf8, false),
        field("n_eligible", DataType::UInt16, false),
    ];
    for kind in SignalKind::PHASE1 {
        fields.push(field(
            &format!("s_{}", kind.code()),
            DataType::Float32,
            true,
        ));
    }
    for kind in SignalKind::PHASE1 {
        fields.push(field(
            &format!("p_{}", kind.code()),
            DataType::Float32,
            true,
        ));
    }
    fields.push(field("u", DataType::Float32, true));
    fields.push(field("u_pct", DataType::Float32, true));
    Arc::new(Schema::new(fields))
}

/// Écrit `scores.parquet`. `scores.ranks` suit l'ordre de
/// `SignalKind::PHASE1`.
pub fn write_scores(
    versification: &Versification,
    signals: &VerseSignals,
    scores: &Scores,
    path: &Path,
) -> Result<(), StoreError> {
    let schema = schema();
    let n = versification.len();
    let mut columns: Vec<ArrayRef> = vec![
        Arc::new(UInt16Array::from_iter_values((0..n).map(|v| v as u16))),
        Arc::new(StringArray::from_iter_values(
            (0..n).map(|v| versification.label_usfm(VerseId(v as u16))),
        )),
        Arc::new(StringArray::from_iter_values(
            (0..n).map(|v| versification.testament_of(v).label_fr()),
        )),
        Arc::new(UInt16Array::from_iter_values(
            signals.n_eligible.iter().copied(),
        )),
    ];
    for kind in SignalKind::PHASE1 {
        let values = signals
            .signal(kind)
            .ok_or_else(|| StoreError::new(path, format!("signal {} absent", kind.code())))?;
        columns.push(nullable_f32(values));
    }
    for rank in &scores.ranks {
        columns.push(nullable_f32(rank));
    }
    columns.push(nullable_f32(&scores.composite));
    columns.push(nullable_f32(&scores.composite_rank));

    let mut writer = TableWriter::create(path, Arc::clone(&schema))?;
    writer.write(&schema, columns)?;
    writer.finish()
}

/// Relit les rangs et le composite de `scores.parquet`.
pub fn read_scores(path: &Path, n_verses: usize) -> Result<Scores, StoreError> {
    let mut ranks = vec![vec![f32::NAN; n_verses]; SignalKind::PHASE1.len()];
    let mut composite = vec![f32::NAN; n_verses];
    let mut composite_rank = vec![f32::NAN; n_verses];
    for batch in read_table(path)? {
        let verse = col_u16(&batch, "verse_id", path)?;
        let read = |name: &str, target: &mut [f32]| -> Result<(), StoreError> {
            let column = col_f32(&batch, name, path)?;
            for i in 0..batch.num_rows() {
                let v = usize::from(verse.value(i));
                let slot = target
                    .get_mut(v)
                    .ok_or_else(|| StoreError::new(path, format!("verset {v} hors bornes")))?;
                if !column.is_null(i) {
                    *slot = column.value(i);
                }
            }
            Ok(())
        };
        for (kind, target) in SignalKind::PHASE1.iter().zip(ranks.iter_mut()) {
            read(&format!("p_{}", kind.code()), target)?;
        }
        read("u", &mut composite)?;
        read("u_pct", &mut composite_rank)?;
    }
    Ok(Scores {
        ranks,
        composite,
        composite_rank,
    })
}
