//! L'application : état, boucle d'image, interactions du canevas.

use crate::camera::{Camera, Flight, ease_in_out};
use crate::canvas::{CanvasFrame, CanvasResources, Uniforms};
use crate::palette::{Palette, PaletteKind};
use crate::scene::{Layout, Metric, Scene};
use crate::scope::{Selection, Summary};
use crate::state::Saved;
use crate::{Options, theme};
use babel_atlas::texts::TextReader;
use babel_core::config::View;
use babel_core::reference;
use babel_core::stats::percentile_ranks;
use egui::{Color32, Key, Modifiers, PointerButton, Pos2, Rect, Vec2};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// Défilement, en points, d'un cran de molette dans egui.
const WHEEL_NOTCH_POINTS: f32 = 50.0;
/// Déplacement au clavier (Maj + flèches), en part de la vue.
const KEY_PAN_SHARE: f32 = 0.15;
/// Zoom minimal, en part de l'échelle qui affiche toute la carte.
const MIN_ZOOM_SHARE: f32 = 0.6;
/// Écart relatif d'échelle en dessous duquel la caméra est considérée comme
/// cadrant toute la carte.
const FIT_TOLERANCE: f32 = 0.005;
/// Images rendues avant une capture demandée en ligne de commande, le temps
/// que la mise en page se stabilise.
const SCREENSHOT_SETTLE_FRAMES: u64 = 4;
/// Temps laissé à l'infobulle avant une capture avec pointeur simulé, en
/// secondes.
const SIMULATED_POINTER_SETTLE: f64 = 0.6;
/// Rang de l'entrée à laquelle le clic simulé enfonce le bouton ; il le
/// relâche à la suivante.
const SIMULATED_CLICK_FRAME: u64 = 8;

/// Taille de cellule atteinte au plus fort du zoom pendant une mesure.
const BENCH_MAX_CELL_PX: f32 = 48.0;
/// Durée d'image au-delà de laquelle la cadence passe sous 120 images/s.
const BENCH_SLOW_FRAME_MS: f32 = 1000.0 / 120.0;
/// Images attendues avant une mesure, le temps que la fenêtre s'agrandisse.
const BENCH_SETTLE_FRAMES: u64 = 20;
/// Images ignorées au début d'une mesure (création des ressources).
const BENCH_WARMUP_FRAMES: u32 = 30;

/// Mesure de performance : un zoom et un panoramique scriptés, sans
/// plafond de fréquence.
struct Bench {
    total: u32,
    frame: u32,
    last: std::time::Instant,
    /// Durée de chaque image, en millisecondes.
    intervals: Vec<f32>,
    /// Temps CPU de chaque image, en millisecondes.
    cpu: Vec<f32>,
}

/// Niveau du zoom sémantique, selon la taille d'une cellule à l'écran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ZoomLevel {
    /// Bible : couleurs seules, noms des sections et des livres.
    Z0,
    /// Livre : numéros de chapitres.
    Z1,
    /// Chapitre : numéro de verset et micro-barres.
    Z2,
    /// Verset : la cellule devient une carte.
    Z3,
    /// Mot (phase 3).
    Z4,
}

impl ZoomLevel {
    pub fn label(self) -> &'static str {
        match self {
            ZoomLevel::Z0 => "Z0 Bible",
            ZoomLevel::Z1 => "Z1 Livre",
            ZoomLevel::Z2 => "Z2 Chapitre",
            ZoomLevel::Z3 => "Z3 Verset",
            ZoomLevel::Z4 => "Z4 Mot",
        }
    }
}

/// Une proposition de la recherche.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub label: String,
    pub detail: String,
    pub selection: Selection,
}

/// Bilan de la sélection courante, gardé tant que ni la sélection ni les
/// valeurs ne changent.
struct CachedSummary {
    selection: Selection,
    version: u64,
    summary: Rc<Summary>,
}

pub struct BabelApp {
    pub(crate) scene: Scene,
    pub(crate) texts: TextReader,
    pub(crate) cfg: View,
    pub(crate) palettes: Vec<Palette>,
    srgb_target: bool,

    pub(crate) layout: Layout,
    /// Transition entre vues : (départ, arrivée, instant de départ).
    transition: (f32, f32, f64),
    pub(crate) camera: Camera,
    flight: Option<Flight>,
    /// Rectangle du canevas à la dernière image.
    pub(crate) view: Rect,
    initialized: bool,

    pub(crate) metric: Metric,
    pub(crate) palette: PaletteKind,
    pub(crate) weights: Vec<f32>,
    pub(crate) enabled: Vec<bool>,
    pub(crate) protestant_canon: bool,
    /// Composite U(v) courant, recalculé à chaque changement de poids.
    pub(crate) composite: Vec<f32>,
    /// Rang percentile du composite, pour l'inspecteur et l'infobulle.
    pub(crate) composite_rank: Vec<f32>,
    /// Valeurs de la métrique affichée, pour les libellés et l'inspecteur.
    pub(crate) shown: Vec<f32>,
    uploads: Vec<(usize, Arc<Vec<f32>>)>,

