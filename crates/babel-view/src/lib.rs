//! Application graphique de Babel : la carte de chaleur de toute la Bible,
//! zoomable en continu (eframe / egui pour les panneaux, wgpu pour le
//! canevas).

mod app;
mod camera;
mod canvas;
mod overlay;
mod palette;
mod panels;
mod scene;
mod state;
mod theme;

use babel_core::config::View;
use std::path::PathBuf;

/// Options de lancement du viewer.
#[derive(Debug, Clone)]
pub struct Options {
    /// Chemin de `atlas.bin`.
    pub atlas: PathBuf,
    /// Chemin de `texts.zst`.
    pub texts: PathBuf,
    /// Fichier où l'état du viewer est enregistré à la fermeture.
    pub state: PathBuf,
    pub view: View,
    /// Référence sur laquelle ouvrir : « Jn 1:1 ».
    pub at: Option<String>,
    /// Recherche lancée à l'ouverture, comme si elle était saisie puis
    /// validée : la première proposition est suivie.
    pub search: Option<String>,
    /// Pointeur simulé à cette position (en points), pour capturer le
    /// survol et l'infobulle sans souris.
    pub pointer: Option<[f32; 2]>,
    /// Métrique de départ : composite, omit, len, intra.
    pub metric: Option<String>,
    /// Vue de départ : mur ou hilbert.
    pub layout: Option<String>,
    /// Enregistre une capture PNG puis quitte.
    pub screenshot: Option<PathBuf>,
    /// Affiche les statistiques de rendu dès l'ouverture (touche F3).
    pub stats: bool,
    /// Mesure de performance sur ce nombre d'images, sans plafond de
    /// fréquence, puis quitte.
    pub bench: Option<u32>,
    /// Ouvre la fenêtre agrandie.
    pub maximize: bool,
    /// Taille de la fenêtre, en points. Par défaut, 1600 × 960.
    pub size: Option<[f32; 2]>,
}

/// Taille de la fenêtre à l'ouverture, en points.
const WINDOW_SIZE: [f32; 2] = [1600.0, 960.0];
const WINDOW_MIN_SIZE: [f32; 2] = [960.0, 600.0];

/// Lance l'interface et ne rend la main qu'à la fermeture de la fenêtre.
pub fn run(options: Options) -> Result<(), String> {
    let maximized = options.maximize;
    let uncapped = options.bench.is_some();
    let viewport = egui::ViewportBuilder::default()
        .with_title("Babel")
        .with_inner_size(options.size.unwrap_or(WINDOW_SIZE))
        .with_min_inner_size(WINDOW_MIN_SIZE)
        .with_maximized(maximized);
    let mut native = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    if uncapped {
        // VSync coupée pour les mesures seulement.
        native.wgpu_options.surface.present_mode = egui_wgpu::wgpu::PresentMode::AutoNoVsync;
    }
    eframe::run_native(
        "Babel",
        native,
        Box::new(move |cc| {
            let app = app::BabelApp::new(cc, options)?;
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())
}
