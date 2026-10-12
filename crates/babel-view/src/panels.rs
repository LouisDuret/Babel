//! Les panneaux : barre supérieure et recherche, panneau « Couches »,
//! inspecteur, barre inférieure.

use crate::app::{BabelApp, Suggestion};
use crate::overlay::{decimal, percentile};
use crate::palette::PaletteKind;
use crate::scene::{Layout, Metric};
use crate::scope::Selection;
use crate::theme;
use babel_core::books::suggest_books;
use babel_core::text::{fold, fold_keep_spaces};
use babel_core::{BOOKS, reference};
use egui::{Align, Color32, Frame, Margin, Rect, RichText, Sense, Stroke, StrokeKind, vec2};

/// Identifiant du champ de recherche, pour lui donner le focus (Ctrl+K).
const SEARCH_ID: &str = "babel-recherche";
/// Largeur du champ de recherche.
const SEARCH_WIDTH: f32 = 320.0;
/// Nombre maximal de propositions affichées.
const MAX_SUGGESTIONS: usize = 9;
/// Longueur minimale d'une recherche plein texte, en caractères.
const MIN_TEXT_QUERY: usize = 3;
/// Poids maximal réglable d'un signal.
const MAX_WEIGHT: f32 = 2.0;
/// Signaux prévus par le cahier des charges et absents de l'atlas.
const FUTURE_SIGNALS: [&str; 2] = ["Alignement", "Sémantique"];

/// Longueur minimale d'une recherche pour proposer une section du canon.
const MIN_SECTION_QUERY: usize = 2;

pub(crate) fn panel_frame() -> Frame {
    Frame::new().fill(theme::BG_PANEL)
}

pub(crate) fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(text)
            .font(theme::secondary())
            .color(theme::TEXT_SECONDARY),
    );
    ui.add_space(2.0);
}

/// Barre horizontale remplie à `share`, de la couleur donnée.
pub(crate) fn bar(ui: &mut egui::Ui, width: f32, share: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 8.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, theme::BORDER_SUBTLE);
    if share.is_finite() {
        let fill =
            Rect::from_min_size(rect.min, vec2(width * share.clamp(0.0, 1.0), rect.height()));
        ui.painter().rect_filled(fill, 3.0, color);
    }
}

impl BabelApp {
    pub(crate) fn top_bar(&mut self, ui: &mut egui::Ui, now: f64) {
        egui::Panel::top("barre-superieure")
            .exact_size(theme::TOP_BAR_HEIGHT)
            .frame(panel_frame().inner_margin(Margin::symmetric(14, 0)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        RichText::new("BABEL")
                            .size(15.0)
                            .strong()
                            .color(theme::TEXT_PRIMARY),
                    );
                    ui.add_space(10.0);
                    self.search_field(ui, now);
                    ui.add_space(10.0);

                    // Choix de la vue : contrôle segmenté.
                    for layout in Layout::ALL {
                        if ui
                            .selectable_label(self.layout == layout, layout.label())
                            .clicked()
                        {
                            self.set_layout(layout, now);
                        }
                    }
                    ui.add_space(10.0);

                    let mut metric = self.metric;
                    egui::ComboBox::from_id_salt("metrique")
                        .selected_text(self.metric_label(metric))
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            for candidate in self.metrics() {
                                ui.selectable_value(
                                    &mut metric,
                                    candidate,
                                    self.metric_label(candidate),
                                );
                            }
                        });
                    if metric != self.metric {
                        self.set_metric(metric);
                    }

