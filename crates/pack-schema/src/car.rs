//! Car (vehicle appearance) document.

use crate::buffer::{AccessorId, BufferBytes, BufferTable, ComponentType, ElementKind};
use crate::common::{Material, Mesh, Rgba, Texture, Vec3};
use crate::validate::Issues;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Car {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub data: BufferTable,
    /// Detail levels, ordered from most to least detailed.
    pub lods: Vec<CarLod>,
    /// Primitive `material` indices refer to this list.
    pub material_slots: Vec<MaterialSlot>,
    pub textures: Vec<Texture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paint: Option<PaintSet>,
    pub wheels: Vec<WheelAnchor>,
    /// Optional shared wheel mesh. It is authored at the origin with its
    /// rotation axis along +X and its outer face towards +X; engines instance
    /// it at every wheel anchor, rotated 180 degrees about +Y for anchors with
    /// a negative X so the outer face always points away from the car. Its
    /// primitives index `material_slots` like the body mesh. This lets a
    /// renderer spin and steer wheels independently of the body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wheel_mesh: Option<Mesh>,
    #[serde(default)]
    pub lights: Vec<LightAnchor>,
    #[serde(default)]
    pub cameras: Vec<CameraAnchor>,
    #[serde(default)]
    pub mirrors: Vec<MirrorAnchor>,
    pub collision: CollisionBox,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CarLod {
    pub mesh: Mesh,
    /// Use this level up to this camera distance; `None` means unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_distance_m: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterialSlot {
    pub name: String,
    pub material: Material,
    /// Whether the body paint option recolours this slot.
    #[serde(default)]
    pub paintable: bool,
}

/// Indexed-colour source texture plus the named colour schemes applied to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PaintSet {
    /// Texture index the indexed pixels replace when painting.
    pub texture: u32,
    /// `u8` scalar palette indices, `width*height` of the texture; index 255 is transparent.
    pub indexed_pixels: AccessorId,
    pub base_palette: Vec<[u8; 4]>,
    pub options: Vec<PaintOption>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaintOption {
    pub name: String,
    #[serde(flatten)]
    pub scheme: PaintScheme,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaintScheme {
    /// Replace selected palette entries; the rest keep their base colour.
    Recolour { entries: Vec<PaletteEntry> },
    /// Replace the whole palette (same length as `base_palette`).
    FullPalette { colors: Vec<[u8; 4]> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PaletteEntry {
    pub index: u8,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WheelAnchor {
    pub name: String,
    /// Hub centre in car space.
    pub position: Vec3,
    pub radius_m: f32,
    pub width_m: f32,
    #[serde(default)]
    pub steered: bool,
    #[serde(default)]
    pub driven: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LightKind {
    Head,
    Brake,
    Reverse,
    Indicator,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LightAnchor {
    pub kind: LightKind,
    pub position: Vec3,
    pub direction: Vec3,
    pub color: Rgba,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CameraAnchor {
    pub name: String,
    pub position: Vec3,
    /// Unit view direction in car space.
    pub forward: Vec3,
    pub fov_deg: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MirrorAnchor {
    pub name: String,
    pub position: Vec3,
    /// Unit direction the mirror looks along, in car space.
    pub forward: Vec3,
    pub width_m: f32,
    pub height_m: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollisionBox {
    /// Half extents `[x, y, z]` (width, height, length), metres.
    pub half_extents_m: Vec3,
    /// Box centre offset from the car origin.
    #[serde(default)]
    pub center: Vec3,
}

fn unit(path: &str, v: Vec3, out: &mut Issues) {
    out.finite(path, &v);
    let l = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if (l - 1.0).abs() > 1e-3 {
        out.add(path, format!("vector length {l} is not 1"));
    }
}

impl Car {
    pub fn validate(&self, path: &str, bytes: Option<&BufferBytes>, out: &mut Issues) {
        if self.id.is_empty() {
            out.add(&format!("{path}.id"), "id must not be empty");
        }
        self.data.validate(&format!("{path}.data"), bytes, out);
        for (i, t) in self.textures.iter().enumerate() {
            t.validate(&format!("{path}.textures[{i}]"), &self.data, out);
        }
        for (i, s) in self.material_slots.iter().enumerate() {
            s.material.validate(
                &format!("{path}.material_slots[{i}].material"),
                self.textures.len(),
                out,
            );
        }
        if self.lods.is_empty() {
            out.add(&format!("{path}.lods"), "car needs at least one LOD");
        }
        let mut last = 0.0_f32;
        for (i, l) in self.lods.iter().enumerate() {
            let p = format!("{path}.lods[{i}]");
            l.mesh.validate(
                &format!("{p}.mesh"),
                &self.data,
                self.material_slots.len(),
                bytes,
                out,
            );
            if let Some(d) = l.max_distance_m {
                out.positive(&format!("{p}.max_distance_m"), d);
                if d <= last {
                    out.add(&p, "max_distance_m must increase with each LOD");
                }
                last = d;
            } else if i + 1 != self.lods.len() {
                out.add(&p, "only the last LOD may omit max_distance_m");
            }
        }
        if let Some(w) = &self.wheel_mesh {
            w.validate(
                &format!("{path}.wheel_mesh"),
                &self.data,
                self.material_slots.len(),
                bytes,
                out,
            );
        }
        if let Some(p) = &self.paint {
            self.validate_paint(p, &format!("{path}.paint"), bytes, out);
        }
        if self.wheels.is_empty() {
            out.add(
                &format!("{path}.wheels"),
                "car needs at least one wheel anchor",
            );
        }
        for (i, w) in self.wheels.iter().enumerate() {
            let p = format!("{path}.wheels[{i}]");
            out.finite(&format!("{p}.position"), &w.position);
            out.positive(&format!("{p}.radius_m"), w.radius_m);
            out.positive(&format!("{p}.width_m"), w.width_m);
        }
        for (i, l) in self.lights.iter().enumerate() {
            let p = format!("{path}.lights[{i}]");
            out.finite(&format!("{p}.position"), &l.position);
            unit(&format!("{p}.direction"), l.direction, out);
        }
        for (i, c) in self.cameras.iter().enumerate() {
            let p = format!("{path}.cameras[{i}]");
            out.finite(&format!("{p}.position"), &c.position);
            unit(&format!("{p}.forward"), c.forward, out);
            out.range(&format!("{p}.fov_deg"), c.fov_deg, 5.0, 170.0);
        }
        for (i, m) in self.mirrors.iter().enumerate() {
            let p = format!("{path}.mirrors[{i}]");
            out.finite(&format!("{p}.position"), &m.position);
            unit(&format!("{p}.forward"), m.forward, out);
            out.positive(&format!("{p}.width_m"), m.width_m);
            out.positive(&format!("{p}.height_m"), m.height_m);
        }
        let h = self.collision.half_extents_m;
        for (i, v) in h.iter().enumerate() {
            out.positive(&format!("{path}.collision.half_extents_m[{i}]"), *v);
        }
        out.finite(&format!("{path}.collision.center"), &self.collision.center);
    }

    fn validate_paint(
        &self,
        p: &PaintSet,
        path: &str,
        bytes: Option<&BufferBytes>,
        out: &mut Issues,
    ) {
        if p.base_palette.is_empty() || p.base_palette.len() > 255 {
            out.add(
                &format!("{path}.base_palette"),
                "palette must have 1..=255 entries",
            );
        }
        let tex = self.textures.get(p.texture as usize);
        out.index(
            &format!("{path}.texture"),
            p.texture as usize,
            self.textures.len(),
            "texture",
        );
        self.data.expect(
            p.indexed_pixels,
            &format!("{path}.indexed_pixels"),
            &[ComponentType::U8],
            ElementKind::Scalar,
            tex.map(|t| t.width * t.height),
            out,
        );
        let n = p.base_palette.len();
        for (i, o) in p.options.iter().enumerate() {
            let pp = format!("{path}.options[{i}]");
            if o.name.is_empty() {
                out.add(&format!("{pp}.name"), "paint option needs a name");
            }
            match &o.scheme {
                PaintScheme::Recolour { entries } => {
                    for (j, e) in entries.iter().enumerate() {
                        out.index(
                            &format!("{pp}.entries[{j}].index"),
                            e.index as usize,
                            n,
                            "palette entry",
                        );
                    }
                }
                PaintScheme::FullPalette { colors } => {
                    if colors.len() != n {
                        out.add(
                            &format!("{pp}.colors"),
                            format!(
                                "full palette has {} entries, base palette has {n}",
                                colors.len()
                            ),
                        );
                    }
                }
            }
        }
        if let Some(Ok(px)) = bytes.map(|b| self.data.read_u32(p.indexed_pixels, b)) {
            if let Some(bad) = px.iter().find(|&&v| v != 255 && v as usize >= n) {
                out.add(
                    &format!("{path}.indexed_pixels"),
                    format!("palette index {bad} out of range ({n} entries)"),
                );
            }
        }
    }
}
