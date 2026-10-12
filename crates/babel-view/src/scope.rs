//! Sélection d'une portée (verset, chapitre, livre, section du canon) et
//! bilan de son contenu.
//!
//! Le bilan est calculé dans le viewer, à partir du composite courant : il
//! suit donc les poids réglés dans le panneau Couches. Tout est pur.

use crate::scene::Scene;
use babel_core::stats::median;
use babel_core::versification::Target;
use std::ops::Range;

/// Ce que l'utilisateur a sélectionné.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Selection {
    Verse(usize),
    /// Chapitre : index du livre, index du chapitre dans le livre.
    Chapter {
        book: usize,
        chapter: usize,
    },
    Book(usize),
    /// Section du canon : index dans `Atlas::sections`.
    Section(usize),
}

impl From<Target> for Selection {
    fn from(target: Target) -> Self {
        match target {
            Target::Verse(id) => Selection::Verse(id.index()),
            Target::Chapter { book, chapter } => Selection::Chapter { book, chapter },
            Target::Book { book } => Selection::Book(book),
        }
    }
}

/// Place d'une sélection dans la hiérarchie : ce que montre le fil d'Ariane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Path {
    pub section: Option<usize>,
    pub book: Option<usize>,
    pub chapter: Option<usize>,
    pub verse: Option<usize>,
}

/// Un élément contenu dans la sélection : un livre d'une section, un
/// chapitre d'un livre, un verset d'un chapitre.
#[derive(Debug, Clone, PartialEq)]
pub struct Child {
    pub selection: Selection,
    pub label: String,
    /// U moyen de l'élément (NaN sans données).
    pub mean: f32,
    pub verses: usize,
}

/// Bilan d'un chapitre, d'un livre ou d'une section.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub books: usize,
    pub chapters: usize,
    pub verses: usize,
    /// Versets qui ont un score.
    pub scored: usize,
    /// Moyenne et médiane de U sur les versets notés (NaN sans données).
    pub mean: f32,
    pub median: f32,
    /// Part des versets notés dont le rang dans la Bible atteint le seuil
    /// « très incertain ».
    pub hot_share: f32,
    /// Rang de la sélection parmi ses pairs, par U moyen décroissant :
    /// (rang à partir de 1, nombre de pairs notés).
    pub rank: Option<(usize, usize)>,
    /// Rang percentile moyen de chaque signal, dans l'ordre de l'atlas.
    pub signals: Vec<f32>,
    /// Médiane du nombre de traductions éligibles.
    pub translations: u16,
    /// Histogramme de U dans la sélection : part des versets notés par
    /// classe, de 0 à 1.
    pub histogram: Vec<f32>,
    /// Le même histogramme pour la Bible entière, comme repère.
    pub baseline: Vec<f32>,
    pub children: Vec<Child>,
    /// Versets les plus incertains de la sélection : (verset, U).
    pub top: Vec<(usize, f32)>,
}

/// Moyenne des valeurs finies d'une plage, et leur nombre.
fn mean_over(values: &[f32], range: Range<usize>) -> (f32, usize) {
    let (mut sum, mut count) = (0.0_f64, 0_usize);
    for value in values.get(range).unwrap_or(&[]) {
        if value.is_finite() {
            sum += f64::from(*value);
            count += 1;
        }
    }
    if count == 0 {
        (f32::NAN, 0)
    } else {
        ((sum / count as f64) as f32, count)
    }
}

/// Histogramme des valeurs finies sur [0, 1], en parts du total.
fn histogram(values: &[f32], bins: usize) -> Vec<f32> {
    let bins = bins.max(1);
    let mut counts = vec![0_usize; bins];
    let mut total = 0_usize;
    for value in values.iter().filter(|v| v.is_finite()) {
        let bin = ((value.clamp(0.0, 1.0) * bins as f32) as usize).min(bins - 1);
        counts[bin] += 1;
        total += 1;
    }
    counts
        .into_iter()
        .map(|c| {
            if total == 0 {
                0.0
            } else {
                c as f32 / total as f32
            }
        })
        .collect()
}

