//! Palettes de données : interpolation en OKLab sur 256 texels.
//!
//! La palette par défaut, « Braise », va de l'indigo nuit au blanc chaud.
//! Sa luminosité OKLab est strictement croissante et ne descend jamais sous
//! un plancher, pour que les versets « sûrs » restent visibles sur le fond.

use egui::Color32;

/// Nombre de texels d'une palette (largeur de la texture LUT).
pub const TEXELS: usize = 256;

/// Luminosité OKLab minimale de la palette Braise.
#[cfg(test)]
pub const BRAISE_LIGHTNESS_FLOOR: f32 = 0.22;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaletteKind {
    Braise,
    Viridis,
    Magma,
    Cividis,
}

impl PaletteKind {
    pub const ALL: [PaletteKind; 4] = [
        PaletteKind::Braise,
        PaletteKind::Viridis,
        PaletteKind::Magma,
        PaletteKind::Cividis,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PaletteKind::Braise => "Braise",
            PaletteKind::Viridis => "Viridis",
            PaletteKind::Magma => "Magma",
            PaletteKind::Cividis => "Cividis",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            PaletteKind::Braise => "braise",
            PaletteKind::Viridis => "viridis",
            PaletteKind::Magma => "magma",
            PaletteKind::Cividis => "cividis",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.code() == code)
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }

    /// Couleurs d'ancrage, de la valeur la plus basse à la plus haute.
    fn stops(self) -> &'static [u32] {
        match self {
            PaletteKind::Braise => &[0x262B52, 0x4B2C73, 0x9C2F5E, 0xE0583A, 0xF9A23B, 0xFFE9A8],
            PaletteKind::Viridis => &[
                0x440154, 0x482878, 0x3E4A89, 0x31688E, 0x26828E, 0x1F9E89, 0x35B779, 0x6DCD59,
                0xB4DE2C, 0xFDE725,
            ],
            PaletteKind::Magma => &[
                0x000004, 0x140E36, 0x3B0F70, 0x641A80, 0x8C2981, 0xB73779, 0xDE4968, 0xF7705C,
                0xFE9F6D, 0xFECF92, 0xFCFDBF,
            ],
            PaletteKind::Cividis => &[
                0x00224E, 0x123570, 0x3B496C, 0x575D6D, 0x707173, 0x8A8678, 0xA59C74, 0xC3B369,
                0xE1CC55, 0xFEE838,
            ],
        }
    }
}

/// Couleurs de la palette divergente « Glace / Feu » du mode comparaison
/// (phase 3) : centrée sur un neutre sombre.
#[cfg(test)]
pub const DIVERGING_STOPS: [u32; 3] = [0x4CC9F0, 0x1A1E26, 0xF77F00];

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn hex_to_linear(hex: u32) -> [f32; 3] {
    let channel = |shift: u32| srgb_to_linear(((hex >> shift) & 0xFF) as f32 / 255.0);
    [channel(16), channel(8), channel(0)]
}

/// sRGB linéaire → OKLab (Ottosson, 2020).
pub fn linear_to_oklab(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb;
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// OKLab → sRGB linéaire.
pub fn oklab_to_linear(lab: [f32; 3]) -> [f32; 3] {
    let [l, a, b] = lab;
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
        -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
    ]
}

/// sRGB linéaire → sRGB encodé, borné à [0, 1].
fn linear_to_encoded(rgb: [f32; 3]) -> [f32; 3] {
    rgb.map(|c| linear_to_srgb(c.clamp(0.0, 1.0)))
}

fn encoded_to_bytes(rgb: [f32; 3]) -> [u8; 3] {
    rgb.map(|c| (c * 255.0).round() as u8)
}

#[cfg(test)]
fn linear_to_bytes(rgb: [f32; 3]) -> [u8; 3] {
    encoded_to_bytes(linear_to_encoded(rgb))
}

/// Bits du demi-flottant (IEEE 754 binary16) le plus proche d'une valeur
/// positive finie. Les valeurs trop petites pour un demi-flottant normal
/// (sous 2⁻¹⁴) donnent zéro.
pub fn half_bits(value: f32) -> u16 {
    let bits = value.max(0.0).to_bits();
    let exponent = ((bits >> 23) & 0xFF) as i32 - 127 + 15;
    let mantissa = bits & 0x007F_FFFF;
    if exponent <= 0 {
        return 0;
    }
    if exponent >= 31 {
        // Infini : hors du domaine des couleurs, jamais atteint.
        return 0x7C00;
    }
    // Arrondi au plus proche ; la retenue passe d'elle-même à l'exposant.
    let half = ((exponent as u32) << 10) | (mantissa >> 13);
    (half + ((mantissa >> 12) & 1)) as u16
}

