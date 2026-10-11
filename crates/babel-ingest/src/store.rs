//! Lecture et écriture Parquet. Les fichiers produits sont lisibles tels
//! quels par Polars et DuckDB.

use crate::corpus::{Corpus, Translation};
use crate::meta::{TextBase, TranslationMeta};
use crate::parse::ParsedText;
use arrow::array::{
    Array, ArrayRef, AsArray, BooleanArray, Float32Array, RecordBatch, StringArray, UInt8Array,
    UInt16Array, UInt32Array,
};
use arrow::datatypes::{
    DataType, Field, Float32Type, Schema, SchemaRef, UInt8Type, UInt16Type, UInt32Type,
};
use babel_core::{TranslationId, VerseId, VerseStatus, Versification};
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("Parquet {path} : {message}")]
pub struct StoreError {
    pub path: PathBuf,
    pub message: String,
}

impl StoreError {
    pub fn new(path: &Path, message: impl std::fmt::Display) -> Self {
        Self {
            path: path.to_path_buf(),
            message: message.to_string(),
        }
    }
}

/// Niveau de compression zstd des fichiers Parquet.
const PARQUET_ZSTD_LEVEL: i32 = 3;

/// Écrivain Parquet : un groupe de lignes par lot écrit.
pub struct TableWriter {
    path: PathBuf,
    writer: ArrowWriter<File>,
}

impl TableWriter {
    pub fn create(path: &Path, schema: SchemaRef) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StoreError::new(path, e))?;
        }
        let file = File::create(path).map_err(|e| StoreError::new(path, e))?;
        let level = ZstdLevel::try_new(PARQUET_ZSTD_LEVEL).map_err(|e| StoreError::new(path, e))?;
        let properties = WriterProperties::builder()
            .set_compression(Compression::ZSTD(level))
            .build();
        let writer = ArrowWriter::try_new(file, schema, Some(properties))
            .map_err(|e| StoreError::new(path, e))?;
        Ok(Self {
            path: path.to_path_buf(),
            writer,
        })
    }

    pub fn write(&mut self, schema: &SchemaRef, columns: Vec<ArrayRef>) -> Result<(), StoreError> {
        let batch = RecordBatch::try_new(Arc::clone(schema), columns)
            .map_err(|e| StoreError::new(&self.path, e))?;
        self.writer
            .write(&batch)
            .map_err(|e| StoreError::new(&self.path, e))?;
        self.writer
            .flush()
            .map_err(|e| StoreError::new(&self.path, e))
    }

    pub fn finish(self) -> Result<(), StoreError> {
        self.writer
            .close()
            .map(|_| ())
            .map_err(|e| StoreError::new(&self.path, e))
    }
}

/// Lit tous les lots d'un fichier Parquet.
pub fn read_table(path: &Path) -> Result<Vec<RecordBatch>, StoreError> {
    let file = File::open(path).map_err(|e| StoreError::new(path, e))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| StoreError::new(path, e))?
        .build()
        .map_err(|e| StoreError::new(path, e))?;
    reader
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| StoreError::new(path, e))
}

fn column<'a>(batch: &'a RecordBatch, name: &str, path: &Path) -> Result<&'a ArrayRef, StoreError> {
    batch
        .column_by_name(name)
        .ok_or_else(|| StoreError::new(path, format!("colonne « {name} » absente")))
}

fn wrong_type(name: &str, path: &Path) -> StoreError {
    StoreError::new(path, format!("colonne « {name} » : type inattendu"))
}

pub fn col_u8<'a>(
    b: &'a RecordBatch,
    name: &str,
    path: &Path,
) -> Result<&'a UInt8Array, StoreError> {
    column(b, name, path)?
        .as_primitive_opt::<UInt8Type>()
        .ok_or_else(|| wrong_type(name, path))
}

pub fn col_u16<'a>(
    b: &'a RecordBatch,
    name: &str,
    path: &Path,
) -> Result<&'a UInt16Array, StoreError> {
    column(b, name, path)?
        .as_primitive_opt::<UInt16Type>()
        .ok_or_else(|| wrong_type(name, path))
}

pub fn col_u32<'a>(
    b: &'a RecordBatch,
    name: &str,
    path: &Path,
) -> Result<&'a UInt32Array, StoreError> {
    column(b, name, path)?
        .as_primitive_opt::<UInt32Type>()
        .ok_or_else(|| wrong_type(name, path))
}