impl Scene {
    /// Livres d'une section : une plage contiguë dans l'ordre du canon.
    pub fn section_books(&self, section: usize) -> Range<usize> {
        let books = &self.atlas.books;
        let is_member = |b: &babel_atlas::AtlasBook| usize::from(b.section) == section;
        match books.iter().position(is_member) {
            Some(first) => {
                let len = books[first..].iter().take_while(|b| is_member(b)).count();
                first..first + len
            }
            None => 0..0,
        }
    }

    /// Plage de versets couverte par une sélection.
    pub fn selection_range(&self, selection: Selection) -> Option<Range<usize>> {
        let books = &self.atlas.books;
        let book_range = |b: &babel_atlas::AtlasBook| b.first as usize..(b.first + b.len) as usize;
        match selection {
            Selection::Verse(verse) => (verse < self.n_verses()).then(|| verse..verse + 1),
            Selection::Chapter { book, chapter } => {
                let c = books.get(book)?.chapters.get(chapter)?;
                Some(c.first as usize..(c.first + c.len) as usize)
            }
            Selection::Book(book) => books.get(book).map(book_range),
            Selection::Section(section) => {
                let members = self.section_books(section);
                let first = books.get(members.start)?;
                let last = books.get(members.end.checked_sub(1)?)?;
                Some(first.first as usize..(last.first + last.len) as usize)
            }
        }
    }

    /// Place de la sélection dans la hiérarchie.
    pub fn path(&self, selection: Selection) -> Path {
        let section_of = |book: usize| self.atlas.books.get(book).map(|b| usize::from(b.section));
        match selection {
            Selection::Section(section) => Path {
                section: Some(section),
                ..Path::default()
            },
            Selection::Book(book) => Path {
                section: section_of(book),
                book: Some(book),
                ..Path::default()
            },
            Selection::Chapter { book, chapter } => Path {
                section: section_of(book),
                book: Some(book),
                chapter: Some(chapter),
                verse: None,
            },
            Selection::Verse(verse) => match self.atlas.verses.get(verse) {
                Some(v) => Path {
                    section: section_of(usize::from(v.book)),
                    book: Some(usize::from(v.book)),
                    chapter: Some(usize::from(v.chapter_index)),
                    verse: Some(verse),
                },
                None => Path::default(),
            },
        }
    }

    /// Nom affiché d'une sélection : « Jean 3:16 », « Jean 3 », « Jean »,
    /// « Évangiles ».
    pub fn selection_label(&self, selection: Selection) -> String {
        let atlas = &self.atlas;
        match selection {
            Selection::Verse(verse) => atlas.label_fr(verse),
            Selection::Chapter { book, chapter } => atlas
                .books
                .get(book)
                .and_then(|b| Some(format!("{} {}", b.name_fr, b.chapters.get(chapter)?.number)))
                .unwrap_or_default(),
            Selection::Book(book) => atlas
                .books
                .get(book)
                .map_or(String::new(), |b| b.name_fr.clone()),
            Selection::Section(section) => atlas.sections.get(section).cloned().unwrap_or_default(),
        }
    }

