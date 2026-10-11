//! `texts.zst` : les textes des traductions de référence, compressés par
//! blocs (traduction × livre) et chargés à la demande par le viewer.

use crate::{AtlasError, TextBlock};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Écrit les blocs les uns à la suite des autres et renvoie leur index.
pub struct TextWriter {
    path: PathBuf,
    file: File,
    offset: u64,
    level: i32,
    blocks: Vec<TextBlock>,
}

impl TextWriter {
    pub fn create(path: &Path, level: i32) -> Result<Self, AtlasError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AtlasError::io(path, e))?;
        }
        let file = File::create(path).map_err(|e| AtlasError::io(path, e))?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            offset: 0,
            level,
            blocks: Vec::new(),
        })
    }

    /// Ajoute le bloc d'un livre : une ligne par verset du livre.
    pub fn push(&mut self, translation: u16, book: u16, verses: &[&str]) -> Result<(), AtlasError> {
        let joined = verses.join("\n");
        let compressed = zstd::encode_all(joined.as_bytes(), self.level)
            .map_err(|e| AtlasError::io(&self.path, e))?;
        self.file
            .write_all(&compressed)
            .map_err(|e| AtlasError::io(&self.path, e))?;
        self.blocks.push(TextBlock {
            translation,
            book,
            offset: self.offset,
            len: compressed.len() as u32,
        });
        self.offset += compressed.len() as u64;
        Ok(())
    }

    pub fn finish(mut self) -> Result<Vec<TextBlock>, AtlasError> {
        self.file
            .flush()
            .map_err(|e| AtlasError::io(&self.path, e))?;
        Ok(self.blocks)
    }
}

/// Lit les blocs à la demande et garde en mémoire ceux déjà décompressés.
pub struct TextReader {
    path: PathBuf,
    file: Option<File>,
    index: HashMap<(u16, u16), (u64, u32)>,
    cache: HashMap<(u16, u16), Vec<String>>,
}

impl TextReader {
    /// `blocks` est l'index stocké dans l'atlas. Le fichier n'est ouvert
    /// qu'à la première lecture ; s'il manque, les textes sont simplement
    /// indisponibles.
    pub fn new(path: &Path, blocks: &[TextBlock]) -> Self {
        Self {
            path: path.to_path_buf(),
            file: None,
            index: blocks
                .iter()
                .map(|b| ((b.translation, b.book), (b.offset, b.len)))
                .collect(),
            cache: HashMap::new(),
        }
    }

    pub fn has(&self, translation: u16, book: u16) -> bool {
        self.index.contains_key(&(translation, book))
    }

    /// Versets d'un livre dans une traduction, une entrée par verset du
    /// livre. `None` si le bloc n'existe pas.
    pub fn book(&mut self, translation: u16, book: u16) -> Result<Option<&[String]>, AtlasError> {
        let key = (translation, book);
        let Some((offset, len)) = self.index.get(&key).copied() else {
            return Ok(None);
        };
        if !self.cache.contains_key(&key) {
            let path = self.path.clone();
            if self.file.is_none() {
                self.file = Some(File::open(&path).map_err(|e| AtlasError::io(&path, e))?);
            }
            let Some(file) = self.file.as_mut() else {
                return Ok(None);
            };
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| AtlasError::io(&path, e))?;
            let mut compressed = vec![0_u8; len as usize];
            file.read_exact(&mut compressed)
                .map_err(|e| AtlasError::io(&path, e))?;
            let bytes =
                zstd::decode_all(compressed.as_slice()).map_err(|e| AtlasError::io(&path, e))?;
            let text = String::from_utf8(bytes)
                .map_err(|_| AtlasError::Invalid(format!("{} : bloc non UTF-8", path.display())))?;
            self.cache
                .insert(key, text.split('\n').map(str::to_string).collect());
        }
        Ok(self.cache.get(&key).map(Vec::as_slice))
    }

    /// Texte d'un verset, par son rang dans le livre.
    pub fn verse(&mut self, translation: u16, book: u16, index_in_book: usize) -> Option<&str> {
        self.book(translation, book)
            .ok()
            .flatten()
            .and_then(|verses| verses.get(index_in_book))
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour() {
        let path =
            std::env::temp_dir().join(format!("babel-test-texts-{}.zst", std::process::id()));
        let mut writer = TextWriter::create(&path, 3).expect("création");
        writer
            .push(0, 0, &["Au commencement", "", "Que la lumière soit"])
            .expect("bloc 1");
        writer.push(0, 1, &["Généalogie"]).expect("bloc 2");
        writer
            .push(
                1,
                0,
                &["In the beginning", "And the earth", "Let there be light"],
            )
            .expect("bloc 3");
        let blocks = writer.finish().expect("fin");
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1].offset, u64::from(blocks[0].len));

        let mut reader = TextReader::new(&path, &blocks);
        assert!(reader.has(1, 0));
        assert!(!reader.has(1, 1));
        assert_eq!(reader.verse(0, 0, 2), Some("Que la lumière soit"));
        // Un verset absent est une ligne vide, pas un décalage.
        assert_eq!(reader.verse(0, 0, 1), Some(""));
        assert_eq!(reader.verse(0, 1, 0), Some("Généalogie"));
        assert_eq!(reader.verse(1, 0, 1), Some("And the earth"));
        assert_eq!(reader.verse(1, 1, 0), None);
        assert_eq!(reader.verse(0, 0, 9), None);
        drop(reader);
        std::fs::remove_file(&path).expect("nettoyage");
    }

    #[test]
    fn fichier_absent() {
        let blocks = [TextBlock {
            translation: 0,
            book: 0,
            offset: 0,
            len: 4,
        }];
        let mut reader = TextReader::new(Path::new("fichier-qui-n-existe-pas.zst"), &blocks);
        assert!(reader.book(0, 0).is_err());
        assert_eq!(reader.verse(0, 0, 0), None);
    }
}