    pub(crate) hovered: Option<usize>,
    hover_since: f64,
    pub(crate) pointer: Option<Pos2>,
    pub(crate) selected: Option<Selection>,
    /// Zones cliquables des noms de livres et de sections, relevées à
    /// l'image précédente.
    pub(crate) label_hits: Vec<(Rect, Selection)>,
    /// Nom de livre ou de section sous le pointeur.
    pub(crate) hovered_label: Option<Selection>,
    /// Vérifier à la prochaine image que la sélection est visible.
    reveal: bool,
    /// Compteur incrémenté à chaque recalcul du composite.
    values_version: u64,
    summary: Option<CachedSummary>,

    pub(crate) search: String,
    pub(crate) suggestions: Vec<Suggestion>,
    pub(crate) focus_search: bool,
    /// Rectangle du champ de recherche, pour y ancrer les propositions.
    pub(crate) search_rect: Rect,
    pub(crate) search_focused: bool,
    /// Textes de la première traduction de référence, repliés pour la
    /// recherche plein texte. Construit à la première recherche.
    pub(crate) search_index: Option<Vec<String>>,

    pub(crate) show_left: bool,
    pub(crate) show_right: bool,
    pub(crate) show_stats: bool,
    pub(crate) frame_ms: f32,

    state_path: PathBuf,
    saved: Option<Saved>,
    initial_at: Option<String>,
    initial_search: Option<String>,
    /// Position de pointeur injectée à chaque image (captures de test).
    simulated_pointer: Option<[f32; 2]>,
    /// Clic simulé à la position du pointeur simulé.
    simulated_click: bool,
    /// Nombre d'entrées déjà passées par le pointeur simulé.
    simulated_inputs: u64,
    screenshot: Option<PathBuf>,
    screenshot_requested: bool,
    capture_dir: PathBuf,
    frames: u64,
    /// Agrandir la fenêtre à la première image.
    maximize: bool,
    bench: Option<Bench>,
}

impl BabelApp {
    pub fn new(cc: &eframe::CreationContext<'_>, options: Options) -> Result<Self, String> {
        theme::apply(&cc.egui_ctx);

        let atlas = babel_atlas::read(&options.atlas).map_err(|e| e.to_string())?;
        let texts = TextReader::new(&options.texts, &atlas.text_blocks);
        let scene = Scene::new(atlas).map_err(|e| e.to_string())?;
        let n_signals = scene.atlas.signals.len();

        // L'état enregistré ne vaut que s'il décrit les mêmes signaux, et
        // une capture en ligne de commande part toujours de l'état par défaut.
        let saved = Saved::load(&options.state)
            .filter(|s| s.weights.len() == n_signals && s.enabled.len() == n_signals)
            .filter(|_| options.screenshot.is_none() && options.bench.is_none());

        let mut weights: Vec<f32> = scene.atlas.signals.iter().map(|s| s.weight).collect();
        let mut enabled = vec![true; n_signals];
        let mut layout = Layout::Wall;
        let mut metric = Metric::Composite;
        let mut palette = PaletteKind::Braise;
        let mut protestant_canon = false;
        let (mut show_left, mut show_right) = (true, true);
        if let Some(s) = &saved {
            weights.clone_from(&s.weights);
            enabled.clone_from(&s.enabled);
            layout = Layout::from_code(&s.layout).unwrap_or(layout);
            metric = metric_from_code(&scene, &s.metric).unwrap_or(metric);
            palette = PaletteKind::from_code(&s.palette).unwrap_or(palette);
            protestant_canon = s.protestant_canon;
            show_left = s.show_left;
            show_right = s.show_right;
        }
        // Les options de la ligne de commande l'emportent.
        if let Some(code) = &options.metric {
            metric = metric_from_code(&scene, code).ok_or_else(|| {
                format!("métrique inconnue : {code} (composite, omit, len, intra)")
            })?;
        }
        if let Some(code) = &options.layout {
            layout = Layout::from_code(code)
                .ok_or_else(|| format!("vue inconnue : {code} (mur, hilbert)"))?;
        }

        let palettes: Vec<Palette> = PaletteKind::ALL.iter().map(|k| Palette::new(*k)).collect();
        let composite = scene.composite(&weights, &enabled);
        let composite_rank = percentile_ranks(&composite);
        let shown = scene.metric(metric, &composite);

        // Un buffer de valeurs par métrique : le composite puis chaque signal.
        let mut values = vec![scene.gpu_values(&composite, protestant_canon)];
        for signal in &scene.atlas.signals {
            values.push(scene.gpu_values(&signal.rank, protestant_canon));
        }
        let render = cc
            .wgpu_render_state
            .as_ref()
            .ok_or("le rendu wgpu n'est pas disponible sur cette machine")?;
        let resources = CanvasResources::new(
            &render.device,
            &render.queue,
            render.target_format,
            &scene.atlas.wall_positions,
            &scene.atlas.hilbert_positions,
            &values,
            &palettes,
        );
        render.renderer.write().callback_resources.insert(resources);

        let transition = if layout == Layout::Hilbert { 1.0 } else { 0.0 };
        let capture_dir = options
            .state
            .parent()
            .map_or_else(|| PathBuf::from("."), |p| p.join("captures"));
        Ok(Self {
            scene,
            texts,
            cfg: options.view,
            palettes,
            srgb_target: render.target_format.is_srgb(),
            layout,
            transition: (transition, transition, 0.0),
            camera: Camera {
                center: Vec2::ZERO,
                scale: 1.0,
            },
            flight: None,
            view: Rect::NOTHING,
            initialized: false,
            metric,
            palette,
            weights,
            enabled,
            protestant_canon,
            composite,
            composite_rank,
            shown,
            uploads: Vec::new(),
            hovered: None,
            hover_since: 0.0,
            pointer: None,
            selected: None,
            label_hits: Vec::new(),
            hovered_label: None,
            reveal: false,
            values_version: 0,
            summary: None,
            search: String::new(),
            suggestions: Vec::new(),
            focus_search: false,
            search_rect: Rect::NOTHING,
            search_focused: false,
            search_index: None,
            show_left,
            show_right,
            show_stats: options.stats,
            frame_ms: 0.0,
            state_path: options.state,
            saved,
            initial_at: options.at,
            initial_search: options.search,
            simulated_pointer: options.pointer,
            simulated_click: options.click,
            simulated_inputs: 0,
            screenshot: options.screenshot,
            screenshot_requested: false,
            capture_dir,
            frames: 0,
            maximize: options.maximize,
            bench: options.bench.map(|total| Bench {
                total: total.max(1),
                frame: 0,
                last: std::time::Instant::now(),
                intervals: Vec::new(),
                cpu: Vec::new(),
            }),
        })
    }

