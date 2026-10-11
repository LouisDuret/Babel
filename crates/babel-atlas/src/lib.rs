//! L'atlas : tout ce que le viewer charge au démarrage, dans un seul
//! fichier binaire (`atlas.bin`, format rkyv).
//!
//! Il contient la versification, les signaux et leurs rangs, les
//! dispositions Mur et Hilbert, les métadonnées des traductions et l'index
//! des blocs de textes.

pub mod layout;
pub mod texts;

use babel_core::Versification;
use babel_core::config::Atlas as AtlasConfig;
use layout::{BookShape, Hilbert, Wall};
use rkyv::rancor;
use rkyv::util::AlignedVec;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Signature en tête de fichier.
const MAGIC: &[u8; 8] = b"BABELATL";
/// Version du format. À incrémenter à chaque changement des structures.
pub const FORMAT_VERSION: u32 = 1;
/// Taille de l'en-tête : signature, version, réserve. Multiple de 16 pour
/// garder la charge utile alignée.
const HEADER_LEN: usize = 16;

#[derive(Debug, Error)]
pub enum AtlasError {
    #[error("{path} : {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{0}")]
    Invalid(String),
    #[error("atlas illisible : {0}")]
    Format(String),
}

impl AtlasError {
    pub fn io(path: &Path, source: std::io::Error) -> Self {
        AtlasError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct AtlasChapter {
    pub number: u16,
    pub first: u32,
    pub len: u32,
}

#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct AtlasBook {
    pub code: String,
    pub name_fr: String,
    pub name_en: String,
    /// Index dans `Atlas::sections`.
    pub section: u8,
    /// 0 = AT, 1 = NT, 2 = deutérocanoniques.
    pub testament: u8,
    pub first: u32,
    pub len: u32,
    pub chapters: Vec<AtlasChapter>,
    /// Rectangle englobant du livre dans la vue Mur : x, y, largeur, hauteur.
    pub wall_rect: [f32; 4],
    /// Ancre du nom du livre dans la vue Mur : coin haut gauche de l'en-tête.
    pub wall_label: [f32; 2],
    /// Centre du livre dans la vue Hilbert.
    pub hilbert_centroid: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct AtlasVerse {
    pub book: u16,
    /// Index du chapitre dans son livre.
    pub chapter_index: u16,
    pub chapter: u16,
    pub verse: u16,
}

#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct AtlasSignal {
    /// Code court : `omit`, `len`, `intra`.
    pub code: String,
    pub label: String,
    /// Valeur brute par verset (NaN si indéfinie).
    pub raw: Vec<f32>,
    /// Rang percentile par verset, dans [0, 1[ (NaN si indéfini).
    pub rank: Vec<f32>,
    /// Poids par défaut dans le composite.
    pub weight: f32,
}

#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct AtlasTranslation {
    pub stem: String,
    pub language: String,
    pub language_name: String,
    pub title: String,
    pub script: String,
    pub family: String,
    pub license: String,
    pub redistributable: bool,
    /// Année, 0 si inconnue.
    pub year: u16,
    pub text_base: String,
    pub excluded: bool,
    pub eligible_books: u16,
    pub weight: f32,
}

