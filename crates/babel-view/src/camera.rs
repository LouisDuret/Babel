//! Caméra 2D du canevas et vols animés.
//!
//! Le monde est mesuré en cellules ; l'échelle est le nombre de pixels
//! (points logiques) d'une cellule à l'écran. Les vols suivent le
//! zoom-panoramique fluide de van Wijk et Nuij (2003).

use egui::{Pos2, Rect, Vec2};

/// Compromis zoom / déplacement des vols : valeur recommandée par van Wijk
/// et Nuij (√2).
const RHO: f32 = std::f32::consts::SQRT_2;

/// Longueur de trajet (en unités du modèle de van Wijk) à partir de
/// laquelle un vol prend sa durée maximale.
const LONG_FLIGHT: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Point du monde au centre de la vue.
    pub center: Vec2,
    /// Pixels par cellule.
    pub scale: f32,
}

impl Camera {
    pub fn world_to_screen(&self, view: Rect, world: Vec2) -> Pos2 {
        view.center() + (world - self.center) * self.scale
    }

    pub fn screen_to_world(&self, view: Rect, screen: Pos2) -> Vec2 {
        self.center + (screen - view.center()) / self.scale
    }

    /// Change l'échelle en gardant fixe le point du monde situé sous
    /// `anchor` : le zoom est toujours ancré sous le curseur.
    pub fn zoom_to(&mut self, view: Rect, anchor: Pos2, scale: f32) {
        let world = self.screen_to_world(view, anchor);
        self.scale = scale;
        self.center = world - (anchor - view.center()) / self.scale;
    }

    /// Déplace la vue de `delta` pixels (le contenu suit le pointeur).
    pub fn pan(&mut self, delta: Vec2) {
        self.center -= delta / self.scale;
    }

    /// Caméra qui cadre la boîte `[min, max]` dans une vue de taille `size`,
    /// avec une marge exprimée en part de la vue.
    pub fn fit(size: Vec2, min: Vec2, max: Vec2, margin: f32) -> Self {
        let extent = (max - min).max(Vec2::splat(1e-3));
        let usable = size * (1.0 - 2.0 * margin).max(0.1);
        let scale = (usable.x / extent.x).min(usable.y / extent.y);
        Self {
            center: (min + max) / 2.0,
            scale,
        }
    }

    /// Boîte du monde visible dans la vue : (min, max).
    pub fn visible(&self, view: Rect) -> (Vec2, Vec2) {
        let half = view.size() / (2.0 * self.scale);
        (self.center - half, self.center + half)
    }
}

/// Accélération puis décélération douces (cubique).
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

#[derive(Debug, Clone, Copy)]
enum Path {
    /// Même centre : zoom pur.
    Zoom { rate: f32 },
    /// Trajet de van Wijk et Nuij.
    Arc { r0: f32, distance: f32 },
}

/// Vol animé d'une caméra à une autre.
#[derive(Debug, Clone, Copy)]
pub struct Flight {
    from: Camera,
    to: Camera,
    /// Largeur de la vue, en pixels.
    width: f32,
    path: Path,
    /// Longueur du trajet dans l'espace (déplacement, zoom).
    length: f32,
    start: f64,
    duration: f64,
}

