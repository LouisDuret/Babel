//! Inspecteur d'une portée : le bilan d'un chapitre, d'un livre ou d'une
//! section du canon, sur le modèle de l'inspecteur d'un verset.

use crate::app::BabelApp;
use crate::overlay::{decimal, percentile};
use crate::panels::{bar, heading};
use crate::scope::{Child, Selection, Summary};
use crate::theme;
use egui::{Align2, Color32, CursorIcon, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};

/// Hauteur de l'histogramme de U.
const HISTOGRAM_HEIGHT: f32 = 64.0;
/// Hauteur d'une ligne de la liste des livres d'une section.
const CHILD_ROW_HEIGHT: f32 = 20.0;
/// Largeur réservée au nom d'un livre dans cette liste.
const CHILD_NAME_WIDTH: f32 = 148.0;
/// Hauteur d'une ligne de la liste des versets les plus incertains.
const TOP_ROW_HEIGHT: f32 = 36.0;
/// Nombre de cases par rangée dans la grille des chapitres ou des versets.
const GRID_COLUMNS: usize = 25;
/// Taille maximale d'une case de cette grille.
const GRID_CELL_MAX: f32 = 18.0;
/// Longueur de l'extrait de texte d'un verset listé, en caractères.
const EXCERPT_CHARS: usize = 50;

/// Entier avec séparateur de milliers insécable : « 3 779 ».
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{a0}');
        }
        out.push(c);
    }
    out
}

/// Quantité accordée : « 1 livre », « 4 livres ».
fn count(n: usize, noun: &str) -> String {
    let plural = if n > 1 { "s" } else { "" };
    format!("{} {noun}{plural}", thousands(n))
}

/// Rang en toutes lettres : « 1er », « 3e ».
fn ordinal(rank: usize) -> String {
    format!("{rank}{}", if rank == 1 { "er" } else { "e" })
}

fn value_or_dash(value: f32, digits: usize) -> String {
    if value.is_finite() {
        decimal(value, digits)
    } else {
        "—".to_string()
    }
}

impl BabelApp {
    /// Couleur d'une valeur de U, ou hachures éteintes sans données.
    fn value_color(&self, value: f32) -> Color32 {
        if value.is_finite() {
            self.current_palette().color(value)
        } else {
            theme::MISSING
        }
    }