    /// Index du buffer de valeurs d'une métrique.
    fn slot(metric: Metric) -> usize {
        match metric {
            Metric::Composite => 0,
            Metric::Signal(index) => index + 1,
        }
    }

    pub(crate) fn metric_label(&self, metric: Metric) -> String {
        match metric {
            Metric::Composite => "Composite".to_string(),
            Metric::Signal(index) => self
                .scene
                .atlas
                .signals
                .get(index)
                .map_or(String::new(), |s| s.label.clone()),
        }
    }

    pub(crate) fn metrics(&self) -> Vec<Metric> {
        std::iter::once(Metric::Composite)
            .chain((0..self.scene.atlas.signals.len()).map(Metric::Signal))
            .collect()
    }

    pub(crate) fn current_palette(&self) -> &Palette {
        &self.palettes[self.palette.index()]
    }

    /// Recalcule le composite après un changement de poids, et renvoie les
    /// buffers concernés au GPU. `all` renvoie aussi les signaux (filtre).
    pub(crate) fn refresh_values(&mut self, all: bool) {
        self.composite = self.scene.composite(&self.weights, &self.enabled);
        self.composite_rank = percentile_ranks(&self.composite);
        self.values_version += 1;
        self.uploads.push((
            0,
            Arc::new(
                self.scene
                    .gpu_values(&self.composite, self.protestant_canon),
            ),
        ));
        if all {
            for (index, signal) in self.scene.atlas.signals.iter().enumerate() {
                let values = self.scene.gpu_values(&signal.rank, self.protestant_canon);
                self.uploads.push((index + 1, Arc::new(values)));
            }
        }
        self.shown = self.scene.metric(self.metric, &self.composite);
    }

    /// Verset sélectionné, si la sélection est un verset.
    pub(crate) fn selected_verse(&self) -> Option<usize> {
        match self.selected {
            Some(Selection::Verse(verse)) => Some(verse),
            _ => None,
        }
    }

    /// Bilan d'une sélection, recalculé seulement quand elle change ou
    /// quand les poids changent. `None` pour un verset.
    pub(crate) fn scope_summary(&mut self, selection: Selection) -> Option<Rc<Summary>> {
        if let Some(cached) = &self.summary
            && cached.selection == selection
            && cached.version == self.values_version
        {
            return Some(Rc::clone(&cached.summary));
        }
        let summary = Rc::new(self.scene.summarize(
            selection,
            &self.composite,
            &self.composite_rank,
            self.cfg.hot_rank,
            self.cfg.histogram_bins,
            self.cfg.top_verses,
        )?);
        self.summary = Some(CachedSummary {
            selection,
            version: self.values_version,
            summary: Rc::clone(&summary),
        });
        Some(summary)
    }

    pub(crate) fn set_metric(&mut self, metric: Metric) {
        self.metric = metric;
        self.shown = self.scene.metric(metric, &self.composite);
    }

    /// Passe à la métrique suivante ou précédente (Tab / Maj+Tab).
    fn cycle_metric(&mut self, delta: isize) {
        let metrics = self.metrics();
        let current = metrics.iter().position(|m| *m == self.metric).unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(metrics.len() as isize) as usize;
        self.set_metric(metrics[next]);
    }

    /// Valeur courante de la transition entre vues (0 = Mur, 1 = Hilbert).
    pub(crate) fn transition_value(&self, now: f64) -> f32 {
        let (from, to, start) = self.transition;
        let duration = self.cfg.view_transition_ms as f64 / 1000.0;
        if duration <= 0.0 {
            return to;
        }
        let t = ((now - start) / duration) as f32;
        from + (to - from) * ease_in_out(t)
    }