pub fn col_f32<'a>(
    b: &'a RecordBatch,
    name: &str,
    path: &Path,
) -> Result<&'a Float32Array, StoreError> {
    column(b, name, path)?
        .as_primitive_opt::<Float32Type>()
        .ok_or_else(|| wrong_type(name, path))
}

pub fn col_str<'a>(
    b: &'a RecordBatch,
    name: &str,
    path: &Path,
) -> Result<&'a StringArray, StoreError> {
    column(b, name, path)?
        .as_string_opt::<i32>()
        .ok_or_else(|| wrong_type(name, path))
}

pub fn col_bool<'a>(
    b: &'a RecordBatch,
    name: &str,
    path: &Path,
) -> Result<&'a BooleanArray, StoreError> {
    column(b, name, path)?
        .as_boolean_opt()
        .ok_or_else(|| wrong_type(name, path))
}

/// Raccourci pour déclarer un champ de schéma.
pub fn field(name: &str, data_type: DataType, nullable: bool) -> Field {
    Field::new(name, data_type, nullable)
}

fn corpus_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        field("translation_id", DataType::UInt16, false),
        field("verse_id", DataType::UInt16, false),
        field("status", DataType::UInt8, false),
        field("len", DataType::UInt16, false),
        field("text", DataType::Utf8, false),
    ]))
}

fn translations_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        field("translation_id", DataType::UInt16, false),
        field("stem", DataType::Utf8, false),
        field("language", DataType::Utf8, false),
        field("language_name", DataType::Utf8, false),
        field("title", DataType::Utf8, false),
        field("script", DataType::Utf8, false),
        field("family", DataType::Utf8, false),
        field("license", DataType::Utf8, false),
        field("redistributable", DataType::Boolean, false),
        field("year", DataType::UInt16, true),
        field("text_base", DataType::Utf8, false),
        field("fingerprint", DataType::Utf8, false),
        field("non_text_lines", DataType::UInt32, false),
    ]))
}

fn verses_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        field("verse_id", DataType::UInt16, false),
        field("ref", DataType::Utf8, false),
        field("book", DataType::Utf8, false),
        field("chapter", DataType::UInt16, false),
        field("verse", DataType::UInt16, false),
    ]))
}

/// Chemins des trois tables du corpus ingéré.
#[derive(Debug, Clone)]
pub struct CorpusPaths {
    /// translation_id, verse_id, status, len, text : une ligne par verset
    /// couvert, un groupe de lignes par traduction.
    pub corpus: PathBuf,
    /// Métadonnées par traduction.
    pub translations: PathBuf,
    /// La versification : verse_id et référence.
    pub verses: PathBuf,
}

impl CorpusPaths {
    pub fn in_dir(data: &Path) -> Self {
        Self {
            corpus: data.join("corpus.parquet"),
            translations: data.join("translations.parquet"),
            verses: data.join("verses.parquet"),
        }
    }
}

