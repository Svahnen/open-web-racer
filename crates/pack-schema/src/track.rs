//! Track document.

use crate::ai::{SegmentLanes, TrackAi};
use crate::buffer::{BufferBytes, BufferTable};
use crate::common::{Material, Mesh, Rgba, Texture, Transform, Vec3};
use crate::validate::Issues;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub data: BufferTable,
    /// World geometry split into chunks for culling.
    pub chunks: Vec<TrackChunk>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    /// Meshes instanced by `props`.
    #[serde(default)]
    pub prop_meshes: Vec<Mesh>,
    #[serde(default)]
    pub props: Vec<PropInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sky: Option<Sky>,
    /// Ordered along the driving direction; the last segment connects to the first when `closed`.
    pub centerline: Vec<CenterlineSegment>,
    #[serde(default)]
    pub closed: bool,
    #[serde(default)]
    pub walls: Vec<WallPolyline>,
    /// Surface palette; per-triangle and per-segment ids index into it.
    pub surfaces: Vec<SurfaceType>,
    pub spawn_slots: Vec<SpawnSlot>,
    /// Ordered gates; index 0 is the start/finish line.
    pub checkpoints: Vec<Gate>,
    #[serde(default)]
    pub variants: VariantFlags,
    #[serde(default = "default_laps")]
    pub default_laps: u32,
    /// Computer-driver data (speed profile, grid, pace trims).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<TrackAi>,
}