/// Bloc compressé de `texts.zst` : les versets d'un livre dans une
/// traduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct TextBlock {
    /// Index dans `Atlas::translations`.
    pub translation: u16,
    pub book: u16,
    pub offset: u64,
    pub len: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct SectionLabel {
    pub section: u8,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Atlas {
    /// Empreinte des entrées (corpus, signaux, configuration).
    pub fingerprint: String,
    /// Noms des sections du canon.
    pub sections: Vec<String>,
    pub books: Vec<AtlasBook>,
    pub verses: Vec<AtlasVerse>,
    pub signals: Vec<AtlasSignal>,
    /// Nombre de traductions éligibles par verset.
    pub n_eligible: Vec<u16>,
    pub translations: Vec<AtlasTranslation>,
    /// Traductions de référence dont le texte est dans `texts.zst` (index
    /// dans `translations`), dans l'ordre d'affichage.
    pub references: Vec<u16>,
    pub text_blocks: Vec<TextBlock>,
    /// Vue Mur : position de chaque verset, en cellules.
    pub wall_positions: Vec<[f32; 2]>,
    pub wall_size: [f32; 2],
    pub wall_sections: Vec<SectionLabel>,
    /// Retour à la ligne des chapitres dans la vue Mur.
    pub wall_wrap: u32,
    /// Rangées réservées au nom de chaque livre dans la vue Mur.
    pub wall_header_rows: u32,
    /// Vue Hilbert.
    pub hilbert_positions: Vec<[f32; 2]>,
    pub hilbert_order: u32,
}

impl Atlas {
    pub fn n_verses(&self) -> usize {
        self.verses.len()
    }

    /// Traductions qui participent aux signaux.
    pub fn n_active_translations(&self) -> usize {
        self.translations
            .iter()
            .filter(|t| !t.excluded && t.eligible_books > 0)
            .count()
    }

    pub fn n_languages(&self) -> usize {
        let mut languages: Vec<&str> = self
            .translations
            .iter()
            .filter(|t| !t.excluded && t.eligible_books > 0)
            .map(|t| t.language.as_str())
            .collect();
        languages.sort_unstable();
        languages.dedup();
        languages.len()
    }

    /// Reconstruit la versification (pour la recherche de références).
    pub fn versification(&self) -> Result<Versification, AtlasError> {
        let mut vref = String::new();
        for v in &self.verses {
            let book = self
                .books
                .get(usize::from(v.book))
                .ok_or_else(|| AtlasError::Invalid("verset rattaché à un livre inconnu".into()))?;
            vref.push_str(&format!("{} {}:{}\n", book.code, v.chapter, v.verse));
        }
        Versification::parse(&vref).map_err(|e| AtlasError::Invalid(e.to_string()))
    }

    /// Référence en français d'un verset : « Jean 3:16 ».
    pub fn label_fr(&self, verse: usize) -> String {
        match self.verses.get(verse) {
            Some(v) => {
                let name = self
                    .books
                    .get(usize::from(v.book))
                    .map_or("?", |b| b.name_fr.as_str());
                format!("{name} {}:{}", v.chapter, v.verse)
            }
            None => String::new(),
        }
    }
}

/// Un signal à embarquer dans l'atlas.
#[derive(Debug, Clone, PartialEq)]
pub struct SignalInput {
    pub code: String,
    pub label: String,
    pub raw: Vec<f32>,
    pub rank: Vec<f32>,
    pub weight: f32,
}

/// Entrées de la construction de l'atlas.
#[derive(Debug, Clone)]
pub struct AtlasInput<'a> {
    pub versification: &'a Versification,
    pub fingerprint: String,
    pub signals: Vec<SignalInput>,
    pub n_eligible: Vec<u16>,
    pub translations: Vec<AtlasTranslation>,
    pub references: Vec<u16>,
    pub text_blocks: Vec<TextBlock>,
}