impl Flight {
    /// Prépare un vol qui démarre à l'instant `now` (en secondes). La durée
    /// va de `min_duration` à `max_duration` selon la distance.
    pub fn new(
        from: Camera,
        to: Camera,
        view_width: f32,
        now: f64,
        min_duration: f64,
        max_duration: f64,
    ) -> Self {
        let width = view_width.max(1.0);
        let (w0, w1) = (width / from.scale, width / to.scale);
        let distance = (to.center - from.center).length();
        // En dessous d'une fraction de cellule, le déplacement est négligé.
        let (path, length) = if distance < 1e-3 {
            let length = (w1 / w0).ln().abs() / RHO;
            let rate = if length > 0.0 {
                (w1 / w0).ln() / length
            } else {
                0.0
            };
            (Path::Zoom { rate }, length)
        } else {
            let rho2 = RHO * RHO;
            let b0 = (w1 * w1 - w0 * w0 + rho2 * rho2 * distance * distance)
                / (2.0 * w0 * rho2 * distance);
            let b1 = (w1 * w1 - w0 * w0 - rho2 * rho2 * distance * distance)
                / (2.0 * w1 * rho2 * distance);
            let r0 = (-b0 + (b0 * b0 + 1.0).sqrt()).ln();
            let r1 = (-b1 + (b1 * b1 + 1.0).sqrt()).ln();
            (Path::Arc { r0, distance }, (r1 - r0) / RHO)
        };
        let share = f64::from((length / LONG_FLIGHT).clamp(0.0, 1.0));
        let duration = min_duration + (max_duration - min_duration) * share;
        Self {
            from,
            to,
            width,
            path,
            length,
            start: now,
            duration,
        }
    }

    /// Caméra à la fraction `t` du vol (0 = départ, 1 = arrivée).
    pub fn at(&self, t: f32) -> Camera {
        if t >= 1.0 {
            return self.to;
        }
        if t <= 0.0 {
            return self.from;
        }
        let s = ease_in_out(t) * self.length;
        let w0 = self.width / self.from.scale;
        match self.path {
            Path::Zoom { rate } => Camera {
                center: self.to.center,
                scale: self.width / (w0 * (rate * s).exp()),
            },
            Path::Arc { r0, distance } => {
                let u = w0 / (RHO * RHO) * (r0.cosh() * (RHO * s + r0).tanh() - r0.sinh());
                let w = w0 * r0.cosh() / (RHO * s + r0).cosh();
                // Borné : l'arrondi flottant ne doit pas faire dépasser l'arrivée.
                let progress = (u / distance).clamp(0.0, 1.0);
                let center = self.from.center + (self.to.center - self.from.center) * progress;
                Camera {
                    center,
                    scale: self.width / w,
                }
            }
        }
    }

    /// Caméra à l'instant `now` et drapeau de fin de vol.
    pub fn sample(&self, now: f64) -> (Camera, bool) {
        if self.duration <= 0.0 {
            return (self.to, true);
        }
        let t = ((now - self.start) / self.duration) as f32;
        (self.at(t), t >= 1.0)
    }

    pub fn target(&self) -> Camera {
        self.to
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    fn view() -> Rect {
        Rect::from_min_size(pos2(100.0, 50.0), vec2(800.0, 600.0))
    }

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-2
    }

    #[test]
    fn aller_retour_monde_ecran() {
        let camera = Camera {
            center: vec2(120.0, 80.0),
            scale: 7.5,
        };
        let world = vec2(131.0, 77.5);
        let screen = camera.world_to_screen(view(), world);
        assert!(close(camera.screen_to_world(view(), screen), world));
        // Le centre du monde tombe au centre de la vue.
        assert_eq!(
            camera.world_to_screen(view(), camera.center),
            view().center()
        );
    }

    #[test]
    fn zoom_ancre_sous_le_curseur() {
        let mut camera = Camera {
            center: vec2(100.0, 100.0),
            scale: 5.0,
        };
        let cursor = pos2(650.0, 200.0);
        let before = camera.screen_to_world(view(), cursor);
        camera.zoom_to(view(), cursor, 37.0);
        assert_eq!(camera.scale, 37.0);
        assert!(close(camera.screen_to_world(view(), cursor), before));
    }

    #[test]
    fn panoramique() {
        let mut camera = Camera {
            center: vec2(10.0, 10.0),
            scale: 4.0,
        };
        camera.pan(vec2(8.0, -4.0));
        assert!(close(camera.center, vec2(8.0, 11.0)));
    }

    #[test]
    fn cadrage() {
        // Une boîte 400 × 100 dans une vue 800 × 600 sans marge : la largeur
        // contraint l'échelle.
        let camera = Camera::fit(vec2(800.0, 600.0), vec2(0.0, 0.0), vec2(400.0, 100.0), 0.0);
        assert_eq!(camera.scale, 2.0);
        assert!(close(camera.center, vec2(200.0, 50.0)));
        let (min, max) = camera.visible(view());
        assert!(min.x <= 0.0 && max.x >= 400.0 && min.y <= 0.0 && max.y >= 100.0);
    }

