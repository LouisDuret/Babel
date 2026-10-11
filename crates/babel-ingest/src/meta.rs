//! Métadonnées par traduction : langue, écriture, famille, licence, année,
//! base textuelle inférée.

use babel_core::{TranslationId, VerseStatus};
use std::collections::HashMap;
use std::path::Path;
use unicode_script::{Script, UnicodeScript};

/// Valeur affichée quand une métadonnée manque.
pub const UNKNOWN: &str = "inconnu";

/// Base textuelle du Nouveau Testament suivie par une traduction (§ 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextBase {
    /// Texte reçu ou byzantin : les versets diagnostiques sont présents.
    Received,
    /// Texte critique : les versets diagnostiques sont absents.
    Critical,
    Mixed,
    /// La traduction ne couvre pas assez de versets diagnostiques.
    Unknown,
}

impl TextBase {
    pub const ALL: [TextBase; 4] = [
        TextBase::Received,
        TextBase::Critical,
        TextBase::Mixed,
        TextBase::Unknown,
    ];

    pub fn code(self) -> &'static str {
        match self {
            TextBase::Received => "recu",
            TextBase::Critical => "critique",
            TextBase::Mixed => "mixte",
            TextBase::Unknown => "inconnu",
        }
    }

    pub fn from_code(code: &str) -> Self {
        TextBase::ALL
            .into_iter()
            .find(|b| b.code() == code)
            .unwrap_or(TextBase::Unknown)
    }

    pub fn label_fr(self) -> &'static str {
        match self {
            TextBase::Received => "Texte reçu / byzantin",
            TextBase::Critical => "Texte critique",
            TextBase::Mixed => "Mixte",
            TextBase::Unknown => "Inconnue",
        }
    }
}

/// Infère la base textuelle à partir de l'état des versets diagnostiques.
///
/// `diagnostic` donne l'état de chaque verset diagnostique dans un livre que
/// la traduction couvre. Les versets fusionnés ne disent rien et sont
/// ignorés. Il faut au moins la moitié des `total` versets diagnostiques
/// pour conclure.
pub fn infer_text_base(
    diagnostic: &[VerseStatus],
    total: usize,
    received_min: f32,
    critical_max: f32,
) -> TextBase {
    let informative: Vec<&VerseStatus> = diagnostic
        .iter()
        .filter(|s| matches!(s, VerseStatus::Present | VerseStatus::Absent))
        .collect();
    if informative.is_empty() || informative.len() * 2 < total {
        return TextBase::Unknown;
    }
    let present = informative
        .iter()
        .filter(|s| ***s == VerseStatus::Present)
        .count();
    let share = present as f32 / informative.len() as f32;
    if share >= received_min {
        TextBase::Received
    } else if share <= critical_max {
        TextBase::Critical
    } else {
        TextBase::Mixed
    }
}

/// Écriture dominante d'un texte, détectée par les propriétés Unicode.
/// Les caractères communs à toutes les écritures (ponctuation, chiffres)
/// sont ignorés.
pub fn dominant_script(text: &str) -> Option<Script> {
    let mut counts: HashMap<Script, u32> = HashMap::new();
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        let script = c.script();
        if !matches!(script, Script::Common | Script::Inherited | Script::Unknown) {
            *counts.entry(script).or_insert(0) += 1;
        }
    }
    // À égalité, l'ordre alphabétique du nom départage, pour rester
    // déterministe.
    counts
        .into_iter()
        .max_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| b.0.full_name().cmp(a.0.full_name()))
        })
        .map(|(script, _)| script)
}

/// Première année plausible (1000 à 2100) lue dans une mention de copyright.
pub fn extract_year(text: &str) -> Option<u16> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i - start == 4
                && let Ok(year) = text[start..i].parse::<u16>()
                && (1000..=2100).contains(&year)
            {
                return Some(year);
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Métadonnées d'une traduction ingérée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationMeta {
    pub id: TranslationId,
    /// Nom du fichier eBible sans extension : `fra-fraLSG`.
    pub stem: String,
    /// Code ISO 639-3, préfixe du nom de fichier.
    pub language: String,
    pub language_name: String,
    pub title: String,
    /// Nom Unicode de l'écriture dominante (`Latin`, `Hebrew`…).
    pub script: String,
    pub family: String,
    pub license: String,
    /// La licence autorise l'affichage du texte dans une application
    /// partagée (mode publication).
    pub redistributable: bool,
    pub year: Option<u16>,
    pub text_base: TextBase,
    /// Empreinte blake3 du fichier source, en hexadécimal.
    pub fingerprint: String,
    pub non_text_lines: u32,
}

/// Sépare `fra-fraLSG` en (`fra`, `fraLSG`).
pub fn split_stem(stem: &str) -> (&str, &str) {
    stem.split_once('-').unwrap_or((stem, stem))
}

