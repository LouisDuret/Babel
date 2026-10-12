//! Le modèle affiché : l'atlas, ses dispositions et la sélection au survol.
//!
//! Tout est pur et sans GPU : positions, inversion analytique des
//! dispositions (grille du Mur, courbe de Hilbert), valeurs par métrique.

use babel_atlas::Atlas;
use babel_atlas::layout::hilbert_xy2d;
use babel_core::Versification;
use egui::{Vec2, vec2};

/// Valeur envoyée au shader pour un verset sans données (hachures).
pub const VALUE_MISSING: f32 = -1.0;
/// Valeur envoyée au shader pour un verset masqué par un filtre.
pub const VALUE_HIDDEN: f32 = -2.0;

/// Index de testament des livres deutérocanoniques dans l'atlas.
const TESTAMENT_DEUTEROCANON: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layout {
    /// Étagères : sections en bandes, livres en blocs, un chapitre par rangée.
    Wall,
    /// Courbe de Hilbert : les livres deviennent des territoires.
    Hilbert,
}

impl Layout {
    pub const ALL: [Layout; 2] = [Layout::Wall, Layout::Hilbert];

    pub fn label(self) -> &'static str {
        match self {
            Layout::Wall => "Mur",
            Layout::Hilbert => "Hilbert",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Layout::Wall => "mur",
            Layout::Hilbert => "hilbert",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|l| l.code() == code.to_lowercase())
    }
}

/// Ce que la carte colore : le composite ou un signal seul.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Metric {
    Composite,
    Signal(usize),
}

/// Cellule vide de la grille de sélection.
const NO_VERSE: u32 = u32::MAX;

pub struct Scene {
    pub atlas: Atlas,
    pub versification: Versification,
    /// Grille du Mur : verset de chaque cellule, ou [`NO_VERSE`].
    wall_grid: Vec<u32>,
    wall_width: usize,
    wall_height: usize,
}

impl Scene {
    pub fn new(atlas: Atlas) -> Result<Self, babel_atlas::AtlasError> {
        let versification = atlas.versification()?;
        let wall_width = atlas.wall_size[0].ceil().max(1.0) as usize;
        let wall_height = atlas.wall_size[1].ceil().max(1.0) as usize;
        let mut wall_grid = vec![NO_VERSE; wall_width * wall_height];
        for (verse, position) in atlas.wall_positions.iter().enumerate() {
            let (x, y) = (position[0] as usize, position[1] as usize);
            if x < wall_width && y < wall_height {
                wall_grid[y * wall_width + x] = verse as u32;
            }
        }
        Ok(Self {
            atlas,
            versification,
            wall_grid,
            wall_width,
            wall_height,
        })
    }

    pub fn n_verses(&self) -> usize {
        self.atlas.verses.len()
    }

    pub fn positions(&self, layout: Layout) -> &[[f32; 2]] {
        match layout {
            Layout::Wall => &self.atlas.wall_positions,
            Layout::Hilbert => &self.atlas.hilbert_positions,
        }
    }

    /// Coin haut gauche de la cellule d'un verset.
    pub fn position(&self, layout: Layout, verse: usize) -> Vec2 {
        self.positions(layout)
            .get(verse)
            .map_or(Vec2::ZERO, |p| vec2(p[0], p[1]))
    }

    /// Boîte englobante de toute la carte : (min, max).
    pub fn bounds(&self, layout: Layout) -> (Vec2, Vec2) {
        match layout {
            Layout::Wall => (
                Vec2::ZERO,
                vec2(self.atlas.wall_size[0], self.atlas.wall_size[1]),
            ),
            Layout::Hilbert => {
                let side = (1_u32 << self.atlas.hilbert_order) as f32;
                // La courbe ne remplit que le début de la grille.
                self.range_bounds(layout, 0..self.n_verses())
                    .unwrap_or((Vec2::ZERO, Vec2::splat(side)))
            }
        }
    }

    /// Boîte englobante d'une plage de versets.
    pub fn range_bounds(
        &self,
        layout: Layout,
        range: std::ops::Range<usize>,
    ) -> Option<(Vec2, Vec2)> {
        let positions = self.positions(layout).get(range)?;
        let first = positions.first()?;
        let mut min = vec2(first[0], first[1]);
        let mut max = min;
        for p in positions {
            min = min.min(vec2(p[0], p[1]));
            max = max.max(vec2(p[0], p[1]));
        }
        Some((min, max + Vec2::splat(1.0)))
    }