    /// Sélection voisine de même nature : verset, chapitre, livre ou section
    /// précédent ou suivant. Reste sur place aux extrémités.
    pub fn step_selection(&self, selection: Selection, delta: isize) -> Selection {
        let books = &self.atlas.books;
        match selection {
            Selection::Verse(verse) => Selection::Verse(self.step_verse(verse, delta)),
            Selection::Chapter { book, chapter } => {
                // Premier verset du chapitre voisin, puis son chapitre.
                let Some(first) = books
                    .get(book)
                    .and_then(|b| b.chapters.get(chapter))
                    .map(|c| c.first as usize)
                else {
                    return selection;
                };
                let next = self.step_chapter(first, delta);
                match self.atlas.verses.get(next) {
                    Some(v) => Selection::Chapter {
                        book: usize::from(v.book),
                        chapter: usize::from(v.chapter_index),
                    },
                    None => selection,
                }
            }
            Selection::Book(book) => {
                let last = books.len().saturating_sub(1) as isize;
                Selection::Book((book as isize + delta).clamp(0, last) as usize)
            }
            Selection::Section(section) => {
                // Les sections présentes, dans l'ordre du canon.
                let mut present: Vec<usize> = Vec::new();
                for b in books {
                    let s = usize::from(b.section);
                    if present.last() != Some(&s) {
                        present.push(s);
                    }
                }
                let Some(current) = present.iter().position(|s| *s == section) else {
                    return selection;
                };
                let next = (current as isize + delta).clamp(0, present.len() as isize - 1);
                Selection::Section(present[next as usize])
            }
        }
    }