    fn transition_active(&self, now: f64) -> bool {
        let (from, to, start) = self.transition;
        from != to && (now - start) * 1000.0 < self.cfg.view_transition_ms as f64
    }

    /// Change de vue : les cellules glissent vers leur nouvelle place et la
    /// caméra recadre la carte dans le même temps.
    pub(crate) fn set_layout(&mut self, layout: Layout, now: f64) {
        if layout == self.layout {
            return;
        }
        let current = self.transition_value(now);
        self.layout = layout;
        self.transition = (
            current,
            if layout == Layout::Hilbert { 1.0 } else { 0.0 },
            now,
        );
        let target = match self.selected {
            Some(selection) => self.camera_for(selection),
            None => Some(self.fit_camera()),
        };
        if let Some(target) = target {
            let duration = self.cfg.view_transition_ms as f64 / 1000.0;
            self.flight = Some(Flight::new(
                self.camera,
                target,
                self.view.width(),
                now,
                duration,
                duration,
            ));
        }
        self.hovered = None;
    }

    /// Caméra qui affiche toute la carte.
    pub(crate) fn fit_camera(&self) -> Camera {
        self.fit_camera_in(self.view.size())
    }

    /// Caméra qui affiche toute la carte dans un canevas de taille `size`.
    fn fit_camera_in(&self, size: Vec2) -> Camera {
        let (min, max) = self.scene.bounds(self.layout);
        Camera::fit(size, min, max, self.cfg.fit_margin)
    }

    /// Le canevas a changé de taille (fenêtre redimensionnée, panneau ouvert
    /// ou fermé). Si la carte entière était affichée, elle le reste ; sinon
    /// le contenu garde sa place à l'écran au lieu de glisser avec le centre.
    fn canvas_resized(&mut self, previous: Vec2) {
        if self.flight.is_some() || previous.x <= 0.0 || previous.y <= 0.0 {
            return;
        }
        let fitted = self.fit_camera_in(previous);
        let at_fit = (self.camera.scale / fitted.scale - 1.0).abs() < FIT_TOLERANCE
            && (self.camera.center - fitted.center).length() * fitted.scale < 1.0;
        if at_fit {
            self.camera = self.fit_camera();
        } else {
            // Le coin haut gauche du canevas reste sur le même point du monde.
            self.camera.center += (self.view.size() - previous) / (2.0 * self.camera.scale);
        }
    }

    /// Après une sélection par clic : si elle n'est plus entièrement
    /// visible, la caméra s'y rend.
    fn reveal_selection(&mut self, now: f64) {
        let Some(selection) = self.selected else {
            return;
        };
        let bounds = self
            .scene
            .selection_range(selection)
            .and_then(|range| self.scene.range_bounds(self.layout, range));
        let Some((min, max)) = bounds else {
            return;
        };
        let (view_min, view_max) = self.camera.visible(self.view);
        let visible = min.x >= view_min.x
            && min.y >= view_min.y
            && max.x <= view_max.x
            && max.y <= view_max.y;
        if !visible && let Some(camera) = self.camera_for(selection) {
            self.fly_to(camera, now);
        }
    }

    fn scale_limits(&self) -> (f32, f32) {
        (
            self.fit_camera().scale * MIN_ZOOM_SHARE,
            self.cfg.max_cell_px,
        )
    }

    /// Caméra qui cadre une sélection, sans dépasser le niveau de zoom
    /// utile : une section reste en Z0, un livre s'ouvre en Z1, un chapitre
    /// en Z2, un verset en Z3.
    pub(crate) fn camera_for(&self, selection: Selection) -> Option<Camera> {
        let range = self.scene.selection_range(selection)?;
        let (min, max) = self.scene.range_bounds(self.layout, range)?;
        let cap = match selection {
            Selection::Section(_) => self.cfg.z1_px,
            Selection::Book(_) => self.cfg.z2_px * 0.7,
            Selection::Chapter { .. } => self.cfg.z3_px * 0.6,
            Selection::Verse(_) => self.cfg.z3_px * 1.6,
        };
        // Un verset seul occupe au plus le tiers de la vue.
        let margin = if matches!(selection, Selection::Verse(_)) {
            0.33
        } else {
            0.12
        };
        let mut camera = Camera::fit(self.view.size(), min, max, margin);
        camera.scale = camera.scale.min(cap);
        Some(camera)
    }

    pub(crate) fn fly_to(&mut self, camera: Camera, now: f64) {
        self.flight = Some(Flight::new(
            self.camera,
            camera,
            self.view.width(),
            now,
            self.cfg.flight_min_ms as f64 / 1000.0,
            self.cfg.flight_max_ms as f64 / 1000.0,
        ));
    }

    /// Sélectionne une portée et vole vers elle.
    pub(crate) fn go(&mut self, selection: Selection, now: f64) {
        self.selected = Some(selection);
        if let Some(camera) = self.camera_for(selection) {
            self.fly_to(camera, now);
        }
    }

    pub(crate) fn fit_all(&mut self, now: f64) {
        let camera = self.fit_camera();
        self.fly_to(camera, now);
    }