/// Écrit le corpus ingéré.
pub fn write_corpus(corpus: &Corpus, paths: &CorpusPaths) -> Result<(), StoreError> {
    let v = &corpus.versification;

    let schema = verses_schema();
    let mut writer = TableWriter::create(&paths.verses, Arc::clone(&schema))?;
    let ids: Vec<VerseId> = (0..v.len()).map(|i| VerseId(i as u16)).collect();
    let refs: Vec<_> = ids.iter().filter_map(|id| v.verse(*id)).collect();
    writer.write(
        &schema,
        vec![
            Arc::new(UInt16Array::from_iter_values(ids.iter().map(|id| id.0))),
            Arc::new(StringArray::from_iter_values(
                ids.iter().map(|id| v.label_usfm(*id)),
            )),
            Arc::new(StringArray::from_iter_values(
                refs.iter()
                    .map(|r| v.books()[usize::from(r.book)].code.as_str()),
            )),
            Arc::new(UInt16Array::from_iter_values(
                refs.iter().map(|r| r.chapter),
            )),
            Arc::new(UInt16Array::from_iter_values(refs.iter().map(|r| r.verse))),
        ],
    )?;
    writer.finish()?;

    let schema = translations_schema();
    let mut writer = TableWriter::create(&paths.translations, Arc::clone(&schema))?;
    let metas: Vec<&TranslationMeta> = corpus.translations.iter().map(|t| &t.meta).collect();
    let strings = |f: fn(&TranslationMeta) -> &str| -> ArrayRef {
        Arc::new(StringArray::from_iter_values(metas.iter().map(|m| f(m))))
    };
    writer.write(
        &schema,
        vec![
            Arc::new(UInt16Array::from_iter_values(metas.iter().map(|m| m.id.0))),
            strings(|m| &m.stem),
            strings(|m| &m.language),
            strings(|m| &m.language_name),
            strings(|m| &m.title),
            strings(|m| &m.script),
            strings(|m| &m.family),
            strings(|m| &m.license),
            Arc::new(BooleanArray::from_iter(
                metas.iter().map(|m| Some(m.redistributable)),
            )),
            Arc::new(UInt16Array::from_iter(metas.iter().map(|m| m.year))),
            strings(|m| m.text_base.code()),
            strings(|m| &m.fingerprint),
            Arc::new(UInt32Array::from_iter_values(
                metas.iter().map(|m| m.non_text_lines),
            )),
        ],
    )?;
    writer.finish()?;

    let schema = corpus_schema();
    let mut writer = TableWriter::create(&paths.corpus, Arc::clone(&schema))?;
    for t in &corpus.translations {
        let covered: Vec<usize> = (0..v.len()).filter(|i| t.status(*i).is_covered()).collect();
        writer.write(
            &schema,
            vec![
                Arc::new(UInt16Array::from_iter_values(
                    covered.iter().map(|_| t.meta.id.0),
                )),
                Arc::new(UInt16Array::from_iter_values(
                    covered.iter().map(|i| *i as u16),
                )),
                Arc::new(UInt8Array::from_iter_values(
                    covered.iter().map(|i| t.status(*i) as u8),
                )),
                Arc::new(UInt16Array::from_iter_values(
                    covered.iter().map(|i| t.text.lengths[*i]),
                )),
                Arc::new(StringArray::from_iter_values(
                    covered.iter().map(|i| t.text.text(*i)),
                )),
            ],
        )?;
    }
    writer.finish()
}

/// Relit la versification écrite par [`write_corpus`].
pub fn read_versification(path: &Path) -> Result<Versification, StoreError> {
    let mut vref = String::new();
    for batch in read_table(path)? {
        let refs = col_str(&batch, "ref", path)?;
        for i in 0..batch.num_rows() {
            vref.push_str(refs.value(i));
            vref.push('\n');
        }
    }
    Versification::parse(&vref).map_err(|e| StoreError::new(path, e))
}

/// Relit les métadonnées des traductions.
pub fn read_translations(path: &Path) -> Result<Vec<TranslationMeta>, StoreError> {
    let mut metas = Vec::new();
    for batch in read_table(path)? {
        let id = col_u16(&batch, "translation_id", path)?;
        let stem = col_str(&batch, "stem", path)?;
        let language = col_str(&batch, "language", path)?;
        let language_name = col_str(&batch, "language_name", path)?;
        let title = col_str(&batch, "title", path)?;
        let script = col_str(&batch, "script", path)?;
        let family = col_str(&batch, "family", path)?;
        let license = col_str(&batch, "license", path)?;
        let redistributable = col_bool(&batch, "redistributable", path)?;
        let year = col_u16(&batch, "year", path)?;
        let text_base = col_str(&batch, "text_base", path)?;
        let fingerprint = col_str(&batch, "fingerprint", path)?;
        let non_text_lines = col_u32(&batch, "non_text_lines", path)?;
        for i in 0..batch.num_rows() {
            metas.push(TranslationMeta {
                id: TranslationId(id.value(i)),
                stem: stem.value(i).to_string(),
                language: language.value(i).to_string(),
                language_name: language_name.value(i).to_string(),
                title: title.value(i).to_string(),
                script: script.value(i).to_string(),
                family: family.value(i).to_string(),
                license: license.value(i).to_string(),
                redistributable: redistributable.value(i),
                year: (!year.is_null(i)).then(|| year.value(i)),
                text_base: TextBase::from_code(text_base.value(i)),
                fingerprint: fingerprint.value(i).to_string(),
                non_text_lines: non_text_lines.value(i),
            });
        }
    }
    Ok(metas)
}

