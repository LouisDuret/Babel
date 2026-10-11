//! Ingestion du corpus eBible : lecture des traductions, métadonnées,
//! contrôle qualité, export Parquet.

pub mod corpus;
pub mod meta;
pub mod parse;
pub mod quality;
pub mod store;

pub use corpus::{
    Corpus, IngestError, Loaded, Rejected, Translation, corpus_from_texts, load_ebible,
};
pub use meta::{TextBase, TranslationMeta};
pub use parse::{ParsedText, RANGE_TOKEN};
pub use store::{CorpusPaths, StoreError};
