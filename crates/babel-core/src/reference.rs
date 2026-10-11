//! Analyse des références saisies dans la recherche : « Jn 3:16 »,
//! « Jean 3 », « 1 Co 13 », « GEN 1:1 ».

use crate::books::{BOOKS, find_book};
use crate::versification::{Target, Versification};

/// Référence telle qu'elle a été saisie : livre de la table [`BOOKS`],
/// chapitre et verset facultatifs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedReference {
    /// Index dans [`BOOKS`].
    pub book: usize,
    pub chapter: Option<u16>,
    pub verse: Option<u16>,
}

/// Analyse une référence en français ou en anglais.
///
/// Le livre est tout ce qui précède le dernier groupe « chapitre » ou
/// « chapitre:verset » ; il peut commencer par un chiffre (« 1 Co 13 »).
/// Les séparateurs acceptés entre chapitre et verset sont `:`, `.`, `,` et
/// `v`.
pub fn parse_reference(input: &str) -> Option<ParsedReference> {
    let s = input.trim();
    let (book_part, chapter, verse) = split_trailing_numbers(s);
    if let Some(book) = find_book(book_part) {
        return Some(ParsedReference {
            book,
            chapter,
            verse,
        });
    }
    // « Psaume 151 » ou « 1 Jn » : le livre est l'entrée entière.
    find_book(s).map(|book| ParsedReference {
        book,
        chapter: None,
        verse: None,
    })
}

/// Sépare « Jean 3:16 » en (« Jean », 3, 16). Sans nombre final, renvoie
/// l'entrée entière.
fn split_trailing_numbers(s: &str) -> (&str, Option<u16>, Option<u16>) {
    let has_letter = |part: &str| part.chars().any(char::is_alphabetic);

    let (Some(last), start_last) = take_number(s, s.len()) else {
        return (s, None, None);
    };

    // Un séparateur puis un autre nombre : « 3:16 ».
    let before = s[..start_last].trim_end();
    if let Some(stripped) = before.strip_suffix([':', '.', ',', 'v']) {
        let stripped = stripped.trim_end();
        if let (Some(first), start_first) = take_number(stripped, stripped.len()) {
            let book = &stripped[..start_first];
            if has_letter(book) {
                return (book, Some(first), Some(last));
            }
        }
    }

    let book = &s[..start_last];
    if has_letter(book) {
        (book, Some(last), None)
    } else {
        (s, None, None)
    }
}

/// Lit le nombre qui se termine à l'octet `end`. Renvoie le nombre et
/// l'octet où il commence.
fn take_number(s: &str, end: usize) -> (Option<u16>, usize) {
    let bytes = s.as_bytes();
    let mut start = end;
    while start > 0 && bytes[start - 1].is_ascii_digit() {
        start -= 1;
    }
    if start == end {
        return (None, end);
    }
    (s[start..end].parse().ok(), start)
}

/// Résout une référence saisie vers une cible de navigation existante.
///
/// Un livre à chapitre unique accepte « Jude 5 » pour le verset 5.
pub fn resolve(parsed: ParsedReference, versification: &Versification) -> Option<Target> {
    let usfm = BOOKS.get(parsed.book)?.usfm;
    let book = versification.book_index(usfm)?;
    let span = &versification.books()[book];
    let single_chapter = span.chapters.len() == 1;
    match (parsed.chapter, parsed.verse) {
        (None, _) => Some(Target::Book { book }),
        (Some(chapter), Some(verse)) => versification.find(book, chapter, verse).map(Target::Verse),
        (Some(number), None) if single_chapter && number != span.chapters[0].number => {
            versification
                .find(book, span.chapters[0].number, number)
                .map(Target::Verse)
        }
        (Some(number), None) => span
            .chapters
            .iter()
            .position(|c| c.number == number)
            .map(|chapter| Target::Chapter { book, chapter }),
    }
}

/// Analyse puis résout en une étape. Si la lecture « livre + nombres » ne
/// désigne rien, l'entrée entière est essayée comme nom de livre.
pub fn lookup(input: &str, versification: &Versification) -> Option<Target> {
    let parsed = parse_reference(input)?;
    resolve(parsed, versification).or_else(|| {
        let whole = find_book(input.trim())?;
        resolve(
            ParsedReference {
                book: whole,
                chapter: None,
                verse: None,
            },
            versification,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::VerseId;

    fn parsed(input: &str) -> Option<(&'static str, Option<u16>, Option<u16>)> {
        parse_reference(input).map(|p| (BOOKS[p.book].usfm, p.chapter, p.verse))
    }

    #[test]
    fn references_du_cahier_des_charges() {
        assert_eq!(parsed("Jn 3:16"), Some(("JHN", Some(3), Some(16))));
        assert_eq!(parsed("Jean 3"), Some(("JHN", Some(3), None)));
        assert_eq!(parsed("1 Co 13"), Some(("1CO", Some(13), None)));
    }

    #[test]
    fn variantes_de_saisie() {
        assert_eq!(parsed("GEN 1:1"), Some(("GEN", Some(1), Some(1))));
        assert_eq!(parsed("john 3.16"), Some(("JHN", Some(3), Some(16))));
        assert_eq!(parsed("Jn3:16"), Some(("JHN", Some(3), Some(16))));
        assert_eq!(parsed("1co13:4"), Some(("1CO", Some(13), Some(4))));
        assert_eq!(parsed("1 Jean 5, 7"), Some(("1JN", Some(5), Some(7))));
        assert_eq!(parsed("Ps 119 v 105"), Some(("PSA", Some(119), Some(105))));
        assert_eq!(parsed("Actes 8 : 37"), Some(("ACT", Some(8), Some(37))));
        assert_eq!(parsed("Lv 5"), Some(("LEV", Some(5), None)));
        assert_eq!(parsed("Ps 2"), Some(("PSA", Some(2), None)));
        assert_eq!(parsed("Apocalypse"), Some(("REV", None, None)));
        assert_eq!(parsed("1 Jn"), Some(("1JN", None, None)));
        assert_eq!(parsed("2 Rois"), Some(("2KI", None, None)));
    }

    #[test]
    fn entrees_invalides() {
        assert_eq!(parsed(""), None);
        assert_eq!(parsed("3:16"), None);
        assert_eq!(parsed("42"), None);
        assert_eq!(parsed("Zorglub 3"), None);
    }

    #[test]
    fn resolution() {
        let vref = "JHN 3:15\nJHN 3:16\nJHN 4:1\nJUD 1:1\nJUD 1:5\n";
        let v = Versification::parse(vref).expect("vref valide");
        assert_eq!(lookup("Jn 3:16", &v), Some(Target::Verse(VerseId(1))));
        assert_eq!(
            lookup("Jean 4", &v),
            Some(Target::Chapter {
                book: 0,
                chapter: 1
            })
        );
        assert_eq!(lookup("Jean", &v), Some(Target::Book { book: 0 }));
        assert_eq!(lookup("Jude 5", &v), Some(Target::Verse(VerseId(4))));
        assert_eq!(
            lookup("Jude 1", &v),
            Some(Target::Chapter {
                book: 1,
                chapter: 0
            })
        );
        assert_eq!(lookup("Jn 9:9", &v), None);
        assert_eq!(lookup("Mt 1:1", &v), None);
    }
}