    /// Verset sous un point du monde : inversion analytique de la
    /// disposition, sans relecture GPU.
    pub fn pick(&self, layout: Layout, world: Vec2) -> Option<usize> {
        if world.x < 0.0 || world.y < 0.0 {
            return None;
        }
        let (x, y) = (world.x as usize, world.y as usize);
        match layout {
            Layout::Wall => {
                if x >= self.wall_width || y >= self.wall_height {
                    return None;
                }
                let verse = self.wall_grid[y * self.wall_width + x];
                (verse != NO_VERSE).then_some(verse as usize)
            }
            Layout::Hilbert => {
                let side = 1_usize << self.atlas.hilbert_order;
                if x >= side || y >= side {
                    return None;
                }
                let index = hilbert_xy2d(self.atlas.hilbert_order, x as u32, y as u32) as usize;
                (index < self.n_verses()).then_some(index)
            }
        }
    }

    /// Composite U(v) pour des poids donnés ; un signal désactivé pèse 0.
    pub fn composite(&self, weights: &[f32], enabled: &[bool]) -> Vec<f32> {
        let ranks: Vec<&[f32]> = self
            .atlas
            .signals
            .iter()
            .map(|s| s.rank.as_slice())
            .collect();
        let effective: Vec<f32> = weights
            .iter()
            .zip(enabled)
            .map(|(w, on)| if *on { w.max(0.0) } else { 0.0 })
            .collect();
        babel_score::composite(&ranks, &effective)
    }

    /// Valeur affichée de chaque verset pour une métrique, dans [0, 1] :
    /// le composite, ou le rang percentile d'un signal. NaN si indéfinie.
    pub fn metric(&self, metric: Metric, composite: &[f32]) -> Vec<f32> {
        match metric {
            Metric::Composite => composite.to_vec(),
            Metric::Signal(index) => self
                .atlas
                .signals
                .get(index)
                .map_or_else(|| vec![f32::NAN; self.n_verses()], |s| s.rank.clone()),
        }
    }

    /// Le verset appartient à un livre deutérocanonique.
    pub fn is_deuterocanon(&self, verse: usize) -> bool {
        self.atlas
            .verses
            .get(verse)
            .and_then(|v| self.atlas.books.get(usize::from(v.book)))
            .is_some_and(|b| b.testament == TESTAMENT_DEUTEROCANON)
    }

    /// Valeurs prêtes pour le GPU : la métrique, [`VALUE_MISSING`] pour un
    /// verset sans données, [`VALUE_HIDDEN`] pour un verset filtré.
    pub fn gpu_values(&self, values: &[f32], protestant_canon: bool) -> Vec<f32> {
        values
            .iter()
            .enumerate()
            .map(|(verse, value)| {
                if protestant_canon && self.is_deuterocanon(verse) {
                    VALUE_HIDDEN
                } else if value.is_finite() {
                    value.clamp(0.0, 1.0)
                } else {
                    VALUE_MISSING
                }
            })
            .collect()
    }

    /// Verset précédent ou suivant, sans sortir de la versification.
    pub fn step_verse(&self, verse: usize, delta: isize) -> usize {
        (verse as isize + delta).clamp(0, self.n_verses() as isize - 1) as usize
    }