    #[test]
    fn vol_part_et_arrive_aux_bons_endroits() {
        let from = Camera {
            center: vec2(10.0, 10.0),
            scale: 40.0,
        };
        let to = Camera {
            center: vec2(300.0, 200.0),
            scale: 60.0,
        };
        let flight = Flight::new(from, to, 800.0, 0.0, 0.45, 0.70);
        let start = flight.at(0.0);
        let end = flight.at(1.0);
        assert!(close(start.center, from.center) && (start.scale - from.scale).abs() < 1e-3);
        assert!(close(end.center, to.center) && (end.scale - to.scale).abs() < 1e-3);
        // Juste avant la fin, la caméra est déjà presque arrivée.
        let almost = flight.at(0.999);
        assert!((almost.center - to.center).length() < 1.0);
        assert!((almost.scale / to.scale - 1.0).abs() < 0.02);
    }

    #[test]
    fn vol_lointain_prend_de_la_hauteur() {
        // Entre deux points éloignés vus de près, la caméra dézoome en route.
        let from = Camera {
            center: vec2(0.0, 0.0),
            scale: 80.0,
        };
        let to = Camera {
            center: vec2(350.0, 0.0),
            scale: 80.0,
        };
        let flight = Flight::new(from, to, 800.0, 0.0, 0.45, 0.70);
        let middle = flight.at(0.5);
        assert!(
            middle.scale < 20.0,
            "échelle à mi-parcours : {}",
            middle.scale
        );
        assert!((middle.center.x - 175.0).abs() < 1.0);
        // La progression le long du trajet est monotone.
        let mut previous = -1.0;
        for i in 0..=50 {
            let x = flight.at(i as f32 / 50.0).center.x;
            assert!(x >= previous - 0.05, "recul de {previous} à {x}");
            previous = x;
        }
    }

    #[test]
    fn vol_sur_place_zoome_seulement() {
        let from = Camera {
            center: vec2(50.0, 50.0),
            scale: 4.0,
        };
        let to = Camera {
            center: vec2(50.0, 50.0),
            scale: 64.0,
        };
        let flight = Flight::new(from, to, 800.0, 0.0, 0.45, 0.70);
        let middle = flight.at(0.5);
        assert!(close(middle.center, to.center));
        // À mi-parcours, la moitié du chemin en échelle logarithmique.
        assert!((middle.scale - 16.0).abs() < 0.1);
        assert!((flight.at(1.0).scale - 64.0).abs() < 1e-3);
    }

    #[test]
    fn duree_selon_la_distance() {
        let near = Flight::new(
            Camera {
                center: vec2(0.0, 0.0),
                scale: 40.0,
            },
            Camera {
                center: vec2(1.0, 0.0),
                scale: 40.0,
            },
            800.0,
            0.0,
            0.45,
            0.70,
        );
        let far = Flight::new(
            Camera {
                center: vec2(0.0, 0.0),
                scale: 400.0,
            },
            Camera {
                center: vec2(400.0, 250.0),
                scale: 400.0,
            },
            800.0,
            0.0,
            0.45,
            0.70,
        );
        assert!(near.duration >= 0.45 && near.duration < 0.55);
        assert!((far.duration - 0.70).abs() < 1e-6);
        // Une durée nulle (animations réduites) saute à l'arrivée.
        let instant = Flight::new(near.from, near.to, 800.0, 0.0, 0.0, 0.0);
        assert!(instant.sample(0.0).1);
    }

    #[test]
    fn interpolation_douce() {
        assert_eq!(ease_in_out(0.0), 0.0);
        assert_eq!(ease_in_out(1.0), 1.0);
        assert_eq!(ease_in_out(0.5), 0.5);
        assert!(ease_in_out(0.25) < 0.25);
    }
}
