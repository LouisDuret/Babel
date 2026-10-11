//! Dispositions des versets : vue Mur (étagères) et vue Hilbert.
//!
//! Les positions sont en unités de cellule : un verset occupe le carré
//! `[x, x + 1[ × [y, y + 1[`. Fonctions pures.

use babel_core::config::Atlas as AtlasConfig;

/// Forme d'un livre pour la mise en page : section et taille des chapitres.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookShape {
    pub section: u8,
    /// Nombre de versets de chaque chapitre.
    pub chapters: Vec<u32>,
}

/// Rectangle en unités de cellule.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Ancre d'un nom de section : le coin haut gauche de sa première colonne
/// sur une étagère.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionAnchor {
    pub section: u8,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Wall {
    /// Position de chaque verset, dans l'ordre de la versification.
    pub positions: Vec<[f32; 2]>,
    /// Largeur et hauteur totales.
    pub size: [f32; 2],
    /// Rectangle englobant de chaque livre. Un livre long s'étend sur
    /// plusieurs colonnes : son rectangle peut alors en recouvrir d'autres.
    pub books: Vec<Rect>,
    /// Ancre du nom de chaque livre : le coin haut gauche de son en-tête.
    pub labels: Vec<[f32; 2]>,
    pub sections: Vec<SectionAnchor>,
    /// Hauteur d'étagère retenue, en rangées.
    pub shelf_rows: u32,
}

/// Ce qu'une colonne empile, de haut en bas.
#[derive(Debug, Clone, Copy)]
enum Item {
    /// En-tête d'un livre : rangées réservées à son nom.
    Header { book: usize, row: usize },
    Chapter {
        book: usize,
        chapter: usize,
        row: usize,
    },
}

/// Une colonne d'étagère : des chapitres empilés, comme une colonne de
/// journal.
#[derive(Debug, Clone)]
struct Column {
    section: u8,
    /// Largeur de la plus longue rangée.
    width: usize,
    rows: usize,
    items: Vec<Item>,
}

impl Column {
    fn new(section: u8) -> Self {
        Self {
            section,
            width: 1,
            rows: 0,
            items: Vec::new(),
        }
    }
}

fn chapter_rows(verses: u32, wrap: usize) -> usize {
    (verses as usize).div_ceil(wrap).max(1)
}

/// Verse les livres dans des colonnes d'au plus `shelf_rows` rangées.
///
/// Chaque livre commence par un en-tête de `header_rows` rangées ; un
/// chapitre n'est jamais coupé ; une nouvelle section ouvre une nouvelle
/// colonne.
fn columns(books: &[BookShape], wrap: usize, shelf_rows: usize, header_rows: usize) -> Vec<Column> {
    let mut out: Vec<Column> = Vec::new();
    for (book_index, book) in books.iter().enumerate() {
        let first_rows = book.chapters.first().map_or(1, |n| chapter_rows(*n, wrap));
        let fits = out.last().is_some_and(|c| {
            c.section == book.section && c.rows + header_rows + first_rows <= shelf_rows
        });
        if !fits {
            out.push(Column::new(book.section));
        }
        if let Some(column) = out.last_mut() {
            column.items.push(Item::Header {
                book: book_index,
                row: column.rows,
            });
            column.rows += header_rows;
        }
        for (chapter, verses) in book.chapters.iter().enumerate() {
            let needed = chapter_rows(*verses, wrap);
            // La colonne est pleine : le livre continue dans la suivante.
            // Un chapitre suit toujours l'en-tête de son livre.
            let full = out.last().is_some_and(|c| {
                c.rows + needed > shelf_rows && !matches!(c.items.last(), Some(Item::Header { .. }))
            });
            if full {
                out.push(Column::new(book.section));
            }
            if let Some(column) = out.last_mut() {
                column.items.push(Item::Chapter {
                    book: book_index,
                    chapter,
                    row: column.rows,
                });
                column.rows += needed;
                column.width = column.width.max((*verses as usize).clamp(1, wrap));
            }
        }
    }
    out
}