    pub(crate) fn zoom_level(&self) -> ZoomLevel {
        let s = self.camera.scale;
        if s < self.cfg.z1_px {
            ZoomLevel::Z0
        } else if s < self.cfg.z2_px {
            ZoomLevel::Z1
        } else if s < self.cfg.z3_px {
            ZoomLevel::Z2
        } else if s < self.cfg.z4_px {
            ZoomLevel::Z3
        } else {
            ZoomLevel::Z4
        }
    }

    /// Première image : cadre la carte, ou reprend l'état demandé.
    fn initialize(&mut self, now: f64) {
        self.initialized = true;
        self.camera = self.fit_camera();
        if let Some(saved) = self.saved.take()
            && saved.scale > 0.0
        {
            self.camera = Camera {
                center: saved.center.into(),
                scale: saved.scale,
            };
        }
        if let Some(at) = self.initial_at.take() {
            match reference::lookup(&at, &self.scene.versification) {
                Some(target) => {
                    let selection = Selection::from(target);
                    self.selected = Some(selection);
                    if let Some(camera) = self.camera_for(selection) {
                        self.camera = camera;
                    }
                }
                None => tracing::warn!("référence introuvable : {at}"),
            }
        }
        // Recherche demandée au lancement : même chemin que la saisie suivie
        // d'Entrée, vol compris.
        if let Some(query) = self.initial_search.take() {
            self.search = query;
            self.compute_suggestions();
            match self.suggestions.first().cloned() {
                Some(first) => self.accept(&first, now),
                None => tracing::warn!("recherche sans résultat : {}", self.search),
            }
        }
    }

    fn animating(&self, now: f64) -> bool {
        self.flight.is_some() || self.transition_active(now)
    }

