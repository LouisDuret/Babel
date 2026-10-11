//! État persistant du viewer : dernière vue, caméra, métrique, poids et
//! filtres, enregistrés dans un fichier TOML à la fermeture.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub layout: String,
    pub metric: String,
    pub palette: String,
    pub weights: Vec<f32>,
    pub enabled: Vec<bool>,
    pub protestant_canon: bool,
    /// Centre de la caméra, en cellules.
    pub center: [f32; 2],
    /// Pixels par cellule.
    pub scale: f32,
    pub show_left: bool,
    pub show_right: bool,
}

impl Saved {
    /// Relit l'état. Un fichier absent ou illisible donne `None` : le
    /// viewer démarre alors sur la carte entière.
    pub fn load(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        toml::from_str(&text).ok()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, text).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour() {
        let saved = Saved {
            layout: "hilbert".into(),
            metric: "len".into(),
            palette: "cividis".into(),
            weights: vec![1.0, 0.5, 2.0],
            enabled: vec![true, false, true],
            protestant_canon: true,
            center: [120.5, 80.25],
            scale: 42.0,
            show_left: false,
            show_right: true,
        };
        let path =
            std::env::temp_dir().join(format!("babel-test-state-{}.toml", std::process::id()));
        saved.save(&path).expect("écriture");
        let back = Saved::load(&path);
        std::fs::remove_file(&path).expect("nettoyage");
        assert_eq!(back, Some(saved));
    }

    #[test]
    fn fichier_absent_ou_illisible() {
        assert_eq!(
            Saved::load(Path::new("fichier-qui-n-existe-pas.toml")),
            None
        );
        let path =
            std::env::temp_dir().join(format!("babel-test-state-bad-{}.toml", std::process::id()));
        std::fs::write(&path, "ceci n'est pas un état").expect("écriture");
        let back = Saved::load(&path);
        std::fs::remove_file(&path).expect("nettoyage");
        assert_eq!(back, None);
    }
}