/// Pose les colonnes sur des étagères de largeur maximale `max_width`.
/// Renvoie le coin de chaque colonne et la taille totale.
fn flow(
    columns: &[Column],
    config: &AtlasConfig,
    max_width: usize,
) -> (Vec<(usize, usize)>, usize, usize) {
    let mut origins = Vec::with_capacity(columns.len());
    let (mut x, mut y) = (0_usize, config.wall_shelf_gap);
    let mut shelf_height = 0_usize;
    let mut width = 0_usize;
    let mut previous_section: Option<u8> = None;
    for column in columns {
        if x > 0 {
            let gap = if previous_section == Some(column.section) {
                config.wall_column_gap
            } else {
                config.wall_section_gap
            };
            if x + gap + column.width > max_width {
                // Nouvelle étagère.
                y += shelf_height + config.wall_shelf_gap;
                x = 0;
                shelf_height = 0;
            } else {
                x += gap;
            }
        }
        origins.push((x, y));
        x += column.width;
        width = width.max(x);
        shelf_height = shelf_height.max(column.rows);
        previous_section = Some(column.section);
    }
    (origins, width, y + shelf_height)
}

/// Nombre maximal d'étagères essayées.
const MAX_SHELVES: usize = 12;

/// Construit la vue Mur : les sections du canon se suivent sur des
/// étagères ; dans une section, les livres s'empilent en colonnes ; chaque
/// chapitre est une rangée de cellules avec retour à la ligne.
///
/// La hauteur d'étagère et la largeur maximale sont choisies pour que la
/// carte entière soit la plus grande possible dans un cadre de rapport
/// `wall_aspect`.
pub fn wall(books: &[BookShape], config: &AtlasConfig) -> Wall {
    let wrap = config.wall_wrap;
    let header = config.wall_header_rows;
    if books.is_empty() {
        return Wall::default();
    }

    // Recherche exhaustive : peu de candidats, chaque essai est immédiat.
    let mut best: Option<(f32, usize, usize)> = None;
    for shelf_rows in config.wall_shelf_rows_min..=config.wall_shelf_rows_max {
        let cols = columns(books, wrap, shelf_rows, header);
        let strip: usize = cols.iter().map(|c| c.width + config.wall_section_gap).sum();
        let widest = cols.iter().map(|c| c.width).max().unwrap_or(1);
        for shelves in 1..=MAX_SHELVES {
            let max_width = (strip / shelves).max(widest);
            let (_, width, height) = flow(&cols, config, max_width);
            // Taille de cellule si la carte remplit un cadre de rapport
            // `wall_aspect` : la plus grande l'emporte.
            let scale = (config.wall_aspect / width as f32).min(1.0 / height as f32);
            if best.is_none_or(|(s, _, _)| scale > s) {
                best = Some((scale, shelf_rows, max_width));
            }
        }
    }
    let (_, shelf_rows, max_width) = best.unwrap_or((0.0, config.wall_shelf_rows_min, usize::MAX));

    let cols = columns(books, wrap, shelf_rows, header);
    let (origins, width, height) = flow(&cols, config, max_width);

    // Index du premier verset de chaque chapitre.
    let mut chapter_start: Vec<Vec<usize>> = Vec::with_capacity(books.len());
    let mut total = 0_usize;
    for book in books {
        let mut starts = Vec::with_capacity(book.chapters.len());
        for verses in &book.chapters {
            starts.push(total);
            total += *verses as usize;
        }
        chapter_start.push(starts);
    }

    let mut positions = vec![[0.0_f32; 2]; total];
    let mut labels = vec![[0.0_f32; 2]; books.len()];
    // Boîte englobante de chaque livre : (x min, y min, x max, y max).
    let mut bounds = vec![(f32::MAX, f32::MAX, f32::MIN, f32::MIN); books.len()];
    let mut sections: Vec<SectionAnchor> = Vec::new();
    let mut previous: Option<(u8, usize)> = None;
    for (column, (x0, y0)) in cols.iter().zip(&origins) {
        // Un nom de section par étagère où la section commence ou reprend.
        if previous != Some((column.section, *y0)) {
            sections.push(SectionAnchor {
                section: column.section,
                x: *x0 as f32,
                y: *y0 as f32,
            });
        }
        previous = Some((column.section, *y0));

        for item in &column.items {
            match *item {
                Item::Header { book, row } => labels[book] = [*x0 as f32, (y0 + row) as f32],
                Item::Chapter { book, chapter, row } => {
                    let verses = books[book].chapters[chapter] as usize;
                    for i in 0..verses {
                        let (x, y) = ((x0 + i % wrap) as f32, (y0 + row + i / wrap) as f32);
                        positions[chapter_start[book][chapter] + i] = [x, y];
                        let b = &mut bounds[book];
                        *b = (b.0.min(x), b.1.min(y), b.2.max(x + 1.0), b.3.max(y + 1.0));
                    }
                }
            }
        }
    }
    let rects = bounds
        .iter()
        .zip(&labels)
        .map(|(b, label)| {
            if b.0 > b.2 {
                // Livre sans verset : son rectangle se réduit à son en-tête.
                Rect {
                    x: label[0],
                    y: label[1],
                    w: 1.0,
                    h: 1.0,
                }
            } else {
                Rect {
                    x: b.0,
                    y: b.1,
                    w: b.2 - b.0,
                    h: b.3 - b.1,
                }
            }
        })
        .collect();

    Wall {
        positions,
        size: [width as f32, (height + config.wall_shelf_gap) as f32],
        books: rects,
        labels,
        sections,
        shelf_rows: shelf_rows as u32,
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hilbert {
    pub order: u32,
    pub positions: Vec<[f32; 2]>,
    /// Côté de la grille : 2^ordre.
    pub side: f32,
    /// Centroïde de chaque livre, où s'affiche son nom.
    pub centroids: Vec<[f32; 2]>,
}

/// Coordonnées du point d'index `d` sur une courbe de Hilbert d'ordre
/// `order` (grille de 2^ordre de côté).
pub fn hilbert_d2xy(order: u32, d: u32) -> (u32, u32) {
    let (mut x, mut y) = (0_u32, 0_u32);
    let mut t = d;
    let mut s = 1_u32;
    while s < (1 << order) {
        let rx = 1 & (t / 2);
        let ry = 1 & (t ^ rx);
        if ry == 0 {
            if rx == 1 {
                x = s - 1 - x;
                y = s - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        x += s * rx;
        y += s * ry;
        t /= 4;
        s *= 2;
    }
    (x, y)
}

/// Index sur la courbe de Hilbert du point (x, y) : l'inverse de
/// [`hilbert_d2xy`], utilisé pour la sélection au survol.
pub fn hilbert_xy2d(order: u32, x: u32, y: u32) -> u32 {
    let (mut x, mut y) = (x, y);
    let mut d = 0_u32;
    let mut s = (1_u32 << order) / 2;
    while s > 0 {
        let rx = u32::from(x & s > 0);
        let ry = u32::from(y & s > 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                x = (1 << order) - 1 - x;
                y = (1 << order) - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        s /= 2;
    }
    d
}

/// Construit la vue Hilbert : l'ordre canonique suit la courbe, les
/// passages contigus forment des zones compactes. `None` si la grille est
/// trop petite pour le nombre de versets.
pub fn hilbert(book_lengths: &[u32], order: u32) -> Option<Hilbert> {
    let total: u32 = book_lengths.iter().sum();
    if u64::from(total) > 1_u64 << (2 * order) {
        return None;
    }
    let positions: Vec<[f32; 2]> = (0..total)
        .map(|d| {
            let (x, y) = hilbert_d2xy(order, d);
            [x as f32, y as f32]
        })
        .collect();
    let mut centroids = Vec::with_capacity(book_lengths.len());
    let mut start = 0_usize;
    for length in book_lengths {
        let end = start + *length as usize;
        let n = (*length).max(1) as f32;
        let sum = positions[start..end]
            .iter()
            .fold([0.0_f32; 2], |a, p| [a[0] + p[0], a[1] + p[1]]);
        centroids.push([sum[0] / n + 0.5, sum[1] / n + 0.5]);
        start = end;
    }
    Some(Hilbert {
        order,
        positions,
        side: (1_u32 << order) as f32,
        centroids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use babel_core::Config;
    use std::collections::HashSet;

    fn config() -> AtlasConfig {
        Config::embedded()
            .expect("configuration du dépôt valide")
            .atlas
    }

    fn sample_books() -> Vec<BookShape> {
        vec![
            BookShape {
                section: 0,
                chapters: vec![31, 25, 24, 26, 32],
            },
            BookShape {
                section: 0,
                chapters: vec![22, 25],
            },
            BookShape {
                section: 1,
                chapters: vec![176, 12, 8],
            },
            BookShape {
                section: 2,
                chapters: vec![21],
            },
        ]
    }

    #[test]
    fn mur_une_cellule_par_verset_sans_recouvrement() {
        let books = sample_books();
        let w = wall(&books, &config());
        let total: u32 = books.iter().flat_map(|b| &b.chapters).sum();
        assert_eq!(w.positions.len(), total as usize);
        let cells: HashSet<(i32, i32)> = w
            .positions
            .iter()
            .map(|p| (p[0] as i32, p[1] as i32))
            .collect();
        assert_eq!(
            cells.len(),
            total as usize,
            "deux versets sur la même cellule"
        );
        for p in &w.positions {
            assert!(p[0] >= 0.0 && p[0] < w.size[0], "x hors de la carte");
            assert!(p[1] >= 0.0 && p[1] < w.size[1], "y hors de la carte");
        }
    }

    #[test]
    fn mur_chaque_verset_dans_le_rectangle_de_son_livre() {
        let books = sample_books();
        let w = wall(&books, &config());
        let mut v = 0;
        for (book, rect) in books.iter().zip(&w.books) {
            for _ in 0..book.chapters.iter().sum::<u32>() {
                let p = w.positions[v];
                assert!(p[0] >= rect.x && p[0] < rect.x + rect.w);
                assert!(p[1] >= rect.y && p[1] < rect.y + rect.h);
                v += 1;
            }
        }
    }

    #[test]
    fn mur_en_tete_au_dessus_du_premier_chapitre() {
        let books = sample_books();
        let cfg = config();
        let w = wall(&books, &cfg);
        let mut first = 0;
        for (book, label) in books.iter().zip(&w.labels) {
            // Le premier verset du livre est juste sous son en-tête.
            assert_eq!(w.positions[first][0], label[0]);
            assert_eq!(
                w.positions[first][1],
                label[1] + cfg.wall_header_rows as f32
            );
            first += book.chapters.iter().sum::<u32>() as usize;
        }
    }

    #[test]
    fn mur_un_livre_long_continue_dans_la_colonne_suivante() {
        // 100 chapitres d'une rangée et des étagères de 40 rangées au plus :
        // le livre occupe plusieurs colonnes, sans couper de chapitre.
        let mut cfg = config();
        cfg.wall_shelf_rows_min = 40;
        cfg.wall_shelf_rows_max = 40;
        let w = wall(
            &[BookShape {
                section: 0,
                chapters: vec![10; 100],
            }],
            &cfg,
        );
        let columns: HashSet<i32> = w.positions.iter().map(|p| p[0] as i32 / 10).collect();
        assert!(columns.len() >= 3);
        // Les dix versets d'un chapitre restent sur la même rangée.
        for chapter in w.positions.chunks(10) {
            assert!(chapter.iter().all(|p| p[1] == chapter[0][1]));
        }
        assert_eq!(w.shelf_rows, 40);
    }

    #[test]
    fn mur_retour_a_la_ligne_apres_quarante_versets() {
        let cfg = config();
        let w = wall(
            &[BookShape {
                section: 0,
                chapters: vec![85],
            }],
            &cfg,
        );
        let wrap = cfg.wall_wrap;
        // Le verset 41 est sous le verset 1.
        assert_eq!(w.positions[wrap][0], w.positions[0][0]);
        assert_eq!(w.positions[wrap][1], w.positions[0][1] + 1.0);
        assert_eq!(
            w.positions[wrap - 1][0],
            w.positions[0][0] + (wrap - 1) as f32
        );
        assert_eq!(w.positions[84][1], w.positions[0][1] + 2.0);
    }

    #[test]
    fn mur_un_chapitre_par_rangee() {
        let w = wall(
            &[BookShape {
                section: 0,
                chapters: vec![10, 10, 10],
            }],
            &config(),
        );
        assert_eq!(w.positions[10][1], w.positions[0][1] + 1.0);
        assert_eq!(w.positions[20][1], w.positions[0][1] + 2.0);
        assert_eq!(w.positions[10][0], w.positions[0][0]);
    }

    #[test]
    fn mur_noms_de_sections() {
        let w = wall(&sample_books(), &config());
        let named: Vec<u8> = w.sections.iter().map(|s| s.section).collect();
        for section in [0, 1, 2] {
            assert!(named.contains(&section));
        }
    }

    #[test]
    fn hilbert_aller_retour() {
        for order in 1..=6 {
            let mut seen = HashSet::new();
            for d in 0..1_u32 << (2 * order) {
                let (x, y) = hilbert_d2xy(order, d);
                assert!(x < 1 << order && y < 1 << order);
                assert_eq!(hilbert_xy2d(order, x, y), d);
                assert!(seen.insert((x, y)));
            }
        }
    }

    #[test]
    fn hilbert_les_voisins_se_touchent() {
        // Deux versets consécutifs occupent des cellules adjacentes.
        let h = hilbert(&[100, 156], 4).expect("256 cellules suffisent");
        for pair in h.positions.windows(2) {
            let distance = (pair[0][0] - pair[1][0]).abs() + (pair[0][1] - pair[1][1]).abs();
            assert_eq!(distance, 1.0);
        }
        assert_eq!(h.centroids.len(), 2);
        assert_eq!(h.side, 16.0);
    }

    #[test]
    fn hilbert_grille_trop_petite() {
        assert!(hilbert(&[300], 4).is_none());
    }
}