    /// Raccourcis clavier globaux (§ 10). Rien n'est intercepté pendant la
    /// saisie dans la recherche.
    fn handle_keys(&mut self, ctx: &egui::Context, now: f64) {
        let typing = ctx.text_edit_focused();
        let (ctrl_k, slash, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::K),
                !typing && i.key_pressed(Key::Slash),
                i.key_pressed(Key::Escape),
            )
        });
        if ctrl_k || slash {
            self.focus_search = true;
            return;
        }
        if typing {
            return;
        }
        if escape {
            self.selected = None;
            self.search.clear();
            self.suggestions.clear();
        }

        // Tab change de métrique au lieu de déplacer le focus.
        let (tab, shift_tab) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Tab),
                i.consume_key(Modifiers::SHIFT, Key::Tab),
            )
        });
        if tab {
            self.cycle_metric(1);
        }
        if shift_tab {
            self.cycle_metric(-1);
        }

        let pressed = |key: Key| ctx.input(|i| i.key_pressed(key));
        let (shift, command) = ctx.input(|i| (i.modifiers.shift, i.modifiers.command));

        if command && pressed(Key::S) {
            self.request_capture(ctx, shift);
            return;
        }
        if command {
            return;
        }

        if pressed(Key::F) {
            self.fit_all(now);
        }
        if pressed(Key::F3) {
            self.show_stats = !self.show_stats;
        }
        if pressed(Key::OpenBracket) {
            self.show_left = !self.show_left;
        }
        if pressed(Key::CloseBracket) {
            self.show_right = !self.show_right;
        }
        if pressed(Key::Num1) {
            self.set_layout(Layout::Wall, now);
        }
        if pressed(Key::Num2) {
            self.set_layout(Layout::Hilbert, now);
        }

        // Zoom au clavier, ancré au centre de la vue.
        let zoom = if pressed(Key::Plus) || pressed(Key::Equals) {
            Some(self.cfg.wheel_zoom * self.cfg.wheel_zoom)
        } else if pressed(Key::Minus) {
            Some(1.0 / (self.cfg.wheel_zoom * self.cfg.wheel_zoom))
        } else {
            None
        };
        if let Some(factor) = zoom {
            let (min, max) = self.scale_limits();
            let base = self.flight.map_or(self.camera, |f| f.target());
            let target = Camera {
                center: base.center,
                scale: (base.scale * factor).clamp(min, max),
            };
            self.fly_to(target, now);
        }

        let arrows = [
            (Key::ArrowLeft, Vec2::new(-1.0, 0.0), -1_isize, false),
            (Key::ArrowRight, Vec2::new(1.0, 0.0), 1, false),
            (Key::ArrowUp, Vec2::new(0.0, -1.0), -1, true),
            (Key::ArrowDown, Vec2::new(0.0, 1.0), 1, true),
        ];
        for (key, direction, step, chapter) in arrows {
            if !pressed(key) {
                continue;
            }
            if shift {
                // Maj + flèche : panoramique.
                let delta = direction * self.view.size() * KEY_PAN_SHARE;
                let mut target = self.flight.map_or(self.camera, |f| f.target());
                target.center += delta / target.scale;
                self.fly_to(target, now);
            } else if let Some(Selection::Verse(verse)) = self.selected {
                // Verset ou chapitre voisin ; la caméra suit la sélection.
                let next = if chapter {
                    self.scene.step_chapter(verse, step)
                } else {
                    self.scene.step_verse(verse, step)
                };
                self.selected = Some(Selection::Verse(next));
                self.follow_selection(now);
            } else if let Some(selection) = self.selected {
                // Chapitre, livre ou section voisin : la caméra s'y rend.
                let next = self.scene.step_selection(selection, step);
                if next != selection {
                    self.go(next, now);
                }
            }
        }
        if pressed(Key::Enter)
            && let Some(selection) = self.selected
        {
            self.go(selection, now);
        }
    }

    /// Ramène le verset sélectionné dans la vue s'il en est sorti, sans
    /// changer d'échelle.
    fn follow_selection(&mut self, now: f64) {
        let Some(verse) = self.selected_verse() else {
            return;
        };
        let center = self.scene.position(self.layout, verse) + Vec2::splat(0.5);
        let base = self.flight.map_or(self.camera, |f| f.target());
        let (min, max) = base.visible(self.view);
        let margin = Vec2::splat(1.5);
        let inside = center.x > min.x + margin.x
            && center.x < max.x - margin.x
            && center.y > min.y + margin.y
            && center.y < max.y - margin.y;
        if !inside {
            self.fly_to(
                Camera {
                    center,
                    scale: base.scale,
                },
                now,
            );
        }
    }

    /// Demande une capture de la fenêtre (Ctrl+S). Le mode poster
    /// (Ctrl+Maj+S, export par tuiles) appartient à la phase 3.
    pub(crate) fn request_capture(&mut self, ctx: &egui::Context, poster: bool) {
        if poster {
            tracing::warn!("le mode poster appartient à la phase 3 ; capture simple à la place");
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
    }

    /// Enregistre la capture quand elle arrive.
    fn handle_screenshot(&mut self, ctx: &egui::Context, now: f64) {
        // Avec un pointeur simulé, laisser à l'infobulle le temps d'apparaître.
        if self.screenshot.is_some()
            && self.simulated_pointer.is_some()
            && !self.screenshot_requested
        {
            ctx.request_repaint();
        }
        // Capture demandée en ligne de commande : attendre que tout soit posé.
        if self.screenshot.is_some()
            && !self.screenshot_requested
            && self.frames >= SCREENSHOT_SETTLE_FRAMES
            && !self.animating(now)
            && (self.simulated_pointer.is_none() || now >= SIMULATED_POINTER_SETTLE)
        {
            self.screenshot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }

        let image = ctx.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                _ => None,
            })
        });
        let Some(image) = image else { return };
        let path = self.screenshot.take().unwrap_or_else(|| {
            let seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            self.capture_dir.join(format!("babel-{seconds}.png"))
        });
        match save_png(&image, &path) {
            Ok(()) => tracing::info!("capture enregistrée : {}", path.display()),
            Err(error) => tracing::error!("capture impossible : {error}"),
        }
        if self.screenshot_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Mesure : fait zoomer et tourner la caméra sur toute la carte, une
    /// image après l'autre, puis affiche les statistiques et ferme.
    fn drive_bench(&mut self, ctx: &egui::Context) {
        let fit = self.fit_camera();
        let (min, max) = self.scene.bounds(self.layout);
        let cpu = self.frame_ms;
        let Some(bench) = self.bench.as_mut() else {
            return;
        };
        // La mesure commence une fois la fenêtre à sa taille définitive.
        if self.frames < BENCH_SETTLE_FRAMES {
            self.camera = fit;
            bench.last = std::time::Instant::now();
            ctx.request_repaint();
            return;
        }

        let now = std::time::Instant::now();
        if bench.frame >= BENCH_WARMUP_FRAMES {
            bench
                .intervals
                .push(now.duration_since(bench.last).as_secs_f32() * 1000.0);
            bench.cpu.push(cpu);
        }
        bench.last = now;
        bench.frame += 1;

        // Deux allers-retours de zoom pendant un tour complet de la carte.
        let phase = bench.frame as f32 / (bench.total + BENCH_WARMUP_FRAMES) as f32;
        let angle = phase * std::f32::consts::TAU;
        let zoom = 0.5 - 0.5 * (2.0 * angle).cos();
        let scale = fit.scale * (BENCH_MAX_CELL_PX / fit.scale).max(1.0).powf(zoom);
        let radius = (max - min) * 0.3;
        let center = (min + max) / 2.0 + Vec2::new(angle.cos() * radius.x, angle.sin() * radius.y);
        self.camera = Camera { center, scale };
        self.flight = None;
        ctx.request_repaint();

        if bench.frame < bench.total + BENCH_WARMUP_FRAMES {
            return;
        }
        let mut sorted = bench.intervals.clone();
        sorted.sort_by(f32::total_cmp);
        let n = sorted.len().max(1);
        let mean = sorted.iter().sum::<f32>() / n as f32;
        let p99 = sorted[(n * 99 / 100).min(n - 1)];
        let worst = sorted[n - 1];
        let cpu_mean = bench.cpu.iter().sum::<f32>() / bench.cpu.len().max(1) as f32;
        let size = self.view.size() * ctx.pixels_per_point();
        println!(
            "Mesure du viewer : {} images, canevas de {:.0} × {:.0} px, vue {}",
            sorted.len(),
            size.x,
            size.y,
            self.layout.label()
        );
        println!(
            "  moyenne      : {:.2} ms par image ({:.0} images/s)",
            mean,
            1000.0 / mean
        );
        println!(
            "  99e centile  : {:.2} ms ({:.0} images/s)",
            p99,
            1000.0 / p99
        );
        println!(
            "  pire image   : {:.2} ms ({:.0} images/s)",
            worst,
            1000.0 / worst
        );
        let slow = sorted
            .iter()
            .filter(|ms| **ms > BENCH_SLOW_FRAME_MS)
            .count();
        println!("  sous 120 img/s : {slow} image(s) sur {}", sorted.len());
        println!("  CPU (egui)   : {:.2} ms par image", cpu_mean);
        self.bench = None;
        self.screenshot_requested = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Le canevas : entrées, rendu wgpu, libellés par-dessus.
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        let previous = self.view;
        self.view = rect;
        let now = ctx.input(|i| i.time);
        if !self.initialized {
            self.initialize(now);
        } else if previous.size() != rect.size() {
            self.canvas_resized(previous.size());
        }
        if std::mem::take(&mut self.reveal) {
            self.reveal_selection(now);
        }

        if let Some(flight) = self.flight {
            let (camera, done) = flight.sample(now);
            self.camera = camera;
            if done {
                self.flight = None;
            }
        }
        if self.bench.is_some() {
            self.drive_bench(&ctx);
        }
        let transition = self.transition_value(now);
        let in_transition = self.transition_active(now);

        // Panoramique à la souris.
        if response.dragged_by(PointerButton::Primary) || response.dragged_by(PointerButton::Middle)
        {
            self.flight = None;
            self.camera.pan(response.drag_delta());
        }

        // Zoom ancré sous le curseur. egui lisse le défilement, ce qui donne
        // son inertie à la molette.
        self.pointer = response.hover_pos();
        if let Some(pointer) = self.pointer {
            let (scroll, pinch) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (scroll * self.cfg.wheel_zoom.ln() / WHEEL_NOTCH_POINTS).exp();
            if (factor - 1.0).abs() > 1e-4 {
                self.flight = None;
                let (min, max) = self.scale_limits();
                let scale = (self.camera.scale * factor).clamp(min, max);
                self.camera.zoom_to(rect, pointer, scale);
            }
        }

        // Survol d'un nom de livre ou de section : il passe avant les
        // cellules. Les zones ont été relevées à l'image précédente.
        let interactive = !in_transition && !response.dragged();
        self.hovered_label = match self.pointer {
            Some(pointer) if interactive => self
                .label_hits
                .iter()
                .rev()
                .find(|(zone, _)| zone.contains(pointer))
                .map(|(_, selection)| *selection),
            _ => None,
        };
        if self.hovered_label.is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // Survol : inversion analytique de la disposition.
        let hovered = match self.pointer {
            Some(pointer) if interactive && self.hovered_label.is_none() => self
                .scene
                .pick(self.layout, self.camera.screen_to_world(rect, pointer)),
            _ => None,
        };
        if hovered != self.hovered {
            self.hovered = hovered;
            self.hover_since = now;
        }

        if response.double_clicked() {
            if let Some(label) = self.hovered_label {
                // Double-clic sur un nom : sélection et vol.
                self.go(label, now);
            } else if let Some(verse) = self.hovered {
                // Double-clic : descend d'un niveau, du livre au verset.
                let book = usize::from(self.scene.atlas.verses[verse].book);
                let chapter = usize::from(self.scene.atlas.verses[verse].chapter_index);
                let selection = match self.zoom_level() {
                    ZoomLevel::Z0 => Selection::Book(book),
                    ZoomLevel::Z1 => Selection::Chapter { book, chapter },
                    _ => Selection::Verse(verse),
                };
                self.go(selection, now);
            }
        } else if response.clicked() {
            // Clic sur un nom de livre ou de section : son bilan. Clic sur
            // un verset : sélection. Clic dans le vide : plus de sélection.
            self.selected = self.hovered_label.or(self.hovered.map(Selection::Verse));
            // L'inspecteur va s'ouvrir et rétrécir le canevas : à l'image
            // suivante, vérifier que la sélection est restée visible.
            self.reveal = self.hovered_label.is_some();
        }

        // Rendu des cellules.
        let pixels_per_point = ctx.pixels_per_point();
        let rounded = self.camera.scale >= self.cfg.z2_px;
        let index = |verse: Option<usize>| verse.map_or(-1, |v| v as i32);
        // Chapitre, livre ou section sélectionné : le reste s'estompe.
        let focus = match self.selected {
            Some(Selection::Verse(_)) | None => None,
            Some(selection) => self.scene.selection_range(selection),
        };
        let (focus_start, focus_end) =
            focus.map_or((0, 0), |range| (range.start as i32, range.end as i32));
        let uniforms = Uniforms {
            center: self.camera.center.into(),
            viewport: (rect.size() * pixels_per_point).into(),
            scale: self.camera.scale * pixels_per_point,
            transition,
            cell_gap: self.cfg.cell_gap,
            corner: if rounded { self.cfg.corner_radius } else { 0.0 },
            hovered: index(self.hovered),
            selected: index(self.selected_verse()),
            srgb_target: u32::from(self.srgb_target),
            focus_start,
            focus_end,
            focus_dim: self.cfg.focus_dim,
            padding: [0.0; 2],
            missing: rgba(theme::MISSING),
            hover_color: [1.0, 1.0, 1.0, theme::HOVER_OPACITY],
            accent: rgba(theme::ACCENT),
            background: rgba(theme::BG_CANVAS),
        };
        let frame = CanvasFrame {
            uniforms,
            metric: Self::slot(self.metric),
            palette: self.palette.index(),
            uploads: std::mem::take(&mut self.uploads),
        };
        ui.painter()
            .add(egui_wgpu::Callback::new_paint_callback(rect, frame));

        let painter = ui.painter_at(rect);
        self.label_hits.clear();
        self.draw_overlays(&painter, rect, transition, in_transition);
        let tooltip_delay = self.cfg.tooltip_delay_ms as f64 / 1000.0;
        if let (Some(verse), Some(pointer)) = (self.hovered, self.pointer) {
            if now - self.hover_since >= tooltip_delay {
                self.draw_tooltip(&painter, rect, pointer, verse);
            } else {
                ctx.request_repaint_after_secs((tooltip_delay - (now - self.hover_since)) as f32);
            }
        }
        if self.show_stats {
            self.draw_stats(&painter, rect, &ctx);
        }

        // Rendu réactif : une image n'est redemandée que pendant une
        // animation. Au repos, le GPU ne travaille pas.
        if self.animating(now) {
            ctx.request_repaint();
        }
    }

    fn save_state(&self) {
        // Une capture en ligne de commande ne modifie pas l'état enregistré.
        if self.screenshot_requested || !self.initialized {
            return;
        }
        let saved = Saved {
            layout: self.layout.code().to_string(),
            metric: metric_code(&self.scene, self.metric),
            palette: self.palette.code().to_string(),
            weights: self.weights.clone(),
            enabled: self.enabled.clone(),
            protestant_canon: self.protestant_canon,
            center: self.camera.center.into(),
            scale: self.camera.scale,
            show_left: self.show_left,
            show_right: self.show_right,
        };
        if let Err(error) = saved.save(&self.state_path) {
            tracing::warn!("état du viewer non enregistré : {error}");
        }
    }
}

