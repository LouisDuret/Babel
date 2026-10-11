//! Mini-corpus synthétique pour les tests des signaux : trois langues de
//! trois traductions, soixante versets, et trois divergences injectées à la
//! main dont le classement attendu est connu.

#![allow(dead_code)]

use babel_core::Config;
use babel_ingest::{Corpus, corpus_from_texts};

pub const N_VERSES: usize = 60;
pub const LANGUAGES: [&str; 3] = ["aaa", "bbb", "ccc"];
pub const PER_LANGUAGE: usize = 3;

/// Verset absent de quatre traductions sur neuf.
pub const OMITTED_VERSE: usize = 10;
/// Verset dont la longueur varie de un à neuf selon la langue.
pub const LENGTH_VERSE: usize = 20;
/// Verset que les traductions d'une même langue rendent sans un mot commun.
pub const INTRA_VERSE: usize = 30;

pub fn config() -> Config {
    Config::embedded().expect("configuration du dépôt valide")
}

pub fn vref() -> String {
    (0..N_VERSES)
        .map(|v| format!("GEN {}:{}\n", v / 30 + 1, v % 30 + 1))
        .collect()
}

/// Générateur congruentiel : un mot pseudo-aléatoire stable par clé.
fn word(key: u64) -> String {
    const SYLLABLES: [&str; 16] = [
        "ka", "mi", "to", "ra", "se", "lu", "no", "pi", "da", "ve", "zo", "fu", "ge", "ha", "ti",
        "bo",
    ];
    let mut state = key
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    // Deux à quatre syllabes : la longueur des versets varie un peu d'une
    // traduction à l'autre, comme dans un vrai corpus.
    let syllables = 2 + (state >> 40) % 3;
    let mut out = String::new();
    for _ in 0..syllables {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        out.push_str(SYLLABLES[(state >> 60) as usize]);
    }
    out
}

fn key(language: usize, translation: usize, verse: usize, position: usize) -> u64 {
    ((language as u64) << 48)
        | ((translation as u64) << 32)
        | ((verse as u64) << 16)
        | position as u64
}

/// Marqueur de traduction « partagé par la langue » dans [`key`].
const SHARED: usize = 0xff;

/// Texte du verset pour la traduction `translation` de la langue `language`.
///
/// Deux mots sur trois sont communs à la langue, le troisième est propre à
/// la traduction : les traductions d'une langue se ressemblent sans être
/// redondantes.
pub fn verse_text(language: usize, translation: usize, verse: usize, seed: u64) -> String {
    let n_words = 6 + (verse * 5) % 7;
    let mut words: Vec<String> = (0..n_words)
        .map(|k| {
            let own = k % 3 == 2 || verse == INTRA_VERSE;
            let who = if own { translation } else { SHARED };
            word(key(language, who, verse, k) ^ seed)
        })
        .collect();
    if verse == LENGTH_VERSE {
        // Même facteur pour toute la langue : le chrF intra-langue ne bouge
        // pas, seule la longueur relative entre langues change.
        let repeat = [1, 3, 9][language % 3];
        let once = words.clone();
        words = (0..repeat).flat_map(|_| once.iter().cloned()).collect();
    }
    words.join(" ")
}

/// Contenu des neuf fichiers de traduction : (nom, texte).
pub fn translations(seed: u64) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (l, language) in LANGUAGES.iter().enumerate() {
        for t in 0..PER_LANGUAGE {
            let index = l * PER_LANGUAGE + t;
            let mut content = String::new();
            for v in 0..N_VERSES {
                // Omission : les quatre premières traductions n'ont pas le verset.
                if !(v == OMITTED_VERSE && index < 4) {
                    content.push_str(&verse_text(l, t, v, seed));
                }
                content.push('\n');
            }
            out.push((format!("{language}-t{t}"), content));
        }
    }
    out
}

pub fn build(translations: &[(String, String)]) -> Corpus {
    let refs: Vec<(&str, &str)> = translations
        .iter()
        .map(|(s, t)| (s.as_str(), t.as_str()))
        .collect();
    let loaded = corpus_from_texts(&vref(), &refs, &config().ingest).expect("corpus synthétique");
    assert!(loaded.rejected.is_empty());
    loaded.corpus
}

pub fn corpus(seed: u64) -> Corpus {
    build(&translations(seed))
}

/// Index de la plus grande valeur finie.
pub fn argmax(values: &[f32]) -> usize {
    values
        .iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, _)| i)
        .expect("au moins une valeur finie")
}
