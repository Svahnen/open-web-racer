//! HUD document: sprite atlas, layout, gauges and digit strips.

use crate::buffer::{BufferBytes, BufferTable};
use crate::common::Texture;
use crate::validate::Issues;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Hud {
    #[serde(default)]
    pub data: BufferTable,
    pub atlas: Texture,
    /// Named pixel rectangles inside the atlas.
    pub rects: BTreeMap<String, AtlasRect>,
    /// Resolution the layout offsets are authored for.
    pub reference_resolution: [u32; 2],
    pub layout: Vec<HudElement>,
    #[serde(default)]
    pub gauges: Vec<Gauge>,
    #[serde(default)]
    pub digit_strips: Vec<DigitStrip>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AtlasRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// What an element shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ElementContent {
    /// A static sprite from `rects`.
    Sprite { rect: String },
    /// Index into `gauges`.
    Gauge { gauge: u32 },
    /// A number rendered with `digit_strips[strip]`; `source` names the value (for example `speed`, `lap`).
    Number {
        strip: u32,
        source: String,
        digits: u8,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HudElement {
    pub id: String,
    pub anchor: Anchor,
    /// Offset in reference-resolution pixels from the anchor point (+x right, +y down).
    #[serde(default)]
    pub offset: [f32; 2],
    #[serde(default = "one")]
    pub scale: f32,
    pub content: ElementContent,
}

fn one() -> f32 {
    1.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GaugeStyle {
    Needle,
    Bar,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Gauge {
    pub id: String,
    pub style: GaugeStyle,
    /// Value name such as `speed` or `engine_rpm`.
    pub source: String,
    pub min: f32,
    pub max: f32,
    /// Sprite for the dial face or bar fill.
    pub rect: String,
    /// Needle sweep in degrees clockwise from straight up.
    #[serde(default)]
    pub angle_min_deg: f32,
    #[serde(default)]
    pub angle_max_deg: f32,
    /// Needle pivot inside the `rect`, in sprite pixels.
    #[serde(default)]
    pub pivot: [f32; 2],
}

/// Glyph rectangles for a bitmap number font: `glyphs[i]` is drawn from `rects[i]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DigitStrip {
    pub id: String,
    /// Characters covered, for example `0123456789:.`.
    pub glyphs: String,
    /// One rect name per glyph.
    pub rects: Vec<String>,
    #[serde(default)]
    pub spacing_px: f32,
}

impl Hud {
    pub fn validate(&self, path: &str, bytes: Option<&BufferBytes>, out: &mut Issues) {
        self.data.validate(&format!("{path}.data"), bytes, out);
        self.atlas
            .validate(&format!("{path}.atlas"), &self.data, out);
        let [rw, rh] = self.reference_resolution;
        if rw == 0 || rh == 0 {
            out.add(&format!("{path}.reference_resolution"), "must be non-zero");
        }
        for (name, r) in &self.rects {
            let p = format!("{path}.rects[{name:?}]");
            if r.w == 0 || r.h == 0 {
                out.add(&p, "rect has zero size");
            }
            if u64::from(r.x) + u64::from(r.w) > u64::from(self.atlas.width)
                || u64::from(r.y) + u64::from(r.h) > u64::from(self.atlas.height)
            {
                out.add(
                    &p,
                    format!(
                        "rect {}+{} x {}+{} exceeds the {}x{} atlas",
                        r.x, r.w, r.y, r.h, self.atlas.width, self.atlas.height
                    ),
                );
            }
        }
        let known = |n: &str| self.rects.contains_key(n);
        for (i, g) in self.gauges.iter().enumerate() {
            let p = format!("{path}.gauges[{i}]");
            if !known(&g.rect) {
                out.add(&format!("{p}.rect"), format!("unknown rect {:?}", g.rect));
            }
            if g.max.is_nan() || g.min.is_nan() || g.max <= g.min {
                out.add(&p, "max must exceed min");
            }
            out.finite(&format!("{p}.pivot"), &g.pivot);
        }
        for (i, s) in self.digit_strips.iter().enumerate() {
            let p = format!("{path}.digit_strips[{i}]");
            if s.glyphs.chars().count() != s.rects.len() {
                out.add(
                    &p,
                    format!(
                        "{} glyphs but {} rects",
                        s.glyphs.chars().count(),
                        s.rects.len()
                    ),
                );
            }
            for r in &s.rects {
                if !known(r) {
                    out.add(&format!("{p}.rects"), format!("unknown rect {r:?}"));
                }
            }
        }
        for (i, e) in self.layout.iter().enumerate() {
            let p = format!("{path}.layout[{i}]");
            out.finite(&format!("{p}.offset"), &e.offset);
            out.positive(&format!("{p}.scale"), e.scale);
            match &e.content {
                ElementContent::Sprite { rect } => {
                    if !known(rect) {
                        out.add(
                            &format!("{p}.content.rect"),
                            format!("unknown rect {rect:?}"),
                        );
                    }
                }
                ElementContent::Gauge { gauge } => {
                    out.index(
                        &format!("{p}.content.gauge"),
                        *gauge as usize,
                        self.gauges.len(),
                        "gauge",
                    );
                }
                ElementContent::Number { strip, digits, .. } => {
                    out.index(
                        &format!("{p}.content.strip"),
                        *strip as usize,
                        self.digit_strips.len(),
                        "digit strip",
                    );
                    if *digits == 0 || *digits > 12 {
                        out.add(&format!("{p}.content.digits"), "digits must be 1..=12");
                    }
                }
            }
        }
    }
}