    /// Premier verset du chapitre précédent ou suivant.
    pub fn step_chapter(&self, verse: usize, delta: isize) -> usize {
        // Liste à plat des premiers versets de chapitre.
        let starts: Vec<usize> = self
            .atlas
            .books
            .iter()
            .flat_map(|b| b.chapters.iter().map(|c| c.first as usize))
            .collect();
        let current = starts.partition_point(|s| *s <= verse).saturating_sub(1);
        let next = (current as isize + delta).clamp(0, starts.len() as isize - 1) as usize;
        starts.get(next).copied().unwrap_or(verse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use babel_atlas::{AtlasInput, SignalInput};
    use babel_core::Config;

    fn scene() -> Scene {
        // Deux livres canoniques et un deutérocanonique.
        let mut vref = String::new();
        for chapter in 1..=3 {
            for verse in 1..=20 {
                vref.push_str(&format!("GEN {chapter}:{verse}\n"));
            }
        }
        for verse in 1..=50 {
            vref.push_str(&format!("JHN 1:{verse}\n"));
        }
        for verse in 1..=10 {
            vref.push_str(&format!("TOB 1:{verse}\n"));
        }
        let v = Versification::parse(&vref).expect("vref valide");
        let n = v.len();
        let signal = |code: &str, offset: usize| SignalInput {
            code: code.into(),
            label: code.into(),
            raw: (0..n).map(|i| ((i + offset) % 10) as f32).collect(),
            rank: (0..n)
                .map(|i| {
                    if i == 5 {
                        f32::NAN
                    } else {
                        ((i + offset) % 10) as f32 / 10.0 + 0.05
                    }
                })
                .collect(),
            weight: 1.0,
        };
        let atlas = babel_atlas::build(
            AtlasInput {
                versification: &v,
                fingerprint: String::new(),
                signals: vec![signal("omit", 0), signal("len", 3)],
                n_eligible: vec![9; n],
                translations: Vec::new(),
                references: Vec::new(),
                text_blocks: Vec::new(),
            },
            &Config::embedded().expect("configuration valide").atlas,
        )
        .expect("atlas");
        Scene::new(atlas).expect("scène")
    }

    #[test]
    fn la_selection_inverse_chaque_disposition() {
        let s = scene();
        for layout in Layout::ALL {
            for verse in 0..s.n_verses() {
                let center = s.position(layout, verse) + Vec2::splat(0.5);
                assert_eq!(
                    s.pick(layout, center),
                    Some(verse),
                    "{layout:?} verset {verse}"
                );
            }
            // Hors de la carte : rien.
            assert_eq!(s.pick(layout, vec2(-1.0, 3.0)), None);
            assert_eq!(s.pick(layout, vec2(1e6, 1e6)), None);
        }
    }

    #[test]
    fn un_espace_entre_colonnes_ne_selectionne_rien() {
        let s = scene();
        // L'en-tête d'un livre est au-dessus de son premier verset.
        let first = s.position(Layout::Wall, 0);
        assert_eq!(s.pick(Layout::Wall, first + vec2(0.5, -0.5)), None);
    }

    #[test]
    fn boites_englobantes() {
        let s = scene();
        let (min, max) = s.bounds(Layout::Wall);
        for verse in 0..s.n_verses() {
            let p = s.position(Layout::Wall, verse);
            assert!(p.x >= min.x && p.x + 1.0 <= max.x && p.y >= min.y && p.y + 1.0 <= max.y);
        }
        // Le chapitre 2 de la Genèse : une rangée de vingt cellules.
        let (min, max) = s.range_bounds(Layout::Wall, 20..40).expect("chapitre");
        assert_eq!(max - min, vec2(20.0, 1.0));
        assert!(s.range_bounds(Layout::Wall, 500..510).is_none());
    }

    #[test]
    fn composite_et_poids() {
        let s = scene();
        let both = s.composite(&[1.0, 1.0], &[true, true]);
        let only_first = s.composite(&[1.0, 1.0], &[true, false]);
        let first_rank = &s.atlas.signals[0].rank;
        assert_eq!(only_first[7], first_rank[7]);
        let expected = (first_rank[7] + s.atlas.signals[1].rank[7]) / 2.0;
        assert!((both[7] - expected).abs() < 1e-6);
        // Verset sans rang : pas de composite.
        assert!(both[5].is_nan());
        // Tous les signaux désactivés : rien à afficher.
        assert!(
            s.composite(&[1.0, 1.0], &[false, false])
                .iter()
                .all(|u| u.is_nan())
        );
    }

    #[test]
    fn valeurs_pour_le_gpu() {
        let s = scene();
        let composite = s.composite(&[1.0, 1.0], &[true, true]);
        let values = s.metric(Metric::Signal(0), &composite);
        let gpu = s.gpu_values(&values, true);
        assert_eq!(gpu[5], VALUE_MISSING);
        assert_eq!(gpu[0], values[0]);
        // Tobie est masqué par le filtre « canon protestant ».
        let tobit = s.n_verses() - 1;
        assert!(s.is_deuterocanon(tobit));
        assert_eq!(gpu[tobit], VALUE_HIDDEN);
        assert_eq!(s.gpu_values(&values, false)[tobit], values[tobit]);
    }

    #[test]
    fn navigation_au_clavier() {
        let s = scene();
        assert_eq!(s.step_verse(0, -1), 0);
        assert_eq!(s.step_verse(10, 1), 11);
        assert_eq!(s.step_verse(s.n_verses() - 1, 1), s.n_verses() - 1);
        // Du milieu du chapitre 2 de la Genèse (verset 25) au chapitre 3.
        assert_eq!(s.step_chapter(25, 1), 40);
        assert_eq!(s.step_chapter(25, -1), 0);
        // Du dernier chapitre de la Genèse au premier de Jean.
        assert_eq!(s.step_chapter(45, 1), 60);
        assert_eq!(s.step_chapter(0, -1), 0);
    }
}
