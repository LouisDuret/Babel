//! Types de base de Babel, partagés par toutes les crates.
//!
//! Tout le pipeline travaille sur [`VerseId`], l'index de ligne dans
//! `vref.txt`. Ce module ne fait aucune entrée-sortie, sauf le chargement de
//! la configuration.

pub mod books;
pub mod config;
pub mod reference;
pub mod stats;
pub mod text;
pub mod types;
pub mod versification;

pub use books::{BOOKS, BookInfo, Section, Testament};
pub use config::Config;
pub use types::{SignalKind, TranslationId, VerseId, VerseStatus};
pub use versification::Versification;
