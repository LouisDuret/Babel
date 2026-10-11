//! Ce qui se dessine par-dessus les cellules : libellés du zoom sémantique,
//! cartes de verset, infobulle, statistiques de rendu.
//!
//! Dans ce prototype le texte du canevas passe par le peintre d'egui ; le
//! façonnage par cosmic-text et glyphon (grec, hébreu) viendra en phase 3.

use crate::app::BabelApp;
use crate::scene::Layout;
use crate::theme;
use babel_core::types::RANGE_TOKEN;
use egui::{Align2, Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2, pos2, vec2};

/// Largeur de l'intervalle d'échelle sur lequel une étiquette apparaît en
/// fondu, en part de son seuil.
const FADE_SPAN: f32 = 0.25;
/// Nombre maximal de cellules détaillées (numéro, micro-barres) par image.
const MAX_DETAILED_CELLS: usize = 6000;
/// Luminance au-dessus de laquelle le texte posé sur une cellule est sombre.
const DARK_INK_ABOVE: f32 = 0.30;

/// Opacité d'une étiquette qui apparaît à partir de `threshold` pixels.
fn fade_in(scale: f32, threshold: f32) -> f32 {
    let t = ((scale - threshold) / (threshold * FADE_SPAN)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Couleur de texte lisible sur une cellule de couleur `cell`.
fn ink(cell: Color32) -> Color32 {
    if theme::relative_luminance(cell) > DARK_INK_ABOVE {
        theme::BG_CANVAS
    } else {
        theme::TEXT_PRIMARY
    }
}

/// Nombre décimal à la française : « 0,87 ».
pub(crate) fn decimal(value: f32, digits: usize) -> String {
    format!("{value:.digits$}").replace('.', ",")
}

/// Rang percentile lisible : « p97 ».
pub(crate) fn percentile(rank: f32) -> String {
    if rank.is_finite() {
        format!("p{:.0}", (rank * 100.0).clamp(0.0, 100.0))
    } else {
        "—".into()
    }
}

impl BabelApp {
    fn to_screen(&self, rect: Rect, world: Vec2) -> Pos2 {
        self.camera.world_to_screen(rect, world)
    }

    /// Couleur d'un verset pour la métrique affichée.
    pub(crate) fn cell_color(&self, verse: usize) -> Color32 {
        match self.shown.get(verse) {
            Some(value) if value.is_finite() => self.current_palette().color(*value),
            _ => theme::MISSING,
        }
    }

    /// Texte d'un verset dans la traduction de référence de rang `rank`.
    pub(crate) fn reference_text(&mut self, rank: usize, verse: usize) -> Option<String> {
        let translation = *self.scene.atlas.references.get(rank)?;
        let v = self.scene.atlas.verses.get(verse)?;
        let book = self.scene.atlas.books.get(usize::from(v.book))?;
        let index = verse.checked_sub(book.first as usize)?;
        let text = self.texts.verse(translation, v.book, index)?;
        match text {
            "" => None,
            RANGE_TOKEN => Some("(fusionné avec le verset précédent)".to_string()),
            other => Some(other.to_string()),
        }
    }

    pub(crate) fn draw_overlays(
        &mut self,
        painter: &Painter,
        rect: Rect,
        transition: f32,
        in_transition: bool,
    ) {
        let scale = self.camera.scale;
        let wall_alpha = 1.0 - transition;
        let hilbert_alpha = transition;

        if wall_alpha > 0.01 {
            self.draw_wall_labels(painter, rect, wall_alpha);
        }
        if hilbert_alpha > 0.01 {
            self.draw_hilbert_labels(painter, rect, hilbert_alpha);
        }
        if !in_transition {
            let detail = fade_in(scale, self.cfg.z2_px);
            if detail > 0.0 {
                self.draw_cells(painter, rect, detail);
            }
            self.draw_selection_marker(painter, rect);
        }
    }

    /// Noms des sections, des livres et numéros de chapitres de la vue Mur.
    fn draw_wall_labels(&self, painter: &Painter, rect: Rect, alpha: f32) {
        let scale = self.camera.scale;
        let atlas = &self.scene.atlas;
        let visible = rect.expand(200.0);

        let section_font = theme::sized((scale * 2.6).clamp(11.0, 22.0));
        for anchor in &atlas.wall_sections {
            let position = self.to_screen(rect, vec2(anchor.x, anchor.y - 1.2));
            if !visible.contains(position) {
                continue;
            }
            let Some(name) = atlas.sections.get(usize::from(anchor.section)) else {
                continue;
            };
            painter.text(
                position,
                Align2::LEFT_BOTTOM,
                name,
                section_font.clone(),
                theme::fade(theme::TEXT_PRIMARY, alpha),
            );
        }

        let header = atlas.wall_header_rows as f32;
        let book_font = theme::sized((scale * 2.2).clamp(9.0, 16.0));
        let color = theme::fade(theme::TEXT_SECONDARY, alpha);
        for book in &atlas.books {
            let position = self.to_screen(
                rect,
                vec2(book.wall_label[0], book.wall_label[1] + header - 0.4),
            );
            if !visible.contains(position) {
                continue;
            }
            // Le nom complet s'il tient dans la colonne, le code USFM sinon.
            let available = book.wall_rect[2].min(atlas.wall_wrap as f32) * scale;
            let full = painter.layout_no_wrap(book.name_fr.clone(), book_font.clone(), color);
            let text = if full.size().x <= available {
                &book.name_fr
            } else {
                &book.code
            };
            painter.text(
                position,
                Align2::LEFT_BOTTOM,
                text,
                book_font.clone(),
                color,
            );
        }

        // Z1 : numéros de chapitres en marge.
        let chapters = fade_in(scale, self.cfg.z1_px) * alpha;
        if chapters > 0.0 {
            let font = theme::mono_sized((scale * 0.55).clamp(9.0, 13.0));
            let color = theme::fade(theme::TEXT_SECONDARY, chapters);
            for book in &atlas.books {
                for chapter in &book.chapters {
                    let first = self.scene.position(Layout::Wall, chapter.first as usize);
                    let position = self.to_screen(rect, first + vec2(-0.3, 0.5));
                    if rect.expand(40.0).contains(position) {
                        painter.text(
                            position,
                            Align2::RIGHT_CENTER,
                            chapter.number,
                            font.clone(),
                            color,
                        );
                    }
                }
            }
        }
    }

    /// Noms des livres au centroïde de leur territoire, vue Hilbert.
    fn draw_hilbert_labels(&self, painter: &Painter, rect: Rect, alpha: f32) {
        let scale = self.camera.scale;
        for book in &self.scene.atlas.books {
            // Côté d'un carré de même aire que le territoire du livre.
            let extent = (book.len as f32).sqrt() * scale;
            let center = self.to_screen(rect, book.hilbert_centroid.into());
            if !rect.expand(extent).contains(center) {
                continue;
            }
            let font = theme::sized((extent * 0.14).clamp(10.0, 22.0));
            let color = theme::fade(theme::TEXT_PRIMARY, alpha);
            let full = painter.layout_no_wrap(book.name_fr.clone(), font.clone(), color);
            let text = if full.size().x <= extent * 0.9 {
                &book.name_fr
            } else if extent >= 34.0 {
                &book.code
            } else {
                continue;
            };
            let galley = painter.layout_no_wrap(text.clone(), font, color);
            let box_rect = Rect::from_center_size(center, galley.size()).expand2(vec2(5.0, 2.0));
            painter.rect_filled(box_rect, 4.0, theme::fade(theme::BG_CANVAS, 0.72 * alpha));
            painter.galley(box_rect.min + vec2(5.0, 2.0), galley, color);
        }
    }

    /// Z2 et Z3 : numéro de verset, micro-barres, puis carte de verset.
    fn draw_cells(&mut self, painter: &Painter, rect: Rect, alpha: f32) {
        let scale = self.camera.scale;
        let (min, max) = self.camera.visible(rect);
        let (x0, y0) = (
            min.x.floor().max(0.0) as usize,
            min.y.floor().max(0.0) as usize,
        );
        let (x1, y1) = (
            max.x.ceil().max(0.0) as usize,
            max.y.ceil().max(0.0) as usize,
        );
        if (x1 - x0) * (y1 - y0) > MAX_DETAILED_CELLS {
            return;
        }
        let card = fade_in(scale, self.cfg.z3_px);
        let inset = self.cfg.cell_gap * scale / 2.0;

        for y in y0..y1 {
            for x in x0..x1 {
                let world = vec2(x as f32, y as f32);
                let Some(verse) = self.scene.pick(self.layout, world + Vec2::splat(0.5)) else {
                    continue;
                };
                let cell = Rect::from_min_size(self.to_screen(rect, world), Vec2::splat(scale))
                    .shrink(inset);
                let ink = ink(self.cell_color(verse));
                let padding = (scale * 0.07).max(3.0);
                let inner = cell.shrink(padding);
                let bars_top = self.draw_micro_bars(painter, inner, verse, ink, alpha);

                if card > 0.0 {
                    let body = Rect::from_min_max(inner.min, pos2(inner.max.x, bars_top - padding));
                    self.draw_card(painter, body, verse, ink, alpha * card);
                }
                if card < 1.0 {
                    let font = theme::mono_sized((scale * 0.22).clamp(9.0, 14.0));
                    let number = self.scene.atlas.verses[verse].verse;
                    painter.text(
                        inner.min,
                        Align2::LEFT_TOP,
                        number,
                        font,
                        theme::fade(ink, alpha * (1.0 - card)),
                    );
                }
            }
        }
    }

    /// Une micro-barre par signal, en bas de la cellule. Renvoie l'ordonnée
    /// du haut de la zone des barres.
    fn draw_micro_bars(
        &self,
        painter: &Painter,
        inner: Rect,
        verse: usize,
        ink: Color32,
        alpha: f32,
    ) -> f32 {
        let signals = &self.scene.atlas.signals;
        let height = (inner.height() * 0.05).clamp(2.0, 6.0);
        let gap = (height * 0.7).max(1.0);
        let mut bottom = inner.max.y;
        for signal in signals.iter().rev() {
            let track = Rect::from_min_max(
                pos2(inner.min.x, bottom - height),
                pos2(inner.max.x, bottom),
            );
            painter.rect_filled(track, 1.0, theme::fade(ink, 0.18 * alpha));
            let rank = signal.rank.get(verse).copied().unwrap_or(f32::NAN);
            if rank.is_finite() {
                let fill = Rect::from_min_size(track.min, vec2(track.width() * rank, height));
                painter.rect_filled(fill, 1.0, theme::fade(ink, 0.85 * alpha));
            }
            bottom -= height + gap;
        }
        bottom + gap
    }

    /// Z3 : la cellule devient une carte avec la référence, la valeur et le
    /// texte de la première traduction de référence.
    fn draw_card(&mut self, painter: &Painter, body: Rect, verse: usize, ink: Color32, alpha: f32) {
        if body.height() < 24.0 {
            return;
        }
        let painter = painter.with_clip_rect(body);
        let color = theme::fade(ink, alpha);
        let v = self.scene.atlas.verses[verse];
        let code = self
            .scene
            .atlas
            .books
            .get(usize::from(v.book))
            .map_or("", |b| b.code.as_str());
        painter.text(
            body.min,
            Align2::LEFT_TOP,
            format!("{code} {}:{}", v.chapter, v.verse),
            theme::mono(),
            color,
        );
        let value = self.shown.get(verse).copied().unwrap_or(f32::NAN);
        if value.is_finite() {
            painter.text(
                body.right_top(),
                Align2::RIGHT_TOP,
                decimal(value, 2),
                theme::mono(),
                color,
            );
        }
        if let Some(text) = self.reference_text(0, verse) {
            let size = (body.width() * 0.07).clamp(11.0, 17.0);
            let galley = painter.layout(text, theme::sized(size), color, body.width());
            painter.galley(body.min + vec2(0.0, theme::SIZE_MONO + 8.0), galley, color);
        }
    }

    /// Contour de la sélection, visible même quand la cellule est minuscule.
    fn draw_selection_marker(&self, painter: &Painter, rect: Rect) {
        let Some(verse) = self.selected else { return };
        let scale = self.camera.scale;
        if scale >= self.cfg.z1_px {
            // Assez grande : le shader dessine déjà l'anneau dans la cellule.
            return;
        }
        let world = self.scene.position(self.layout, verse);
        let cell = Rect::from_min_size(self.to_screen(rect, world), Vec2::splat(scale));
        painter.rect_stroke(
            cell.expand(3.0),
            2.0,
            Stroke::new(1.5, theme::ACCENT),
            StrokeKind::Outside,
        );
    }

    /// Infobulle du verset survolé : référence, valeur, percentile, nombre
    /// de traductions et micro-barres.
    pub(crate) fn draw_tooltip(&self, painter: &Painter, rect: Rect, pointer: Pos2, verse: usize) {
        const WIDTH: f32 = 236.0;
        const PADDING: f32 = 10.0;
        const LINE: f32 = 18.0;
        const BAR_WIDTH: f32 = 84.0;
        let signals = &self.scene.atlas.signals;
        let height = PADDING * 2.0 + LINE * (3.0 + signals.len() as f32) + 4.0;

        // À droite et sous le pointeur, replié dans le canevas si besoin.
        let mut origin = pointer + vec2(16.0, 18.0);
        if origin.x + WIDTH > rect.max.x {
            origin.x = pointer.x - 16.0 - WIDTH;
        }
        if origin.y + height > rect.max.y {
            origin.y = pointer.y - 12.0 - height;
        }
        let frame = Rect::from_min_size(origin, vec2(WIDTH, height));
        // Ombre légère sous l'infobulle.
        painter.rect_filled(
            frame.translate(vec2(0.0, 3.0)).expand(1.0),
            8.0,
            Color32::from_black_alpha(70),
        );
        painter.rect(
            frame,
            f32::from(theme::RADIUS_CONTROL),
            theme::BG_ELEVATED,
            Stroke::new(1.0, theme::BORDER_SUBTLE),
            StrokeKind::Inside,
        );

        let left = origin.x + PADDING;
        let right = origin.x + WIDTH - PADDING;
        let mut y = origin.y + PADDING;
        painter.text(
            pos2(left, y),
            Align2::LEFT_TOP,
            self.scene.atlas.label_fr(verse),
            theme::base(),
            theme::TEXT_PRIMARY,
        );
        y += LINE;

        let composite = self.composite.get(verse).copied().unwrap_or(f32::NAN);
        let rank = self.composite_rank.get(verse).copied().unwrap_or(f32::NAN);
        let summary = if composite.is_finite() {
            format!("U {}  ·  {}", decimal(composite, 2), percentile(rank))
        } else {
            "U —  ·  pas de données".to_string()
        };
        painter.text(
            pos2(left, y),
            Align2::LEFT_TOP,
            summary,
            theme::mono(),
            theme::TEXT_PRIMARY,
        );
        y += LINE;

        let count = self.scene.atlas.n_eligible.get(verse).copied().unwrap_or(0);
        painter.text(
            pos2(left, y),
            Align2::LEFT_TOP,
            format!("{count} traductions"),
            theme::secondary(),
            theme::TEXT_SECONDARY,
        );
        y += LINE + 4.0;

        for signal in signals {
            let rank = signal.rank.get(verse).copied().unwrap_or(f32::NAN);
            painter.text(
                pos2(left, y),
                Align2::LEFT_TOP,
                &signal.label,
                theme::secondary(),
                theme::TEXT_SECONDARY,
            );
            let track = Rect::from_min_size(
                pos2(right - BAR_WIDTH - 34.0, y + 4.0),
                vec2(BAR_WIDTH, 6.0),
            );
            painter.rect_filled(track, 2.0, theme::BORDER_SUBTLE);
            if rank.is_finite() {
                let fill = Rect::from_min_size(track.min, vec2(BAR_WIDTH * rank, 6.0));
                painter.rect_filled(fill, 2.0, self.current_palette().color(rank));
            }
            painter.text(
                pos2(right, y),
                Align2::RIGHT_TOP,
                percentile(rank),
                theme::mono_sized(theme::SIZE_SECONDARY),
                theme::TEXT_SECONDARY,
            );
            y += LINE;
        }
    }

    /// Statistiques de rendu (F3).
    pub(crate) fn draw_stats(&self, painter: &Painter, rect: Rect, ctx: &egui::Context) {
        let dt = ctx.input(|i| i.stable_dt);
        let lines = [
            format!("{:.0} images/s", if dt > 0.0 { 1.0 / dt } else { 0.0 }),
            format!("CPU {} ms par image", decimal(self.frame_ms, 2)),
            format!("{} cellules, 1 appel de dessin", self.scene.n_verses()),
            format!(
                "{} px par cellule ({})",
                decimal(self.camera.scale, 1),
                self.zoom_level().label()
            ),
        ];
        let origin = rect.min + vec2(12.0, 12.0);
        let frame = Rect::from_min_size(origin, vec2(250.0, 12.0 + 16.0 * lines.len() as f32));
        painter.rect_filled(frame, 6.0, theme::fade(theme::BG_ELEVATED, 0.92));
        for (i, line) in lines.iter().enumerate() {
            painter.text(
                origin + vec2(8.0, 6.0 + 16.0 * i as f32),
                Align2::LEFT_TOP,
                line,
                theme::mono_sized(theme::SIZE_SECONDARY),
                theme::TEXT_SECONDARY,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fondu_des_etiquettes() {
        assert_eq!(fade_in(10.0, 12.0), 0.0);
        assert_eq!(fade_in(12.0, 12.0), 0.0);
        assert_eq!(fade_in(15.0, 12.0), 1.0);
        let half = fade_in(13.5, 12.0);
        assert!((half - 0.5).abs() < 1e-5);
    }

    #[test]
    fn nombres_a_la_francaise() {
        assert_eq!(decimal(0.871, 2), "0,87");
        assert_eq!(percentile(0.972), "p97");
        assert_eq!(percentile(f32::NAN), "—");
    }

    #[test]
    fn encre_selon_le_fond() {
        assert_eq!(ink(Color32::from_rgb(0xFF, 0xE9, 0xA8)), theme::BG_CANVAS);
        assert_eq!(
            ink(Color32::from_rgb(0x26, 0x2B, 0x52)),
            theme::TEXT_PRIMARY
        );
    }
}
