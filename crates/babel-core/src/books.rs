//! Table des livres : codes USFM, noms français et anglais, abréviations,
//! section du canon et testament.

use crate::text::fold;

/// Testament auquel appartient un livre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Testament {
    Old,
    New,
    Deuterocanon,
}

impl Testament {
    pub const ALL: [Testament; 3] = [Testament::Old, Testament::New, Testament::Deuterocanon];

    /// Libellé court affiché dans l'interface.
    pub fn label_fr(self) -> &'static str {
        match self {
            Testament::Old => "AT",
            Testament::New => "NT",
            Testament::Deuterocanon => "DC",
        }
    }

    pub fn name_fr(self) -> &'static str {
        match self {
            Testament::Old => "Ancien Testament",
            Testament::New => "Nouveau Testament",
            Testament::Deuterocanon => "Deutérocanoniques",
        }
    }
}

/// Section du canon : une bande de la vue Mur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Section {
    Pentateuch,
    Historical,
    Poetic,
    Prophets,
    Gospels,
    Acts,
    Epistles,
    Revelation,
    Deuterocanon,
    Other,
}

impl Section {
    pub const ALL: [Section; 10] = [
        Section::Pentateuch,
        Section::Historical,
        Section::Poetic,
        Section::Prophets,
        Section::Gospels,
        Section::Acts,
        Section::Epistles,
        Section::Revelation,
        Section::Deuterocanon,
        Section::Other,
    ];

    pub fn name_fr(self) -> &'static str {
        match self {
            Section::Pentateuch => "Pentateuque",
            Section::Historical => "Historiques",
            Section::Poetic => "Poétiques",
            Section::Prophets => "Prophètes",
            Section::Gospels => "Évangiles",
            Section::Acts => "Actes",
            Section::Epistles => "Épîtres",
            Section::Revelation => "Apocalypse",
            Section::Deuterocanon => "Deutérocanoniques",
            Section::Other => "Autres",
        }
    }

    pub fn testament(self) -> Testament {
        match self {
            Section::Pentateuch | Section::Historical | Section::Poetic | Section::Prophets => {
                Testament::Old
            }
            Section::Gospels | Section::Acts | Section::Epistles | Section::Revelation => {
                Testament::New
            }
            Section::Deuterocanon | Section::Other => Testament::Deuterocanon,
        }
    }

    pub fn index(self) -> usize {
        Section::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

/// Description statique d'un livre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookInfo {
    /// Code USFM à trois caractères, tel qu'il apparaît dans `vref.txt`.
    pub usfm: &'static str,
    pub name_fr: &'static str,
    pub name_en: &'static str,
    /// Abréviations reconnues par la recherche, séparées par des espaces
    /// (les espaces internes d'une abréviation sont omis : « 1co »).
    pub aliases: &'static str,
    pub section: Section,
}

impl BookInfo {
    pub fn testament(&self) -> Testament {
        self.section.testament()
    }
}

macro_rules! book {
    ($usfm:literal, $fr:literal, $en:literal, $aliases:literal, $section:ident) => {
        BookInfo {
            usfm: $usfm,
            name_fr: $fr,
            name_en: $en,
            aliases: $aliases,
            section: Section::$section,
        }
    };
}