/// Construit l'atlas : calcule les dispositions et assemble les données.
pub fn build(input: AtlasInput<'_>, config: &AtlasConfig) -> Result<Atlas, AtlasError> {
    let v = input.versification;
    let n = v.len();
    for signal in &input.signals {
        if signal.raw.len() != n || signal.rank.len() != n {
            return Err(AtlasError::Invalid(format!(
                "le signal {} n'a pas une valeur par verset",
                signal.code
            )));
        }
    }
    if input.n_eligible.len() != n {
        return Err(AtlasError::Invalid(
            "n_eligible n'a pas une valeur par verset".into(),
        ));
    }

    let shapes: Vec<BookShape> = v
        .books()
        .iter()
        .map(|b| BookShape {
            section: b.section.index() as u8,
            chapters: b.chapters.iter().map(|c| c.len).collect(),
        })
        .collect();
    let wall: Wall = layout::wall(&shapes, config);
    let lengths: Vec<u32> = v.books().iter().map(|b| b.len).collect();
    let hilbert: Hilbert = layout::hilbert(&lengths, config.hilbert_order).ok_or_else(|| {
        AtlasError::Invalid(format!(
            "atlas.hilbert_order = {} : grille trop petite pour {n} versets",
            config.hilbert_order
        ))
    })?;

    let books = v
        .books()
        .iter()
        .enumerate()
        .map(|(i, b)| AtlasBook {
            code: b.code.clone(),
            name_fr: b.name_fr.clone(),
            name_en: b.name_en.clone(),
            section: b.section.index() as u8,
            testament: b.testament() as u8,
            first: b.first,
            len: b.len,
            chapters: b
                .chapters
                .iter()
                .map(|c| AtlasChapter {
                    number: c.number,
                    first: c.first,
                    len: c.len,
                })
                .collect(),
            wall_rect: [
                wall.books[i].x,
                wall.books[i].y,
                wall.books[i].w,
                wall.books[i].h,
            ],
            wall_label: wall.labels[i],
            hilbert_centroid: hilbert.centroids[i],
        })
        .collect();

    let verses = (0..n)
        .filter_map(|i| {
            let r = v.verse(babel_core::VerseId(i as u16))?;
            Some(AtlasVerse {
                book: r.book,
                chapter_index: v.chapter_of(i) as u16,
                chapter: r.chapter,
                verse: r.verse,
            })
        })
        .collect();

    Ok(Atlas {
        fingerprint: input.fingerprint,
        sections: babel_core::Section::ALL
            .iter()
            .map(|s| s.name_fr().to_string())
            .collect(),
        books,
        verses,
        signals: input
            .signals
            .into_iter()
            .map(|s| AtlasSignal {
                code: s.code,
                label: s.label,
                raw: s.raw,
                rank: s.rank,
                weight: s.weight,
            })
            .collect(),
        n_eligible: input.n_eligible,
        translations: input.translations,
        references: input.references,
        text_blocks: input.text_blocks,
        wall_positions: wall.positions,
        wall_size: wall.size,
        wall_sections: wall
            .sections
            .iter()
            .map(|s| SectionLabel {
                section: s.section,
                x: s.x,
                y: s.y,
            })
            .collect(),
        wall_wrap: config.wall_wrap as u32,
        wall_header_rows: config.wall_header_rows as u32,
        hilbert_positions: hilbert.positions,
        hilbert_order: hilbert.order,
    })
}

/// Sérialise l'atlas : en-tête de 16 octets puis charge utile rkyv.
pub fn to_bytes(atlas: &Atlas) -> Result<Vec<u8>, AtlasError> {
    let payload =
        rkyv::to_bytes::<rancor::Error>(atlas).map_err(|e| AtlasError::Format(e.to_string()))?;
    let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&[0_u8; 4]);
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Relit un atlas, en validant son contenu.
pub fn from_bytes(bytes: &[u8]) -> Result<Atlas, AtlasError> {
    if bytes.len() < HEADER_LEN || &bytes[..MAGIC.len()] != MAGIC {
        return Err(AtlasError::Format(
            "ce fichier n'est pas un atlas Babel".into(),
        ));
    }
    let version = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    if version != FORMAT_VERSION {
        return Err(AtlasError::Format(format!(
            "version {version} du format, {FORMAT_VERSION} attendue : relancer `babel atlas`"
        )));
    }
    // La charge utile doit être alignée pour être validée et relue.
    let mut aligned: AlignedVec = AlignedVec::with_capacity(bytes.len() - HEADER_LEN);
    aligned.extend_from_slice(&bytes[HEADER_LEN..]);
    rkyv::from_bytes::<Atlas, rancor::Error>(&aligned)
        .map_err(|e| AtlasError::Format(e.to_string()))
}

pub fn write(atlas: &Atlas, path: &Path) -> Result<(), AtlasError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AtlasError::io(path, e))?;
    }
    std::fs::write(path, to_bytes(atlas)?).map_err(|e| AtlasError::io(path, e))
}