/// Ce que les tables de métadonnées d'eBible disent d'une traduction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Described {
    pub language_name: Option<String>,
    pub title: Option<String>,
    pub family: Option<String>,
    pub license: Option<String>,
    pub redistributable: Option<bool>,
    pub year: Option<u16>,
}

#[derive(Debug, Clone, Default)]
struct CsvEntry {
    language_name: String,
    title: String,
    redistributable: bool,
    copyright: String,
}

/// Tables `translations.csv`, `lang_details.tsv` et `licences.tsv` du dossier
/// `metadata/` d'eBible. Chaque table est facultative.
#[derive(Debug, Clone, Default)]
pub struct MetadataTables {
    /// Par identifiant de traduction (`fraLSG`).
    translations: HashMap<String, CsvEntry>,
    /// Par nom de fichier (`fra-fraLSG.txt`) : (nom de langue, famille).
    languages: HashMap<String, (String, String)>,
    /// Par identifiant de traduction : (type de licence, années).
    licences: HashMap<String, (String, String)>,
}

impl MetadataTables {
    /// Charge les tables présentes dans `dir`. Les fichiers absents ou
    /// illisibles sont ignorés : les métadonnées sont alors « inconnu ».
    pub fn load(dir: &Path) -> Self {
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
        Self {
            translations: read("translations.csv")
                .map(|t| parse_translations_csv(&t))
                .unwrap_or_default(),
            languages: read("lang_details.tsv")
                .map(|t| parse_lang_details(&t))
                .unwrap_or_default(),
            licences: read("licences.tsv")
                .map(|t| parse_licences(&t))
                .unwrap_or_default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.translations.is_empty() && self.languages.is_empty() && self.licences.is_empty()
    }

    /// Rassemble ce que les tables savent de la traduction `stem`.
    pub fn describe(&self, stem: &str) -> Described {
        let (_, project) = split_stem(stem);
        // Les noms de fichiers remplacent « - » par « _ » dans l'identifiant.
        let dashed = project.replace('_', "-");
        let csv = self
            .translations
            .get(project)
            .or_else(|| self.translations.get(&dashed));
        let licence = self
            .licences
            .get(project)
            .or_else(|| self.licences.get(&dashed));
        let language = self.languages.get(&format!("{stem}.txt"));

        let non_empty = |s: &String| (!s.trim().is_empty()).then(|| s.trim().to_string());
        Described {
            language_name: csv
                .and_then(|c| non_empty(&c.language_name))
                .or_else(|| language.and_then(|l| non_empty(&l.0))),
            title: csv.and_then(|c| non_empty(&c.title)),
            family: language.and_then(|l| non_empty(&l.1)),
            license: licence.and_then(|l| non_empty(&l.0)),
            redistributable: csv.map(|c| c.redistributable),
            year: licence
                .and_then(|l| extract_year(&l.1))
                .or_else(|| csv.and_then(|c| extract_year(&c.copyright))),
        }
    }
}

fn csv_reader(text: &str, delimiter: u8, quoting: bool) -> csv::Reader<&[u8]> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .quoting(quoting)
        .flexible(true)
        .from_reader(text.as_bytes())
}

/// Index des colonnes nommées dans l'en-tête.
fn columns<const N: usize>(headers: &csv::StringRecord, names: [&str; N]) -> [Option<usize>; N] {
    names.map(|name| headers.iter().position(|h| h.trim() == name))
}

fn field(record: &csv::StringRecord, column: Option<usize>) -> String {
    column
        .and_then(|i| record.get(i))
        .unwrap_or("")
        .trim()
        .to_string()
}

fn parse_translations_csv(text: &str) -> HashMap<String, CsvEntry> {
    let mut reader = csv_reader(text, b',', true);
    let Ok(headers) = reader.headers().cloned() else {
        return HashMap::new();
    };
    let [id, name, title, redistributable, copyright] = columns(
        &headers,
        [
            "translationId",
            "languageNameInEnglish",
            "title",
            "Redistributable",
            "Copyright",
        ],
    );
    let mut map = HashMap::new();
    for record in reader.records().flatten() {
        let key = field(&record, id);
        if key.is_empty() {
            continue;
        }
        let entry = CsvEntry {
            language_name: field(&record, name),
            title: field(&record, title),
            redistributable: field(&record, redistributable).eq_ignore_ascii_case("true"),
            copyright: field(&record, copyright),
        };
        // Le fichier `eng-eng_kjv.txt` correspond à l'identifiant `eng-kjv`.
        if key.contains('-') {
            map.insert(key.replace('-', "_"), entry.clone());
        }
        map.insert(key, entry);
    }
    map
}