/// Livres connus, dans l'ordre de `vref.txt` (AT, NT, deutérocanoniques).
pub const BOOKS: &[BookInfo] = &[
    book!("GEN", "Genèse", "Genesis", "gn ge gen", Pentateuch),
    book!("EXO", "Exode", "Exodus", "ex exo exod", Pentateuch),
    book!("LEV", "Lévitique", "Leviticus", "lv lev le", Pentateuch),
    book!("NUM", "Nombres", "Numbers", "nb nu num nomb", Pentateuch),
    book!(
        "DEU",
        "Deutéronome",
        "Deuteronomy",
        "dt deu deut",
        Pentateuch
    ),
    book!("JOS", "Josué", "Joshua", "jos josh", Historical),
    book!("JDG", "Juges", "Judges", "jg jdg jug judg", Historical),
    book!("RUT", "Ruth", "Ruth", "rt ru rut", Historical),
    book!("1SA", "1 Samuel", "1 Samuel", "1s 1sa 1sam", Historical),
    book!("2SA", "2 Samuel", "2 Samuel", "2s 2sa 2sam", Historical),
    book!("1KI", "1 Rois", "1 Kings", "1r 1ro 1k 1ki 1kgs", Historical),
    book!("2KI", "2 Rois", "2 Kings", "2r 2ro 2k 2ki 2kgs", Historical),
    book!(
        "1CH",
        "1 Chroniques",
        "1 Chronicles",
        "1ch 1chr",
        Historical
    ),
    book!(
        "2CH",
        "2 Chroniques",
        "2 Chronicles",
        "2ch 2chr",
        Historical
    ),
    book!("EZR", "Esdras", "Ezra", "esd ezr", Historical),
    book!("NEH", "Néhémie", "Nehemiah", "ne neh", Historical),
    book!("EST", "Esther", "Esther", "est esth", Historical),
    book!("JOB", "Job", "Job", "jb job", Poetic),
    book!("PSA", "Psaumes", "Psalms", "ps psa psaume psalm", Poetic),
    book!("PRO", "Proverbes", "Proverbs", "pr pro prov", Poetic),
    book!(
        "ECC",
        "Ecclésiaste",
        "Ecclesiastes",
        "ec ecc eccl qo qoh qohelet",
        Poetic
    ),
    book!(
        "SNG",
        "Cantique des cantiques",
        "Song of Songs",
        "ct ca cant sng song sos cantique",
        Poetic
    ),
    book!("ISA", "Ésaïe", "Isaiah", "es is isa esa isaie", Prophets),
    book!("JER", "Jérémie", "Jeremiah", "jr jer", Prophets),
    book!("LAM", "Lamentations", "Lamentations", "lm la lam", Prophets),
    book!("EZK", "Ézéchiel", "Ezekiel", "ez ezk eze ezek", Prophets),
    book!("DAN", "Daniel", "Daniel", "dn da dan", Prophets),
    book!("HOS", "Osée", "Hosea", "os ho hos", Prophets),
    book!("JOL", "Joël", "Joel", "jl jol joe", Prophets),
    book!("AMO", "Amos", "Amos", "am amo", Prophets),
    book!("OBA", "Abdias", "Obadiah", "ab abd ob oba obad", Prophets),
    book!("JON", "Jonas", "Jonah", "jon", Prophets),
    book!("MIC", "Michée", "Micah", "mi mic", Prophets),
    book!("NAM", "Nahum", "Nahum", "na nah nam", Prophets),
    book!("HAB", "Habacuc", "Habakkuk", "ha hab", Prophets),
    book!("ZEP", "Sophonie", "Zephaniah", "so soph zep zeph", Prophets),
    book!("HAG", "Aggée", "Haggai", "ag agg hag", Prophets),
    book!("ZEC", "Zacharie", "Zechariah", "za zac zec zech", Prophets),
    book!("MAL", "Malachie", "Malachi", "ml mal", Prophets),
    book!("MAT", "Matthieu", "Matthew", "mt mat matt", Gospels),
    book!("MRK", "Marc", "Mark", "mc mr mk mrk", Gospels),
    book!("LUK", "Luc", "Luke", "lc lu lk luk", Gospels),
    book!("JHN", "Jean", "John", "jn jhn joh", Gospels),
    book!("ACT", "Actes", "Acts", "ac act", Acts),
    book!("ROM", "Romains", "Romans", "rm ro rom", Epistles),
    book!(
        "1CO",
        "1 Corinthiens",
        "1 Corinthians",
        "1co 1cor",
        Epistles
    ),
    book!(
        "2CO",
        "2 Corinthiens",
        "2 Corinthians",
        "2co 2cor",
        Epistles
    ),
    book!("GAL", "Galates", "Galatians", "ga gal", Epistles),
    book!("EPH", "Éphésiens", "Ephesians", "ep eph", Epistles),
    book!("PHP", "Philippiens", "Philippians", "ph php phil", Epistles),
    book!("COL", "Colossiens", "Colossians", "col", Epistles),
    book!(
        "1TH",
        "1 Thessaloniciens",
        "1 Thessalonians",
        "1th 1thes 1thess",
        Epistles
    ),
    book!(
        "2TH",
        "2 Thessaloniciens",
        "2 Thessalonians",
        "2th 2thes 2thess",
        Epistles
    ),
    book!("1TI", "1 Timothée", "1 Timothy", "1tm 1ti 1tim", Epistles),
    book!("2TI", "2 Timothée", "2 Timothy", "2tm 2ti 2tim", Epistles),
    book!("TIT", "Tite", "Titus", "tt tit", Epistles),
    book!("PHM", "Philémon", "Philemon", "phm phlm philem", Epistles),
    book!("HEB", "Hébreux", "Hebrews", "he heb", Epistles),
    book!("JAS", "Jacques", "James", "jc jac jas jm", Epistles),
    book!("1PE", "1 Pierre", "1 Peter", "1p 1pi 1pe 1pet", Epistles),
    book!("2PE", "2 Pierre", "2 Peter", "2p 2pi 2pe 2pet", Epistles),
    book!("1JN", "1 Jean", "1 John", "1jn 1jo 1joh", Epistles),
    book!("2JN", "2 Jean", "2 John", "2jn 2jo 2joh", Epistles),
    book!("3JN", "3 Jean", "3 John", "3jn 3jo 3joh", Epistles),
    book!("JUD", "Jude", "Jude", "jd jud jude", Epistles),
    book!(
        "REV",
        "Apocalypse",
        "Revelation",
        "ap apo apoc rv re rev",
        Revelation
    ),
    book!("TOB", "Tobie", "Tobit", "tb tob", Deuterocanon),
    book!("JDT", "Judith", "Judith", "jdt jdth", Deuterocanon),
    book!(
        "ESG",
        "Esther grec",
        "Esther (Greek)",
        "esg estg",
        Deuterocanon
    ),
    book!("WIS", "Sagesse", "Wisdom", "sg sag wis", Deuterocanon),
    book!(
        "SIR",
        "Siracide",
        "Sirach",
        "si sir ecclesiastique",
        Deuterocanon
    ),
    book!("BAR", "Baruch", "Baruch", "ba bar", Deuterocanon),
    book!(
        "LJE",
        "Lettre de Jérémie",
        "Letter of Jeremiah",
        "lje ljr",
        Deuterocanon
    ),
    book!(
        "S3Y",
        "Cantique des trois jeunes gens",
        "Song of the Three Young Men",
        "s3y",
        Deuterocanon
    ),
    book!("SUS", "Suzanne", "Susanna", "sus suz", Deuterocanon),
    book!(
        "BEL",
        "Bel et le dragon",
        "Bel and the Dragon",
        "bel",
        Deuterocanon
    ),
    book!(
        "1MA",
        "1 Maccabées",
        "1 Maccabees",
        "1m 1ma 1mac 1macc",
        Deuterocanon
    ),
    book!(
        "2MA",
        "2 Maccabées",
        "2 Maccabees",
        "2m 2ma 2mac 2macc",
        Deuterocanon
    ),
    book!(
        "3MA",
        "3 Maccabées",
        "3 Maccabees",
        "3m 3ma 3mac 3macc",
        Deuterocanon
    ),
    book!(
        "4MA",
        "4 Maccabées",
        "4 Maccabees",
        "4m 4ma 4mac 4macc",
        Deuterocanon
    ),
    book!("1ES", "1 Esdras", "1 Esdras", "1es 1esd", Deuterocanon),
    book!("2ES", "2 Esdras", "2 Esdras", "2es 2esd", Deuterocanon),
    book!(
        "MAN",
        "Prière de Manassé",
        "Prayer of Manasseh",
        "man",
        Deuterocanon
    ),
    book!("PS2", "Psaume 151", "Psalm 151", "ps2 ps151", Deuterocanon),
    book!("ODA", "Odes", "Odes", "oda odes", Deuterocanon),
    book!(
        "PSS",
        "Psaumes de Salomon",
        "Psalms of Solomon",
        "pss",
        Deuterocanon
    ),
    book!(
        "EZA",
        "Apocalypse d'Esdras",
        "Apocalypse of Ezra",
        "eza",
        Deuterocanon
    ),
    book!("JUB", "Jubilés", "Jubilees", "jub", Deuterocanon),
    book!("ENO", "Hénoch", "Enoch", "eno hen", Deuterocanon),
];