pub fn read(path: &Path) -> Result<Atlas, AtlasError> {
    let bytes = std::fs::read(path).map_err(|e| AtlasError::io(path, e))?;
    from_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use babel_core::Config;

    fn sample() -> Atlas {
        let vref = "GEN 1:1\nGEN 1:2\nGEN 2:1\nMAT 1:1\nMAT 1:2\n";
        let v = Versification::parse(vref).expect("vref valide");
        let config = Config::embedded().expect("configuration valide").atlas;
        let input = AtlasInput {
            versification: &v,
            fingerprint: "abc".into(),
            signals: vec![SignalInput {
                code: "omit".into(),
                label: "Omission".into(),
                raw: vec![0.0, 0.5, f32::NAN, 0.1, 0.0],
                rank: vec![0.25, 0.9, f32::NAN, 0.6, 0.25],
                weight: 1.0,
            }],
            n_eligible: vec![5, 5, 0, 7, 7],
            translations: vec![AtlasTranslation {
                stem: "fra-fraLSG".into(),
                language: "fra".into(),
                language_name: "French".into(),
                title: "Louis Segond 1910".into(),
                script: "Latin".into(),
                family: "Indo-European".into(),
                license: "Public Domain".into(),
                redistributable: true,
                year: 1910,
                text_base: "recu".into(),
                excluded: false,
                eligible_books: 2,
                weight: 1.0,
            }],
            references: vec![0],
            text_blocks: vec![TextBlock {
                translation: 0,
                book: 0,
                offset: 0,
                len: 10,
            }],
        };
        build(input, &config).expect("atlas")
    }

    /// Compare deux atlas en tenant compte des NaN.
    fn same(a: &Atlas, b: &Atlas) -> bool {
        format!("{a:?}") == format!("{b:?}")
    }

    #[test]
    fn construction() {
        let atlas = sample();
        assert_eq!(atlas.n_verses(), 5);
        assert_eq!(atlas.books.len(), 2);
        assert_eq!(atlas.books[1].code, "MAT");
        assert_eq!(atlas.books[1].testament, 1);
        assert_eq!(
            atlas.sections[usize::from(atlas.books[1].section)],
            "Évangiles"
        );
        assert_eq!(atlas.verses[2].chapter_index, 1);
        assert_eq!(atlas.wall_positions.len(), 5);
        assert_eq!(atlas.hilbert_positions.len(), 5);
        assert_eq!(atlas.label_fr(4), "Matthieu 1:2");
        assert_eq!(atlas.n_active_translations(), 1);
        assert_eq!(atlas.n_languages(), 1);
    }

    #[test]
    fn aller_retour_binaire() {
        let atlas = sample();
        let bytes = to_bytes(&atlas).expect("sérialisation");
        let back = from_bytes(&bytes).expect("relecture");
        assert!(same(&atlas, &back));
        assert_eq!(back.versification().expect("versification").len(), 5);
    }

    #[test]
    fn refuse_un_fichier_etranger_ou_perime() {
        assert!(matches!(
            from_bytes(b"pas un atlas du tout"),
            Err(AtlasError::Format(_))
        ));
        let mut bytes = to_bytes(&sample()).expect("sérialisation");
        bytes[8] = 99;
        let err = from_bytes(&bytes).expect_err("version inconnue");
        assert!(err.to_string().contains("version 99"));
        // Charge utile tronquée : la validation rkyv la refuse.
        let bytes = to_bytes(&sample()).expect("sérialisation");
        assert!(from_bytes(&bytes[..bytes.len() / 2]).is_err());
    }

    #[test]
    fn refuse_un_signal_de_mauvaise_taille() {
        let v = Versification::parse("GEN 1:1\nGEN 1:2\n").expect("vref valide");
        let config = Config::embedded().expect("configuration valide").atlas;
        let input = AtlasInput {
            versification: &v,
            fingerprint: String::new(),
            signals: vec![SignalInput {
                code: "omit".into(),
                label: "Omission".into(),
                raw: vec![0.0],
                rank: vec![0.5],
                weight: 1.0,
            }],
            n_eligible: vec![1, 1],
            translations: Vec::new(),
            references: Vec::new(),
            text_blocks: Vec::new(),
        };
        assert!(matches!(build(input, &config), Err(AtlasError::Invalid(_))));
    }
}