/// Code d'une métrique pour la ligne de commande et l'état enregistré.
fn metric_code(scene: &Scene, metric: Metric) -> String {
    match metric {
        Metric::Composite => "composite".to_string(),
        Metric::Signal(index) => scene
            .atlas
            .signals
            .get(index)
            .map_or(String::new(), |s| s.code.clone()),
    }
}

fn metric_from_code(scene: &Scene, code: &str) -> Option<Metric> {
    if code.eq_ignore_ascii_case("composite") || code.eq_ignore_ascii_case("u") {
        return Some(Metric::Composite);
    }
    scene
        .atlas
        .signals
        .iter()
        .position(|s| s.code.eq_ignore_ascii_case(code))
        .map(Metric::Signal)
}

/// Couleur du thème en composantes sRGB normalisées, pour le shader.
fn rgba(color: Color32) -> [f32; 4] {
    [
        f32::from(color.r()) / 255.0,
        f32::from(color.g()) / 255.0,
        f32::from(color.b()) / 255.0,
        f32::from(color.a()) / 255.0,
    ]
}

fn save_png(image: &egui::ColorImage, path: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let (width, height) = (image.width() as u32, image.height() as u32);
    let buffer = image::RgbaImage::from_raw(width, height, image.as_raw().to_vec())
        .ok_or("dimensions de capture incohérentes")?;
    buffer.save(path).map_err(|e| e.to_string())
}

