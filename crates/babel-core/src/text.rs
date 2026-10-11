//! Petites fonctions de texte partagées.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Forme de comparaison d'un nom : minuscules, sans diacritiques, sans
/// espaces ni ponctuation. « 1 Co. » et « 1co » donnent la même clé.
pub fn fold(s: &str) -> String {
    s.nfd()
        .filter(|c| !is_combining_mark(*c))
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Forme de comparaison d'un texte libre : minuscules, sans diacritiques,
/// espaces conservés. Sert à la recherche plein texte du viewer.
pub fn fold_keep_spaces(s: &str) -> String {
    s.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Normalisation Unicode NFC, appliquée à tous les textes à l'ingestion.
pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_retire_accents_et_espaces() {
        assert_eq!(fold("1 Co."), "1co");
        assert_eq!(fold("Ésaïe"), "esaie");
        assert_eq!(fold("  Jean "), "jean");
    }

    #[test]
    fn nfc_compose() {
        // « e » + accent aigu combinant devient « é » précomposé.
        assert_eq!(nfc("e\u{301}"), "\u{e9}");
        assert_eq!(nfc("déjà"), "déjà");
    }
}