/// Retrouve un livre par son code USFM exact (`"GEN"`).
pub fn book_by_usfm(code: &str) -> Option<&'static BookInfo> {
    BOOKS.iter().find(|b| b.usfm == code)
}

/// Informations d'un livre absent de la table : le code sert de nom.
pub fn book_info_or_unknown(code: &str) -> BookInfo {
    book_by_usfm(code).copied().unwrap_or(BookInfo {
        usfm: "???",
        name_fr: "Livre inconnu",
        name_en: "Unknown book",
        aliases: "",
        section: Section::Other,
    })
}

/// Cherche un livre à partir d'un nom ou d'une abréviation, en français ou
/// en anglais. Renvoie l'index dans [`BOOKS`].
///
/// L'ordre de priorité est : code USFM, abréviation exacte, nom exact, puis
/// premier livre dont un nom commence par la requête.
pub fn find_book(query: &str) -> Option<usize> {
    let q = fold(query);
    if q.is_empty() {
        return None;
    }
    let exact = BOOKS.iter().position(|b| {
        fold(b.usfm) == q
            || b.aliases.split(' ').any(|a| a == q)
            || fold(b.name_fr) == q
            || fold(b.name_en) == q
    });
    exact.or_else(|| {
        BOOKS
            .iter()
            .position(|b| fold(b.name_fr).starts_with(&q) || fold(b.name_en).starts_with(&q))
    })
}

