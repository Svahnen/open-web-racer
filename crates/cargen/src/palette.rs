//! The eight-entry palette shared by all cars and the paint schemes.
//!
//! Palette index = [`crate::geom::Role`] discriminant. Entries 0 (body paint)
//! and 1 (accent) are the paint slot; the rest are fixed.

use crate::design::Design;

pub struct PaintScheme {
    pub id: &'static str,
    pub name: &'static str,
    pub body: [u8; 4],
    pub accent: [u8; 4],
}

pub const PAINTS: [PaintScheme; 8] = [
    PaintScheme {
        id: "signal-red",
        name: "Signal Red",
        body: [200, 38, 34, 255],
        accent: [236, 236, 230, 255],
    },
    PaintScheme {
        id: "harbour-blue",
        name: "Harbour Blue",
        body: [36, 92, 176, 255],
        accent: [240, 200, 40, 255],
    },
    PaintScheme {
        id: "meadow-green",
        name: "Meadow Green",
        body: [48, 140, 72, 255],
        accent: [30, 30, 34, 255],
    },
    PaintScheme {
        id: "solar-yellow",
        name: "Solar Yellow",
        body: [240, 196, 36, 255],
        accent: [30, 30, 34, 255],
    },
    PaintScheme {
        id: "ember-orange",
        name: "Ember Orange",
        body: [232, 108, 30, 255],
        accent: [230, 230, 226, 255],
    },
    PaintScheme {
        id: "graphite",
        name: "Graphite",
        body: [74, 78, 86, 255],
        accent: [200, 60, 50, 255],
    },
    PaintScheme {
        id: "pearl-white",
        name: "Pearl White",
        body: [232, 232, 228, 255],
        accent: [36, 92, 176, 255],
    },
    PaintScheme {
        id: "plum",
        name: "Plum",
        body: [120, 48, 120, 255],
        accent: [240, 200, 40, 255],
    },
];

/// Fixed entries 2..=7: glass, tyre, rim, trim, front lamp, rear lamp.
pub const FIXED: [[u8; 4]; 6] = [
    [30, 52, 66, 255],
    [28, 28, 32, 255],
    [200, 203, 208, 255],
    [40, 42, 46, 255],
    [250, 244, 200, 255],
    [200, 28, 32, 255],
];

pub fn palette(scheme: &PaintScheme) -> [[u8; 4]; 8] {
    let mut p = [[0; 4]; 8];
    p[0] = scheme.body;
    p[1] = scheme.accent;
    p[2..].copy_from_slice(&FIXED);
    p
}

pub fn default_palette(design: &Design) -> [[u8; 4]; 8] {
    palette(&PAINTS[design.default_paint])
}

/// sRGB byte to linear float, rounded for stable text output.
pub fn linear(c: u8) -> f64 {
    let s = f64::from(c) / 255.0;
    let l = if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    };
    (l * 10_000.0).round() / 10_000.0
}
