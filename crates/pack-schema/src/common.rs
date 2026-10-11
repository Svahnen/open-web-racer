//! Types shared by track and car documents: meshes, materials, textures.
//!
//! Units: metres, radians unless a field name says otherwise. Coordinate
//! system: right-handed, +Y up, -Z forward for local frames. Colours are
//! linear-light RGBA in [0, 1] unless stored as 8-bit.

use crate::buffer::{AccessorId, BufferBytes, BufferTable, ComponentType, ElementKind};
use crate::validate::Issues;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type Vec3 = [f32; 3];
pub type Rgba = [f32; 4];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    #[serde(default)]
    pub translation: Vec3,
    /// Unit quaternion `[x, y, z, w]`.
    #[serde(default = "identity_rotation")]
    pub rotation: [f32; 4],
    #[serde(default = "unit_scale")]
    pub scale: Vec3,
}

fn identity_rotation() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

fn unit_scale() -> Vec3 {
    [1.0, 1.0, 1.0]
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: identity_rotation(),
            scale: unit_scale(),
        }
    }
}

impl Transform {
    pub fn validate(&self, path: &str, out: &mut Issues) {
        out.finite(&format!("{path}.translation"), &self.translation);
        out.finite(&format!("{path}.scale"), &self.scale);
        out.finite(&format!("{path}.rotation"), &self.rotation);
        let len = self.rotation.iter().map(|x| x * x).sum::<f32>().sqrt();
        if (len - 1.0).abs() > 1e-3 {
            out.add(
                &format!("{path}.rotation"),
                format!("quaternion length {len} is not 1"),
            );
        }
    }
}

/// A draw range of an indexed triangle list that uses one material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Primitive {
    /// Index into the owning document's material (or material-slot) list.
    pub material: u32,
    pub first_index: u32,
    pub index_count: u32,
}

/// Indexed triangle mesh. All vertex attributes share one vertex count.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Mesh {
    pub name: String,
    /// `f32` vec3, metres.
    pub positions: AccessorId,
    /// `f32` vec3, unit length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normals: Option<AccessorId>,
    /// `f32` vec2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv: Option<AccessorId>,
    /// Vertex colour: normalized `u8` vec4 or `f32` vec4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<AccessorId>,
    /// `u16` or `u32` scalar; triangle list.
    pub indices: AccessorId,
    pub primitives: Vec<Primitive>,
    /// Optional `u8` scalar, one surface-type index per triangle (tracks only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triangle_surface: Option<AccessorId>,
}