fn parse_lang_details(text: &str) -> HashMap<String, (String, String)> {
    let mut reader = csv_reader(text, b'\t', false);
    let Ok(headers) = reader.headers().cloned() else {
        return HashMap::new();
    };
    let [file, lang, family] = columns(&headers, ["file", "lang", "family"]);
    reader
        .records()
        .flatten()
        .map(|r| (field(&r, file), (field(&r, lang), field(&r, family))))
        .filter(|(key, _)| !key.is_empty())
        .collect()
}

fn parse_licences(text: &str) -> HashMap<String, (String, String)> {
    let mut reader = csv_reader(text, b'\t', false);
    let Ok(headers) = reader.headers().cloned() else {
        return HashMap::new();
    };
    let [id, kind, years] = columns(&headers, ["ID", "Licence Type", "Copyright Years"]);
    reader
        .records()
        .flatten()
        .map(|r| (field(&r, id), (field(&r, kind), field(&r, years))))
        .filter(|(key, _)| !key.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use VerseStatus::{Absent, Present, RangeContinuation};

    #[test]
    fn base_textuelle() {
        let infer = |d: &[VerseStatus]| infer_text_base(d, 4, 0.75, 0.25);
        assert_eq!(
            infer(&[Present, Present, Present, Present]),
            TextBase::Received
        );
        assert_eq!(infer(&[Absent, Absent, Absent, Absent]), TextBase::Critical);
        assert_eq!(infer(&[Present, Present, Absent, Absent]), TextBase::Mixed);
        assert_eq!(
            infer(&[Present, Absent, Absent, Absent]),
            TextBase::Critical
        );
        // Trop peu de versets informatifs pour conclure.
        assert_eq!(infer(&[Present]), TextBase::Unknown);
        assert_eq!(
            infer(&[RangeContinuation, RangeContinuation, RangeContinuation]),
            TextBase::Unknown
        );
        assert_eq!(infer(&[]), TextBase::Unknown);
    }

    #[test]
    fn ecriture_dominante() {
        assert_eq!(
            dominant_script("Au commencement, Dieu créa"),
            Some(Script::Latin)
        );
        assert_eq!(dominant_script("Ἐν ἀρχῇ ἦν ὁ λόγος"), Some(Script::Greek));
        assert_eq!(dominant_script("בְּרֵאשִׁית בָּרָא אֱלֹהִים"), Some(Script::Hebrew));
        assert_eq!(
            dominant_script("В начале сотворил Бог (Gen 1)"),
            Some(Script::Cyrillic)
        );
        assert_eq!(dominant_script("12:34 — …"), None);
    }

    #[test]
    fn annee() {
        assert_eq!(extract_year("copyright © 1990"), Some(1990));
        assert_eq!(
            extract_year("Copyright © 1999-2014 Unit Bahasa"),
            Some(1999)
        );
        assert_eq!(extract_year("public domain"), None);
        assert_eq!(extract_year("ISBN 97815313 et 12345"), None);
    }

    #[test]
    fn tables_ebible() {
        let csv = "\u{feff}\"languageCode\",\"translationId\",\"languageNameInEnglish\",\"title\",\"Redistributable\",\"Copyright\"\n\
                   \"fra\",\"fraLSG\",\"French\",\"Louis Segond 1910\",\"True\",\"public domain\"\n\
                   \"eng\",\"eng-kjv\",\"English\",\"King James, Version\",\"False\",\"Copyright © 1769\"\n";
        let lang = "file\tcode\tlang\tfamily\tcountry\nfra-fraLSG.txt\tfra\tFrench\tIndo-European\tFrance\n";
        let lic = "ID\tFile\tLanguage\tDialect\tVernacular Title\tLicence Type\tLicence Version\tCC Licence Link\tCopyright Holder\tCopyright Years\n\
                   fraLSG\tx\tFrench\t\t\tPublic Domain\t\t\t\tcopyright © 1910\n";
        let tables = MetadataTables {
            translations: parse_translations_csv(csv),
            languages: parse_lang_details(lang),
            licences: parse_licences(lic),
        };
        let d = tables.describe("fra-fraLSG");
        assert_eq!(d.language_name.as_deref(), Some("French"));
        assert_eq!(d.title.as_deref(), Some("Louis Segond 1910"));
        assert_eq!(d.family.as_deref(), Some("Indo-European"));
        assert_eq!(d.license.as_deref(), Some("Public Domain"));
        assert_eq!(d.redistributable, Some(true));
        assert_eq!(d.year, Some(1910));

        // Identifiant avec tiret, fichier avec tiret bas ; virgule entre guillemets.
        let d = tables.describe("eng-eng_kjv");
        assert_eq!(d.title.as_deref(), Some("King James, Version"));
        assert_eq!(d.redistributable, Some(false));
        assert_eq!(d.year, Some(1769));

        assert_eq!(tables.describe("xxx-xxx"), Described::default());
    }
}