    /// Bilan de la sélection : en-tête, signaux, distribution, contenu,
    /// versets les plus incertains.
    pub(crate) fn scope_inspector(&mut self, ui: &mut egui::Ui, selection: Selection, now: f64) {
        let Some(summary) = self.scope_summary(selection) else {
            return;
        };
        let width = ui.available_width();
        let path = self.scene.path(selection);

        // Nature de la sélection et ce qui la contient.
        let parent = |parent: Option<Selection>| {
            parent.map_or(String::new(), |p| self.scene.selection_label(p))
        };
        let (kind, context, peers) = match selection {
            Selection::Section(section) => {
                let testament = self
                    .scene
                    .section_books(section)
                    .next()
                    .and_then(|book| self.scene.atlas.books.get(book))
                    .and_then(|book| {
                        ["Ancien Testament", "Nouveau Testament", "Deutérocanoniques"]
                            .get(usize::from(book.testament))
                            .copied()
                    })
                    .unwrap_or("");
                ("SECTION", testament.to_string(), "sections")
            }
            Selection::Book(_) => (
                "LIVRE",
                parent(path.section.map(Selection::Section)),
                "livres",
            ),
            Selection::Chapter { .. } => (
                "CHAPITRE",
                parent(path.book.map(Selection::Book)),
                "chapitres",
            ),
            Selection::Verse(_) => return,
        };
        ui.label(
            RichText::new(format!("{kind} · {context}"))
                .font(theme::secondary())
                .color(theme::TEXT_SECONDARY),
        );
        ui.label(
            RichText::new(self.scene.selection_label(selection))
                .size(theme::SIZE_TITLE)
                .color(theme::TEXT_PRIMARY),
        );

        // U moyen en grand, rang parmi les pairs.
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("U {}", value_or_dash(summary.mean, 2)))
                    .size(theme::SIZE_DISPLAY)
                    .color(theme::TEXT_PRIMARY),
            );
            if let Some((rank, total)) = summary.rank {
                ui.label(
                    RichText::new(format!("{} sur {total} {peers}", ordinal(rank)))
                        .font(theme::mono())
                        .color(theme::ACCENT),
                )
                .on_hover_text(
                    "Classement par U moyen décroissant : le 1er est le plus incertain.",
                );
            }
        });

        let mut counts: Vec<String> = Vec::new();
        if matches!(selection, Selection::Section(_)) {
            counts.push(count(summary.books, "livre"));
        }
        if !matches!(selection, Selection::Chapter { .. }) {
            counts.push(count(summary.chapters, "chapitre"));
        }
        counts.push(count(summary.verses, "verset"));
        let small = |text: String| {
            RichText::new(text)
                .font(theme::secondary())
                .color(theme::TEXT_SECONDARY)
        };
        ui.label(small(counts.join(" · ")));

        if summary.scored == 0 {
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "Aucun verset noté : trop peu de traductions éligibles couvrent cette \
                     sélection.",
                )
                .color(theme::TEXT_SECONDARY),
            );
            return;
        }
        ui.label(small(format!(
            "U moyen de {} {} · médiane {} · {} traductions (médiane)",
            thousands(summary.scored),
            if summary.scored > 1 {
                "versets notés"
            } else {
                "verset noté"
            },
            decimal(summary.median, 2),
            summary.translations
        )));
        ui.label(small(format!(
            "{:.0} % de versets très incertains (top {:.0} % de la Bible)",
            summary.hot_share * 100.0,
            (1.0 - self.cfg.hot_rank) * 100.0
        )));
        ui.add_space(10.0);

        // Une barre par signal : son rang percentile moyen.
        for (index, mean) in summary.signals.iter().enumerate() {
            let label = self.scene.atlas.signals[index].label.clone();
            let color = if mean.is_finite() {
                self.current_palette().color(*mean)
            } else {
                theme::BORDER_STRONG
            };
            ui.horizontal(|ui| {
                ui.add_sized(
                    vec2(86.0, 16.0),
                    egui::Label::new(RichText::new(label).color(theme::TEXT_SECONDARY)),
                );
                bar(ui, width - 86.0 - 110.0, *mean, color);
                ui.label(
                    RichText::new(format!("moy. {}", percentile(*mean)))
                        .font(theme::mono_sized(theme::SIZE_SECONDARY))
                        .color(theme::TEXT_SECONDARY),
                );
            });
        }
        ui.add_space(10.0);
        ui.separator();

        heading(ui, "DISTRIBUTION DE U");
        self.histogram(ui, &summary);
        ui.label(small(
            "De U = 0 à U = 1. Le trait marque la Bible entière.".to_string(),
        ));
        ui.add_space(6.0);
        ui.separator();

        // Le contenu : livres d'une section, chapitres d'un livre, versets
        // d'un chapitre. Un clic y descend.
        let mut clicked = match selection {
            Selection::Section(_) => {
                heading(ui, "LIVRES");
                // Lignes jointives : le survol dessine une bande continue.
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    self.child_rows(ui, &summary.children)
                })
                .inner
            }
            Selection::Book(_) => {
                heading(ui, "CHAPITRES");
                self.child_grid(ui, &summary.children, "Chapitre")
            }
            _ => {
                heading(ui, "VERSETS");
                self.child_grid(ui, &summary.children, "Verset")
            }
        };
        ui.add_space(8.0);
        ui.separator();

        heading(ui, "VERSETS LES PLUS INCERTAINS");
        let top = ui
            .scope(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                self.top_rows(ui, &summary.top)
            })
            .inner;
        if let Some(verse) = top {
            clicked = Some(Selection::Verse(verse));
        }
        if let Some(target) = clicked {
            self.go(target, now);
        }
    }

    /// Histogramme de U dans la sélection, avec la Bible entière en repère.
    fn histogram(&self, ui: &mut egui::Ui, summary: &Summary) {
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), HISTOGRAM_HEIGHT), Sense::hover());
        let painter = ui.painter_at(rect);
        let bins = summary.histogram.len().max(1);
        let peak = summary
            .histogram
            .iter()
            .chain(&summary.baseline)
            .fold(f32::MIN_POSITIVE, |m, share| m.max(*share));
        let step = rect.width() / bins as f32;
        for (i, share) in summary.histogram.iter().enumerate() {
            let left = rect.min.x + i as f32 * step;
            let height = share / peak * rect.height();
            let column = Rect::from_min_max(
                pos2(left + 1.0, rect.max.y - height),
                pos2(left + step - 1.0, rect.max.y),
            );
            let color = self.current_palette().color((i as f32 + 0.5) / bins as f32);
            painter.rect_filled(column, 1.0, color);
            if let Some(reference) = summary.baseline.get(i) {
                let y = rect.max.y - reference / peak * rect.height();
                painter.hline(
                    left..=left + step,
                    y.max(rect.min.y + 0.5),
                    Stroke::new(1.0, theme::TEXT_SECONDARY),
                );
            }
        }
        painter.hline(
            rect.x_range(),
            rect.max.y - 0.5,
            Stroke::new(1.0, theme::BORDER_STRONG),
        );
    }

    /// Liste des livres d'une section : nom, barre de U moyen, valeur.
    /// Renvoie le livre cliqué.
    fn child_rows(&self, ui: &mut egui::Ui, children: &[Child]) -> Option<Selection> {
        let mut clicked = None;
        let width = ui.available_width();
        for child in children {
            let (rect, response) =
                ui.allocate_exact_size(vec2(width, CHILD_ROW_HEIGHT), Sense::click());
            let response = response.on_hover_cursor(CursorIcon::PointingHand);
            let painter = ui.painter_at(rect);
            let hovered = response.hovered();
            if hovered {
                painter.rect_filled(rect, 4.0, theme::BG_ELEVATED);
            }
            // Le nom, coupé s'il déborde.
            let name =
                Rect::from_min_max(rect.min, pos2(rect.min.x + CHILD_NAME_WIDTH, rect.max.y));
            painter.with_clip_rect(name).text(
                rect.left_center() + vec2(4.0, 0.0),
                Align2::LEFT_CENTER,
                &child.label,
                theme::base(),
                if hovered {
                    theme::ACCENT
                } else {
                    theme::TEXT_PRIMARY
                },
            );
            let track = Rect::from_min_max(
                pos2(rect.min.x + CHILD_NAME_WIDTH + 6.0, rect.center().y - 3.0),
                pos2(rect.max.x - 42.0, rect.center().y + 3.0),
            );
            painter.rect_filled(track, 2.0, theme::BORDER_SUBTLE);
            if child.mean.is_finite() {
                let fill = Rect::from_min_size(
                    track.min,
                    vec2(track.width() * child.mean.clamp(0.0, 1.0), track.height()),
                );
                painter.rect_filled(fill, 2.0, self.value_color(child.mean));
            }
            painter.text(
                rect.right_center() - vec2(4.0, 0.0),
                Align2::RIGHT_CENTER,
                value_or_dash(child.mean, 2),
                theme::mono_sized(theme::SIZE_SECONDARY),
                theme::TEXT_SECONDARY,
            );
            if response.clicked() {
                clicked = Some(child.selection);
            }
        }
        clicked
    }

    /// Grille des chapitres d'un livre ou des versets d'un chapitre, chaque
    /// case colorée par son U. Renvoie la case cliquée.
    fn child_grid(&self, ui: &mut egui::Ui, children: &[Child], noun: &str) -> Option<Selection> {
        if children.is_empty() {
            return None;
        }
        let per_row = children.len().clamp(1, GRID_COLUMNS);
        let size = (ui.available_width() / per_row as f32).min(GRID_CELL_MAX);
        let rows = children.len().div_ceil(per_row);
        let (rect, response) = ui.allocate_exact_size(
            vec2(size * per_row as f32, size * rows as f32),
            Sense::click(),
        );
        let cell = |i: usize| {
            Rect::from_min_size(
                rect.min + vec2((i % per_row) as f32 * size, (i / per_row) as f32 * size),
                vec2(size, size),
            )
            .shrink(1.0)
        };
        let painter = ui.painter_at(rect);
        for (i, child) in children.iter().enumerate() {
            painter.rect_filled(cell(i), 2.0, self.value_color(child.mean));
        }

        // Case sous le pointeur.
        let hovered = response.hover_pos().and_then(|pointer| {
            let offset = pointer - rect.min;
            let index = (offset.y / size) as usize * per_row + (offset.x / size) as usize;
            (offset.x >= 0.0 && offset.y >= 0.0 && index < children.len()).then_some(index)
        });
        let index = hovered?;
        let child = &children[index];
        painter.rect_stroke(
            cell(index),
            2.0,
            Stroke::new(1.5, theme::TEXT_PRIMARY),
            StrokeKind::Outside,
        );
        let response = response
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text_at_pointer(format!(
                "{noun} {} · U {}",
                child.label,
                value_or_dash(child.mean, 2)
            ));
        response.clicked().then_some(child.selection)
    }

    /// Liste des versets les plus incertains : référence, U, début du texte
    /// de référence. Renvoie le verset cliqué.
    fn top_rows(&mut self, ui: &mut egui::Ui, top: &[(usize, f32)]) -> Option<usize> {
        let mut clicked = None;
        let width = ui.available_width();
        for (verse, value) in top {
            let excerpt = self.reference_text(0, *verse).map(|text| {
                let mut short: String = text.chars().take(EXCERPT_CHARS).collect();
                if text.chars().count() > EXCERPT_CHARS {
                    short.push('…');
                }
                short
            });
            let (rect, response) =
                ui.allocate_exact_size(vec2(width, TOP_ROW_HEIGHT), Sense::click());
            let response = response.on_hover_cursor(CursorIcon::PointingHand);
            let painter = ui.painter_at(rect);
            let hovered = response.hovered();
            if hovered {
                painter.rect_filled(rect, 4.0, theme::BG_ELEVATED);
            }
            let chip = Rect::from_min_size(rect.min + vec2(4.0, 7.0), vec2(8.0, 8.0));
            painter.rect_filled(chip, 2.0, self.value_color(*value));
            painter.text(
                rect.min + vec2(18.0, 3.0),
                Align2::LEFT_TOP,
                self.scene.atlas.label_fr(*verse),
                theme::base(),
                if hovered {
                    theme::ACCENT
                } else {
                    theme::TEXT_PRIMARY
                },
            );
            painter.text(
                pos2(rect.max.x - 4.0, rect.min.y + 4.0),
                Align2::RIGHT_TOP,
                decimal(*value, 2),
                theme::mono_sized(theme::SIZE_SECONDARY),
                theme::TEXT_SECONDARY,
            );
            if let Some(excerpt) = excerpt {
                painter.text(
                    rect.min + vec2(18.0, 20.0),
                    Align2::LEFT_TOP,
                    excerpt,
                    theme::secondary(),
                    theme::TEXT_SECONDARY,
                );
            }
            if response.clicked() {
                clicked = Some(*verse);
            }
        }
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombres_et_accords() {
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(879), "879");
        assert_eq!(thousands(3779), "3\u{a0}779");
        assert_eq!(thousands(41899), "41\u{a0}899");
        assert_eq!(thousands(1_000_000), "1\u{a0}000\u{a0}000");
        assert_eq!(count(1, "livre"), "1 livre");
        assert_eq!(count(4, "livre"), "4 livres");
        assert_eq!(ordinal(1), "1er");
        assert_eq!(ordinal(12), "12e");
        assert_eq!(value_or_dash(f32::NAN, 2), "—");
        assert_eq!(value_or_dash(0.5, 2), "0,50");
    }
}
