//! Jetons du design system (dark mode). Tous les composants lisent leurs
//! couleurs et leurs tailles ici ; aucune couleur n'est écrite ailleurs.
//!
//! Règle d'ensemble : les couleurs chaudes sont réservées aux données, le
//! cyan à l'interaction. L'interface reste en gris-bleu très sombres.

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

/// Fond du canevas.
pub const BG_CANVAS: Color32 = Color32::from_rgb(0x0A, 0x0C, 0x10);
/// Panneaux latéraux et barres.
pub const BG_PANEL: Color32 = Color32::from_rgb(0x12, 0x15, 0x1B);
/// Infobulles, menus, cartes de verset.
pub const BG_ELEVATED: Color32 = Color32::from_rgb(0x1A, 0x1E, 0x26);
/// Séparateurs.
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(0x25, 0x2A, 0x34);
/// Contours des contrôles actifs.
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x34, 0x3B, 0x48);
/// Texte principal.
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xE7, 0xE9, 0xEE);
/// Libellés, légendes.
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0x9A, 0xA3, 0xB2);
/// Indications non essentielles uniquement.
pub const TEXT_TERTIARY: Color32 = Color32::from_rgb(0x64, 0x6D, 0x7C);
/// Sélection, focus clavier, curseurs actifs.
pub const ACCENT: Color32 = Color32::from_rgb(0x22, 0xD3, 0xEE);
/// Contour de la cellule survolée : blanc, à cette opacité.
pub const HOVER_OPACITY: f32 = 0.6;
/// Hachures d'un verset sans données.
pub const MISSING: Color32 = Color32::from_rgb(0x2A, 0x2F, 0x3A);

/// Rayon des panneaux.
pub const RADIUS_PANEL: u8 = 8;
/// Rayon des contrôles.
pub const RADIUS_CONTROL: u8 = 6;

/// Hauteur de la barre supérieure.
pub const TOP_BAR_HEIGHT: f32 = 48.0;
/// Hauteur de la barre inférieure.
pub const BOTTOM_BAR_HEIGHT: f32 = 28.0;
/// Largeur du panneau « Couches ».
pub const LEFT_PANEL_WIDTH: f32 = 280.0;
/// Largeur de l'inspecteur.
pub const RIGHT_PANEL_WIDTH: f32 = 380.0;

/// Taille de base de l'interface.
pub const SIZE_BASE: f32 = 13.0;
/// Taille secondaire : libellés, légendes.
pub const SIZE_SECONDARY: f32 = 11.0;
/// Références et valeurs, en chasse fixe.
pub const SIZE_MONO: f32 = 12.0;
/// Titres de l'inspecteur.
pub const SIZE_TITLE: f32 = 20.0;
pub const SIZE_DISPLAY: f32 = 28.0;

/// Durée des transitions de l'interface, en secondes.
pub const UI_TRANSITION: f32 = 0.15;

pub fn base() -> FontId {
    FontId::new(SIZE_BASE, FontFamily::Proportional)
}

pub fn secondary() -> FontId {
    FontId::new(SIZE_SECONDARY, FontFamily::Proportional)
}

pub fn mono() -> FontId {
    FontId::new(SIZE_MONO, FontFamily::Monospace)
}

/// Police à chasse fixe d'une taille donnée, arrondie au pixel.
///
/// Les étiquettes du canevas grandissent avec le zoom. Sans arrondi, chaque
/// image demanderait une taille de police inédite, donc de nouveaux glyphes
/// à tramer : l'arrondi borne le nombre de tailles en circulation.
pub fn mono_sized(size: f32) -> FontId {
    FontId::new(size.round(), FontFamily::Monospace)
}

/// Police proportionnelle d'une taille donnée, arrondie au pixel.
pub fn sized(size: f32) -> FontId {
    FontId::new(size.round(), FontFamily::Proportional)
}

/// Couleur avec une opacité multipliée par `alpha` (0 à 1).
pub fn fade(color: Color32, alpha: f32) -> Color32 {
    color.gamma_multiply(alpha.clamp(0.0, 1.0))
}

/// Luminance relative WCAG d'une couleur sRGB.
pub fn relative_luminance(color: Color32) -> f32 {
    let channel = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

/// Rapport de contraste WCAG entre deux couleurs opaques.
#[cfg(test)]
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Applique le thème sombre au contexte egui.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);

    let mut v = egui::Visuals::dark();
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_ELEVATED;
    v.extreme_bg_color = BG_CANVAS;
    v.faint_bg_color = BG_ELEVATED;
    v.code_bg_color = BG_ELEVATED;
    v.override_text_color = Some(TEXT_PRIMARY);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = fade(ACCENT, 0.35);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.window_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    v.window_corner_radius = CornerRadius::same(RADIUS_PANEL);
    v.menu_corner_radius = CornerRadius::same(RADIUS_CONTROL);
    v.slider_trailing_fill = true;

    let control = CornerRadius::same(RADIUS_CONTROL);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BG_PANEL;
    w.noninteractive.weak_bg_fill = BG_PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    w.noninteractive.corner_radius = control;

    w.inactive.bg_fill = BG_ELEVATED;
    w.inactive.weak_bg_fill = BG_ELEVATED;
    w.inactive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    w.inactive.corner_radius = control;

    w.hovered.bg_fill = BORDER_SUBTLE;
    w.hovered.weak_bg_fill = BORDER_SUBTLE;
    w.hovered.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    w.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    w.hovered.corner_radius = control;

    w.active.bg_fill = BORDER_STRONG;
    w.active.weak_bg_fill = BORDER_STRONG;
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    w.active.corner_radius = control;

    w.open.bg_fill = BG_ELEVATED;
    w.open.weak_bg_fill = BG_ELEVATED;
    w.open.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    w.open.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    w.open.corner_radius = control;

    ctx.set_visuals_of(egui::Theme::Dark, v);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Body, base()),
            (TextStyle::Button, base()),
            (TextStyle::Small, secondary()),
            (TextStyle::Monospace, mono()),
            (TextStyle::Heading, sized(SIZE_TITLE)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.animation_time = UI_TRANSITION;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tout texte essentiel respecte WCAG AA (≥ 4,5:1 sur son fond).
    #[test]
    fn contraste_wcag_aa() {
        const AA: f32 = 4.5;
        for background in [BG_CANVAS, BG_PANEL, BG_ELEVATED] {
            assert!(contrast(TEXT_PRIMARY, background) >= AA);
            assert!(contrast(TEXT_SECONDARY, background) >= AA);
            assert!(contrast(ACCENT, background) >= AA);
        }
    }

    #[test]
    fn contraste_connu() {
        let ratio = contrast(Color32::WHITE, Color32::BLACK);
        assert!((ratio - 21.0).abs() < 1e-3);
    }
}
