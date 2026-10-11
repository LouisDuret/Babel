//! Versification : la liste ordonnée des versets de `vref.txt`.

use crate::books::{Section, Testament, book_by_usfm};
use crate::types::VerseId;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VersificationError {
    #[error("vref ligne {line} : référence illisible « {text} » (format attendu : GEN 1:1)")]
    Malformed { line: usize, text: String },
    #[error("vref compte {found} lignes, plus que les {max} qu'un VerseId (u16) peut indexer")]
    TooManyVerses { found: usize, max: usize },
    #[error("vref est vide")]
    Empty,
}

/// Référence d'un verset : index du livre dans la versification, chapitre,
/// verset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VerseRef {
    pub book: u16,
    pub chapter: u16,
    pub verse: u16,
}

/// Un chapitre : une suite contiguë de versets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterSpan {
    pub number: u16,
    pub first: u32,
    pub len: u32,
}

impl ChapterSpan {
    pub fn range(&self) -> std::ops::Range<usize> {
        self.first as usize..(self.first + self.len) as usize
    }
}

/// Un livre : une suite contiguë de chapitres.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookSpan {
    /// Code USFM lu dans `vref.txt`.
    pub code: String,
    pub name_fr: String,
    pub name_en: String,
    pub section: Section,
    pub first: u32,
    pub len: u32,
    pub chapters: Vec<ChapterSpan>,
}

impl BookSpan {
    pub fn testament(&self) -> Testament {
        self.section.testament()
    }

    pub fn range(&self) -> std::ops::Range<usize> {
        self.first as usize..(self.first + self.len) as usize
    }
}