/// Relit le corpus écrit par [`write_corpus`].
pub fn read_corpus(paths: &CorpusPaths) -> Result<Corpus, StoreError> {
    let versification = read_versification(&paths.verses)?;
    let metas = read_translations(&paths.translations)?;
    let n = versification.len();
    let path = &paths.corpus;

    let mut status = vec![vec![VerseStatus::Absent; n]; metas.len()];
    let mut lengths = vec![vec![0_u16; n]; metas.len()];
    let mut texts: Vec<Vec<String>> = vec![vec![String::new(); n]; metas.len()];

    let file = File::open(path).map_err(|e| StoreError::new(path, e))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| StoreError::new(path, e))?
        .build()
        .map_err(|e| StoreError::new(path, e))?;
    for batch in reader {
        let batch = batch.map_err(|e| StoreError::new(path, e))?;
        let translation = col_u16(&batch, "translation_id", path)?;
        let verse = col_u16(&batch, "verse_id", path)?;
        let state = col_u8(&batch, "status", path)?;
        let len = col_u16(&batch, "len", path)?;
        let text = col_str(&batch, "text", path)?;
        for i in 0..batch.num_rows() {
            let (t, v) = (
                usize::from(translation.value(i)),
                usize::from(verse.value(i)),
            );
            if t >= metas.len() || v >= n {
                return Err(StoreError::new(
                    path,
                    format!("ligne hors bornes (traduction {t}, verset {v})"),
                ));
            }
            status[t][v] = VerseStatus::from_u8(state.value(i))
                .ok_or_else(|| StoreError::new(path, "statut de verset inconnu"))?;
            lengths[t][v] = len.value(i);
            texts[t][v] = text.value(i).to_string();
        }
    }

    let translations = metas
        .into_iter()
        .zip(status)
        .zip(lengths)
        .zip(texts)
        .map(|(((meta, status), lengths), texts)| {
            let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
            let mut text = ParsedText::from_parts(status, lengths, &refs);
            text.non_text_lines = meta.non_text_lines;
            Translation { meta, text }
        })
        .collect();
    Ok(Corpus {
        versification,
        translations,
    })
}

/// Relit une seule traduction de `corpus.parquet`, sans charger le reste.
///
/// [`write_corpus`] écrit un groupe de lignes par traduction, dans l'ordre :
/// le groupe `translation` est celui de la traduction demandée.
pub fn read_translation(
    path: &Path,
    n_verses: usize,
    translation: TranslationId,
) -> Result<ParsedText, StoreError> {
    let file = File::open(path).map_err(|e| StoreError::new(path, e))?;
    let builder =
        ParquetRecordBatchReaderBuilder::try_new(file).map_err(|e| StoreError::new(path, e))?;
    if translation.index() >= builder.metadata().num_row_groups() {
        return Err(StoreError::new(
            path,
            format!("traduction {} absente", translation.0),
        ));
    }
    let reader = builder
        .with_row_groups(vec![translation.index()])
        .build()
        .map_err(|e| StoreError::new(path, e))?;

    let mut status = vec![VerseStatus::Absent; n_verses];
    let mut lengths = vec![0_u16; n_verses];
    let mut texts = vec![String::new(); n_verses];
    for batch in reader {
        let batch = batch.map_err(|e| StoreError::new(path, e))?;
        let id = col_u16(&batch, "translation_id", path)?;
        let verse = col_u16(&batch, "verse_id", path)?;
        let state = col_u8(&batch, "status", path)?;
        let len = col_u16(&batch, "len", path)?;
        let text = col_str(&batch, "text", path)?;
        for i in 0..batch.num_rows() {
            let v = usize::from(verse.value(i));
            if id.value(i) != translation.0 || v >= n_verses {
                return Err(StoreError::new(
                    path,
                    "les groupes de lignes ne suivent pas l'ordre des traductions : relancer `babel ingest`",
                ));
            }
            status[v] = VerseStatus::from_u8(state.value(i))
                .ok_or_else(|| StoreError::new(path, "statut de verset inconnu"))?;
            lengths[v] = len.value(i);
            texts[v] = text.value(i).to_string();
        }
    }
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    Ok(ParsedText::from_parts(status, lengths, &refs))
}

/// Nombre de lignes d'un fichier Parquet, lu dans ses métadonnées.
pub fn row_count(path: &Path) -> Result<u64, StoreError> {
    let file = File::open(path).map_err(|e| StoreError::new(path, e))?;
    let builder =
        ParquetRecordBatchReaderBuilder::try_new(file).map_err(|e| StoreError::new(path, e))?;
    Ok(u64::try_from(builder.metadata().file_metadata().num_rows()).unwrap_or(0))
}
