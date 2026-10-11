//! Lecture d'un fichier de traduction eBible : une ligne par verset, dans
//! l'ordre de `vref.txt`. Fonctions pures, sans entrée-sortie.

use babel_core::VerseStatus;
use babel_core::text::nfc;
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

pub use babel_core::types::RANGE_TOKEN;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("{found} lignes au lieu des {expected} de vref.txt")]
    LineCount { expected: usize, found: usize },
}

/// Texte d'une traduction, verset par verset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedText {
    pub status: Vec<VerseStatus>,
    /// Longueur en graphèmes Unicode du texte de la ligne (0 si absent).
    pub lengths: Vec<u16>,
    /// Textes concaténés ; `offsets[v]..offsets[v + 1]` délimite le verset v.
    text: String,
    offsets: Vec<u32>,
    /// Nombre de lignes dont le contenu ne ressemble pas à du texte.
    pub non_text_lines: u32,
}

impl ParsedText {
    /// Assemble une traduction à partir de ses colonnes (relecture Parquet).
    pub fn from_parts(status: Vec<VerseStatus>, lengths: Vec<u16>, texts: &[&str]) -> Self {
        let mut text = String::with_capacity(texts.iter().map(|t| t.len()).sum());
        let mut offsets = Vec::with_capacity(texts.len() + 1);
        offsets.push(0);
        for t in texts {
            text.push_str(t);
            offsets.push(text.len() as u32);
        }
        Self {
            status,
            lengths,
            text,
            offsets,
            non_text_lines: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.status.len()
    }

    pub fn is_empty(&self) -> bool {
        self.status.is_empty()
    }

    /// Texte de la ligne du verset (vide si absent ou fusionné).
    pub fn text(&self, verse: usize) -> &str {
        match (self.offsets.get(verse), self.offsets.get(verse + 1)) {
            (Some(a), Some(b)) => &self.text[*a as usize..*b as usize],
            _ => "",
        }
    }

    /// Nombre de versets couverts (texte propre ou fusionné).
    pub fn covered(&self) -> usize {
        self.status.iter().filter(|s| s.is_covered()).count()
    }

    /// Tous les textes de la traduction, concaténés sans séparateur.
    pub fn full_text(&self) -> &str {
        &self.text
    }
}

/// Analyse le contenu d'un fichier de traduction.
///
/// - ligne vide : verset absent ;
/// - `<range>` : verset fusionné, le texte est sur la ligne du premier
///   verset de la plage, qui devient « tête de plage » ;
/// - sinon : texte du verset, normalisé en NFC.
///
/// Le fichier doit compter exactement `n_verses` lignes.
pub fn parse_translation(
    content: &str,
    n_verses: usize,
    non_text_letter_share: f32,
) -> Result<ParsedText, ParseError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let found = content.lines().count();
    if found != n_verses {
        return Err(ParseError::LineCount {
            expected: n_verses,
            found,
        });
    }

    let mut status = Vec::with_capacity(n_verses);
    let mut lengths = Vec::with_capacity(n_verses);
    let mut text = String::with_capacity(content.len());
    let mut offsets = Vec::with_capacity(n_verses + 1);
    offsets.push(0_u32);
    let mut non_text_lines = 0_u32;

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() {
            status.push(VerseStatus::Absent);
            lengths.push(0);
        } else if line == RANGE_TOKEN {
            mark_range_head(&mut status);
            status.push(VerseStatus::RangeContinuation);
            lengths.push(0);
        } else {
            let normalized = nfc(line);
            let graphemes = normalized.graphemes(true).count();
            lengths.push(u16::try_from(graphemes).unwrap_or(u16::MAX));
            if !looks_like_text(&normalized, non_text_letter_share) {
                non_text_lines += 1;
            }
            text.push_str(&normalized);
            status.push(VerseStatus::Present);
        }
        offsets.push(text.len() as u32);
    }

    Ok(ParsedText {
        status,
        lengths,
        text,
        offsets,
        non_text_lines,
    })
}