/// Valeur d'un demi-flottant normal ou nul, pour vérifier la texture.
#[cfg(test)]
pub fn half_value(bits: u16) -> f32 {
    let exponent = i32::from((bits >> 10) & 0x1F);
    let mantissa = f32::from(bits & 0x03FF) / 1024.0;
    if exponent == 0 {
        0.0
    } else {
        (1.0 + mantissa) * 2.0_f32.powi(exponent - 15)
    }
}

/// Luminosité OKLab d'une couleur sRGB 8 bits.
#[cfg(test)]
pub fn lightness(rgb: [u8; 3]) -> f32 {
    lightness_encoded(rgb.map(|c| f32::from(c) / 255.0))
}

/// Luminosité OKLab d'une couleur sRGB encodée, en flottants.
#[cfg(test)]
pub fn lightness_encoded(rgb: [f32; 3]) -> f32 {
    linear_to_oklab(rgb.map(srgb_to_linear))[0]
}

/// Simulation de deutéranopie (Machado, Oliveira et Fernandes, 2009,
/// sévérité 1), appliquée en sRGB linéaire.
#[cfg(test)]
pub fn simulate_deuteranopia(rgb: [u8; 3]) -> [u8; 3] {
    let [r, g, b] = rgb.map(|c| srgb_to_linear(f32::from(c) / 255.0));
    linear_to_bytes([
        0.367_322 * r + 0.860_646 * g - 0.227_968 * b,
        0.280_085 * r + 0.672_501 * g + 0.047_413 * b,
        -0.011_820 * r + 0.042_940 * g + 0.968_881 * b,
    ])
}

/// Une palette prête à l'emploi : 256 texels sRGB.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    /// Couleurs sRGB encodées en flottants : le contenu de la texture LUT.
    /// Huit bits par canal ne suffisent pas à distinguer 256 texels sur
    /// les segments sombres de la palette.
    pub values: Vec<[f32; 3]>,
    /// Les mêmes couleurs sur 8 bits, pour les éléments dessinés par egui.
    pub texels: Vec<[u8; 3]>,
}

impl Palette {
    /// Interpole les couleurs d'ancrage en OKLab, à intervalles égaux.
    pub fn from_stops(stops: &[u32]) -> Self {
        let lab: Vec<[f32; 3]> = stops
            .iter()
            .map(|h| linear_to_oklab(hex_to_linear(*h)))
            .collect();
        let segments = lab.len().saturating_sub(1).max(1) as f32;
        let values: Vec<[f32; 3]> = (0..TEXELS)
            .map(|i| {
                let t = i as f32 / (TEXELS - 1) as f32 * segments;
                let k = (t.floor() as usize).min(lab.len().saturating_sub(2));
                let f = t - k as f32;
                let (a, b) = (lab[k], lab[(k + 1).min(lab.len() - 1)]);
                let mixed = [0, 1, 2].map(|c| a[c] + (b[c] - a[c]) * f);
                linear_to_encoded(oklab_to_linear(mixed))
            })
            .collect();
        let texels = values.iter().map(|v| encoded_to_bytes(*v)).collect();
        Self { values, texels }
    }

    pub fn new(kind: PaletteKind) -> Self {
        Self::from_stops(kind.stops())
    }

    #[cfg(test)]
    pub fn diverging() -> Self {
        Self::from_stops(&DIVERGING_STOPS)
    }

    /// Couleur d'une valeur dans [0, 1].
    pub fn color(&self, value: f32) -> Color32 {
        let index = (value.clamp(0.0, 1.0) * (TEXELS - 1) as f32).round() as usize;
        let [r, g, b] = self.texels[index.min(TEXELS - 1)];
        Color32::from_rgb(r, g, b)
    }

    /// Octets de la texture LUT (256 × 1, RGBA en demi-flottants).
    pub fn rgba16f(&self) -> Vec<u8> {
        self.values
            .iter()
            .flat_map(|[r, g, b]| [*r, *g, *b, 1.0])
            .flat_map(|c| half_bits(c).to_le_bytes())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(rgb: [u8; 3]) -> u32 {
        (u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2])
    }