    /// Dresse le bilan d'un chapitre, d'un livre ou d'une section. `None`
    /// pour un verset, qui a son propre inspecteur.
    ///
    /// `composite` est U(v) et `composite_rank` son rang percentile dans la
    /// Bible ; `hot_rank` est le rang à partir duquel un verset est dit
    /// très incertain.
    pub fn summarize(
        &self,
        selection: Selection,
        composite: &[f32],
        composite_rank: &[f32],
        hot_rank: f32,
        bins: usize,
        top: usize,
    ) -> Option<Summary> {
        if matches!(selection, Selection::Verse(_)) {
            return None;
        }
        let range = self.selection_range(selection)?;
        let atlas = &self.atlas;
        let books = &atlas.books;
        let book_range = |b: &babel_atlas::AtlasBook| b.first as usize..(b.first + b.len) as usize;
        let chapter_range =
            |c: &babel_atlas::AtlasChapter| c.first as usize..(c.first + c.len) as usize;
        let (mean, scored) = mean_over(composite, range.clone());

        let mut finite: Vec<f32> = composite[range.clone()]
            .iter()
            .copied()
            .filter(|u| u.is_finite())
            .collect();
        let hot = composite_rank[range.clone()]
            .iter()
            .zip(&composite[range.clone()])
            .filter(|(rank, u)| u.is_finite() && **rank >= hot_rank)
            .count();

        // Pairs : les éléments de même nature dans toute la Bible.
        let peers: Vec<f32> = match selection {
            Selection::Section(_) => {
                let mut sections: Vec<usize> = Vec::new();
                for b in books {
                    let s = usize::from(b.section);
                    if !sections.contains(&s) {
                        sections.push(s);
                    }
                }
                sections
                    .into_iter()
                    .filter_map(|s| self.selection_range(Selection::Section(s)))
                    .map(|r| mean_over(composite, r).0)
                    .collect()
            }
            Selection::Book(_) => books
                .iter()
                .map(|b| mean_over(composite, book_range(b)).0)
                .collect(),
            Selection::Chapter { .. } | Selection::Verse(_) => books
                .iter()
                .flat_map(|b| &b.chapters)
                .map(|c| mean_over(composite, chapter_range(c)).0)
                .collect(),
        };
        let rank = mean.is_finite().then(|| {
            let scored_peers = peers.iter().filter(|m| m.is_finite()).count();
            let above = peers.iter().filter(|m| **m > mean).count();
            (above + 1, scored_peers)
        });

        let children: Vec<Child> = match selection {
            Selection::Section(section) => self
                .section_books(section)
                .map(|index| {
                    let b = &books[index];
                    Child {
                        selection: Selection::Book(index),
                        label: b.name_fr.clone(),
                        mean: mean_over(composite, book_range(b)).0,
                        verses: b.len as usize,
                    }
                })
                .collect(),
            Selection::Book(book) => books[book]
                .chapters
                .iter()
                .enumerate()
                .map(|(chapter, c)| Child {
                    selection: Selection::Chapter { book, chapter },
                    label: c.number.to_string(),
                    mean: mean_over(composite, chapter_range(c)).0,
                    verses: c.len as usize,
                })
                .collect(),
            Selection::Chapter { .. } | Selection::Verse(_) => range
                .clone()
                .map(|verse| Child {
                    selection: Selection::Verse(verse),
                    label: atlas.verses[verse].verse.to_string(),
                    mean: composite[verse],
                    verses: 1,
                })
                .collect(),
        };

        let (n_books, n_chapters) = match selection {
            Selection::Section(section) => {
                let members = self.section_books(section);
                let chapters = books[members.clone()]
                    .iter()
                    .map(|b| b.chapters.len())
                    .sum();
                (members.len(), chapters)
            }
            Selection::Book(book) => (1, books[book].chapters.len()),
            Selection::Chapter { .. } | Selection::Verse(_) => (1, 1),
        };

        let mut ranked: Vec<(usize, f32)> = range
            .clone()
            .map(|verse| (verse, composite[verse]))
            .filter(|(_, u)| u.is_finite())
            .collect();
        // À valeur égale, l'ordre du canon départage.
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked.truncate(top);

        let mut eligible: Vec<f32> = atlas.n_eligible[range.clone()]
            .iter()
            .map(|n| f32::from(*n))
            .collect();

        Some(Summary {
            books: n_books,
            chapters: n_chapters,
            verses: range.len(),
            scored,
            mean,
            median: median(&mut finite).unwrap_or(f32::NAN),
            hot_share: if scored == 0 {
                0.0
            } else {
                hot as f32 / scored as f32
            },
            rank,
            signals: atlas
                .signals
                .iter()
                .map(|s| mean_over(&s.rank, range.clone()).0)
                .collect(),
            translations: median(&mut eligible).unwrap_or(0.0).round() as u16,
            histogram: histogram(&composite[range], bins),
            baseline: histogram(composite, bins),
            children,
            top: ranked,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use babel_atlas::{AtlasInput, SignalInput};
    use babel_core::stats::percentile_ranks;
    use babel_core::{Config, VerseId, Versification};

    /// Genèse (3 chapitres de 20 versets), Exode (1 × 10), Jean (1 × 50),
    /// Tobie (1 × 10) : deux livres dans le Pentateuque, un dans les
    /// Évangiles, un deutérocanonique.
    fn scene() -> Scene {
        let mut vref = String::new();
        for chapter in 1..=3 {
            for verse in 1..=20 {
                vref.push_str(&format!("GEN {chapter}:{verse}\n"));
            }
        }
        for verse in 1..=10 {
            vref.push_str(&format!("EXO 1:{verse}\n"));
        }
        for verse in 1..=50 {
            vref.push_str(&format!("JHN 1:{verse}\n"));
        }
        for verse in 1..=10 {
            vref.push_str(&format!("TOB 1:{verse}\n"));
        }
        let v = Versification::parse(&vref).expect("vref valide");
        let n = v.len();
        let atlas = babel_atlas::build(
            AtlasInput {
                versification: &v,
                fingerprint: String::new(),
                signals: vec![SignalInput {
                    code: "omit".into(),
                    label: "Omission".into(),
                    raw: vec![0.0; n],
                    rank: (0..n).map(|i| i as f32 / n as f32).collect(),
                    weight: 1.0,
                }],
                n_eligible: (0..n).map(|i| if i < 70 { 9 } else { 21 }).collect(),
                translations: Vec::new(),
                references: Vec::new(),
                text_blocks: Vec::new(),
            },
            &Config::embedded().expect("configuration valide").atlas,
        )
        .expect("atlas");
        Scene::new(atlas).expect("scène")
    }

    /// U fictif : 0,2 dans la Genèse, 0,6 dans l'Exode, 0,95 dans Jean, et
    /// rien dans Tobie. Un verset de la Genèse 2 est très incertain.
    fn composite(scene: &Scene) -> Vec<f32> {
        let mut u: Vec<f32> = (0..scene.n_verses())
            .map(|verse| match verse {
                0..60 => 0.2,
                60..70 => 0.6,
                70..120 => 0.95,
                _ => f32::NAN,
            })
            .collect();
        u[25] = 0.99;
        u
    }

    const BINS: usize = 10;
    const TOP: usize = 3;
    const HOT: f32 = 0.9;

    fn summary(scene: &Scene, selection: Selection) -> Summary {
        let u = composite(scene);
        let ranks = percentile_ranks(&u);
        scene
            .summarize(selection, &u, &ranks, HOT, BINS, TOP)
            .expect("bilan")
    }

    #[test]
    fn plages_des_selections() {
        let s = scene();
        assert_eq!(s.selection_range(Selection::Verse(5)), Some(5..6));
        assert_eq!(
            s.selection_range(Selection::Chapter {
                book: 0,
                chapter: 1
            }),
            Some(20..40)
        );
        assert_eq!(s.selection_range(Selection::Book(1)), Some(60..70));
        // Le Pentateuque réunit la Genèse et l'Exode.
        assert_eq!(s.section_books(0), 0..2);
        assert_eq!(s.selection_range(Selection::Section(0)), Some(0..70));
        // Section sans livre dans ce corpus, livre inexistant.
        assert_eq!(s.selection_range(Selection::Section(2)), None);
        assert_eq!(s.selection_range(Selection::Book(9)), None);
    }

    #[test]
    fn noms_et_fil_d_ariane() {
        let s = scene();
        assert_eq!(s.selection_label(Selection::Verse(25)), "Genèse 2:6");
        let chapter = Selection::Chapter {
            book: 0,
            chapter: 1,
        };
        assert_eq!(s.selection_label(chapter), "Genèse 2");
        assert_eq!(s.selection_label(Selection::Book(2)), "Jean");
        assert_eq!(s.selection_label(Selection::Section(0)), "Pentateuque");

        let path = s.path(Selection::Verse(25));
        assert_eq!(
            path,
            Path {
                section: Some(0),
                book: Some(0),
                chapter: Some(1),
                verse: Some(25)
            }
        );
        assert_eq!(s.path(Selection::Book(2)).section, Some(4));
        assert_eq!(s.path(Selection::Book(2)).chapter, None);
        assert_eq!(s.path(Selection::Section(4)).book, None);
    }

    #[test]
    fn conversion_depuis_une_cible() {
        assert_eq!(
            Selection::from(Target::Verse(VerseId(7))),
            Selection::Verse(7)
        );
        assert_eq!(
            Selection::from(Target::Book { book: 3 }),
            Selection::Book(3)
        );
    }

    #[test]
    fn bilan_d_un_livre() {
        let s = scene();
        let b = summary(&s, Selection::Book(0));
        assert_eq!((b.books, b.chapters, b.verses, b.scored), (1, 3, 60, 60));
        // 59 versets à 0,2 et un à 0,99.
        let expected = (59.0 * 0.2 + 0.99) / 60.0;
        assert!((b.mean - expected).abs() < 1e-5);
        assert!((b.median - 0.2).abs() < 1e-6);
        // Troisième des trois livres notés : Jean puis l'Exode le précèdent.
        assert_eq!(b.rank, Some((3, 3)));
        assert_eq!(b.translations, 9);
        // Un seul verset au-dessus du seuil « très incertain ».
        assert!((b.hot_share - 1.0 / 60.0).abs() < 1e-6);

        // Un chapitre par enfant ; le chapitre 2 porte le verset aberrant.
        assert_eq!(b.children.len(), 3);
        assert_eq!(b.children[1].label, "2");
        assert_eq!(
            b.children[1].selection,
            Selection::Chapter {
                book: 0,
                chapter: 1
            }
        );
        assert!(b.children[1].mean > b.children[0].mean);
        assert_eq!(b.top[0], (25, 0.99));
        assert_eq!(b.top.len(), TOP);
        // À valeur égale, l'ordre du canon.
        assert_eq!((b.top[1].0, b.top[2].0), (0, 1));
    }

    #[test]
    fn bilan_d_une_section() {
        let s = scene();
        let b = summary(&s, Selection::Section(0));
        assert_eq!((b.books, b.chapters, b.verses), (2, 4, 70));
        assert_eq!(b.children.len(), 2);
        assert_eq!(b.children[0].label, "Genèse");
        assert_eq!(b.children[1].selection, Selection::Book(1));
        assert!((b.children[1].mean - 0.6).abs() < 1e-6);
        // Deux sections notées : les Évangiles (0,95) devant le Pentateuque.
        assert_eq!(b.rank, Some((2, 2)));
        assert_eq!(summary(&s, Selection::Section(4)).rank, Some((1, 2)));
        // Signal : rang moyen des 70 premiers versets sur 130.
        assert!((b.signals[0] - 34.5 / 130.0).abs() < 1e-5);
    }

    #[test]
    fn bilan_d_un_chapitre() {
        let s = scene();
        let chapter = Selection::Chapter {
            book: 0,
            chapter: 1,
        };
        let b = summary(&s, chapter);
        assert_eq!((b.books, b.chapters, b.verses), (1, 1, 20));
        // Un verset par enfant.
        assert_eq!(b.children.len(), 20);
        assert_eq!(b.children[5].selection, Selection::Verse(25));
        assert_eq!(b.children[5].label, "6");
        // Cinq chapitres notés ; ceux de Jean et de l'Exode le dépassent.
        assert_eq!(b.rank, Some((3, 5)));
    }

    #[test]
    fn histogramme() {
        let s = scene();
        let b = summary(&s, Selection::Book(0));
        assert_eq!(b.histogram.len(), BINS);
        // 59 versets dans la classe [0,2 ; 0,3[ et un dans la dernière.
        assert!((b.histogram[2] - 59.0 / 60.0).abs() < 1e-6);
        assert!((b.histogram[9] - 1.0 / 60.0).abs() < 1e-6);
        assert!((b.histogram.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        // Le repère couvre les 120 versets notés de la Bible.
        assert!((b.baseline[9] - 51.0 / 120.0).abs() < 1e-6);
    }

    #[test]
    fn selection_sans_donnees() {
        let s = scene();
        let b = summary(&s, Selection::Book(3));
        assert_eq!((b.verses, b.scored), (10, 0));
        assert!(b.mean.is_nan() && b.median.is_nan());
        assert_eq!(b.rank, None);
        assert!(b.top.is_empty());
        assert_eq!(b.hot_share, 0.0);
        // Un verset n'a pas de bilan : il a son propre inspecteur.
        let u = composite(&s);
        assert!(
            s.summarize(Selection::Verse(0), &u, &u, HOT, BINS, TOP)
                .is_none()
        );
    }

    #[test]
    fn voisins() {
        let s = scene();
        assert_eq!(s.step_selection(Selection::Book(0), 1), Selection::Book(1));
        assert_eq!(s.step_selection(Selection::Book(0), -1), Selection::Book(0));
        assert_eq!(s.step_selection(Selection::Book(3), 1), Selection::Book(3));
        // Du Pentateuque aux Évangiles : les sections absentes sont sautées.
        assert_eq!(
            s.step_selection(Selection::Section(0), 1),
            Selection::Section(4)
        );
        // Du dernier chapitre de la Genèse au premier de l'Exode.
        let last = Selection::Chapter {
            book: 0,
            chapter: 2,
        };
        assert_eq!(
            s.step_selection(last, 1),
            Selection::Chapter {
                book: 1,
                chapter: 0
            }
        );
        assert_eq!(
            s.step_selection(Selection::Verse(5), -1),
            Selection::Verse(4)
        );
    }
}