/// Le verset qui précède une suite de `<range>` porte le texte de la plage.
fn mark_range_head(status: &mut [VerseStatus]) {
    let head = status
        .iter()
        .rposition(|s| *s != VerseStatus::RangeContinuation);
    if let Some(i) = head
        && status[i] == VerseStatus::Present
    {
        status[i] = VerseStatus::RangeHead;
    }
}

/// Une ligne est textuelle si assez de ses caractères (hors espaces) sont
/// des lettres et si elle ne contient pas de balisage USFM ou XML.
pub fn looks_like_text(line: &str, min_letter_share: f32) -> bool {
    if line.contains('\\') || line.contains('<') {
        return false;
    }
    let (mut letters, mut total) = (0_u32, 0_u32);
    for c in line.chars().filter(|c| !c.is_whitespace()) {
        total += 1;
        if c.is_alphabetic() {
            letters += 1;
        }
    }
    total > 0 && letters as f32 >= min_letter_share * total as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use VerseStatus::{Absent, Present, RangeContinuation, RangeHead};

    const SHARE: f32 = 0.5;

    #[test]
    fn lignes_vides_et_plages() {
        let content = "Au commencement\n\nPremier et second\n<range>\n<range>\nSuite\n";
        let p = parse_translation(content, 6, SHARE).expect("six lignes");
        assert_eq!(
            p.status,
            vec![
                Present,
                Absent,
                RangeHead,
                RangeContinuation,
                RangeContinuation,
                Present
            ]
        );
        assert_eq!(p.text(0), "Au commencement");
        assert_eq!(p.text(1), "");
        assert_eq!(p.text(2), "Premier et second");
        assert_eq!(p.text(3), "");
        assert_eq!(p.text(5), "Suite");
        assert_eq!(p.lengths, vec![15, 0, 17, 0, 0, 5]);
        assert_eq!(p.covered(), 5);
    }

    #[test]
    fn plage_sans_tete() {
        // Un `<range>` après une ligne vide n'a pas de tête : il reste
        // fusionné, et la ligne vide reste absente.
        let p = parse_translation("\n<range>\nTexte\n", 3, SHARE).expect("trois lignes");
        assert_eq!(p.status, vec![Absent, RangeContinuation, Present]);
    }

    #[test]
    fn normalisation_nfc_et_graphemes() {
        // « é » décomposé (e + accent combinant) : un seul graphème, et le
        // texte stocké est précomposé.
        let p = parse_translation("e\u{301}te\u{301}\n", 1, SHARE).expect("une ligne");
        assert_eq!(p.text(0), "\u{e9}t\u{e9}");
        assert_eq!(p.lengths, vec![3]);
    }

    #[test]
    fn fins_de_ligne_windows_et_bom() {
        let p = parse_translation("\u{feff}Un\r\n\r\nTrois\r\n", 3, SHARE).expect("trois lignes");
        assert_eq!(p.status, vec![Present, Absent, Present]);
        assert_eq!(p.text(0), "Un");
    }

    #[test]
    fn derniers_versets_vides_comptes() {
        // Les lignes vides finales sont des versets absents, pas du remplissage.
        let p = parse_translation("Un\n\n\n", 3, SHARE).expect("trois lignes");
        assert_eq!(p.status, vec![Present, Absent, Absent]);
    }

    #[test]
    fn nombre_de_lignes_verifie() {
        assert_eq!(
            parse_translation("Un\nDeux\n", 3, SHARE),
            Err(ParseError::LineCount {
                expected: 3,
                found: 2
            })
        );
        assert_eq!(
            parse_translation("", 3, SHARE),
            Err(ParseError::LineCount {
                expected: 3,
                found: 0
            })
        );
    }

    #[test]
    fn lignes_non_textuelles() {
        assert!(looks_like_text("Au commencement, Dieu créa.", SHARE));
        assert!(!looks_like_text("12 : 34 — 56", SHARE));
        assert!(!looks_like_text("\\v 3 texte balisé", SHARE));
        assert!(!looks_like_text("<note>texte</note>", SHARE));
        let p = parse_translation("Bon texte\n1 2 3 4\n", 2, SHARE).expect("deux lignes");
        assert_eq!(p.non_text_lines, 1);
    }
}