fn default_laps() -> u32 {
    3
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackChunk {
    pub mesh: Mesh,
    /// Axis-aligned bounds `[min, max]` in metres, for culling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<[Vec3; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PropKind {
    Static,
    /// Plays a looping texture animation or continuous motion.
    Animated,
    /// Activates when a vehicle enters `trigger_radius_m`.
    Triggered,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PropInstance {
    /// Index into `prop_meshes`.
    pub mesh: u32,
    pub transform: Transform,
    pub kind: PropKind,
    /// Animation rate for `animated` props (cycles per second).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation_rate_hz: Option<f32>,
    /// Activation radius for `triggered` props.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_radius_m: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SkyBackdrop {
    /// Horizontal strips of a panorama, left to right, each a texture index.
    Panorama { strips: Vec<u32> },
    /// Vertical gradient from zenith to horizon.
    Gradient { zenith: Rgba, horizon: Rgba },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HorizonParams {
    /// Horizon line as a fraction of screen height from the top, at zero pitch.
    pub height_fraction: f32,
    /// Panorama pixels shift by this many full turns per full turn of heading.
    #[serde(default = "one")]
    pub parallax: f32,
    #[serde(default)]
    pub haze_color: Option<Rgba>,
    #[serde(default)]
    pub haze_distance_m: Option<f32>,
}

fn one() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sky {
    pub backdrop: SkyBackdrop,
    pub horizon: HorizonParams,
    /// Colour above the backdrop and below the horizon respectively.
    pub fill_above: Rgba,
    pub fill_below: Rgba,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CenterlineSegment {
    /// Centre point at the start of the segment, metres.
    pub position: Vec3,
    /// Unit forward direction.
    pub forward: Vec3,
    /// Unit up direction (encodes banking and slope).
    pub up: Vec3,
    pub length_m: f32,
    pub left_width_m: f32,
    pub right_width_m: f32,
    /// Index into `surfaces`.
    pub surface: u32,
    /// Lane layout of this segment for computer drivers. When absent, lanes
    /// are derived from the segment widths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lanes: Option<SegmentLanes>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WallSide {
    Left,
    Right,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WallPolyline {
    pub points: Vec<Vec3>,
    pub height_m: f32,
    pub side: WallSide,
    #[serde(default)]
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FrictionClass {
    Paved,
    Wet,
    Loose,
    Grass,
    Ice,
    Rough,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceType {
    pub name: String,
    pub friction_class: FrictionClass,
    /// Tyre friction coefficient (dimensionless).
    pub friction: f32,
    /// Rolling resistance coefficient (dimensionless).
    #[serde(default)]
    pub rolling_resistance: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpawnSlot {
    pub position: Vec3,
    /// Heading in radians, counter-clockwise from +X seen from above.
    pub heading_rad: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    pub position: Vec3,
    /// Unit direction a vehicle must cross in.
    pub normal: Vec3,
    pub width_m: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VariantFlags {
    #[serde(default)]
    pub allow_mirror: bool,
    #[serde(default)]
    pub allow_reverse: bool,
}

fn unit(path: &str, v: Vec3, out: &mut Issues) {
    out.finite(path, &v);
    let l = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if (l - 1.0).abs() > 1e-3 {
        out.add(path, format!("vector length {l} is not 1"));
    }
}

impl Track {
    pub fn validate(&self, path: &str, bytes: Option<&BufferBytes>, out: &mut Issues) {
        if self.id.is_empty() {
            out.add(&format!("{path}.id"), "id must not be empty");
        }
        self.data.validate(&format!("{path}.data"), bytes, out);
        for (i, t) in self.textures.iter().enumerate() {
            t.validate(&format!("{path}.textures[{i}]"), &self.data, out);
        }
        for (i, m) in self.materials.iter().enumerate() {
            m.validate(&format!("{path}.materials[{i}]"), self.textures.len(), out);
        }
        if self.chunks.is_empty() {
            out.add(
                &format!("{path}.chunks"),
                "track needs at least one geometry chunk",
            );
        }
        for (i, c) in self.chunks.iter().enumerate() {
            let p = format!("{path}.chunks[{i}]");
            c.mesh.validate(
                &format!("{p}.mesh"),
                &self.data,
                self.materials.len(),
                bytes,
                out,
            );
            if let Some(m) = c.mesh.triangle_surface {
                if let Some(Ok(v)) = bytes.map(|b| self.data.read_u32(m, b)) {
                    if let Some(bad) = v.iter().find(|&&s| s as usize >= self.surfaces.len()) {
                        out.add(
                            &format!("{p}.mesh.triangle_surface"),
                            format!(
                                "surface id {bad} out of range ({} surfaces)",
                                self.surfaces.len()
                            ),
                        );
                    }
                }
            }
            if let Some([lo, hi]) = c.bounds {
                out.finite(&format!("{p}.bounds"), &[lo, hi].concat());
                if lo.iter().zip(hi.iter()).any(|(a, b)| a > b) {
                    out.add(&format!("{p}.bounds"), "min exceeds max");
                }
            }
        }
        for (i, m) in self.prop_meshes.iter().enumerate() {
            m.validate(
                &format!("{path}.prop_meshes[{i}]"),
                &self.data,
                self.materials.len(),
                bytes,
                out,
            );
        }
        for (i, p) in self.props.iter().enumerate() {
            let pp = format!("{path}.props[{i}]");
            out.index(
                &format!("{pp}.mesh"),
                p.mesh as usize,
                self.prop_meshes.len(),
                "prop mesh",
            );
            p.transform.validate(&format!("{pp}.transform"), out);
            match p.kind {
                PropKind::Animated => match p.animation_rate_hz {
                    Some(r) => out.positive(&format!("{pp}.animation_rate_hz"), r),
                    None => out.add(&pp, "animated props require animation_rate_hz"),
                },
                PropKind::Triggered => match p.trigger_radius_m {
                    Some(r) => out.positive(&format!("{pp}.trigger_radius_m"), r),
                    None => out.add(&pp, "triggered props require trigger_radius_m"),
                },
                PropKind::Static => {}
            }
        }
        if let Some(s) = &self.sky {
            let p = format!("{path}.sky");
            match &s.backdrop {
                SkyBackdrop::Panorama { strips } => {
                    if strips.is_empty() {
                        out.add(
                            &format!("{p}.backdrop.strips"),
                            "panorama needs at least one strip",
                        );
                    }
                    for (i, t) in strips.iter().enumerate() {
                        out.index(
                            &format!("{p}.backdrop.strips[{i}]"),
                            *t as usize,
                            self.textures.len(),
                            "texture",
                        );
                    }
                }
                SkyBackdrop::Gradient { zenith, horizon } => {
                    out.finite(&format!("{p}.backdrop"), &[*zenith, *horizon].concat());
                }
            }
            out.range(
                &format!("{p}.horizon.height_fraction"),
                s.horizon.height_fraction,
                0.0,
                1.0,
            );
            if let Some(d) = s.horizon.haze_distance_m {
                out.positive(&format!("{p}.horizon.haze_distance_m"), d);
            }
        }
        if self.centerline.len() < 2 {
            out.add(
                &format!("{path}.centerline"),
                "centerline needs at least 2 segments",
            );
        }
        for (i, s) in self.centerline.iter().enumerate() {
            let p = format!("{path}.centerline[{i}]");
            out.finite(&format!("{p}.position"), &s.position);
            unit(&format!("{p}.forward"), s.forward, out);
            unit(&format!("{p}.up"), s.up, out);
            let dot: f32 = s.forward.iter().zip(s.up.iter()).map(|(a, b)| a * b).sum();
            if dot.abs() > 0.999 {
                out.add(&p, "forward and up must not be parallel");
            }
            out.positive(&format!("{p}.length_m"), s.length_m);
            out.range(&format!("{p}.left_width_m"), s.left_width_m, 0.0, 1000.0);
            out.range(&format!("{p}.right_width_m"), s.right_width_m, 0.0, 1000.0);
            if s.left_width_m + s.right_width_m <= 0.0 {
                out.add(&p, "segment has zero total width");
            }
            out.index(
                &format!("{p}.surface"),
                s.surface as usize,
                self.surfaces.len(),
                "surface",
            );
            if let Some(l) = &s.lanes {
                l.validate(&format!("{p}.lanes"), out);
            }
        }
        if let Some(ai) = &self.ai {
            ai.validate(&format!("{path}.ai"), self.centerline.len(), out);
        }
        for (i, w) in self.walls.iter().enumerate() {
            let p = format!("{path}.walls[{i}]");
            if w.points.len() < 2 {
                out.add(&p, "wall polyline needs at least 2 points");
            }
            for pt in &w.points {
                out.finite(&format!("{p}.points"), pt);
            }
            out.positive(&format!("{p}.height_m"), w.height_m);
        }
        if self.surfaces.is_empty() {
            out.add(
                &format!("{path}.surfaces"),
                "at least one surface type is required",
            );
        }
        for (i, s) in self.surfaces.iter().enumerate() {
            let p = format!("{path}.surfaces[{i}]");
            out.range(&format!("{p}.friction"), s.friction, 0.0, 5.0);
            out.range(
                &format!("{p}.rolling_resistance"),
                s.rolling_resistance,
                0.0,
                1.0,
            );
        }
        if self.spawn_slots.is_empty() {
            out.add(
                &format!("{path}.spawn_slots"),
                "at least one spawn slot is required",
            );
        }
        for (i, s) in self.spawn_slots.iter().enumerate() {
            let p = format!("{path}.spawn_slots[{i}]");
            out.finite(&format!("{p}.position"), &s.position);
            out.finite(&format!("{p}.heading_rad"), &[s.heading_rad]);
        }
        if self.checkpoints.is_empty() {
            out.add(
                &format!("{path}.checkpoints"),
                "at least one checkpoint (the finish line) is required",
            );
        }
        for (i, g) in self.checkpoints.iter().enumerate() {
            let p = format!("{path}.checkpoints[{i}]");
            out.finite(&format!("{p}.position"), &g.position);
            unit(&format!("{p}.normal"), g.normal, out);
            out.positive(&format!("{p}.width_m"), g.width_m);
        }
        if self.default_laps == 0 {
            out.add(&format!("{path}.default_laps"), "must be at least 1");
        }
    }
}