/// Tous les livres dont un nom ou une abréviation commence par la requête,
/// pour les suggestions de la recherche.
pub fn suggest_books(query: &str, limit: usize) -> Vec<usize> {
    let q = fold(query);
    if q.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<usize> = Vec::new();
    if let Some(best) = find_book(query) {
        out.push(best);
    }
    for (i, b) in BOOKS.iter().enumerate() {
        if out.len() >= limit {
            break;
        }
        let hit = fold(b.name_fr).starts_with(&q)
            || fold(b.name_en).starts_with(&q)
            || b.aliases.split(' ').any(|a| a.starts_with(&q));
        if hit && !out.contains(&i) {
            out.push(i);
        }
    }
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usfm(query: &str) -> Option<&'static str> {
        find_book(query).map(|i| BOOKS[i].usfm)
    }

    #[test]
    fn abbreviations_francaises_et_anglaises() {
        assert_eq!(usfm("Jn"), Some("JHN"));
        assert_eq!(usfm("Jean"), Some("JHN"));
        assert_eq!(usfm("John"), Some("JHN"));
        assert_eq!(usfm("1 Co"), Some("1CO"));
        assert_eq!(usfm("1 Corinthiens"), Some("1CO"));
        assert_eq!(usfm("Mt"), Some("MAT"));
        assert_eq!(usfm("Mc"), Some("MRK"));
        assert_eq!(usfm("Lc"), Some("LUK"));
        assert_eq!(usfm("Ac"), Some("ACT"));
        assert_eq!(usfm("Rm"), Some("ROM"));
        assert_eq!(usfm("Gn"), Some("GEN"));
        assert_eq!(usfm("Ap"), Some("REV"));
        assert_eq!(usfm("Rev"), Some("REV"));
        assert_eq!(usfm("Ésaïe"), Some("ISA"));
        assert_eq!(usfm("esaie"), Some("ISA"));
        assert_eq!(usfm("Jon"), Some("JON"));
        assert_eq!(usfm("Jude"), Some("JUD"));
        assert_eq!(usfm("Ph"), Some("PHP"));
        assert_eq!(usfm("Phm"), Some("PHM"));
        assert_eq!(usfm("1 Jn"), Some("1JN"));
    }

    #[test]
    fn prefixes() {
        assert_eq!(usfm("Gene"), Some("GEN"));
        assert_eq!(usfm("apoca"), Some("REV"));
        assert_eq!(usfm("xyz"), None);
        assert_eq!(usfm(""), None);
    }

    #[test]
    fn codes_usfm_uniques() {
        for (i, a) in BOOKS.iter().enumerate() {
            for b in &BOOKS[i + 1..] {
                assert_ne!(a.usfm, b.usfm);
            }
        }
    }

    #[test]
    fn abreviations_non_ambigues() {
        // Une même abréviation ne doit pas désigner deux livres.
        for (i, a) in BOOKS.iter().enumerate() {
            for b in &BOOKS[i + 1..] {
                for alias in a.aliases.split(' ') {
                    assert!(
                        !b.aliases.split(' ').any(|x| x == alias),
                        "abréviation « {alias} » partagée par {} et {}",
                        a.usfm,
                        b.usfm
                    );
                }
            }
        }
    }
}