/// Cible d'une navigation : un livre, un chapitre ou un verset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Book { book: usize },
    Chapter { book: usize, chapter: usize },
    Verse(VerseId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versification {
    refs: Vec<VerseRef>,
    /// Index du chapitre dans son livre, par verset.
    chapter_index: Vec<u16>,
    books: Vec<BookSpan>,
}

impl Versification {
    /// Lit le contenu de `vref.txt` (une référence `GEN 1:1` par ligne).
    pub fn parse(text: &str) -> Result<Self, VersificationError> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() {
            return Err(VersificationError::Empty);
        }
        let max = usize::from(u16::MAX);
        if lines.len() > max {
            return Err(VersificationError::TooManyVerses {
                found: lines.len(),
                max,
            });
        }

        let mut refs = Vec::with_capacity(lines.len());
        let mut chapter_index = Vec::with_capacity(lines.len());
        let mut books: Vec<BookSpan> = Vec::new();

        for (i, raw) in lines.iter().enumerate() {
            let malformed = || VersificationError::Malformed {
                line: i + 1,
                text: (*raw).to_string(),
            };
            let line = raw.trim();
            let (code, rest) = line.split_once(' ').ok_or_else(malformed)?;
            let (chapter, verse) = rest.split_once(':').ok_or_else(malformed)?;
            let chapter: u16 = chapter.trim().parse().map_err(|_| malformed())?;
            let verse: u16 = verse.trim().parse().map_err(|_| malformed())?;

            if books.last().is_none_or(|b| b.code != code) {
                let info = book_by_usfm(code);
                books.push(BookSpan {
                    code: code.to_string(),
                    name_fr: info.map_or(code, |b| b.name_fr).to_string(),
                    name_en: info.map_or(code, |b| b.name_en).to_string(),
                    section: info.map_or(Section::Other, |b| b.section),
                    first: i as u32,
                    len: 0,
                    chapters: Vec::new(),
                });
            }
            let book_index = books.len() - 1;
            let book = &mut books[book_index];
            if book.chapters.last().is_none_or(|c| c.number != chapter) {
                book.chapters.push(ChapterSpan {
                    number: chapter,
                    first: i as u32,
                    len: 0,
                });
            }
            book.len += 1;
            let chapter_idx = book.chapters.len() - 1;
            book.chapters[chapter_idx].len += 1;

            refs.push(VerseRef {
                book: book_index as u16,
                chapter,
                verse,
            });
            chapter_index.push(chapter_idx as u16);
        }

        Ok(Self {
            refs,
            chapter_index,
            books,
        })
    }

    /// Nombre de versets (lignes de `vref.txt`).
    pub fn len(&self) -> usize {
        self.refs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.refs.is_empty()
    }

    pub fn books(&self) -> &[BookSpan] {
        &self.books
    }

    pub fn verse(&self, id: VerseId) -> Option<VerseRef> {
        self.refs.get(id.index()).copied()
    }

    /// Index du livre d'un verset.
    pub fn book_of(&self, verse: usize) -> usize {
        self.refs.get(verse).map_or(0, |r| usize::from(r.book))
    }

    /// Index du chapitre (dans son livre) d'un verset.
    pub fn chapter_of(&self, verse: usize) -> usize {
        self.chapter_index.get(verse).map_or(0, |c| usize::from(*c))
    }

    pub fn testament_of(&self, verse: usize) -> Testament {
        self.books
            .get(self.book_of(verse))
            .map_or(Testament::Deuterocanon, BookSpan::testament)
    }

    pub fn book_index(&self, usfm: &str) -> Option<usize> {
        self.books.iter().position(|b| b.code == usfm)
    }

    /// Verset désigné par (livre, chapitre, verset), s'il existe.
    pub fn find(&self, book: usize, chapter: u16, verse: u16) -> Option<VerseId> {
        let b = self.books.get(book)?;
        let c = b.chapters.iter().find(|c| c.number == chapter)?;
        c.range()
            .find(|i| self.refs[*i].verse == verse)
            .map(|i| VerseId(i as u16))
    }

    /// Variante par code USFM : `find_usfm("ACT", 8, 37)`.
    pub fn find_usfm(&self, usfm: &str, chapter: u16, verse: u16) -> Option<VerseId> {
        self.find(self.book_index(usfm)?, chapter, verse)
    }

    /// Référence au format de `vref.txt` : `JHN 3:16`.
    pub fn label_usfm(&self, id: VerseId) -> String {
        match self.verse(id) {
            Some(r) => {
                format!(
                    "{} {}:{}",
                    self.books[usize::from(r.book)].code,
                    r.chapter,
                    r.verse
                )
            }
            None => format!("#{id}"),
        }
    }

    /// Référence en français : `Jean 3:16`.
    pub fn label_fr(&self, id: VerseId) -> String {
        match self.verse(id) {
            Some(r) => {
                format!(
                    "{} {}:{}",
                    self.books[usize::from(r.book)].name_fr,
                    r.chapter,
                    r.verse
                )
            }
            None => format!("#{id}"),
        }
    }

    /// Plage de versets couverte par une cible de navigation.
    pub fn target_range(&self, target: Target) -> Option<std::ops::Range<usize>> {
        match target {
            Target::Book { book } => self.books.get(book).map(BookSpan::range),
            Target::Chapter { book, chapter } => self
                .books
                .get(book)?
                .chapters
                .get(chapter)
                .map(ChapterSpan::range),
            Target::Verse(id) => (id.index() < self.len()).then(|| id.index()..id.index() + 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VREF: &str = "GEN 1:1\nGEN 1:2\nGEN 2:1\nMAT 1:1\nMAT 1:2\nMAT 2:1\nXXX 1:1\n";

    #[test]
    fn lit_livres_et_chapitres() {
        let v = Versification::parse(VREF).expect("vref valide");
        assert_eq!(v.len(), 7);
        assert_eq!(v.books().len(), 3);
        assert_eq!(v.books()[0].code, "GEN");
        assert_eq!(v.books()[0].chapters.len(), 2);
        assert_eq!(v.books()[1].first, 3);
        assert_eq!(v.books()[1].len, 3);
        assert_eq!(v.books()[2].section, Section::Other);
        assert_eq!(v.books()[2].name_fr, "XXX");
    }

    #[test]
    fn recherche_et_libelles() {
        let v = Versification::parse(VREF).expect("vref valide");
        assert_eq!(v.find_usfm("MAT", 1, 2), Some(VerseId(4)));
        assert_eq!(v.find_usfm("MAT", 9, 9), None);
        assert_eq!(v.label_fr(VerseId(4)), "Matthieu 1:2");
        assert_eq!(v.label_usfm(VerseId(2)), "GEN 2:1");
        assert_eq!(v.chapter_of(2), 1);
        assert_eq!(v.book_of(5), 1);
    }

    #[test]
    fn accepte_crlf_et_bom() {
        let v = Versification::parse("\u{feff}GEN 1:1\r\nGEN 1:2\r\n").expect("vref valide");
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn refuse_les_lignes_illisibles() {
        let err = Versification::parse("GEN 1:1\nGEN un:2\n");
        assert_eq!(
            err,
            Err(VersificationError::Malformed {
                line: 2,
                text: "GEN un:2".to_string()
            })
        );
        assert_eq!(Versification::parse(""), Err(VersificationError::Empty));
    }

    #[test]
    fn refuse_plus_de_65535_lignes() {
        let big: String = (0..70_000)
            .map(|i| format!("GEN 1:{}\n", i % 60_000))
            .collect();
        assert!(matches!(
            Versification::parse(&big),
            Err(VersificationError::TooManyVerses { .. })
        ));
    }
}