                    egui::ComboBox::from_id_salt("palette")
                        .selected_text(self.palette.label())
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for kind in PaletteKind::ALL {
                                ui.selectable_value(&mut self.palette, kind, kind.label());
                            }
                        });

                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Capture").on_hover_text("Ctrl+S").clicked() {
                            self.request_capture(ui.ctx(), false);
                        }
                        if ui.button("Tout afficher").on_hover_text("F").clicked() {
                            self.fit_all(now);
                        }
                    });
                });
            });
    }

    fn search_field(&mut self, ui: &mut egui::Ui, now: f64) {
        let id = egui::Id::new(SEARCH_ID);
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .id(id)
                .hint_text("Rechercher « Jn 3:16 »…   Ctrl+K")
                .desired_width(SEARCH_WIDTH),
        );
        if self.focus_search {
            self.focus_search = false;
            response.request_focus();
        }
        if response.changed() {
            self.compute_suggestions();
        }
        // Entrée : la première proposition.
        if response.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some(first) = self.suggestions.first().cloned()
        {
            self.accept(&first, now);
        }
        if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.search.clear();
            self.suggestions.clear();
            ui.memory_mut(|m| m.surrender_focus(id));
        }
        self.search_rect = response.rect;
        self.search_focused = response.has_focus();
    }

    pub(crate) fn accept(&mut self, suggestion: &Suggestion, now: f64) {
        self.go(suggestion.selection, now);
        self.search.clear();
        self.suggestions.clear();
    }

    /// Nature d'une sélection, en clair, pour les propositions.
    fn selection_detail(&self, selection: Selection) -> String {
        let atlas = &self.scene.atlas;
        match selection {
            Selection::Verse(_) => "verset".to_string(),
            Selection::Chapter { .. } => "chapitre".to_string(),
            Selection::Book(book) => atlas
                .books
                .get(book)
                .map_or(String::new(), |b| format!("livre · {}", b.name_en)),
            Selection::Section(section) => {
                let books = self.scene.section_books(section).len();
                let plural = if books > 1 { "s" } else { "" };
                format!("section · {books} livre{plural}")
            }
        }
    }

    /// Propositions pour la recherche : référence exacte, sections du
    /// canon, livres, puis texte libre dans la première traduction de
    /// référence.
    pub(crate) fn compute_suggestions(&mut self) {
        self.suggestions.clear();
        let query = self.search.trim().to_string();
        if query.is_empty() {
            return;
        }

        let mut found: Vec<Selection> = Vec::new();
        if let Some(target) = reference::lookup(&query, &self.scene.versification) {
            found.push(Selection::from(target));
        }
        // Sections dont le nom commence par la recherche : « Évangiles ».
        let folded = fold(&query);
        if folded.chars().count() >= MIN_SECTION_QUERY {
            for (section, name) in self.scene.atlas.sections.iter().enumerate() {
                if fold(name).starts_with(&folded) && !self.scene.section_books(section).is_empty()
                {
                    found.push(Selection::Section(section));
                }
            }
        }
        for index in suggest_books(&query, MAX_SUGGESTIONS) {
            if let Some(book) = self.scene.versification.book_index(BOOKS[index].usfm) {
                found.push(Selection::Book(book));
            }
        }

        let mut suggestions: Vec<Suggestion> = Vec::new();
        for selection in found {
            if !suggestions.iter().any(|s| s.selection == selection) {
                suggestions.push(Suggestion {
                    label: self.scene.selection_label(selection),
                    detail: self.selection_detail(selection),
                    selection,
                });
            }
        }
        self.suggestions = suggestions;

        if query.chars().count() >= MIN_TEXT_QUERY && self.suggestions.len() < MAX_SUGGESTIONS {
            self.search_text(&query);
        }
        self.suggestions.truncate(MAX_SUGGESTIONS);
    }

    /// Recherche plein texte, insensible à la casse et aux accents.
    fn search_text(&mut self, query: &str) {
        if self.search_index.is_none() {
            let n = self.scene.n_verses();
            let index: Vec<String> = (0..n)
                .map(|verse| {
                    self.reference_text(0, verse)
                        .map_or(String::new(), |t| fold_keep_spaces(&t))
                })
                .collect();
            self.search_index = Some(index);
        }
        let needle = fold_keep_spaces(query);
        let Some(index) = &self.search_index else {
            return;
        };
        let hits: Vec<usize> = index
            .iter()
            .enumerate()
            .filter(|(_, text)| text.contains(&needle))
            .map(|(verse, _)| verse)
            .take(MAX_SUGGESTIONS.saturating_sub(self.suggestions.len()))
            .collect();
        for verse in hits {
            let excerpt: String = self
                .reference_text(0, verse)
                .unwrap_or_default()
                .chars()
                .take(54)
                .collect();
            self.suggestions.push(Suggestion {
                label: self.scene.atlas.label_fr(verse),
                detail: format!("{excerpt}…"),
                selection: Selection::Verse(verse),
            });
        }
    }

    /// Liste des propositions, sous le champ de recherche.
    pub(crate) fn search_popup(&mut self, ctx: &egui::Context, now: f64) {
        if self.suggestions.is_empty() || self.search.trim().is_empty() {
            return;
        }
        let position = self.search_rect.left_bottom() + vec2(0.0, 6.0);
        let mut chosen: Option<Suggestion> = None;
        egui::Area::new(egui::Id::new("babel-propositions"))
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::BG_ELEVATED)
                    .stroke(Stroke::new(1.0, theme::BORDER_SUBTLE))
                    .corner_radius(f32::from(theme::RADIUS_CONTROL))
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_width(SEARCH_WIDTH + 80.0);
                        for (index, suggestion) in self.suggestions.iter().enumerate() {
                            let text = RichText::new(&suggestion.label).color(theme::TEXT_PRIMARY);
                            let row = ui.horizontal(|ui| {
                                let clicked = ui.selectable_label(index == 0, text).clicked();
                                ui.label(
                                    RichText::new(&suggestion.detail)
                                        .font(theme::secondary())
                                        .color(theme::TEXT_SECONDARY),
                                );
                                clicked
                            });
                            if row.inner {
                                chosen = Some(suggestion.clone());
                            }
                        }
                    });
            });
        if let Some(suggestion) = chosen {
            self.accept(&suggestion, now);
        }
    }

    pub(crate) fn layers_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("couches")
            .exact_size(theme::LEFT_PANEL_WIDTH)
            .resizable(false)
            .frame(panel_frame().inner_margin(Margin::same(14)))
            .show(ui, |ui| {
                heading(ui, "COUCHES");
                if ui
                    .selectable_label(self.metric == Metric::Composite, "▣  Composite")
                    .clicked()
                {
                    self.set_metric(Metric::Composite);
                }
                ui.add_space(4.0);

                let mut weights_changed = false;
                for index in 0..self.scene.atlas.signals.len() {
                    let label = self.scene.atlas.signals[index].label.clone();
                    ui.horizontal(|ui| {
                        // Interrupteur : le signal entre ou non dans le composite.
                        weights_changed |= ui.checkbox(&mut self.enabled[index], "").changed();
                        let selected = self.metric == Metric::Signal(index);
                        if ui.selectable_label(selected, label).clicked() {
                            self.set_metric(Metric::Signal(index));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(26.0);
                        ui.spacing_mut().slider_width = 150.0;
                        let slider = egui::Slider::new(&mut self.weights[index], 0.0..=MAX_WEIGHT)
                            .show_value(false);
                        weights_changed |= ui.add_enabled(self.enabled[index], slider).changed();
                        ui.label(
                            RichText::new(decimal(self.weights[index], 2))
                                .font(theme::mono())
                                .color(theme::TEXT_SECONDARY),
                        );
                    });
                }
                if weights_changed {
                    // Le composite se recalcule en direct.
                    self.refresh_values(false);
                }
                for name in FUTURE_SIGNALS {
                    ui.add_enabled(
                        false,
                        egui::Label::new(
                            RichText::new(format!("○  {name} (phase 2)"))
                                .color(theme::TEXT_TERTIARY),
                        ),
                    );
                }

                ui.add_space(8.0);
                ui.separator();
                heading(ui, "PALETTE");
                ui.horizontal_wrapped(|ui| {
                    for kind in PaletteKind::ALL {
                        ui.selectable_value(&mut self.palette, kind, kind.label());
                    }
                });

                ui.add_space(8.0);
                ui.separator();
                heading(ui, "FILTRES");
                if ui
                    .checkbox(&mut self.protestant_canon, "Canon protestant")
                    .on_hover_text("Masque les livres deutérocanoniques")
                    .changed()
                {
                    self.refresh_values(true);
                }
                ui.label(
                    RichText::new("Familles, écritures, base textuelle, licence : phase 3.")
                        .font(theme::secondary())
                        .color(theme::TEXT_TERTIARY),
                );

                ui.with_layout(egui::Layout::bottom_up(Align::LEFT), |ui| {
                    let atlas = &self.scene.atlas;
                    let lines = [
                        format!("{} versets", atlas.n_verses()),
                        format!("{} langues", atlas.n_languages()),
                        format!(
                            "{} traductions actives sur {}",
                            atlas.n_active_translations(),
                            atlas.translations.len()
                        ),
                    ];
                    for line in lines {
                        ui.label(
                            RichText::new(line)
                                .font(theme::secondary())
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                    heading(ui, "CORPUS");
                });
            });
    }

    pub(crate) fn inspector_panel(&mut self, ui: &mut egui::Ui, now: f64) {
        let Some(selection) = self.selected else {
            return;
        };
        egui::Panel::right("inspecteur")
            .exact_size(theme::RIGHT_PANEL_WIDTH)
            .resizable(false)
            .frame(panel_frame().inner_margin(Margin::same(16)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| match selection {
                    Selection::Verse(verse) => self.inspector(ui, verse, now),
                    // Chapitre, livre ou section : le bilan de son contenu.
                    scope => self.scope_inspector(ui, scope, now),
                });
            });
    }

    fn inspector(&mut self, ui: &mut egui::Ui, verse: usize, now: f64) {
        let width = ui.available_width();
        let composite = self.composite.get(verse).copied().unwrap_or(f32::NAN);
        let rank = self.composite_rank.get(verse).copied().unwrap_or(f32::NAN);

        // Référence, U en grand, percentile.
        ui.label(
            RichText::new(self.scene.atlas.label_fr(verse))
                .size(theme::SIZE_TITLE)
                .color(theme::TEXT_PRIMARY),
        );
        ui.horizontal(|ui| {
            let value = if composite.is_finite() {
                decimal(composite, 2)
            } else {
                "—".to_string()
            };
            ui.label(
                RichText::new(format!("U {value}"))
                    .size(theme::SIZE_DISPLAY)
                    .color(theme::TEXT_PRIMARY),
            );
            ui.label(
                RichText::new(percentile(rank))
                    .font(theme::mono())
                    .color(theme::ACCENT),
            );
        });
        let count = self.scene.atlas.n_eligible.get(verse).copied().unwrap_or(0);
        ui.label(
            RichText::new(format!(
                "{count} traductions éligibles · intervalle de confiance : phase 2"
            ))
            .font(theme::secondary())
            .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(10.0);

        // Une barre par signal, avec son percentile.
        for index in 0..self.scene.atlas.signals.len() {
            let signal = &self.scene.atlas.signals[index];
            let rank = signal.rank.get(verse).copied().unwrap_or(f32::NAN);
            let raw = signal.raw.get(verse).copied().unwrap_or(f32::NAN);
            let color = if rank.is_finite() {
                self.current_palette().color(rank)
            } else {
                theme::BORDER_STRONG
            };
            ui.horizontal(|ui| {
                ui.add_sized(
                    vec2(86.0, 16.0),
                    egui::Label::new(RichText::new(&signal.label).color(theme::TEXT_SECONDARY)),
                );
                bar(ui, width - 86.0 - 110.0, rank, color);
                let raw = if raw.is_finite() {
                    decimal(raw, 3)
                } else {
                    "—".to_string()
                };
                ui.label(
                    RichText::new(format!("{} · {raw}", percentile(rank)))
                        .font(theme::mono_sized(theme::SIZE_SECONDARY))
                        .color(theme::TEXT_SECONDARY),
                );
            });
        }
        ui.add_space(10.0);
        ui.separator();

        // Traductions de référence.
        heading(ui, "TRADUCTIONS DE RÉFÉRENCE");
        let references = self.scene.atlas.references.clone();
        if references.is_empty() {
            ui.label(
                RichText::new("Aucune traduction de référence dans l'atlas.")
                    .color(theme::TEXT_TERTIARY),
            );
        }
        for (rank, translation) in references.iter().enumerate() {
            let title = self
                .scene
                .atlas
                .translations
                .get(usize::from(*translation))
                .map_or(String::new(), |t| t.title.clone());
            ui.label(
                RichText::new(title)
                    .font(theme::secondary())
                    .color(theme::ACCENT),
            );
            match self.reference_text(rank, verse) {
                Some(text) => ui.label(RichText::new(text).color(theme::TEXT_PRIMARY)),
                None => ui.label(RichText::new("(verset absent)").color(theme::TEXT_TERTIARY)),
            };
            ui.add_space(6.0);
        }
        ui.separator();

        self.chapter_strip(ui, verse, now);
        ui.add_space(8.0);
        ui.separator();
        ui.label(
            RichText::new(
                "Texte source en tuiles, distribution par traduction, traductions les plus \
                 divergentes et vérité terrain : phases 2 et 3.",
            )
            .font(theme::secondary())
            .color(theme::TEXT_TERTIARY),
        );
    }

    /// Mini-bande de chaleur du chapitre, pour le contexte. Un clic
    /// sélectionne le verset.
    fn chapter_strip(&mut self, ui: &mut egui::Ui, verse: usize, now: f64) {
        heading(ui, "CHAPITRE");
        let v = self.scene.atlas.verses[verse];
        let Some(chapter) = self
            .scene
            .atlas
            .books
            .get(usize::from(v.book))
            .and_then(|b| b.chapters.get(usize::from(v.chapter_index)))
        else {
            return;
        };
        let (first, len) = (chapter.first as usize, chapter.len as usize);
        let width = ui.available_width();
        // Vingt-cinq cases par rangée au plus.
        let per_row = len.clamp(1, 25);
        let size = (width / per_row as f32).min(18.0);
        let rows = len.div_ceil(per_row);
        let (rect, response) = ui.allocate_exact_size(
            vec2(size * per_row as f32, size * rows as f32),
            Sense::click(),
        );
        let painter = ui.painter_at(rect);
        for i in 0..len {
            let cell = Rect::from_min_size(
                rect.min + vec2((i % per_row) as f32 * size, (i / per_row) as f32 * size),
                vec2(size, size),
            )
            .shrink(1.0);
            painter.rect_filled(cell, 2.0, self.cell_color(first + i));
            if first + i == verse {
                painter.rect_stroke(
                    cell,
                    2.0,
                    Stroke::new(1.5, theme::ACCENT),
                    StrokeKind::Outside,
                );
            }
        }
        if response.clicked()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let offset = pointer - rect.min;
            let index = (offset.y / size) as usize * per_row + (offset.x / size) as usize;
            if index < len {
                self.selected = Some(Selection::Verse(first + index));
                let _ = now;
            }
        }
    }

    pub(crate) fn bottom_bar(&mut self, ui: &mut egui::Ui, now: f64) {
        egui::Panel::bottom("barre-inferieure")
            .exact_size(theme::BOTTOM_BAR_HEIGHT)
            .frame(panel_frame().inner_margin(Margin::symmetric(14, 0)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    self.breadcrumb(ui, now);
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let atlas = &self.scene.atlas;
                        let mut status = format!(
                            "{} traductions · {} langues",
                            atlas.n_active_translations(),
                            atlas.n_languages()
                        );
                        if self.show_stats {
                            let dt = ui.input(|i| i.stable_dt);
                            if dt > 0.0 {
                                status.push_str(&format!(" · {:.0} fps", 1.0 / dt));
                            }
                        }
                        ui.label(
                            RichText::new(status)
                                .font(theme::mono_sized(theme::SIZE_SECONDARY))
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.add_space(16.0);
                        self.legend(ui);
                    });
                });
            });
    }

    /// Fil d'Ariane cliquable : Bible › NT › Évangiles › Jean › 1 › 1. Chaque
    /// maillon sélectionne sa portée et y vole ; le dernier, en cyan, est la
    /// sélection courante.
    fn breadcrumb(&mut self, ui: &mut egui::Ui, now: f64) {
        let current = self.selected;
        let crumb =
            |ui: &mut egui::Ui, text: String, selection: Option<Selection>| {
                let active = selection.is_some() && selection == current;
                ui.add(
                    egui::Label::new(RichText::new(text).font(theme::secondary()).color(
                        if active {
                            theme::ACCENT
                        } else {
                            theme::TEXT_PRIMARY
                        },
                    ))
                    .sense(Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            };
        let separator = |ui: &mut egui::Ui| {
            ui.label(
                RichText::new("›")
                    .font(theme::secondary())
                    .color(theme::TEXT_TERTIARY),
            );
        };

        if crumb(ui, "Bible".to_string(), None) {
            self.selected = None;
            self.fit_all(now);
        }
        // La sélection, ou à défaut ce qui est sous le pointeur.
        let Some(shown) = self
            .selected
            .or(self.hovered_label)
            .or(self.hovered.map(Selection::Verse))
        else {
            return;
        };
        let path = self.scene.path(shown);
        let Some(section) = path.section else {
            return;
        };
        let section_name = self.scene.selection_label(Selection::Section(section));
        let testament = self
            .scene
            .section_books(section)
            .next()
            .and_then(|book| self.scene.atlas.books.get(book))
            .and_then(|book| ["AT", "NT", "DC"].get(usize::from(book.testament)).copied())
            .unwrap_or("");

        separator(ui);
        ui.label(
            RichText::new(testament)
                .font(theme::secondary())
                .color(theme::TEXT_SECONDARY),
        );
        let mut clicked: Option<Selection> = None;
        separator(ui);
        let selection = Selection::Section(section);
        if crumb(ui, section_name, Some(selection)) {
            clicked = Some(selection);
        }
        if let Some(book) = path.book {
            separator(ui);
            let selection = Selection::Book(book);
            if crumb(ui, self.scene.selection_label(selection), Some(selection)) {
                clicked = Some(selection);
            }
            if let Some(chapter) = path.chapter {
                separator(ui);
                let selection = Selection::Chapter { book, chapter };
                let number = self.scene.atlas.books[book].chapters[chapter].number;
                if crumb(ui, number.to_string(), Some(selection)) {
                    clicked = Some(selection);
                }
            }
        }
        if let Some(verse) = path.verse {
            separator(ui);
            let selection = Selection::Verse(verse);
            let number = self.scene.atlas.verses[verse].verse;
            if crumb(ui, number.to_string(), Some(selection)) {
                clicked = Some(selection);
            }
        }
        if let Some(selection) = clicked {
            self.go(selection, now);
        }
    }

    /// Légende de la palette : dégradé de 0 à 1 et nom de la métrique.
    fn legend(&self, ui: &mut egui::Ui) {
        const WIDTH: f32 = 140.0;
        const STEPS: usize = 70;
        let small = |text: String| {
            RichText::new(text)
                .font(theme::mono_sized(theme::SIZE_SECONDARY))
                .color(theme::TEXT_SECONDARY)
        };
        // Disposition de droite à gauche : les éléments sont ajoutés à l'envers.
        ui.label(small("1".to_string()));
        let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, 8.0), Sense::hover());
        let step = WIDTH / STEPS as f32;
        for i in 0..STEPS {
            let cell =
                Rect::from_min_size(rect.min + vec2(i as f32 * step, 0.0), vec2(step + 0.5, 8.0));
            ui.painter().rect_filled(
                cell,
                0.0,
                self.current_palette().color(i as f32 / (STEPS - 1) as f32),
            );
        }
        ui.label(small("0".to_string()));
        ui.label(small(self.metric_label(self.metric)));
    }
}