impl Mesh {
    pub fn validate(
        &self,
        path: &str,
        table: &BufferTable,
        material_count: usize,
        bytes: Option<&BufferBytes>,
        out: &mut Issues,
    ) {
        use ComponentType::{F32, U8, U16, U32};
        let vcount = table
            .accessors
            .get(self.positions as usize)
            .map(|a| a.count);
        table.expect(
            self.positions,
            &format!("{path}.positions"),
            &[F32],
            ElementKind::Vec3,
            None,
            out,
        );
        if let Some(n) = self.normals {
            table.expect(
                n,
                &format!("{path}.normals"),
                &[F32],
                ElementKind::Vec3,
                vcount,
                out,
            );
        }
        if let Some(n) = self.uv {
            table.expect(
                n,
                &format!("{path}.uv"),
                &[F32],
                ElementKind::Vec2,
                vcount,
                out,
            );
        }
        if let Some(n) = self.colors {
            table.expect(
                n,
                &format!("{path}.colors"),
                &[F32, U8],
                ElementKind::Vec4,
                vcount,
                out,
            );
        }
        let icount = table.accessors.get(self.indices as usize).map(|a| a.count);
        table.expect(
            self.indices,
            &format!("{path}.indices"),
            &[U16, U32],
            ElementKind::Scalar,
            None,
            out,
        );
        if let Some(c) = icount {
            if c % 3 != 0 {
                out.add(
                    &format!("{path}.indices"),
                    format!("count {c} is not a multiple of 3"),
                );
            }
            if let (Some(b), Some(v)) = (bytes, vcount) {
                if let Ok(idx) = table.read_u32(self.indices, b) {
                    if let Some(bad) = idx.iter().find(|&&i| i >= v) {
                        out.add(
                            &format!("{path}.indices"),
                            format!("index {bad} references a vertex beyond the {v} available"),
                        );
                    }
                }
            }
        }
        if self.primitives.is_empty() {
            out.add(
                &format!("{path}.primitives"),
                "mesh needs at least one primitive",
            );
        }
        for (i, p) in self.primitives.iter().enumerate() {
            let pp = format!("{path}.primitives[{i}]");
            out.index(
                &format!("{pp}.material"),
                p.material as usize,
                material_count,
                "material",
            );
            if p.index_count % 3 != 0 {
                out.add(
                    &pp,
                    format!("index_count {} is not a multiple of 3", p.index_count),
                );
            }
            if let Some(c) = icount {
                if u64::from(p.first_index) + u64::from(p.index_count) > u64::from(c) {
                    out.add(
                        &pp,
                        format!(
                            "range {}+{} exceeds the {c} indices of the mesh",
                            p.first_index, p.index_count
                        ),
                    );
                }
            }
        }
        if let (Some(s), Some(c)) = (self.triangle_surface, icount) {
            table.expect(
                s,
                &format!("{path}.triangle_surface"),
                &[U8],
                ElementKind::Scalar,
                Some(c / 3),
                out,
            );
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AlphaMode {
    #[default]
    Opaque,
    /// Fragments below `alpha_cutoff` are discarded.
    Cutout,
    /// Standard source-over blending.
    Blend,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WrapMode {
    #[default]
    Repeat,
    Clamp,
    Mirror,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Texture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// RGBA8 pixels, row-major from the top-left: `u8` vec4 normalized, `width*height` elements.
    pub pixels: AccessorId,
    #[serde(default)]
    pub wrap_u: WrapMode,
    #[serde(default)]
    pub wrap_v: WrapMode,
    /// Smooth filtering; false means nearest neighbour.
    #[serde(default = "yes")]
    pub smooth: bool,
}

fn yes() -> bool {
    true
}

/// Maximum texture edge accepted by validation.
pub const MAX_TEXTURE_EDGE: u32 = 8192;

impl Texture {
    pub fn validate(&self, path: &str, table: &BufferTable, out: &mut Issues) {
        if self.width == 0
            || self.height == 0
            || self.width > MAX_TEXTURE_EDGE
            || self.height > MAX_TEXTURE_EDGE
        {
            out.add(
                path,
                format!(
                    "size {}x{} must be within 1..={MAX_TEXTURE_EDGE}",
                    self.width, self.height
                ),
            );
            return;
        }
        table.expect(
            self.pixels,
            &format!("{path}.pixels"),
            &[ComponentType::U8],
            ElementKind::Vec4,
            Some(self.width * self.height),
            out,
        );
        if let Some(a) = table.accessors.get(self.pixels as usize) {
            if !a.normalized {
                out.add(
                    &format!("{path}.pixels"),
                    "RGBA8 accessor must be normalized",
                );
            }
        }
    }
}

/// Texture swapped over time: `frames` are texture indices played at `rate_hz`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextureAnimation {
    pub frames: Vec<u32>,
    pub rate_hz: f32,
    #[serde(default = "yes")]
    pub looping: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub name: String,
    /// Index into the document's texture list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<u32>,
    #[serde(default = "white")]
    pub base_color: Rgba,
    #[serde(default)]
    pub alpha_mode: AlphaMode,
    #[serde(default = "half")]
    pub alpha_cutoff: f32,
    #[serde(default)]
    pub double_sided: bool,
    /// Ignore lighting (self-lit, e.g. sky and lamps).
    #[serde(default)]
    pub unlit: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<TextureAnimation>,
}

fn white() -> Rgba {
    [1.0; 4]
}

fn half() -> f32 {
    0.5
}

impl Material {
    pub fn validate(&self, path: &str, texture_count: usize, out: &mut Issues) {
        if let Some(t) = self.texture {
            out.index(
                &format!("{path}.texture"),
                t as usize,
                texture_count,
                "texture",
            );
        }
        for (i, c) in self.base_color.iter().enumerate() {
            out.range(&format!("{path}.base_color[{i}]"), *c, 0.0, 1.0);
        }
        out.range(&format!("{path}.alpha_cutoff"), self.alpha_cutoff, 0.0, 1.0);
        if let Some(a) = &self.animation {
            let pa = format!("{path}.animation");
            if a.frames.len() < 2 {
                out.add(&pa, "animation needs at least 2 frames");
            }
            for (i, f) in a.frames.iter().enumerate() {
                out.index(
                    &format!("{pa}.frames[{i}]"),
                    *f as usize,
                    texture_count,
                    "texture",
                );
            }
            out.positive(&format!("{pa}.rate_hz"), a.rate_hz);
        }
    }
}