impl eframe::App for BabelApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let started = std::time::Instant::now();
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.frames += 1;
        if self.maximize {
            // Certaines plateformes ignorent l'état demandé à la création.
            self.maximize = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }

        self.handle_keys(&ctx, now);
        self.top_bar(ui, now);
        self.bottom_bar(ui, now);
        if self.show_left {
            self.layers_panel(ui);
        }
        if self.show_right && self.selected.is_some() {
            self.inspector_panel(ui, now);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG_CANVAS))
            .show(ui, |ui| self.canvas(ui));
        self.search_popup(&ctx, now);
        self.handle_screenshot(&ctx, now);

        self.frame_ms = started.elapsed().as_secs_f32() * 1000.0;
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // Pointeur simulé : sert à capturer le survol et l'infobulle sans
        // souris, pour les captures de test.
        if let Some([x, y]) = self.simulated_pointer {
            let pos = egui::pos2(x, y);
            raw_input.events.push(egui::Event::PointerMoved(pos));
            // Compté ici et non par image : une image peut passer deux fois
            // dans l'interface, pas dans ce crochet.
            self.simulated_inputs += 1;
            let pressed = match self.simulated_inputs {
                SIMULATED_CLICK_FRAME => Some(true),
                input if input == SIMULATED_CLICK_FRAME + 1 => Some(false),
                _ => None,
            };
            if let (true, Some(pressed)) = (self.simulated_click, pressed) {
                raw_input.events.push(egui::Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                });
            }
        }
    }

    fn on_exit(&mut self) {
        self.save_state();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        rgba(theme::BG_CANVAS)
    }
}
