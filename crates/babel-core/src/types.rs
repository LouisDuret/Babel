//! Identifiants compacts utilisés par tout le pipeline.

use std::fmt;

/// Jeton eBible d'un verset fusionné avec le précédent.
pub const RANGE_TOKEN: &str = "<range>";

/// Index de ligne dans `vref.txt`. Le corpus compte moins de 65 536 lignes,
/// donc un `u16` suffit ; la versification refuse tout fichier plus long.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VerseId(pub u16);

impl VerseId {
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

impl fmt::Display for VerseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Index d'une traduction dans le corpus ingéré (ordre alphabétique des
/// fichiers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TranslationId(pub u16);

impl TranslationId {
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// État d'un verset dans une traduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum VerseStatus {
    /// Ligne vide : le verset est absent de la traduction.
    Absent = 0,
    /// Le verset a son propre texte.
    Present = 1,
    /// Premier verset d'une plage fusionnée : sa ligne porte le texte de
    /// toute la plage.
    RangeHead = 2,
    /// Verset suivant d'une plage fusionnée (jeton `<range>`).
    RangeContinuation = 3,
}

impl VerseStatus {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(VerseStatus::Absent),
            1 => Some(VerseStatus::Present),
            2 => Some(VerseStatus::RangeHead),
            3 => Some(VerseStatus::RangeContinuation),
            _ => None,
        }
    }

    /// Le verset est couvert par la traduction (texte propre ou fusionné).
    pub fn is_covered(self) -> bool {
        !matches!(self, VerseStatus::Absent)
    }

    /// Le verset a un texte à lui seul : seul cas retenu par les signaux de
    /// longueur et de sémantique.
    pub fn has_own_text(self) -> bool {
        matches!(self, VerseStatus::Present)
    }

    /// Compte pour le signal d'omission : vide ou fusionné.
    pub fn counts_as_omitted(self) -> bool {
        matches!(self, VerseStatus::Absent | VerseStatus::RangeContinuation)
    }
}

/// Codes des signaux, dans l'ordre d'affichage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SignalKind {
    Omit,
    Len,
    Intra,
    Align,
    Sem,
}

impl SignalKind {
    /// Signaux calculés par le prototype (phase 1).
    pub const PHASE1: [SignalKind; 3] = [SignalKind::Omit, SignalKind::Len, SignalKind::Intra];
    pub const ALL: [SignalKind; 5] = [
        SignalKind::Omit,
        SignalKind::Len,
        SignalKind::Intra,
        SignalKind::Align,
        SignalKind::Sem,
    ];

    pub fn code(self) -> &'static str {
        match self {
            SignalKind::Omit => "omit",
            SignalKind::Len => "len",
            SignalKind::Intra => "intra",
            SignalKind::Align => "align",
            SignalKind::Sem => "sem",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        SignalKind::ALL.into_iter().find(|s| s.code() == code)
    }

    pub fn label_fr(self) -> &'static str {
        match self {
            SignalKind::Omit => "Omission",
            SignalKind::Len => "Longueur",
            SignalKind::Intra => "Intra-langue",
            SignalKind::Align => "Alignement",
            SignalKind::Sem => "Sémantique",
        }
    }
}
