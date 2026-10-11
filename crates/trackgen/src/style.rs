//! Theme styling: palettes, surface kinds and terrain shape parameters.

use crate::math::{fbm2, lerp, smoothstep};
use crate::spec::Theme;

pub type Rgb = [f64; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShoulderKind {
    Sand,
    Gravel,
    Concrete,
    RedDirt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarrierKind {
    Concrete,
    Armco,
    Wood,
    Stone,
}

#[derive(Clone, Copy, Debug)]
pub struct SkyStyle {
    pub zenith: Rgb,
    pub mid: Rgb,
    pub horizon: Rgb,
    pub cloud_cover: f64,
    pub cloud_color: Rgb,
    pub hill_far: Rgb,
    pub hill_near: Rgb,
    /// Peak hill heights in degrees of elevation (far, near).
    pub hill_height: (f64, f64),
    /// Peak height in degrees of a far snow-capped range (0 = none).
    pub peaks: f64,
    /// Flat-topped (mesa-like) silhouettes instead of rolling ones.
    pub buttes: bool,
    pub stars: bool,
    /// Sun azimuth and elevation in radians, if visible.
    pub sun: Option<(f64, f64)>,
    pub glow: Rgb,
}

#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub theme: Theme,
    pub bank_gain: f64,
    pub max_bank_deg: f64,
    pub shoulder_width: f64,
    pub shoulder: ShoulderKind,
    pub barrier: BarrierKind,
    pub asphalt: Rgb,
    pub marking: Rgb,
    pub edge_marking: Rgb,
    pub kerb_a: Rgb,
    pub kerb_b: Rgb,
    pub grass_a: Rgb,
    pub grass_b: Rgb,
    pub verge: Rgb,
    pub sand: Rgb,
    pub rock_dark: Rgb,
    pub rock_light: Rgb,
    pub water_deep: Rgb,
    pub water_light: Rgb,
    /// Brightness multiplier for lit world materials (night is dark).
    pub light: f64,
    pub night: bool,
    pub sky: SkyStyle,
    pub sea_level: Option<f64>,
    /// Fraction of the road length that carries tall trees etc. (0..1).
    pub vegetation: f64,
}

pub fn style_for(theme: Theme) -> Style {
    match theme {
        Theme::Coastline => Style {
            theme,
            bank_gain: 5.0,
            max_bank_deg: 6.0,
            shoulder_width: 5.0,
            shoulder: ShoulderKind::Sand,
            barrier: BarrierKind::Armco,
            asphalt: [0.27, 0.28, 0.30],
            marking: [0.92, 0.92, 0.90],
            edge_marking: [0.92, 0.92, 0.90],
            kerb_a: [0.86, 0.12, 0.10],
            kerb_b: [0.93, 0.93, 0.90],
            grass_a: [0.30, 0.52, 0.20],
            grass_b: [0.45, 0.60, 0.25],
            verge: [0.52, 0.50, 0.30],
            sand: [0.86, 0.78, 0.58],
            rock_dark: [0.38, 0.35, 0.32],
            rock_light: [0.62, 0.58, 0.52],
            water_deep: [0.05, 0.30, 0.50],
            water_light: [0.25, 0.62, 0.78],
            light: 1.0,
            night: false,
            sky: SkyStyle {
                zenith: [0.14, 0.38, 0.78],
                mid: [0.38, 0.62, 0.90],
                horizon: [0.80, 0.90, 0.97],
                cloud_cover: 0.42,
                cloud_color: [1.0, 1.0, 1.0],
                hill_far: [0.55, 0.66, 0.78],
                hill_near: [0.32, 0.46, 0.40],
                hill_height: (5.0, 3.0),
                peaks: 0.0,
                buttes: false,
                stars: false,
                sun: Some((2.2, 0.62)),
                glow: [1.0, 0.95, 0.8],
            },
            sea_level: Some(-1.0),
            vegetation: 0.5,
        },
        Theme::PineRidge => Style {
            theme,
            bank_gain: 8.5,
            max_bank_deg: 12.0,
            shoulder_width: 2.8,
            shoulder: ShoulderKind::Gravel,
            barrier: BarrierKind::Wood,
            asphalt: [0.24, 0.25, 0.27],
            marking: [0.93, 0.90, 0.78],
            edge_marking: [0.93, 0.93, 0.93],
            kerb_a: [0.90, 0.55, 0.10],
            kerb_b: [0.92, 0.92, 0.88],
            grass_a: [0.16, 0.36, 0.14],
            grass_b: [0.28, 0.46, 0.18],
            verge: [0.38, 0.30, 0.20],
            sand: [0.55, 0.48, 0.36],
            rock_dark: [0.30, 0.29, 0.30],
            rock_light: [0.56, 0.55, 0.54],
            water_deep: [0.08, 0.26, 0.34],
            water_light: [0.30, 0.50, 0.58],
            light: 0.95,
            night: false,
            sky: SkyStyle {
                zenith: [0.28, 0.42, 0.62],
                mid: [0.55, 0.68, 0.82],
                horizon: [0.84, 0.88, 0.92],
                cloud_cover: 0.62,
                cloud_color: [0.97, 0.97, 0.98],
                hill_far: [0.50, 0.58, 0.68],
                hill_near: [0.18, 0.30, 0.24],
                hill_height: (11.0, 7.0),
                peaks: 19.0,
                buttes: false,
                stars: false,
                sun: Some((0.9, 0.5)),
                glow: [1.0, 0.96, 0.85],
            },
            sea_level: None,
            vegetation: 1.0,
        },
        Theme::RedMesa => Style {
            theme,
            bank_gain: 4.0,
            max_bank_deg: 5.0,
            shoulder_width: 7.0,
            shoulder: ShoulderKind::RedDirt,
            barrier: BarrierKind::Concrete,
            asphalt: [0.30, 0.29, 0.29],
            marking: [0.95, 0.95, 0.92],
            edge_marking: [0.97, 0.80, 0.15],
            kerb_a: [0.15, 0.15, 0.17],
            kerb_b: [0.95, 0.80, 0.12],
            grass_a: [0.62, 0.45, 0.28],
            grass_b: [0.70, 0.52, 0.30],
            verge: [0.66, 0.38, 0.22],
            sand: [0.78, 0.55, 0.34],
            rock_dark: [0.45, 0.20, 0.12],
            rock_light: [0.80, 0.42, 0.22],
            water_deep: [0.1, 0.3, 0.4],
            water_light: [0.3, 0.5, 0.6],
            light: 1.0,
            night: false,
            sky: SkyStyle {
                zenith: [0.20, 0.45, 0.82],
                mid: [0.55, 0.72, 0.92],
                horizon: [0.96, 0.84, 0.70],
                cloud_cover: 0.16,
                cloud_color: [1.0, 0.97, 0.92],
                hill_far: [0.80, 0.55, 0.45],
                hill_near: [0.60, 0.30, 0.20],
                hill_height: (9.0, 6.0),
                peaks: 0.0,
                buttes: true,
                stars: false,
                sun: Some((4.0, 0.9)),
                glow: [1.0, 0.92, 0.75],
            },
            sea_level: None,
            vegetation: 0.15,
        },
        Theme::NightHarbour => Style {
            theme,
            bank_gain: 3.0,
            max_bank_deg: 4.0,
            shoulder_width: 4.0,
            shoulder: ShoulderKind::Concrete,
            barrier: BarrierKind::Concrete,
            asphalt: [0.20, 0.21, 0.24],
            marking: [0.95, 0.95, 0.90],
            edge_marking: [0.98, 0.82, 0.18],
            kerb_a: [0.95, 0.75, 0.10],
            kerb_b: [0.10, 0.10, 0.12],
            grass_a: [0.20, 0.30, 0.20],
            grass_b: [0.24, 0.34, 0.22],
            verge: [0.34, 0.34, 0.36],
            sand: [0.40, 0.40, 0.42],
            rock_dark: [0.20, 0.20, 0.24],
            rock_light: [0.40, 0.40, 0.46],
            water_deep: [0.02, 0.05, 0.12],
            water_light: [0.10, 0.20, 0.35],
            light: 0.48,
            night: true,
            sky: SkyStyle {
                zenith: [0.01, 0.02, 0.08],
                mid: [0.04, 0.07, 0.18],
                horizon: [0.30, 0.22, 0.26],
                cloud_cover: 0.30,
                cloud_color: [0.22, 0.24, 0.34],
                hill_far: [0.10, 0.12, 0.20],
                hill_near: [0.04, 0.05, 0.09],
                hill_height: (4.0, 2.5),
                peaks: 0.0,
                buttes: false,
                stars: true,
                sun: None,
                glow: [0.9, 0.9, 1.0],
            },
            sea_level: Some(-2.2),
            vegetation: 0.05,
        },
    }
}

/// Absolute terrain height far from the road for a theme, before blending
/// with the road-hugging near field.
pub fn far_height(theme: Theme, seed: u64, x: f64, z: f64) -> f64 {
    match theme {
        Theme::Coastline => {
            let hills = fbm2(seed, x / 520.0 + 11.0, z / 520.0 + 5.0, 4, 0, 0);
            let inland = (z + 330.0).max(0.0);
            let land = 3.0 + 16.0 * hills + 0.045 * inland.min(500.0);
            // sea lies north of z = -330
            let sea = smoothstep(-330.0 + 40.0, -330.0 - 90.0, z);
            lerp(land, -7.0, sea)
        }
        Theme::PineRidge => {
            // broad massif plus sharper summits: snow-capped ridges far from the road
            let ridge = fbm2(seed, x / 650.0 + 3.0, z / 650.0 + 9.0, 5, 0, 0);
            let summit = fbm2(seed ^ 0x51, x / 240.0 + 1.0, z / 240.0 + 4.0, 4, 0, 0);
            let detail = fbm2(seed ^ 77, x / 90.0, z / 90.0, 3, 0, 0);
            let mass = (ridge - 0.28).max(0.0);
            let crest = 1.0 - (2.0 * summit - 1.0).abs();
            16.0 + 210.0 * mass * mass.sqrt() * 1.7
                + 46.0 * crest * crest * smoothstep(0.34, 0.6, ridge)
                + 7.0 * detail
        }
        Theme::RedMesa => {
            let m = fbm2(seed, x / 430.0 + 2.0, z / 430.0 + 7.0, 3, 0, 0);
            let mesa = smoothstep(0.58, 0.64, m);
            let detail = fbm2(seed ^ 5, x / 60.0, z / 60.0, 3, 0, 0);
            14.0 + 42.0 * mesa + 4.0 * detail + 5.0 * m
        }
        Theme::NightHarbour => {
            let m = fbm2(seed, x / 260.0 + 4.0, z / 260.0 + 1.0, 3, 0, 0);
            let land = smoothstep(0.40, 0.46, m);
            lerp(-3.4, 0.0, land) + 0.3 * fbm2(seed ^ 9, x / 30.0, z / 30.0, 2, 0, 0)
        }
    }
}

/// Amplitude (metres) of local ground undulation near the road.
pub fn bump_amplitude(theme: Theme) -> f64 {
    match theme {
        Theme::Coastline => 1.8,
        Theme::PineRidge => 3.0,
        Theme::RedMesa => 1.2,
        Theme::NightHarbour => 0.15,
    }
}