    #[test]
    fn oklab_aller_retour() {
        for color in [0x000000_u32, 0xFFFFFF, 0x262B52, 0xE0583A, 0x22D3EE] {
            let linear = hex_to_linear(color);
            let back = oklab_to_linear(linear_to_oklab(linear));
            assert_eq!(hex(linear_to_bytes(back)), color);
        }
        // Le blanc a une luminosité de 1, le noir de 0.
        assert!((lightness([255, 255, 255]) - 1.0).abs() < 1e-3);
        assert!(lightness([0, 0, 0]).abs() < 1e-3);
    }

    #[test]
    fn braise_passe_par_ses_ancres() {
        let p = Palette::new(PaletteKind::Braise);
        assert_eq!(p.texels.len(), TEXELS);
        assert_eq!(hex(p.texels[0]), 0x262B52);
        assert_eq!(hex(p.texels[TEXELS - 1]), 0xFFE9A8);
        // 255 / 5 = 51 : chaque ancre tombe exactement sur un texel.
        assert_eq!(hex(p.texels[51]), 0x4B2C73);
        assert_eq!(hex(p.texels[153]), 0xE0583A);
    }

    #[test]
    fn braise_luminosite_strictement_croissante() {
        // La luminosité est mesurée sur le contenu exact de la texture
        // LUT : les demi-flottants envoyés au GPU.
        let p = Palette::new(PaletteKind::Braise);
        let bytes = p.rgba16f();
        assert_eq!(bytes.len(), TEXELS * 8);
        let l: Vec<f32> = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|texel| {
                let channel =
                    |i: usize| half_value(u16::from_le_bytes([texel[2 * i], texel[2 * i + 1]]));
                assert_eq!(channel(3), 1.0);
                lightness_encoded([channel(0), channel(1), channel(2)])
            })
            .collect();
        for i in 1..TEXELS {
            assert!(l[i] > l[i - 1], "texel {i} : {} après {}", l[i], l[i - 1]);
        }
    }

    #[test]
    fn demi_flottants() {
        for value in [0.0_f32, 1.0, 0.5, 0.25, 0.149_019_6, 0.999, 1.0 / 3.0] {
            let back = half_value(half_bits(value));
            assert!((back - value).abs() <= value * 5e-4, "{value} → {back}");
        }
        assert_eq!(half_bits(1.0), 0x3C00);
        assert_eq!(half_bits(0.5), 0x3800);
        assert_eq!(half_bits(-1.0), 0);
    }

    #[test]
    fn braise_plancher_de_luminosite() {
        let p = Palette::new(PaletteKind::Braise);
        assert!(lightness(p.texels[0]) >= BRAISE_LIGHTNESS_FLOOR);
    }

    #[test]
    fn braise_lisible_en_deuteranopie() {
        // Sous simulation de deutéranopie, la luminosité continue de
        // croître d'un bout à l'autre et garde une large amplitude.
        let p = Palette::new(PaletteKind::Braise);
        let l: Vec<f32> = p
            .texels
            .iter()
            .map(|t| lightness(simulate_deuteranopia(*t)))
            .collect();
        for i in 1..TEXELS {
            assert!(
                l[i] >= l[i - 1] - 2e-3,
                "texel {i} : {} après {}",
                l[i],
                l[i - 1]
            );
        }
        assert!(l[TEXELS - 1] - l[0] > 0.5);
    }

    #[test]
    fn palettes_alternatives_croissantes() {
        for kind in [
            PaletteKind::Viridis,
            PaletteKind::Magma,
            PaletteKind::Cividis,
        ] {
            let p = Palette::new(kind);
            let l: Vec<f32> = p.texels.iter().map(|t| lightness(*t)).collect();
            for i in 1..TEXELS {
                assert!(l[i] >= l[i - 1] - 2e-3, "{} texel {i}", kind.label());
            }
        }
    }

    #[test]
    fn divergente_centree_sur_un_neutre_sombre() {
        let p = Palette::diverging();
        assert_eq!(hex(p.texels[0]), 0x4CC9F0);
        assert_eq!(hex(p.texels[TEXELS - 1]), 0xF77F00);
        let middle = lightness(p.texels[TEXELS / 2]);
        assert!(middle < lightness(p.texels[0]) && middle < lightness(p.texels[TEXELS - 1]));
    }

    #[test]
    fn couleur_et_texture() {
        let p = Palette::new(PaletteKind::Braise);
        assert_eq!(p.color(0.0), Color32::from_rgb(0x26, 0x2B, 0x52));
        assert_eq!(p.color(2.0), Color32::from_rgb(0xFF, 0xE9, 0xA8));
        assert_eq!(p.rgba16f().len(), TEXELS * 8);
    }
}
