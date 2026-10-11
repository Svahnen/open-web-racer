//! Project-owned 3D scene geometry and browser renderer.
//!
//! Scene and mesh data are independent of any game format. The synthetic
//! circuit is deliberately generic and exists as a renderer/asset-pipeline
//! fixture; compatibility assets can only be added from sanitized specs.

use racer_sim::{TrackRuntime, Vec2};
use std::collections::{HashMap, HashSet};

pub const MAX_RENDER_VEHICLE_PROFILES: usize = 8;
const MAX_PROFILE_TEXTURE_LAYERS: usize = 256;
const MAX_PROFILE_TEXTURE_BYTES: usize = 64 * 1024 * 1024;
const MAX_PROFILE_GEOMETRY_BYTES: usize = 32 * 1024 * 1024;
const MAX_PROFILE_MATERIALS: usize = 4096;
const MAX_PROFILE_MESHES: usize = 4096 + MAX_RENDER_VEHICLE_PROFILES;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    pub const UP: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    pub fn dot(self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z
    }
    pub fn cross(self, rhs: Self) -> Self {
        Self::new(
            self.y * rhs.z - self.z * rhs.y,
            self.z * rhs.x - self.x * rhs.z,
            self.x * rhs.y - self.y * rhs.x,
        )
    }
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn normalized(self) -> Self {
        let n = self.length();
        if n > 1.0e-6 {
            self * (1.0 / n)
        } else {
            Self::UP
        }
    }
}
impl std::ops::Add for Vec3 {
    type Output = Self;
    fn add(self, r: Self) -> Self {
        Self::new(self.x + r.x, self.y + r.y, self.z + r.z)
    }
}
impl std::ops::Sub for Vec3 {
    type Output = Self;
    fn sub(self, r: Self) -> Self {
        Self::new(self.x - r.x, self.y - r.y, self.z - r.z)
    }
}
impl std::ops::Mul<f32> for Vec3 {
    type Output = Self;
    fn mul(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
}

fn atlas_rect_is_well_formed(uv_offset: [f32; 2], uv_scale: [f32; 2]) -> bool {
    uv_offset.iter().all(|v| v.is_finite() && *v >= 0.0)
        && uv_scale
            .iter()
            .zip(uv_offset)
            .all(|(scale, offset)| scale.is_finite() && *scale >= 0.0 && offset + *scale <= 1.0)
}

/// A material slot references an RGBA texture and carries a tint. UVs are
/// authored per vertex; multiple meshes may share the same material slot.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub tint: [f32; 4],
    /// Project preview compositing policy. Missing descriptor values decode as
    /// `Opaque` to preserve existing scenes and cutout behavior.
    pub alpha_mode: MaterialAlphaMode,
    /// Texture-array page containing this image.
    pub texture: usize,
    /// Atlas coordinates of the first source texel's center.
    pub uv_offset: [f32; 2],
    /// Atlas-space distance from the first to last source texel centers.
    pub uv_scale: [f32; 2],
    pub wrap_u: UvWrap,
    pub wrap_v: UvWrap,
    /// Optional frame sequence for animated track textures
    /// Frame 0 equals the base `texture`/`uv_*` fields.
    pub animation: Option<MaterialAnimation>,
}

/// Ticks per second assumed for texture animation clocks (project preview
/// policy; its tick unit is unverified).
pub const MATERIAL_ANIMATION_TICKS_PER_SECOND: f64 = 32.0;
/// Upper bound on frames in one animated material.
pub const MAX_MATERIAL_ANIMATION_FRAMES: usize = 255;

/// One atlas rectangle of an animated material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialFrame {
    pub texture: usize,
    pub uv_offset: [f32; 2],
    pub uv_scale: [f32; 2],
}

/// Frame sequence advanced as `(tick div period_ticks) mod frames.len()`.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialAnimation {
    pub period_ticks: u32,
    pub frames: Vec<MaterialFrame>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaterialAlphaMode {
    #[default]
    Opaque,
    StraightAlpha,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UvWrap {
    #[default]
    Clamp,
    Repeat,
}

impl Material {
    pub fn whole_layer(
        name: impl Into<String>,
        tint: [f32; 4],
        texture: usize,
        uv_wrap: UvWrap,
    ) -> Self {
        Self::atlas_layer(
            name,
            tint,
            texture,
            [0.0, 0.0],
            [1.0, 1.0],
            uv_wrap,
            uv_wrap,
        )
    }

    /// Construct a texture-array atlas reference. Offset and scale address
    /// the first/last source texel centers within an equal-sized page.
    pub fn atlas_layer(
        name: impl Into<String>,
        tint: [f32; 4],
        texture: usize,
        uv_offset: [f32; 2],
        uv_scale: [f32; 2],
        wrap_u: UvWrap,
        wrap_v: UvWrap,
    ) -> Self {
        Self {
            name: name.into(),
            tint,
            alpha_mode: MaterialAlphaMode::Opaque,
            texture,
            uv_offset,
            uv_scale,
            wrap_u,
            wrap_v,
            animation: None,
        }
    }

    pub fn atlas_uv_is_well_formed(&self) -> bool {
        atlas_rect_is_well_formed(self.uv_offset, self.uv_scale)
            && self.animation.as_ref().is_none_or(|animation| {
                animation.period_ticks > 0
                    && (1..=MAX_MATERIAL_ANIMATION_FRAMES).contains(&animation.frames.len())
                    && animation
                        .frames
                        .iter()
                        .all(|frame| atlas_rect_is_well_formed(frame.uv_offset, frame.uv_scale))
            })
    }

    /// True when every frame of the animation addresses an existing layer.
    pub fn frames_reference_layers(&self, layer_count: usize) -> bool {
        self.animation.as_ref().is_none_or(|animation| {
            animation
                .frames
                .iter()
                .all(|frame| frame.texture < layer_count)
        })
    }

    /// Atlas rectangle shown at `tick`; static materials always show the base.
    pub fn frame_at(&self, tick: u64) -> MaterialFrame {
        match &self.animation {
            Some(animation) if !animation.frames.is_empty() && animation.period_ticks > 0 => {
                let index =
                    (tick / u64::from(animation.period_ticks)) % animation.frames.len() as u64;
                animation.frames[index as usize]
            }
            _ => MaterialFrame {
                texture: self.texture,
                uv_offset: self.uv_offset,
                uv_scale: self.uv_scale,
            },
        }
    }
}

/// Same-sized RGBA8 image layers bound as a 2D texture array. A material's
/// `texture` field selects a layer, keeping texture references explicit in the
/// generic mesh boundary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RgbaTextureArray {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Vec<u8>>,
}

impl RgbaTextureArray {
    pub fn is_well_formed(&self) -> bool {
        let Some(layer_bytes) = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            return false;
        };
        self.width > 0
            && self.height > 0
            && !self.layers.is_empty()
            && self.layers.iter().all(|layer| layer.len() == layer_bytes)
    }
}

#[cfg(any(target_arch = "wasm32", test))]
const TRACK_SURFACE_GRID_AXIS: usize = 64;
#[cfg(any(target_arch = "wasm32", test))]
const MAX_TRACK_SURFACE_MESHES: usize = 512;
#[cfg(any(target_arch = "wasm32", test))]
const MAX_TRACK_SURFACE_TRIANGLES: usize = 200_000;
#[cfg(any(target_arch = "wasm32", test))]
const MAX_TRACK_SURFACE_CELL_REFS: usize = 1_000_000;
#[cfg(any(target_arch = "wasm32", test))]
const MAX_TRACK_SURFACE_CELL_TRIANGLES: usize = 4_096;
#[cfg(any(target_arch = "wasm32", test))]
const MAX_TRACK_SURFACE_LEVELS: usize = 8;
#[cfg(any(target_arch = "wasm32", test))]
const TRACK_SURFACE_LEVEL_MERGE_EPSILON: f32 = 0.05;
#[cfg(any(target_arch = "wasm32", test))]
const TRACK_SURFACE_AMBIGUITY_EPSILON: f32 = 0.01;
pub const MAX_RENDER_VEHICLES: usize = 8;

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct SurfaceTriangle {
    a: Vec3,
    b: Vec3,
    c: Vec3,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Copy, Debug, Default)]
struct SurfaceHeightLevel {
    sum: f32,
    count: u32,
}

#[cfg(any(target_arch = "wasm32", test))]
impl SurfaceHeightLevel {
    fn height(self) -> f32 {
        self.sum / self.count.max(1) as f32
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct TrackSurfaceSample {
    height: f32,
    distinct_levels: u8,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
enum TrackSurfaceLookup {
    Hit(TrackSurfaceSample),
    Miss,
    Ambiguous,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum TrackSurfaceOutcome {
    #[default]
    Uninitialized,
    Sampled {
        distinct_levels: u8,
    },
    Miss,
    Ambiguous,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Copy, Debug, Default)]
struct TrackSurfaceState {
    last_valid: Option<TrackSurfaceSample>,
    outcome: TrackSurfaceOutcome,
}

#[cfg(any(target_arch = "wasm32", test))]
impl TrackSurfaceState {
    fn apply_lookup(
        &mut self,
        lookup: TrackSurfaceLookup,
        provisional_origin_z: f32,
        vehicle_min_z_scaled: f32,
    ) -> f32 {
        match lookup {
            TrackSurfaceLookup::Hit(sample) => {
                self.last_valid = Some(sample);
                self.outcome = TrackSurfaceOutcome::Sampled {
                    distinct_levels: sample.distinct_levels,
                };
                sample.height - vehicle_min_z_scaled
            }
            TrackSurfaceLookup::Miss => {
                self.outcome = TrackSurfaceOutcome::Miss;
                self.last_valid
                    .map(|sample| sample.height - vehicle_min_z_scaled)
                    .unwrap_or(provisional_origin_z)
            }
            TrackSurfaceLookup::Ambiguous => {
                self.outcome = TrackSurfaceOutcome::Ambiguous;
                self.last_valid
                    .map(|sample| sample.height - vehicle_min_z_scaled)
                    .unwrap_or(provisional_origin_z)
            }
        }
    }

    fn previous_ground_height(self, provisional_origin_z: f32, vehicle_min_z_scaled: f32) -> f32 {
        self.last_valid
            .map(|sample| sample.height)
            .unwrap_or(provisional_origin_z + vehicle_min_z_scaled)
    }
}

/// Bounded XY grid over only descriptor-designated static track triangles.
/// It is a display alignment aid; it does not affect simulation or collision.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Debug)]
struct TrackSurface {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    triangles: Vec<SurfaceTriangle>,
    cells: Vec<Vec<u32>>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl TrackSurface {
    fn build(scene: &Scene3d, track_mesh_indices: &[u32]) -> Result<Self, &'static str> {
        if track_mesh_indices.is_empty()
            || track_mesh_indices.len() > MAX_TRACK_SURFACE_MESHES
            || track_mesh_indices
                .iter()
                .enumerate()
                .any(|(index, mesh)| track_mesh_indices[..index].contains(mesh))
        {
            return Err("track surface mesh list is empty or oversized");
        }
        let mut triangles = Vec::new();
        let mut bounds = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        let mut matched_track_instances = 0usize;
        for instance in scene
            .instances
            .iter()
            .filter(|instance| track_mesh_indices.contains(&instance.mesh_index))
        {
            matched_track_instances += 1;
            let mesh = scene
                .meshes
                .get(instance.mesh_index as usize)
                .ok_or("track surface references a missing mesh")?;
            if !mesh.is_well_formed() {
                return Err("track surface mesh is malformed");
            }
            if triangles.len().saturating_add(mesh.indices.len() / 3) > MAX_TRACK_SURFACE_TRIANGLES
            {
                return Err("track surface triangle budget exceeded");
            }
            for tri in mesh.indices.chunks_exact(3) {
                let a = instance.transform_point(mesh.vertices[tri[0] as usize].position);
                let b = instance.transform_point(mesh.vertices[tri[1] as usize].position);
                let c = instance.transform_point(mesh.vertices[tri[2] as usize].position);
                let area2 = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
                if !area2.is_finite() || area2.abs() <= 1.0e-7 {
                    continue;
                }
                bounds[0] = bounds[0].min(a.x.min(b.x).min(c.x));
                bounds[1] = bounds[1].min(a.y.min(b.y).min(c.y));
                bounds[2] = bounds[2].max(a.x.max(b.x).max(c.x));
                bounds[3] = bounds[3].max(a.y.max(b.y).max(c.y));
                triangles.push(SurfaceTriangle { a, b, c });
            }
        }
        if matched_track_instances == 0
            || triangles.is_empty()
            || !bounds.iter().all(|value| value.is_finite())
            || bounds[0] >= bounds[2]
            || bounds[1] >= bounds[3]
            || !(bounds[2] - bounds[0]).is_finite()
            || !(bounds[3] - bounds[1]).is_finite()
        {
            return Err("track surface has no finite XY triangles");
        }

        let mut cells = vec![Vec::new(); TRACK_SURFACE_GRID_AXIS * TRACK_SURFACE_GRID_AXIS];
        let mut cell_refs = 0usize;
        for (index, triangle) in triangles.iter().enumerate() {
            let tri_min_x = triangle.a.x.min(triangle.b.x).min(triangle.c.x);
            let tri_min_y = triangle.a.y.min(triangle.b.y).min(triangle.c.y);
            let tri_max_x = triangle.a.x.max(triangle.b.x).max(triangle.c.x);
            let tri_max_y = triangle.a.y.max(triangle.b.y).max(triangle.c.y);
            let x0 = surface_cell(tri_min_x, bounds[0], bounds[2]);
            let y0 = surface_cell(tri_min_y, bounds[1], bounds[3]);
            let x1 = surface_cell(tri_max_x, bounds[0], bounds[2]);
            let y1 = surface_cell(tri_max_y, bounds[1], bounds[3]);
            let references = (x1 - x0 + 1).saturating_mul(y1 - y0 + 1);
            cell_refs = cell_refs
                .checked_add(references)
                .filter(|count| *count <= MAX_TRACK_SURFACE_CELL_REFS)
                .ok_or("track surface grid reference budget exceeded")?;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let cell = &mut cells[y * TRACK_SURFACE_GRID_AXIS + x];
                    if cell.len() >= MAX_TRACK_SURFACE_CELL_TRIANGLES {
                        return Err("track surface cell candidate budget exceeded");
                    }
                    cell.push(index as u32);
                }
            }
        }
        Ok(Self {
            min_x: bounds[0],
            min_y: bounds[1],
            max_x: bounds[2],
            max_y: bounds[3],
            triangles,
            cells,
        })
    }

    fn sample(&self, x: f32, y: f32, previous_height: f32) -> TrackSurfaceLookup {
        if !x.is_finite()
            || !y.is_finite()
            || !previous_height.is_finite()
            || x < self.min_x
            || x > self.max_x
            || y < self.min_y
            || y > self.max_y
        {
            return TrackSurfaceLookup::Miss;
        }
        let cell_x = surface_cell(x, self.min_x, self.max_x);
        let cell_y = surface_cell(y, self.min_y, self.max_y);
        let Some(cell) = self.cells.get(cell_y * TRACK_SURFACE_GRID_AXIS + cell_x) else {
            return TrackSurfaceLookup::Miss;
        };
        let mut levels = [SurfaceHeightLevel::default(); MAX_TRACK_SURFACE_LEVELS];
        let mut level_count = 0usize;
        for &triangle_index in cell {
            let triangle = self.triangles[triangle_index as usize];
            let Some(height) = triangle_height_at_xy(triangle, x, y) else {
                continue;
            };
            if let Some(level) = levels[..level_count]
                .iter_mut()
                .find(|level| (level.height() - height).abs() <= TRACK_SURFACE_LEVEL_MERGE_EPSILON)
            {
                level.sum += height;
                level.count += 1;
            } else if level_count < MAX_TRACK_SURFACE_LEVELS {
                levels[level_count] = SurfaceHeightLevel {
                    sum: height,
                    count: 1,
                };
                level_count += 1;
            } else {
                return TrackSurfaceLookup::Ambiguous;
            }
        }
        if level_count == 0 {
            return TrackSurfaceLookup::Miss;
        }
        let mut nearest_index = 0usize;
        let mut nearest_distance = (levels[0].height() - previous_height).abs();
        let mut tied = false;
        for (index, level) in levels.iter().enumerate().take(level_count).skip(1) {
            let distance = (level.height() - previous_height).abs();
            if (distance - nearest_distance).abs() <= TRACK_SURFACE_AMBIGUITY_EPSILON {
                tied = true;
            } else if distance < nearest_distance {
                nearest_index = index;
                nearest_distance = distance;
                tied = false;
            }
        }
        if tied {
            return TrackSurfaceLookup::Ambiguous;
        }
        TrackSurfaceLookup::Hit(TrackSurfaceSample {
            height: levels[nearest_index].height(),
            distinct_levels: level_count as u8,
        })
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn surface_cell(value: f32, min: f32, max: f32) -> usize {
    (((value - min) / (max - min) * TRACK_SURFACE_GRID_AXIS as f32) as usize)
        .min(TRACK_SURFACE_GRID_AXIS - 1)
}

#[cfg(any(target_arch = "wasm32", test))]
fn triangle_height_at_xy(triangle: SurfaceTriangle, x: f32, y: f32) -> Option<f32> {
    let (a, b, c) = (triangle.a, triangle.b, triangle.c);
    let denominator = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
    if !denominator.is_finite() || denominator.abs() <= 1.0e-7 {
        return None;
    }
    let u = ((b.y - c.y) * (x - c.x) + (c.x - b.x) * (y - c.y)) / denominator;
    let v = ((c.y - a.y) * (x - c.x) + (a.x - c.x) * (y - c.y)) / denominator;
    let w = 1.0 - u - v;
    const EDGE_TOLERANCE: f32 = 1.0e-5;
    if u < -EDGE_TOLERANCE || v < -EDGE_TOLERANCE || w < -EDGE_TOLERANCE {
        return None;
    }
    let height = u * a.z + v * b.z + w * c.z;
    height.is_finite().then_some(height)
}

/// Keep wgpu-hal's GL backend from mistaking a square D2 array whose layer
/// count is divisible by six for a cube-compatible texture. The extra layer
/// is storage-only: decoded scene data and material-reference validation keep
/// the logical source layer count and indexing, while the GPU upload and view
/// span the initialized storage depth.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn texture_array_storage_layers(
    width: u32,
    height: u32,
    layers: usize,
    max_layers: u32,
) -> Option<u32> {
    let layers = u32::try_from(layers).ok()?;
    if layers == 0 {
        return None;
    }
    let storage_layers = if width == height && (layers == 1 || layers % 6 == 0) {
        layers.checked_add(1)
    } else {
        Some(layers)
    }?;
    (storage_layers <= max_layers).then_some(storage_layers)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshVertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: [f32; 2],
    pub material: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexedMesh {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
}

impl IndexedMesh {
    pub fn triangle(&mut self, a: MeshVertex, b: MeshVertex, c: MeshVertex) {
        let base = self.vertices.len() as u32;
        self.vertices.extend([a, b, c]);
        self.indices.extend([base, base + 1, base + 2]);
    }
    pub fn quad(&mut self, points: [Vec3; 4], uv: [[f32; 2]; 4], material: u32) {
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalized();
        let mut vertices = points.map(|position| MeshVertex {
            position,
            normal,
            uv: [0.0, 0.0],
            material,
        });
        for i in 0..4 {
            vertices[i].uv = uv[i];
        }
        let base = self.vertices.len() as u32;
        self.vertices.extend(vertices);
        self.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    pub fn is_well_formed(&self) -> bool {
        !self.vertices.is_empty()
            && !self.indices.is_empty()
            && self.indices.len() % 3 == 0
            && self
                .indices
                .iter()
                .all(|i| (*i as usize) < self.vertices.len())
            && self.vertices.iter().all(|v| {
                v.position.x.is_finite()
                    && v.position.y.is_finite()
                    && v.position.z.is_finite()
                    && v.normal.x.is_finite()
                    && v.normal.y.is_finite()
                    && v.normal.z.is_finite()
                    && v.uv.iter().all(|n| n.is_finite())
            })
    }
}

/// Distance fog and scene grading for project-owned scenes. Imported scenes
/// never carry one, so their shading is unchanged. Distances are view-space
/// metres; `amount` is the largest blend toward `color` (reached at `end`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneFog {
    pub color: [f32; 3],
    pub start: f32,
    pub end: f32,
    pub amount: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene3d {
    pub meshes: Vec<IndexedMesh>,
    /// Meshes drawn in the camera-centered background pass. These mesh IDs
    /// must not also be instantiated or designated as track geometry.
    pub background_mesh_indices: Vec<u32>,
    /// Reusable local-space meshes instantiated at world poses. Static content
    /// uses identity transforms; vehicle poses can change without rebuilding it.
    pub instances: Vec<MeshInstance>,
    pub materials: Vec<Material>,
    pub textures: RgbaTextureArray,
}

/// Validate the optional camera-centered mesh set and return its maximum
/// distance from the camera origin for an independent background projection.
fn validate_background_meshes(scene: &Scene3d, track_mesh_indices: &[u32]) -> Result<f32, String> {
    const MAX_MESHES: usize = 64;
    const MAX_VERTICES: usize = 65_536;
    const MAX_INDICES: usize = 196_608;
    const MAX_COORDINATE: f32 = 1_000_000.0;
    if scene.background_mesh_indices.len() > MAX_MESHES {
        return Err("3D scene has too many background meshes".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut total_vertices = 0usize;
    let mut total_indices = 0usize;
    let mut min_radius = f32::INFINITY;
    let mut max_radius = 0.0_f32;
    for &mesh_index in &scene.background_mesh_indices {
        let Some(mesh) = scene.meshes.get(mesh_index as usize) else {
            return Err("3D scene has an invalid background mesh reference".into());
        };
        if !seen.insert(mesh_index)
            || track_mesh_indices.contains(&mesh_index)
            || scene.instances.iter().any(|i| i.mesh_index == mesh_index)
            || !mesh.is_well_formed()
            || mesh
                .vertices
                .iter()
                .any(|v| v.material as usize >= scene.materials.len())
        {
            return Err("3D background meshes overlap foreground geometry or are malformed".into());
        }
        total_vertices = total_vertices.saturating_add(mesh.vertices.len());
        total_indices = total_indices.saturating_add(mesh.indices.len());
        for vertex in &mesh.vertices {
            let p = vertex.position;
            if [p.x, p.y, p.z].iter().any(|c| c.abs() > MAX_COORDINATE) {
                return Err("3D background coordinates exceed supported bounds".into());
            }
            let radius = p.length();
            if !radius.is_finite() {
                return Err("3D background geometry has a non-finite extent".into());
            }
            min_radius = min_radius.min(radius);
            max_radius = max_radius.max(radius);
        }
    }
    if total_vertices > MAX_VERTICES || total_indices > MAX_INDICES {
        return Err("3D background geometry exceeds supported bounds".into());
    }
    if !scene.background_mesh_indices.is_empty() && min_radius <= 0.01 {
        return Err("background geometry must remain away from the camera origin".into());
    }
    Ok(max_radius)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshInstance {
    pub mesh_index: u32,
    pub translation: Vec3,
    /// Rotation around Z; vehicle local forward is +X.
    pub heading: f32,
    /// Up axis for a preview vehicle resting on a selected support triangle.
    /// Static meshes and network poses use world up.
    pub ground_normal: Vec3,
    /// Positive uniform scale applied in local mesh coordinates.
    pub scale: f32,
}

/// Local vehicle profile identity. The semantic ID comes from the bounded
/// project profile list; the fingerprint binds it to one locally imported car.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VehicleProfileIdentity {
    pub vehicle_profile_id: String,
    pub vehicle_source_fingerprint: String,
}

impl VehicleProfileIdentity {
    pub fn new(vehicle_profile_id: &str, vehicle_source_fingerprint: &str) -> Result<Self, String> {
        if vehicle_profile_id.is_empty()
            || vehicle_profile_id.len() > 64
            || !vehicle_profile_id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err("vehicle profile ID is outside the supported semantic ID format".into());
        }
        if vehicle_source_fingerprint.len() != 64
            || !vehicle_source_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("vehicle source fingerprint must be lowercase SHA-256 hex".into());
        }
        Ok(Self {
            vehicle_profile_id: vehicle_profile_id.to_owned(),
            vehicle_source_fingerprint: vehicle_source_fingerprint.to_owned(),
        })
    }
}

/// Append only the designated vehicle mesh from another validated imported
/// scene. Its used materials and texture pages are remapped into the local
/// scene; source track geometry, props, and template instances are never copied.
pub fn append_vehicle_profile_mesh(
    target: &mut Scene3d,
    source: &Scene3d,
    vehicle_instance_index: usize,
) -> Result<MeshInstance, String> {
    let mut candidate = target.clone();
    let instance =
        append_vehicle_profile_mesh_in_place(&mut candidate, source, vehicle_instance_index)?;
    *target = candidate;
    Ok(instance)
}

/// Mutate an already-private candidate scene after all input and aggregate
/// limits are checked. Public callers use the atomic wrapper above.
fn append_vehicle_profile_mesh_in_place(
    target: &mut Scene3d,
    source: &Scene3d,
    vehicle_instance_index: usize,
) -> Result<MeshInstance, String> {
    if !target.textures.is_well_formed() || !source.textures.is_well_formed() {
        return Err("vehicle profile has malformed texture pages".into());
    }
    if target.textures.width != source.textures.width
        || target.textures.height != source.textures.height
    {
        return Err("vehicle profile texture dimensions do not match the active scene".into());
    }
    let mut instance = source
        .instances
        .get(vehicle_instance_index)
        .copied()
        .ok_or_else(|| "vehicle profile has no designated vehicle instance".to_string())?;
    let source_mesh = source
        .meshes
        .get(instance.mesh_index as usize)
        .ok_or_else(|| "vehicle profile references a missing mesh".to_string())?;
    if !source_mesh.is_well_formed() {
        return Err("vehicle profile mesh is malformed".into());
    }
    if !instance.scale.is_finite() || instance.scale <= 0.0 {
        return Err("vehicle profile instance scale is invalid".into());
    }
    let geometry_bytes = source_mesh
        .vertices
        .len()
        .checked_mul(std::mem::size_of::<MeshVertex>())
        .and_then(|bytes| {
            source_mesh
                .indices
                .len()
                .checked_mul(std::mem::size_of::<u32>())
                .and_then(|indices| bytes.checked_add(indices))
        })
        .ok_or_else(|| "vehicle profile geometry size overflow".to_string())?;
    if geometry_bytes > MAX_PROFILE_GEOMETRY_BYTES {
        return Err("vehicle profile geometry exceeds its memory budget".into());
    }

    let source_materials = source_mesh
        .vertices
        .iter()
        .map(|vertex| vertex.material as usize)
        .collect::<HashSet<_>>();
    if source_materials.is_empty() {
        return Err("vehicle profile mesh has no materials".into());
    }
    let mut source_materials = source_materials.into_iter().collect::<Vec<_>>();
    source_materials.sort_unstable();
    let mut page_remap = HashMap::new();
    let mut new_page_sources = Vec::<usize>::new();
    for &source_index in &source_materials {
        let material = source
            .materials
            .get(source_index)
            .ok_or_else(|| "vehicle profile references a missing material".to_string())?;
        if !material.atlas_uv_is_well_formed()
            || material.tint.iter().any(|value| !value.is_finite())
        {
            return Err("vehicle profile has a malformed material".into());
        }
        let source_page = source
            .textures
            .layers
            .get(material.texture)
            .ok_or_else(|| "vehicle profile material references a missing texture".to_string())?;
        let texture_index = match target
            .textures
            .layers
            .iter()
            .position(|page| page == source_page)
        {
            Some(index) => index,
            None => match new_page_sources
                .iter()
                .position(|&index| source.textures.layers[index] == *source_page)
            {
                Some(index) => target.textures.layers.len() + index,
                None => {
                    let index = target.textures.layers.len() + new_page_sources.len();
                    new_page_sources.push(material.texture);
                    index
                }
            },
        };
        page_remap.insert(material.texture, texture_index);
    }
    let total_texture_bytes = target
        .textures
        .layers
        .iter()
        .map(Vec::len)
        .chain(
            new_page_sources
                .iter()
                .map(|&index| source.textures.layers[index].len()),
        )
        .try_fold(0usize, |sum, bytes| sum.checked_add(bytes))
        .ok_or_else(|| "vehicle profile texture size overflow".to_string())?;
    if target.textures.layers.len() + new_page_sources.len() > MAX_PROFILE_TEXTURE_LAYERS
        || total_texture_bytes > MAX_PROFILE_TEXTURE_BYTES
    {
        return Err("registered vehicle profile textures exceed their aggregate budget".into());
    }
    if target.materials.len() + source_materials.len() > MAX_PROFILE_MATERIALS {
        return Err("registered vehicle profile materials exceed their aggregate budget".into());
    }
    if target.meshes.len() >= MAX_PROFILE_MESHES {
        return Err("registered vehicle profile meshes exceed their aggregate budget".into());
    }
    let final_material_count = target
        .materials
        .len()
        .checked_add(source_materials.len())
        .ok_or_else(|| "vehicle profile material count overflow".to_string())?;
    if final_material_count > u32::MAX as usize || target.meshes.len() > u32::MAX as usize {
        return Err("vehicle profile indices exceed supported bounds".into());
    }

    target.textures.layers.extend(
        new_page_sources
            .into_iter()
            .map(|index| source.textures.layers[index].clone()),
    );
    let mut material_remap = HashMap::new();
    for source_index in source_materials {
        let mut material = source.materials[source_index].clone();
        material.texture = *page_remap
            .get(&material.texture)
            .ok_or_else(|| "vehicle profile texture remap is incomplete".to_string())?;
        let target_material_index = target.materials.len() as u32;
        target.materials.push(material);
        material_remap.insert(source_index as u32, target_material_index);
    }
    let mut mesh = source_mesh.clone();
    for vertex in &mut mesh.vertices {
        vertex.material = *material_remap
            .get(&vertex.material)
            .ok_or_else(|| "vehicle profile material remap is incomplete".to_string())?;
    }
    instance.mesh_index = target.meshes.len() as u32;
    target.meshes.push(mesh);
    Ok(instance)
}

fn valid_source_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn vehicle_mesh_geometry_bytes(scene: &Scene3d, instance: MeshInstance) -> Result<usize, String> {
    let mesh = scene
        .meshes
        .get(instance.mesh_index as usize)
        .ok_or_else(|| "vehicle profile references a missing mesh".to_string())?;
    mesh.vertices
        .len()
        .checked_mul(std::mem::size_of::<MeshVertex>())
        .and_then(|bytes| {
            mesh.indices
                .len()
                .checked_mul(std::mem::size_of::<u32>())
                .and_then(|indices| bytes.checked_add(indices))
        })
        .ok_or_else(|| "vehicle profile geometry size overflow".to_string())
}

impl MeshInstance {
    fn basis(self) -> (Vec3, Vec3, Vec3) {
        let up = self.ground_normal.normalized();
        let flat_forward = Vec3::new(self.heading.cos(), self.heading.sin(), 0.0);
        let projected = flat_forward - up * flat_forward.dot(up);
        let fallback = Vec3::new(1.0, 0.0, 0.0) - up * up.x;
        let forward = if projected.length() > 1.0e-6 {
            projected.normalized()
        } else {
            fallback.normalized()
        };
        let left = up.cross(forward).normalized();
        let forward = left.cross(up).normalized();
        (forward, left, up)
    }

    pub fn transform_point(self, point: Vec3) -> Vec3 {
        let point = point * self.scale;
        let (forward, left, up) = self.basis();
        self.translation + forward * point.x + left * point.y + up * point.z
    }

    pub fn transform_normal(self, normal: Vec3) -> Vec3 {
        let (forward, left, up) = self.basis();
        (forward * normal.x + left * normal.y + up * normal.z).normalized()
    }

    #[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
    fn transform_for_frame(self) -> MeshInstanceFrameTransform {
        let (forward, left, up) = self.basis();
        MeshInstanceFrameTransform {
            translation: self.translation,
            scale: self.scale,
            forward,
            left,
            up,
        }
    }
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
#[derive(Clone, Copy)]
struct MeshInstanceFrameTransform {
    translation: Vec3,
    scale: f32,
    forward: Vec3,
    left: Vec3,
    up: Vec3,
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
impl MeshInstanceFrameTransform {
    fn transform_point(self, point: Vec3) -> Vec3 {
        let point = point * self.scale;
        self.translation + self.forward * point.x + self.left * point.y + self.up * point.z
    }

    fn transform_normal(self, normal: Vec3) -> Vec3 {
        (self.forward * normal.x + self.left * normal.y + self.up * normal.z).normalized()
    }
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
fn vehicle_instances_for_poses(
    template: MeshInstance,
    poses: &[(Vec3, f32)],
) -> Result<Vec<MeshInstance>, &'static str> {
    if poses.len() > MAX_RENDER_VEHICLES {
        return Err("render vehicle count exceeds the protocol limit");
    }
    let mut instances = Vec::with_capacity(poses.len());
    for &(position, heading) in poses {
        if !position.x.is_finite()
            || !position.y.is_finite()
            || !position.z.is_finite()
            || !heading.is_finite()
        {
            return Err("render vehicle pose contains a non-finite value");
        }
        instances.push(MeshInstance {
            translation: position,
            heading,
            ..template
        });
    }
    Ok(instances)
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
fn compose_render_instances(
    static_instances: &[MeshInstance],
    vehicle_template: MeshInstance,
    poses: &[(Vec3, f32)],
    hidden_vehicle_index: Option<usize>,
) -> Result<Vec<MeshInstance>, &'static str> {
    if hidden_vehicle_index.is_some_and(|index| index >= poses.len()) {
        return Err("hidden render vehicle index is outside the pose list");
    }
    let visible_poses = poses.len() - usize::from(hidden_vehicle_index.is_some());
    let total = static_instances
        .len()
        .checked_add(visible_poses)
        .ok_or("render instance count overflow")?;
    let mut instances = Vec::with_capacity(total);
    instances.extend_from_slice(static_instances);
    if let Some(hidden_index) = hidden_vehicle_index {
        let visible = poses
            .iter()
            .enumerate()
            .filter_map(|(index, pose)| (index != hidden_index).then_some(*pose))
            .collect::<Vec<_>>();
        instances.extend(vehicle_instances_for_poses(vehicle_template, &visible)?);
    } else {
        instances.extend(vehicle_instances_for_poses(vehicle_template, poses)?);
    }
    Ok(instances)
}

fn compose_profiled_render_instances(
    static_instances: &[MeshInstance],
    templates: &[MeshInstance],
    poses: &[(Vec3, f32, Vec3)],
    hidden_vehicle_index: Option<usize>,
) -> Result<Vec<MeshInstance>, &'static str> {
    if templates.len() != poses.len() {
        return Err("vehicle profile count does not match the pose list");
    }
    if poses.len() > MAX_RENDER_VEHICLES {
        return Err("render vehicle count exceeds the protocol limit");
    }
    if hidden_vehicle_index.is_some_and(|index| index >= poses.len()) {
        return Err("hidden render vehicle index is outside the pose list");
    }
    let total = static_instances
        .len()
        .checked_add(poses.len() - usize::from(hidden_vehicle_index.is_some()))
        .ok_or("render instance count overflow")?;
    let mut instances = Vec::with_capacity(total);
    instances.extend_from_slice(static_instances);
    for (index, (template, &(position, heading, ground_normal))) in
        templates.iter().zip(poses).enumerate()
    {
        if Some(index) == hidden_vehicle_index {
            continue;
        }
        if !position.x.is_finite()
            || !position.y.is_finite()
            || !position.z.is_finite()
            || !heading.is_finite()
            || !ground_normal.x.is_finite()
            || !ground_normal.y.is_finite()
            || !ground_normal.z.is_finite()
            || ground_normal.length() <= 1.0e-6
            || ground_normal.z <= 0.0
        {
            return Err(
                "render vehicle pose contains an invalid ground normal or non-finite value",
            );
        }
        instances.push(MeshInstance {
            translation: position,
            heading,
            ground_normal: ground_normal.normalized(),
            ..*template
        });
    }
    Ok(instances)
}

fn active_mirror_local_index(visible: bool, local_vehicle_index: Option<usize>) -> Option<usize> {
    if visible { local_vehicle_index } else { None }
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
fn vehicle_roster_changed(current: Option<&str>, next: &str) -> bool {
    current != Some(next)
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
fn vehicle_camera_pose(
    poses: &[(Vec3, f32)],
    local_vehicle_index: Option<usize>,
    fallback: (Vec3, f32),
) -> Result<(Vec3, f32), &'static str> {
    match local_vehicle_index {
        Some(index) => poses
            .get(index)
            .copied()
            .ok_or("local render vehicle index is outside the pose list"),
        None => Ok(fallback),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera3d {
    pub position: Vec3,
    pub target: Vec3,
    pub vertical_fov_radians: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewCameraMode {
    #[default]
    Chase,
    Wide,
    Close,
    Road,
}

impl PreviewCameraMode {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "chase" => Some(Self::Chase),
            "wide" => Some(Self::Wide),
            "close" => Some(Self::Close),
            "road" | "hood" => Some(Self::Road),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Chase => "chase",
            Self::Wide => "wide",
            Self::Close => "close",
            Self::Road => "road",
        }
    }
}

impl Camera3d {
    pub fn chase(position: Vec2, heading: f64) -> Self {
        Self::chase_world(
            Vec3::new(position.x as f32, position.y as f32, 0.0),
            heading as f32,
        )
    }

    pub fn chase_world(position: Vec3, heading: f32) -> Self {
        let h = heading;
        let forward = Vec3::new(h.cos(), h.sin(), 0.0);
        let target = position + Vec3::new(0.0, 0.0, 0.45) + forward * 2.5;
        Self {
            position: target - forward * 14.0 + Vec3::new(0.0, 0.0, 7.0),
            target,
            vertical_fov_radians: 0.92,
            near: 0.1,
            far: 500.0,
        }
    }

    /// Build a neutral preview view around a vehicle pose. The modes expose
    /// common race-view categories; their offsets are project choices, not
    /// measurements of a reference camera.
    pub fn preview_world(position: Vec3, heading: f32, mode: PreviewCameraMode) -> Self {
        match mode {
            PreviewCameraMode::Chase => Self::chase_world(position, heading),
            PreviewCameraMode::Wide => {
                let forward = Vec3::new(heading.cos(), heading.sin(), 0.0);
                let target = position + Vec3::new(0.0, 0.0, 0.55) + forward * 4.0;
                Self {
                    position: target - forward * 23.0 + Vec3::new(0.0, 0.0, 11.5),
                    target,
                    vertical_fov_radians: 1.05,
                    near: 0.1,
                    far: 500.0,
                }
            }
            PreviewCameraMode::Close => {
                let forward = Vec3::new(heading.cos(), heading.sin(), 0.0);
                Self {
                    position: position - forward * 8.0 + Vec3::new(0.0, 0.0, 3.4),
                    target: position + Vec3::new(0.0, 0.0, 0.5) + forward * 2.5,
                    vertical_fov_radians: 0.92,
                    near: 0.1,
                    far: 500.0,
                }
            }
            PreviewCameraMode::Road => {
                let forward = Vec3::new(heading.cos(), heading.sin(), 0.0);
                Self {
                    position: position + forward * 2.5 + Vec3::new(0.0, 0.0, 0.65),
                    target: position + forward * 30.0 + Vec3::new(0.0, 0.0, 0.45),
                    vertical_fov_radians: 1.05,
                    near: 0.1,
                    far: 500.0,
                }
            }
        }
    }

    /// Look back along the road from a low point behind the vehicle. This is
    /// a temporary preview view and does not change the vehicle or simulation.
    pub fn look_behind_world(position: Vec3, heading: f32) -> Self {
        let forward = Vec3::new(heading.cos(), heading.sin(), 0.0);
        Self {
            position: position - forward * 4.0 + Vec3::new(0.0, 0.0, 1.5),
            target: position - forward * 30.0 + Vec3::new(0.0, 0.0, 0.8),
            vertical_fov_radians: 1.0,
            near: 0.1,
            far: 500.0,
        }
    }

    /// Project-owned camera for the rear-view inset. It sits slightly forward
    /// of the vehicle center and looks back along the local supported plane;
    /// the vehicle mesh itself is excluded from the inset scene.
    pub fn rear_mirror_world(position: Vec3, heading: f32, up: Vec3) -> Self {
        let up = up.normalized();
        let planar_forward = Vec3::new(heading.cos(), heading.sin(), 0.0);
        let forward = (planar_forward - up * planar_forward.dot(up)).normalized();
        Self {
            position: position + forward * 1.4 + up * 1.25,
            target: position - forward * 32.0 + up * 0.85,
            vertical_fov_radians: 0.82,
            near: 0.1,
            far: 400.0,
        }
    }
    pub fn orbit(target: Vec3, yaw: f32, pitch: f32, distance: f32) -> Self {
        let pitch = pitch.clamp(0.08, 1.35);
        let distance = distance.clamp(2.0, 80.0);
        let horizontal = distance * pitch.cos();
        Self {
            position: target
                + Vec3::new(
                    -yaw.cos() * horizontal,
                    -yaw.sin() * horizontal,
                    distance * pitch.sin(),
                ),
            target,
            vertical_fov_radians: 0.92,
            near: 0.1,
            far: 500.0,
        }
    }
    /// Right-handed camera projection with WebGPU's `[0, 1]` depth range.
    pub fn project(&self, point: Vec3, aspect: f32) -> [f32; 4] {
        self.project_with_up(point, aspect, Vec3::UP)
    }

    /// Project with an explicit camera up axis. The rear-view preview uses
    /// the local supported surface normal; other project cameras keep world up.
    pub fn project_with_up(&self, point: Vec3, aspect: f32, camera_up: Vec3) -> [f32; 4] {
        let forward = (self.target - self.position).normalized();
        let mut right = forward.cross(camera_up.normalized());
        if right.length() <= 1.0e-5 {
            right = forward.cross(Vec3::UP);
        }
        let right = right.normalized();
        let up = right.cross(forward).normalized();
        let rel = point - self.position;
        let x = rel.dot(right);
        let y = rel.dot(up);
        // Keep homogeneous view-space depth unmodified. Clamping points behind
        // the eye to a positive `w` distorts triangles crossing the eye plane
        // and can corrupt the depth buffer; the GPU clips those primitives.
        let z = rel.dot(forward);
        let f = 1.0 / (self.vertical_fov_radians * 0.5).tan();
        let a = self.far / (self.far - self.near);
        [x * f / aspect, y * f, a * z - self.near * a, z]
    }
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
#[derive(Clone, Copy)]
struct CameraFrameProjection {
    position: Vec3,
    forward: Vec3,
    right: Vec3,
    up: Vec3,
    focal_scale: f32,
    depth_scale: f32,
    depth_offset: f32,
    aspect: f32,
}

#[allow(dead_code)]
impl CameraFrameProjection {
    fn new(camera: Camera3d, aspect: f32, camera_up: Vec3) -> Self {
        let forward = (camera.target - camera.position).normalized();
        let mut right = forward.cross(camera_up.normalized());
        if right.length() <= 1.0e-5 {
            right = forward.cross(Vec3::UP);
        }
        let right = right.normalized();
        let up = right.cross(forward).normalized();
        let focal_scale = 1.0 / (camera.vertical_fov_radians * 0.5).tan();
        let depth_scale = camera.far / (camera.far - camera.near);
        Self {
            position: camera.position,
            forward,
            right,
            up,
            focal_scale,
            depth_scale,
            depth_offset: camera.near * depth_scale,
            aspect,
        }
    }

    fn project(self, point: Vec3) -> [f32; 4] {
        let relative = point - self.position;
        let x = relative.dot(self.right);
        let y = relative.dot(self.up);
        // Keep homogeneous view-space depth unmodified. Clamping points behind
        // the eye distorts triangles crossing the eye plane and can corrupt the
        // depth buffer; the GPU clips those primitives.
        let z = relative.dot(self.forward);
        [
            x * self.focal_scale / self.aspect,
            y * self.focal_scale,
            self.depth_scale * z - self.depth_offset,
            z,
        ]
    }
}

/// A generic, project-owned fixture scene with indexed 3D ground, road, curbs,
/// scenery, and an articulated-by-transform car. It intentionally contains no
/// reference-game layout or art.
fn synthetic_textures() -> RgbaTextureArray {
    let width = 16;
    let height = 16;
    let mut light = Vec::with_capacity(width * height * 4);
    let mut asphalt = Vec::with_capacity(width * height * 4);
    let mut cutout = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            light.extend_from_slice(&[248, 248, 248, 255]);
            let base = if ((x / 2 + y / 2) & 1) == 0 { 222 } else { 200 };
            let grain = ((x * 13 + y * 7) % 9) as u8;
            let shade = base - grain;
            asphalt.extend_from_slice(&[shade, shade, shade, 255]);
            let alpha = if ((x / 2 + y / 2) & 1) == 0 { 0 } else { 255 };
            cutout.extend_from_slice(&[255, 255, 255, alpha]);
        }
    }
    RgbaTextureArray {
        width: width as u32,
        height: height as u32,
        layers: vec![light, asphalt, cutout],
    }
}

/// Project-owned placeholder car: chassis, cabin and four wheels, front along
/// +X in local coordinates, resting on Z = 0. The arguments are the material
/// indices used for the paint, rubber and glass parts.
pub fn project_car_mesh(paint: u32, rubber: u32, glass: u32) -> IndexedMesh {
    let mut car_mesh = IndexedMesh::default();
    // Chassis box: front points along +X in local coordinates.
    box_mesh(
        &mut car_mesh,
        Vec3::new(0.0, 0.0, 0.48),
        Vec3::new(2.15, 0.95, 0.42),
        paint,
    );
    box_mesh(
        &mut car_mesh,
        Vec3::new(-0.25, 0.0, 0.83),
        Vec3::new(0.92, 0.72, 0.34),
        glass,
    );
    for x in [-1.30, 1.25] {
        for y in [-0.98, 0.98] {
            box_mesh(
                &mut car_mesh,
                Vec3::new(x, y, 0.25),
                Vec3::new(0.48, 0.17, 0.32),
                rubber,
            );
        }
    }
    car_mesh
}

pub fn synthetic_scene(track: &TrackRuntime, car: Vec2, heading: f64) -> Scene3d {
    let mut scene = Scene3d {
        meshes: Vec::new(),
        background_mesh_indices: Vec::new(),
        instances: Vec::new(),
        materials: vec![
            Material::whole_layer("grass", [0.25, 0.43, 0.25, 1.0], 0, UvWrap::Clamp),
            Material::whole_layer("asphalt", [0.30, 0.32, 0.35, 1.0], 1, UvWrap::Repeat),
            Material::whole_layer("curb", [0.88, 0.86, 0.72, 1.0], 0, UvWrap::Clamp),
            Material::whole_layer("car-paint", [0.83, 0.23, 0.10, 1.0], 0, UvWrap::Clamp),
            Material::whole_layer("rubber", [0.07, 0.08, 0.09, 1.0], 1, UvWrap::Repeat),
            Material::whole_layer("glass", [0.12, 0.48, 0.62, 1.0], 0, UvWrap::Clamp),
        ],
        textures: synthetic_textures(),
    };
    let mut ground = IndexedMesh::default();
    ground.quad(
        [
            Vec3::new(-90.0, -90.0, -0.15),
            Vec3::new(90.0, -90.0, -0.15),
            Vec3::new(90.0, 90.0, -0.15),
            Vec3::new(-90.0, 90.0, -0.15),
        ],
        [[0.0, 0.0], [24.0, 0.0], [24.0, 24.0], [0.0, 24.0]],
        0,
    );
    scene.meshes.push(ground);
    scene.instances.push(MeshInstance {
        mesh_index: 0,
        translation: Vec3::ZERO,
        heading: 0.0,
        ground_normal: Vec3::UP,
        scale: 1.0,
    });
    let route = track
        .checkpoints
        .iter()
        .map(|g| g.center)
        .collect::<Vec<_>>();
    let mut road = IndexedMesh::default();
    if route.len() > 2 {
        let left = route_offset(&route, 5.15);
        let right = route_offset(&route, -5.15);
        let curb_left = route_offset(&route, 5.78);
        let curb_right = route_offset(&route, -5.78);
        let mut distance = vec![0.0_f32; route.len() + 1];
        for i in 0..route.len() {
            distance[i + 1] =
                distance[i] + (route[(i + 1) % route.len()] - route[i]).length() as f32;
        }
        for i in 0..route.len() {
            let j = (i + 1) % route.len();
            let v3 = |p: Vec2| Vec3::new(p.x as f32, p.y as f32, 0.02);
            let u0 = distance[i] * 0.18;
            let u1 = distance[i + 1] * 0.18;
            road.quad(
                [v3(right[i]), v3(right[j]), v3(left[j]), v3(left[i])],
                [[u0, 0.0], [u1, 0.0], [u1, 1.0], [u0, 1.0]],
                1,
            );
            road.quad(
                [v3(left[i]), v3(left[j]), v3(curb_left[j]), v3(curb_left[i])],
                [[u0, 0.0], [u1, 0.0], [u1, 1.0], [u0, 1.0]],
                2,
            );
            road.quad(
                [
                    v3(curb_right[i]),
                    v3(curb_right[j]),
                    v3(right[j]),
                    v3(right[i]),
                ],
                [[u0, 0.0], [u1, 0.0], [u1, 1.0], [u0, 1.0]],
                2,
            );
        }
    }
    scene.meshes.push(road);
    scene.instances.push(MeshInstance {
        mesh_index: 1,
        translation: Vec3::ZERO,
        heading: 0.0,
        ground_normal: Vec3::UP,
        scale: 1.0,
    });
    let car_mesh = project_car_mesh(3, 4, 5);
    scene.meshes.push(car_mesh);
    scene.instances.push(MeshInstance {
        mesh_index: 2,
        translation: Vec3::new(car.x as f32, car.y as f32, 0.0),
        heading: heading as f32,
        ground_normal: Vec3::UP,
        scale: 1.0,
    });
    scene
}

/// A vehicle body ready to be placed in a scene: local-space mesh (forward
/// +X, up +Z, wheels resting on z = 0) whose vertices index `materials`
/// (each a tint on texture layer 0 of the synthetic texture array).
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleMesh {
    pub mesh: IndexedMesh,
    pub materials: Vec<Material>,
}

/// The project circuit with `vehicle` as the designated vehicle (instance 2)
/// instead of the built-in box car.
pub fn synthetic_scene_with_vehicle(track: &TrackRuntime, vehicle: &VehicleMesh) -> Scene3d {
    let mut scene = synthetic_scene(track, Vec2::new(0.0, 0.0), 0.0);
    let base = scene.materials.len() as u32;
    scene.materials.extend(vehicle.materials.iter().cloned());
    let mut mesh = vehicle.mesh.clone();
    for vertex in &mut mesh.vertices {
        vertex.material += base;
    }
    scene.meshes[2] = mesh;
    scene
}

/// A one-vehicle scene that can be registered as an extra vehicle profile of
/// a project scene (same texture page as [`synthetic_scene`]).
pub fn vehicle_source_scene(vehicle: &VehicleMesh) -> Scene3d {
    Scene3d {
        meshes: vec![vehicle.mesh.clone()],
        background_mesh_indices: Vec::new(),
        instances: vec![MeshInstance {
            mesh_index: 0,
            translation: Vec3::ZERO,
            heading: 0.0,
            ground_normal: Vec3::UP,
            scale: 1.0,
        }],
        materials: vehicle.materials.clone(),
        textures: synthetic_textures(),
    }
}

/// A one-vehicle scene whose single texture page is solid white at the given
/// size, so the vehicle can be registered as an extra profile of a scene whose
/// pages are `width` x `height` and include an identical white page (the
/// materials of a [`VehicleMesh`] address page 0 and only tint it).
pub fn vehicle_source_scene_on_white_page(
    vehicle: &VehicleMesh,
    width: u32,
    height: u32,
) -> Scene3d {
    let mut scene = vehicle_source_scene(vehicle);
    scene.textures = RgbaTextureArray {
        width,
        height,
        layers: vec![vec![255u8; width as usize * height as usize * 4]],
    };
    scene
}

/// Large project-owned synthetic scene for renderer throughput measurement:
/// the generic circuit plus a ground grid of about `track_triangles` triangles
/// and `prop_instances` small box instances. Designated vehicle instance is 2
/// and the support meshes are `[0, 1]`, exactly as in [`synthetic_scene`].
pub fn perf_fixture_scene(
    track: &TrackRuntime,
    prop_instances: u32,
    track_triangles: u32,
) -> Scene3d {
    let mut scene = synthetic_scene(track, Vec2::new(0.0, 0.0), 0.0);
    let cells = ((track_triangles / 2).max(1) as f32).sqrt().ceil() as u32;
    let extent = 180.0_f32;
    let step = extent / cells as f32;
    let mut grid = IndexedMesh::default();
    for row in 0..cells {
        for column in 0..cells {
            let x = -extent * 0.5 + column as f32 * step;
            let y = -extent * 0.5 + row as f32 * step;
            let z = -0.3 + 0.05 * (((row * 7 + column * 3) % 5) as f32);
            grid.quad(
                [
                    Vec3::new(x, y, z),
                    Vec3::new(x + step, y, z),
                    Vec3::new(x + step, y + step, z),
                    Vec3::new(x, y + step, z),
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                (row + column) % 3,
            );
        }
    }
    let grid_index = scene.meshes.len() as u32;
    scene.meshes.push(grid);
    scene.instances.push(MeshInstance {
        mesh_index: grid_index,
        translation: Vec3::ZERO,
        heading: 0.0,
        ground_normal: Vec3::UP,
        scale: 1.0,
    });
    let mut prop = IndexedMesh::default();
    box_mesh(
        &mut prop,
        Vec3::new(0.0, 0.0, 0.6),
        Vec3::new(0.4, 0.4, 0.6),
        2,
    );
    let prop_index = scene.meshes.len() as u32;
    scene.meshes.push(prop);
    for index in 0..prop_instances {
        let x = ((index * 37) % 360) as f32 * 0.5 - 90.0;
        let y = ((index * 91) % 340) as f32 * 0.5 - 85.0;
        scene.instances.push(MeshInstance {
            mesh_index: prop_index,
            translation: Vec3::new(x, y, 0.0),
            heading: index as f32 * 0.1,
            ground_normal: Vec3::UP,
            scale: 1.0,
        });
    }
    scene
}

fn route_offset(route: &[Vec2], offset: f64) -> Vec<Vec2> {
    (0..route.len())
        .map(|i| {
            let previous = route[(i + route.len() - 1) % route.len()];
            let current = route[i];
            let next = route[(i + 1) % route.len()];
            let incoming = current - previous;
            let outgoing = next - current;
            let normalize = |v: Vec2| {
                let length = v.length().max(1.0e-6);
                Vec2::new(v.x / length, v.y / length)
            };
            let incoming = normalize(incoming);
            let outgoing = normalize(outgoing);
            let before = Vec2::new(-incoming.y, incoming.x);
            let after = Vec2::new(-outgoing.y, outgoing.x);
            let mut miter = normalize(Vec2::new(before.x + after.x, before.y + after.y));
            if miter.length() < 0.5 {
                miter = after;
            }
            let denominator = (miter.x * after.x + miter.y * after.y).abs().max(0.25);
            current + miter * (offset / denominator)
        })
        .collect()
}

fn box_mesh(mesh: &mut IndexedMesh, center: Vec3, half: Vec3, material: u32) {
    let (x, y, z) = (half.x, half.y, half.z);
    let (a, b, c) = (center.x, center.y, center.z);
    for (p, n) in [
        (
            [
                Vec3::new(a - x, b - y, c - z),
                Vec3::new(a - x, b + y, c - z),
                Vec3::new(a - x, b + y, c + z),
                Vec3::new(a - x, b - y, c + z),
            ],
            Vec3::new(-1.0, 0.0, 0.0),
        ),
        (
            [
                Vec3::new(a + x, b - y, c - z),
                Vec3::new(a + x, b + y, c - z),
                Vec3::new(a + x, b + y, c + z),
                Vec3::new(a + x, b - y, c + z),
            ],
            Vec3::new(1.0, 0.0, 0.0),
        ),
        (
            [
                Vec3::new(a - x, b - y, c - z),
                Vec3::new(a + x, b - y, c - z),
                Vec3::new(a + x, b - y, c + z),
                Vec3::new(a - x, b - y, c + z),
            ],
            Vec3::new(0.0, -1.0, 0.0),
        ),
        (
            [
                Vec3::new(a - x, b + y, c - z),
                Vec3::new(a + x, b + y, c - z),
                Vec3::new(a + x, b + y, c + z),
                Vec3::new(a - x, b + y, c + z),
            ],
            Vec3::new(0.0, 1.0, 0.0),
        ),
        (
            [
                Vec3::new(a - x, b - y, c - z),
                Vec3::new(a + x, b - y, c - z),
                Vec3::new(a + x, b + y, c - z),
                Vec3::new(a - x, b + y, c - z),
            ],
            Vec3::new(0.0, 0.0, -1.0),
        ),
        (
            [
                Vec3::new(a - x, b - y, c + z),
                Vec3::new(a + x, b - y, c + z),
                Vec3::new(a + x, b + y, c + z),
                Vec3::new(a - x, b + y, c + z),
            ],
            Vec3::UP,
        ),
    ] {
        let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let mut v = p.map(|position| MeshVertex {
            position,
            normal: n,
            uv: [0.0, 0.0],
            material,
        });
        for i in 0..4 {
            v[i].uv = uv[i];
        }
        mesh.triangle(v[0], v[1], v[2]);
        mesh.triangle(v[0], v[2], v[3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_test_mesh(x: f32, material: u32) -> IndexedMesh {
        let mut mesh = IndexedMesh::default();
        mesh.triangle(
            MeshVertex {
                position: Vec3::new(x, 0.0, 0.0),
                normal: Vec3::UP,
                uv: [0.0, 0.0],
                material,
            },
            MeshVertex {
                position: Vec3::new(x + 1.0, 0.0, 0.0),
                normal: Vec3::UP,
                uv: [1.0, 0.0],
                material,
            },
            MeshVertex {
                position: Vec3::new(x, 1.0, 0.0),
                normal: Vec3::UP,
                uv: [0.0, 1.0],
                material,
            },
        );
        mesh
    }

    fn profile_test_scene(car_page: [u8; 4]) -> Scene3d {
        Scene3d {
            meshes: vec![profile_test_mesh(0.0, 0), profile_test_mesh(10.0, 0)],
            instances: vec![
                MeshInstance {
                    mesh_index: 0,
                    translation: Vec3::ZERO,
                    heading: 0.0,
                    ground_normal: Vec3::UP,
                    scale: 1.0,
                },
                MeshInstance {
                    mesh_index: 1,
                    translation: Vec3::ZERO,
                    heading: 0.0,
                    ground_normal: Vec3::UP,
                    scale: 1.0,
                },
            ],
            materials: vec![Material::whole_layer(
                "profile-test",
                [1.0; 4],
                0,
                UvWrap::Clamp,
            )],
            textures: RgbaTextureArray {
                width: 1,
                height: 1,
                layers: vec![car_page.to_vec()],
            },
            ..Scene3d::default()
        }
    }

    fn profile_test_source(car_page: [u8; 4]) -> Scene3d {
        Scene3d {
            meshes: vec![profile_test_mesh(-20.0, 0), profile_test_mesh(20.0, 1)],
            instances: vec![
                MeshInstance {
                    mesh_index: 0,
                    translation: Vec3::ZERO,
                    heading: 0.0,
                    ground_normal: Vec3::UP,
                    scale: 1.0,
                },
                MeshInstance {
                    mesh_index: 1,
                    translation: Vec3::ZERO,
                    heading: 0.0,
                    ground_normal: Vec3::UP,
                    scale: 2.0,
                },
            ],
            materials: vec![
                Material::whole_layer("source-track", [1.0; 4], 0, UvWrap::Clamp),
                Material::whole_layer("source-car", [1.0; 4], 1, UvWrap::Clamp),
            ],
            textures: RgbaTextureArray {
                width: 1,
                height: 1,
                layers: vec![[90, 90, 90, 255].to_vec(), car_page.to_vec()],
            },
            ..Scene3d::default()
        }
    }

    #[test]
    fn vehicle_profile_identity_requires_semantic_id_and_lowercase_sha256() {
        assert!(VehicleProfileIdentity::new("vehicle-profile-synth-a", &"a".repeat(64)).is_ok());
        assert!(VehicleProfileIdentity::new("Vehicle-profile-synth-a", &"a".repeat(64)).is_err());
        assert!(VehicleProfileIdentity::new("vehicle-profile-synth-a", &"A".repeat(64)).is_err());
        assert!(VehicleProfileIdentity::new("vehicle-profile-synth-a", "abc").is_err());
    }

    #[test]
    fn vehicle_profile_extracts_only_car_mesh_and_remaps_used_materials_and_textures() {
        let mut target = profile_test_scene([1, 2, 3, 255]);
        let source = profile_test_source([90, 80, 70, 255]);
        let vehicle = append_vehicle_profile_mesh(&mut target, &source, 1).unwrap();
        assert_eq!(
            target.meshes.len(),
            3,
            "the source track mesh is not copied"
        );
        assert_eq!(
            target.instances.len(),
            2,
            "templates are not static instances"
        );
        assert_eq!(vehicle.mesh_index, 2);
        assert_eq!(vehicle.scale, 2.0);
        assert_eq!(target.meshes[2].vertices[0].position.x, 20.0);
        assert!(target.meshes[2].vertices.iter().all(|v| v.material == 1));
        assert_eq!(target.materials.len(), 2);
        assert_eq!(target.materials[1].texture, 1);
        assert_eq!(target.textures.layers.len(), 2);
        assert_eq!(target.textures.layers[1], [90, 80, 70, 255]);
    }

    #[test]
    fn vehicle_profile_material_remap_uses_sorted_source_material_ids() {
        let mut target = profile_test_scene([1, 2, 3, 255]);
        let mut source = profile_test_source([90, 80, 70, 255]);
        source.meshes[1].vertices[0].material = 1;
        source.meshes[1].vertices[1].material = 0;
        source.meshes[1].vertices[2].material = 1;
        append_vehicle_profile_mesh(&mut target, &source, 1).unwrap();
        assert_eq!(target.materials[1].name, "source-track");
        assert_eq!(target.materials[2].name, "source-car");
        assert_eq!(target.meshes[2].vertices[0].material, 2);
        assert_eq!(target.meshes[2].vertices[1].material, 1);
        assert_eq!(target.meshes[2].vertices[2].material, 2);
    }

    #[test]
    fn vehicle_profile_registration_reuses_identical_pages_and_fails_atomically() {
        let page = [9, 8, 7, 255];
        let mut target = profile_test_scene(page);
        let source = profile_test_source(page);
        append_vehicle_profile_mesh(&mut target, &source, 1).unwrap();
        assert_eq!(target.textures.layers.len(), 1);

        let before = target.clone();
        let mut malformed = profile_test_source([1, 1, 1, 255]);
        malformed.meshes[1].vertices[0].material = 7;
        assert!(append_vehicle_profile_mesh(&mut target, &malformed, 1).is_err());
        assert_eq!(
            target, before,
            "a rejected registration leaves the target unchanged"
        );
    }

    #[test]
    fn vehicle_profile_texture_budget_is_preflighted_without_target_mutation() {
        let mut target = profile_test_scene([1, 1, 1, 255]);
        target.textures.layers = (0..MAX_PROFILE_TEXTURE_LAYERS)
            .map(|value| vec![value as u8, 0, 0, 255])
            .collect();
        target.textures.layers[1] = [9, 9, 9, 255].to_vec();
        let before = target.clone();
        let source = profile_test_source([8, 8, 8, 255]);
        assert!(append_vehicle_profile_mesh(&mut target, &source, 1).is_err());
        assert_eq!(target, before);
    }

    #[test]
    fn vehicle_profile_material_and_mesh_budgets_are_preflighted() {
        let source = profile_test_source([8, 8, 8, 255]);
        let mut too_many_materials = profile_test_scene([1, 1, 1, 255]);
        too_many_materials.materials.resize(
            MAX_PROFILE_MATERIALS,
            Material::whole_layer("bounded", [1.0; 4], 0, UvWrap::Clamp),
        );
        let materials_before = too_many_materials.clone();
        assert!(append_vehicle_profile_mesh(&mut too_many_materials, &source, 1).is_err());
        assert_eq!(too_many_materials, materials_before);

        let mut too_many_meshes = profile_test_scene([1, 1, 1, 255]);
        too_many_meshes
            .meshes
            .resize(MAX_PROFILE_MESHES, IndexedMesh::default());
        let meshes_before = too_many_meshes.clone();
        assert!(append_vehicle_profile_mesh(&mut too_many_meshes, &source, 1).is_err());
        assert_eq!(too_many_meshes, meshes_before);
    }

    fn background_validation_scene() -> Scene3d {
        let mut mesh = IndexedMesh::default();
        mesh.quad(
            [
                Vec3::new(-4.0, 8.0, 6.0),
                Vec3::new(4.0, 8.0, 6.0),
                Vec3::new(4.0, 8.0, 10.0),
                Vec3::new(-4.0, 8.0, 10.0),
            ],
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            0,
        );
        Scene3d {
            meshes: vec![mesh],
            background_mesh_indices: vec![0],
            materials: vec![Material::whole_layer("test", [1.0; 4], 0, UvWrap::Clamp)],
            ..Scene3d::default()
        }
    }

    #[test]
    fn background_mesh_validation_is_bounded_and_disjoint() {
        assert_eq!(
            validate_background_meshes(&Scene3d::default(), &[]),
            Ok(0.0)
        );
        let valid = background_validation_scene();
        assert!(validate_background_meshes(&valid, &[]).unwrap() > 10.0);

        let mut malformed = valid.clone();
        malformed.background_mesh_indices.push(0);
        assert!(validate_background_meshes(&malformed, &[]).is_err());

        let mut malformed = valid.clone();
        malformed.background_mesh_indices[0] = 1;
        assert!(validate_background_meshes(&malformed, &[]).is_err());

        let valid = background_validation_scene();
        assert!(validate_background_meshes(&valid, &[0]).is_err());

        let mut malformed = background_validation_scene();
        malformed.instances.push(MeshInstance {
            mesh_index: 0,
            translation: Vec3::ZERO,
            heading: 0.0,
            ground_normal: Vec3::UP,
            scale: 1.0,
        });
        assert!(validate_background_meshes(&malformed, &[]).is_err());
    }

    #[test]
    fn background_mesh_validation_rejects_excess_geometry_and_bad_coordinates() {
        let mut too_many_vertices = background_validation_scene();
        let mesh = &mut too_many_vertices.meshes[0];
        let vertex = mesh.vertices[0];
        mesh.vertices.resize(65_537, vertex);
        assert!(validate_background_meshes(&too_many_vertices, &[]).is_err());

        let mut too_many_indices = background_validation_scene();
        let mesh = &mut too_many_indices.meshes[0];
        mesh.indices.resize(196_611, 0);
        assert!(validate_background_meshes(&too_many_indices, &[]).is_err());

        let mut near_origin = background_validation_scene();
        for vertex in &mut near_origin.meshes[0].vertices {
            vertex.position = Vec3::ZERO;
        }
        assert!(validate_background_meshes(&near_origin, &[]).is_err());

        let mut nonfinite = background_validation_scene();
        nonfinite.meshes[0].vertices[0].position.x = f32::NAN;
        assert!(validate_background_meshes(&nonfinite, &[]).is_err());

        let mut malformed_material = background_validation_scene();
        malformed_material.meshes[0].vertices[0].material = 1;
        assert!(validate_background_meshes(&malformed_material, &[]).is_err());
    }

    #[test]
    fn dynamic_vehicles_reuse_template_without_touching_static_instances() {
        let template = MeshInstance {
            mesh_index: 7,
            translation: Vec3::new(1.0, 2.0, 3.0),
            heading: 0.4,
            ground_normal: Vec3::UP,
            scale: 2.0,
        };
        let static_instances = [MeshInstance {
            mesh_index: 2,
            translation: Vec3::new(40.0, -3.0, 1.5),
            heading: 0.0,
            ground_normal: Vec3::UP,
            scale: 1.0,
        }];
        let initial_static = static_instances;
        let instances = compose_render_instances(
            &static_instances,
            template,
            &[
                (Vec3::new(-4.0, 8.0, 0.5), 0.25),
                (Vec3::new(12.0, -6.0, 2.0), -1.0),
            ],
            None,
        )
        .unwrap();
        assert_eq!(instances.len(), 3);
        assert_eq!(instances[0], initial_static[0]);
        let vehicles = &instances[1..];
        assert_eq!(vehicles.len(), 2);
        assert!(vehicles.iter().all(|vehicle| vehicle.mesh_index == 7));
        assert!(vehicles.iter().all(|vehicle| vehicle.scale == 2.0));
        assert_eq!(vehicles[0].translation, Vec3::new(-4.0, 8.0, 0.5));
        assert_eq!(vehicles[0].heading, 0.25);
        assert_eq!(vehicles[1].translation, Vec3::new(12.0, -6.0, 2.0));
        assert_eq!(vehicles[1].heading, -1.0);
        assert_eq!(static_instances, initial_static);
        assert_eq!(vehicles.len() + static_instances.len(), instances.len());
    }

    #[test]
    fn dynamic_vehicle_instances_enforce_protocol_count_and_finite_poses() {
        let template = MeshInstance {
            mesh_index: 0,
            translation: Vec3::ZERO,
            heading: 0.0,
            ground_normal: Vec3::UP,
            scale: 1.0,
        };
        let eight = vec![(Vec3::ZERO, 0.0); MAX_RENDER_VEHICLES];
        assert_eq!(
            vehicle_instances_for_poses(template, &eight).unwrap().len(),
            8
        );
        let nine = vec![(Vec3::ZERO, 0.0); MAX_RENDER_VEHICLES + 1];
        assert_eq!(
            vehicle_instances_for_poses(template, &nine).unwrap_err(),
            "render vehicle count exceeds the protocol limit"
        );
        assert_eq!(
            vehicle_instances_for_poses(template, &[(Vec3::new(f32::NAN, 0.0, 0.0), 0.0)])
                .unwrap_err(),
            "render vehicle pose contains a non-finite value"
        );
    }

    #[test]
    fn profiled_vehicle_instances_keep_per_car_mesh_pose_and_ground_normal() {
        let templates = [
            MeshInstance {
                mesh_index: 4,
                translation: Vec3::ZERO,
                heading: 0.0,
                ground_normal: Vec3::UP,
                scale: 1.0,
            },
            MeshInstance {
                mesh_index: 9,
                translation: Vec3::ZERO,
                heading: 0.0,
                ground_normal: Vec3::UP,
                scale: 1.5,
            },
        ];
        let poses = [
            (Vec3::new(2.0, 3.0, 4.0), 0.25, Vec3::UP),
            (Vec3::new(-5.0, 8.0, 2.0), -0.75, Vec3::new(0.0, 0.6, 0.8)),
        ];
        let all = compose_profiled_render_instances(&[], &templates, &poses, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].mesh_index, 4);
        assert_eq!(all[1].mesh_index, 9);
        assert_eq!(all[0].translation, poses[0].0);
        assert_eq!(all[1].translation, poses[1].0);
        assert_eq!(all[1].heading, poses[1].1);
        assert!((all[1].ground_normal.length() - 1.0).abs() < 1.0e-6);
        assert_eq!(all[1].ground_normal, poses[1].2);

        let mirror = compose_profiled_render_instances(&[], &templates, &poses, Some(0)).unwrap();
        assert_eq!(mirror.len(), 1);
        assert_eq!(
            mirror[0].mesh_index, 9,
            "mirror omits self and keeps the guest model"
        );
        assert_eq!(mirror[0].translation, poses[1].0);
        assert_eq!(mirror[0].ground_normal, poses[1].2);
        assert!(compose_profiled_render_instances(&[], &templates[..1], &poses, None).is_err());
    }

    #[test]
    fn vehicle_surface_state_roster_resets_when_membership_or_order_changes() {
        let stable = r#"["driver-a","driver-b"]"#;
        assert!(!vehicle_roster_changed(Some(stable), stable));
        assert!(vehicle_roster_changed(
            Some(stable),
            r#"["driver-b","driver-a"]"#
        ));
        assert!(vehicle_roster_changed(
            Some(stable),
            r#"["driver-a","driver-c"]"#
        ));
        assert!(vehicle_roster_changed(None, "[]"));
    }

    #[test]
    fn vehicle_camera_tracks_only_local_index_and_has_no_local_fallback() {
        let remote = (Vec3::new(8.0, 9.0, 1.0), 1.2);
        let local = (Vec3::new(-3.0, 4.0, 0.5), -0.2);
        let fallback = (Vec3::new(100.0, 100.0, 0.0), 0.0);
        let poses = [remote, local];
        assert_eq!(vehicle_camera_pose(&poses, Some(1), fallback), Ok(local));
        assert_eq!(vehicle_camera_pose(&poses, None, fallback), Ok(fallback));
        assert_eq!(
            vehicle_camera_pose(&poses, Some(2), fallback),
            Err("local render vehicle index is outside the pose list")
        );
    }

    #[test]
    fn mirror_visibility_gates_pending_mirror_owner_and_can_be_reenabled() {
        assert_eq!(active_mirror_local_index(false, Some(1)), None);
        assert_eq!(active_mirror_local_index(true, Some(1)), Some(1));
        assert_eq!(active_mirror_local_index(true, None), None);
    }

    fn surface_quad(z: f32) -> IndexedMesh {
        let mut mesh = IndexedMesh::default();
        mesh.quad(
            [
                Vec3::new(0.0, 0.0, z),
                Vec3::new(10.0, 0.0, z + 1.0),
                Vec3::new(10.0, 10.0, z + 1.0),
                Vec3::new(0.0, 10.0, z),
            ],
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            0,
        );
        mesh
    }

    fn surface_scene(meshes: Vec<IndexedMesh>) -> Scene3d {
        let instances = (0..meshes.len())
            .map(|mesh_index| MeshInstance {
                mesh_index: mesh_index as u32,
                translation: Vec3::ZERO,
                heading: 0.0,
                ground_normal: Vec3::UP,
                scale: 1.0,
            })
            .collect();
        Scene3d {
            meshes,
            instances,
            ..Scene3d::default()
        }
    }

    #[test]
    fn track_surface_sampler_interpolates_slope_and_misses_outside() {
        let scene = surface_scene(vec![surface_quad(0.0)]);
        let surface = TrackSurface::build(&scene, &[0]).unwrap();
        let TrackSurfaceLookup::Hit(sample) = surface.sample(5.0, 5.0, 0.5) else {
            panic!("sloped road sample should hit exactly one surface");
        };
        assert!((sample.height - 0.5).abs() < 1.0e-5);
        assert_eq!(sample.distinct_levels, 1);
        assert_eq!(surface.sample(-0.1, 5.0, 0.0), TrackSurfaceLookup::Miss);
    }

    #[test]
    fn track_surface_sampler_uses_declared_meshes_and_continuity_for_overlaps() {
        let scene = surface_scene(vec![surface_quad(0.0), surface_quad(100.0)]);
        let surface = TrackSurface::build(&scene, &[0]).unwrap();
        assert!(matches!(
            surface.sample(5.0, 5.0, 100.5),
            TrackSurfaceLookup::Hit(TrackSurfaceSample {
                height,
                distinct_levels: 1
            }) if (height - 0.5).abs() < 1.0e-5
        ));

        let overlapping = TrackSurface::build(&scene, &[0, 1]).unwrap();
        assert!(matches!(
            overlapping.sample(5.0, 5.0, 100.5),
            TrackSurfaceLookup::Hit(TrackSurfaceSample {
                height,
                distinct_levels: 2
            }) if (height - 100.5).abs() < 1.0e-5
        ));
        assert_eq!(
            overlapping.sample(5.0, 5.0, 50.5),
            TrackSurfaceLookup::Ambiguous
        );
    }

    #[test]
    fn track_surface_miss_preserves_last_height_and_quality_metadata() {
        let mut state = TrackSurfaceState::default();
        let position_z = state.apply_lookup(
            TrackSurfaceLookup::Hit(TrackSurfaceSample {
                height: 3.5,
                distinct_levels: 1,
            }),
            20.0,
            0.25,
        );
        assert!((position_z - 3.25).abs() < 1.0e-6);
        assert_eq!(
            state.outcome,
            TrackSurfaceOutcome::Sampled { distinct_levels: 1 }
        );
        assert_eq!(state.previous_ground_height(20.0, 0.25), 3.5);
        let on_miss = state.apply_lookup(TrackSurfaceLookup::Miss, 20.0, 0.25);
        assert!((on_miss - position_z).abs() < 1.0e-6);
        assert_eq!(state.outcome, TrackSurfaceOutcome::Miss);
        assert_eq!(state.last_valid.unwrap().height, 3.5);
        let on_ambiguous = state.apply_lookup(TrackSurfaceLookup::Ambiguous, 20.0, 0.25);
        assert!((on_ambiguous - position_z).abs() < 1.0e-6);
        assert_eq!(state.outcome, TrackSurfaceOutcome::Ambiguous);
        assert_eq!(state.last_valid.unwrap().distinct_levels, 1);

        let mut uninitialized = TrackSurfaceState::default();
        assert_eq!(
            uninitialized.apply_lookup(TrackSurfaceLookup::Miss, 7.0, 0.25),
            7.0
        );
        assert_eq!(uninitialized.outcome, TrackSurfaceOutcome::Miss);
        assert_eq!(uninitialized.previous_ground_height(7.0, 0.25), 7.25);
    }

    #[test]
    fn track_surface_precompute_enforces_triangle_and_cell_work_budgets() {
        let mut oversized_mesh = surface_quad(0.0);
        oversized_mesh.indices = [0_u32, 1, 2]
            .into_iter()
            .cycle()
            .take((MAX_TRACK_SURFACE_TRIANGLES + 1) * 3)
            .collect();
        let oversized = surface_scene(vec![oversized_mesh]);
        assert_eq!(
            TrackSurface::build(&oversized, &[0]).unwrap_err(),
            "track surface triangle budget exceeded"
        );

        let mut crowded_mesh = IndexedMesh::default();
        let vertex = |x, y| MeshVertex {
            position: Vec3::new(x, y, 0.0),
            normal: Vec3::UP,
            uv: [0.0, 0.0],
            material: 0,
        };
        crowded_mesh.triangle(vertex(0.0, 0.0), vertex(64.0, 0.0), vertex(0.0, 64.0));
        for _ in 0..(MAX_TRACK_SURFACE_CELL_TRIANGLES + 1) {
            crowded_mesh.triangle(vertex(31.8, 31.8), vertex(32.0, 31.8), vertex(31.8, 32.0));
        }
        let crowded = surface_scene(vec![crowded_mesh]);
        assert_eq!(
            TrackSurface::build(&crowded, &[0]).unwrap_err(),
            "track surface cell candidate budget exceeded"
        );
    }

    #[test]
    fn square_array_storage_avoids_gl_cube_target_heuristic() {
        for source_layers in [6_u32, 12, 132, 246, 252] {
            assert_eq!(
                texture_array_storage_layers(256, 256, source_layers as usize, 2048),
                Some(source_layers + 1)
            );
        }
        for source_layers in [129_u32, 245, 247, 256] {
            assert_eq!(
                texture_array_storage_layers(256, 256, source_layers as usize, 2048),
                Some(source_layers)
            );
        }
        assert_eq!(texture_array_storage_layers(256, 128, 246, 2048), Some(246));
        assert_eq!(texture_array_storage_layers(16, 16, 1, 2048), Some(2));
        assert_eq!(texture_array_storage_layers(256, 256, 0, 2048), None);
        assert_eq!(texture_array_storage_layers(256, 256, 246, 246), None);
        assert!(texture_array_storage_layers(256, 256, u32::MAX as usize + 1, u32::MAX).is_none());
    }

    #[test]
    fn synthetic_fixture_is_indexed_and_has_real_depth() {
        let scene = synthetic_scene(&TrackRuntime::synthetic_circuit(), Vec2::new(0.0, 0.0), 0.2);
        assert!(scene.meshes.iter().all(IndexedMesh::is_well_formed));
        let positions = scene
            .meshes
            .iter()
            .flat_map(|m| m.vertices.iter().map(|v| v.position.z));
        let (min, max) = positions.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), z| {
            (lo.min(z), hi.max(z))
        });
        assert!(max - min > 1.0, "fixture has no useful vertical geometry");
        assert!(
            scene
                .meshes
                .iter()
                .flat_map(|m| &m.vertices)
                .any(|v| v.uv[0] > 1.0)
        );
    }
    #[test]
    fn perspective_projection_changes_scale_with_depth() {
        let camera = Camera3d {
            position: Vec3::ZERO,
            target: Vec3::new(1.0, 0.0, 0.0),
            vertical_fov_radians: 1.0,
            near: 0.1,
            far: 100.0,
        };
        let near = camera.project(Vec3::new(4.0, 1.0, 0.0), 1.0);
        let far = camera.project(Vec3::new(8.0, 1.0, 0.0), 1.0);
        assert!((near[0] / near[3]).abs() > (far[0] / far[3]).abs());
        assert!(near[2] / near[3] < far[2] / far[3]);
        let camera = Camera3d::chase(Vec2::new(-20.0, 0.0), 0.0);
        let ground = camera.project(Vec3::new(-20.0, 0.0, -0.15), 1.0);
        let road = camera.project(Vec3::new(-20.0, 0.0, 0.02), 1.0);
        assert!(road[2] / road[3] < ground[2] / ground[3]);
    }

    #[test]
    fn rear_mirror_camera_faces_behind_and_honors_supported_up_axis() {
        let level = Camera3d::rear_mirror_world(Vec3::ZERO, 0.0, Vec3::UP);
        let direction = (level.target - level.position).normalized();
        assert!(direction.x < -0.99);
        assert!(level.far >= 32.0);
        let point = Vec3::new(-12.0, 2.0, 1.5);
        let level_projection = level.project_with_up(point, 1.8, Vec3::UP);
        let rolled_up = Vec3::new(0.0, 0.5, 0.8660254);
        let rolled_projection = level.project_with_up(point, 1.8, rolled_up);
        assert!(level_projection.iter().all(|value| value.is_finite()));
        assert!(rolled_projection.iter().all(|value| value.is_finite()));
        assert!(
            (level_projection[1] / level_projection[3]
                - rolled_projection[1] / rolled_projection[3])
                .abs()
                > 0.05
        );
    }

    #[test]
    fn preview_camera_modes_are_distinct_finite_and_keep_chase_as_default() {
        let position = Vec3::new(3.0, -4.0, 2.0);
        let heading = 0.7;
        assert_eq!(PreviewCameraMode::default(), PreviewCameraMode::Chase);
        assert_eq!(
            PreviewCameraMode::from_name("chase"),
            Some(PreviewCameraMode::Chase)
        );
        assert_eq!(
            PreviewCameraMode::from_name("wide"),
            Some(PreviewCameraMode::Wide)
        );
        assert_eq!(
            PreviewCameraMode::from_name("close"),
            Some(PreviewCameraMode::Close)
        );
        assert_eq!(
            PreviewCameraMode::from_name("road"),
            Some(PreviewCameraMode::Road)
        );
        assert_eq!(
            PreviewCameraMode::from_name("hood"),
            Some(PreviewCameraMode::Road)
        );
        assert_eq!(PreviewCameraMode::from_name("unknown"), None);

        let chase = Camera3d::preview_world(position, heading, PreviewCameraMode::Chase);
        assert_eq!(chase, Camera3d::chase_world(position, heading));
        let wide = Camera3d::preview_world(position, heading, PreviewCameraMode::Wide);
        let close = Camera3d::preview_world(position, heading, PreviewCameraMode::Close);
        let road = Camera3d::preview_world(position, heading, PreviewCameraMode::Road);
        assert_ne!(wide, chase);
        assert_ne!(close, chase);
        assert_ne!(road, chase);
        let forward = Vec3::new(heading.cos(), heading.sin(), 0.0);
        assert!((road.position - position).dot(forward) > 1.0);
        assert!((wide.position - position).dot(forward) < -15.0);

        let look_behind = Camera3d::look_behind_world(position, heading);
        assert!((look_behind.position - position).dot(forward) < 0.0);
        assert!((look_behind.target - look_behind.position).dot(forward) < 0.0);

        for camera in [chase, wide, close, road, look_behind] {
            assert!(camera.position.x.is_finite());
            assert!(camera.position.y.is_finite());
            assert!(camera.position.z.is_finite());
            assert!(camera.target.x.is_finite());
            assert!(camera.target.y.is_finite());
            assert!(camera.target.z.is_finite());
            let projected = camera.project(position + forward * 12.0, 16.0 / 9.0);
            assert!(projected.iter().all(|component| component.is_finite()));
        }
    }

    #[test]
    fn camera_keeps_behind_eye_and_near_plane_points_for_hardware_clipping() {
        let camera = Camera3d::chase(Vec2::new(-20.0, 0.0), 0.0);
        let forward = (camera.target - camera.position).normalized();
        let behind_eye = camera.project(camera.position - forward, 1.0);
        assert!(
            behind_eye[3] < 0.0,
            "behind-eye clip w must remain negative"
        );
        let before_near = camera.project(camera.position + forward * 0.05, 1.0);
        assert!(before_near[3] > 0.0 && before_near[3] < camera.near);
        assert!(before_near[2] < 0.0, "pre-near-plane depth must be clipped");
    }

    #[test]
    fn cached_frame_projection_matches_per_point_projection() {
        let camera = Camera3d::rear_mirror_world(
            Vec3::new(13.5, -7.25, 2.0),
            0.73,
            Vec3::new(-0.18, 0.27, 0.946).normalized(),
        );
        let up = Vec3::new(-0.18, 0.27, 0.946).normalized();
        let aspect = 16.0 / 9.0;
        let cached = CameraFrameProjection::new(camera, aspect, up);
        let forward = (camera.target - camera.position).normalized();
        for point in [
            Vec3::ZERO,
            camera.position,
            camera.position - forward,
            camera.position + forward * (camera.near * 0.5),
            camera.position + forward * camera.near,
            camera.position + forward * (camera.near + 0.001),
            camera.position + forward * camera.far,
            camera.position + forward * (camera.far + 1.0),
            Vec3::new(-104.5, 76.25, -11.0),
        ] {
            assert_eq!(
                cached.project(point),
                camera.project_with_up(point, aspect, up),
                "cached camera basis changed projection for {point:?}"
            );
        }

        let parallel_up = (camera.target - camera.position).normalized();
        let cached_fallback = CameraFrameProjection::new(camera, aspect, parallel_up);
        for point in [Vec3::ZERO, camera.position + Vec3::UP * 2.0] {
            assert_eq!(
                cached_fallback.project(point),
                camera.project_with_up(point, aspect, parallel_up),
                "cached parallel-up fallback changed projection for {point:?}"
            );
        }
    }

    #[test]
    fn cached_instance_basis_matches_per_call_world_transform() {
        let instance = MeshInstance {
            mesh_index: 0,
            translation: Vec3::new(-21.0, 5.5, 3.25),
            heading: -1.17,
            ground_normal: Vec3::new(0.21, -0.31, 0.927).normalized(),
            scale: 1.35,
        };
        let cached = instance.transform_for_frame();
        for point in [
            Vec3::ZERO,
            Vec3::new(1.0, -2.0, 0.75),
            Vec3::new(-14.0, 38.0, 5.25),
        ] {
            assert_eq!(
                cached.transform_point(point),
                instance.transform_point(point)
            );
            assert_eq!(
                cached.transform_normal(point),
                instance.transform_normal(point)
            );
        }
    }

    #[test]
    fn synthetic_scene_uses_distinct_rgba_material_texture_layers() {
        let scene = synthetic_scene(&TrackRuntime::synthetic_circuit(), Vec2::new(0.0, 0.0), 0.0);
        assert!(scene.textures.is_well_formed());
        assert!(scene.textures.layers.len() >= 2);
        assert_ne!(scene.textures.layers[0], scene.textures.layers[1]);
        assert!(scene.materials.iter().any(|material| material.texture == 0));
        assert!(scene.materials.iter().any(|material| material.texture == 1));
    }

    #[test]
    fn local_vehicle_mesh_instance_applies_heading_and_world_pose() {
        let instance = MeshInstance {
            mesh_index: 0,
            translation: Vec3::new(10.0, 20.0, 2.0),
            heading: std::f32::consts::FRAC_PI_2,
            ground_normal: Vec3::UP,
            scale: 2.0,
        };
        let front = instance.transform_point(Vec3::new(1.0, 0.0, 0.5));
        assert!((front.x - 10.0).abs() < 1.0e-5);
        assert!((front.y - 22.0).abs() < 1.0e-5);
        assert!((front.z - 3.0).abs() < 1.0e-5);
        let normal = instance.transform_normal(Vec3::UP);
        assert!((normal - Vec3::UP).length() < 1.0e-5);
    }

    #[test]
    fn tilted_vehicle_basis_is_orthonormal_and_preserves_right_handed_normals() {
        let up = Vec3::new(0.3, -0.4, 0.8660254).normalized();
        let instance = MeshInstance {
            mesh_index: 0,
            translation: Vec3::ZERO,
            heading: 1.1,
            ground_normal: up,
            scale: 1.0,
        };
        let forward = instance.transform_normal(Vec3::new(1.0, 0.0, 0.0));
        let left = instance.transform_normal(Vec3::new(0.0, 1.0, 0.0));
        let transformed_up = instance.transform_normal(Vec3::UP);
        assert!(forward.dot(left).abs() < 1.0e-5);
        assert!(forward.dot(transformed_up).abs() < 1.0e-5);
        assert!(left.dot(transformed_up).abs() < 1.0e-5);
        assert!((forward.cross(left) - transformed_up).length() < 1.0e-5);
        assert!((transformed_up - up).length() < 1.0e-5);
    }

    #[test]
    fn tilted_local_base_corners_contact_the_sampled_plane() {
        let up = Vec3::new(-0.5, -0.25, 1.0).normalized();
        let height = 3.5;
        let min_z = 0.8;
        let instance = MeshInstance {
            mesh_index: 0,
            translation: Vec3::new(0.0, 0.0, height - min_z / up.z),
            heading: 0.4,
            ground_normal: up,
            scale: 1.0,
        };
        for x in [-1.2, 1.2] {
            for y in [-0.7, 0.7] {
                let point = instance.transform_point(Vec3::new(x, y, min_z));
                assert!((up.dot(point - Vec3::new(0.0, 0.0, height))).abs() < 2.0e-5);
            }
        }
    }
}

/// Browser WGPU backend. WebGPU and WebGL2 both use the same depth-tested
/// indexed scene, texture sampling, materials, and perspective camera.
#[cfg(target_arch = "wasm32")]
pub mod wgpu_backend {
    use super::*;
    use bytemuck::{Pod, Zeroable};
    use js_sys::{Function, Promise, Reflect};
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    use wgpu::SurfaceError;
    use wgpu::util::DeviceExt;

    const MAX_MIRROR_EXPANDED_VERTICES: usize = 1_000_000;
    const MAX_MIRROR_EXPANDED_INDICES: usize = 3_000_000;
    // Conservative retained-cache budget across CPU triangle metadata and GPU
    // vertex/index buffers. It is checked before expanding scene instances.
    const MAX_MIRROR_CACHE_BYTES: u64 = 128 * 1024 * 1024;

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    struct GpuVertex {
        clip: [f32; 4],
        normal: [f32; 3],
        uv: [f32; 2],
        tint: [f32; 4],
        texture_layer: u32,
        uv_scale: [f32; 2],
        uv_offset: [f32; 2],
        uv_wrap: [u32; 2],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    struct MirrorWorldVertex {
        world_position: [f32; 4],
        normal: [f32; 3],
        uv: [f32; 2],
        tint: [f32; 4],
        texture_layer: u32,
        uv_scale: [f32; 2],
        uv_offset: [f32; 2],
        uv_wrap: [u32; 2],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    struct MirrorCameraUniform {
        position: [f32; 4],
        forward: [f32; 4],
        right: [f32; 4],
        up: [f32; 4],
        projection: [f32; 4],
        /// Fog colour (rgb) and largest blend; zero means no fog and no grading.
        fog: [f32; 4],
        /// Fog start and end distance.
        fog_range: [f32; 4],
    }

    #[derive(Clone, Copy)]
    struct MirrorTriangle {
        centroid: Vec3,
        source_order: u32,
        indices: [u32; 3],
    }

    #[derive(Clone, Copy)]
    struct MirrorTriangleSortKey {
        depth: f32,
        source_order: u32,
        dynamic: bool,
        triangle_index: usize,
    }

    struct MirrorGeometryCache {
        scene_vertex_buffer: wgpu::Buffer,
        background_vertex_buffer: wgpu::Buffer,
        static_opaque_index_buffer: wgpu::Buffer,
        dynamic_opaque_index_buffer: wgpu::Buffer,
        transparent_index_buffer: wgpu::Buffer,
        background_index_buffer: wgpu::Buffer,
        scene_vertex_count: u32,
        background_index_count: u32,
        static_opaque_index_count: u32,
        background_extent: f32,
        dynamic_vertex_capacity: usize,
        dynamic_opaque_index_capacity: usize,
        transparent_index_capacity: usize,
        transparent_triangle_capacity: usize,
        static_transparent_triangles: Vec<MirrorTriangle>,
        static_triangle_count: u32,
        /// Static vertices whose material is animated (main view only).
        animated_vertices: Vec<AnimatedVertex>,
        /// Last frame written to the GPU for each animated material.
        animated_materials: Vec<AnimatedMaterialState>,
    }

    #[derive(Clone, Copy)]
    struct AnimatedVertex {
        index: u32,
        slot: u32,
        vertex: MirrorWorldVertex,
    }

    #[derive(Clone, Copy)]
    struct AnimatedMaterialState {
        material: u32,
        applied: Option<MaterialFrame>,
    }

    #[derive(Default)]
    struct MirrorFrameScratch {
        world_positions: Vec<Vec3>,
        dynamic_vertices: Vec<MirrorWorldVertex>,
        dynamic_opaque_indices: Vec<u32>,
        dynamic_transparent_triangles: Vec<MirrorTriangle>,
        sort_keys: Vec<MirrorTriangleSortKey>,
        sorted_transparent_indices: Vec<u32>,
    }

    impl MirrorFrameScratch {
        fn for_geometry(geometry: &MirrorGeometryCache) -> Self {
            Self {
                world_positions: Vec::with_capacity(geometry.dynamic_vertex_capacity),
                dynamic_vertices: Vec::with_capacity(geometry.dynamic_vertex_capacity),
                dynamic_opaque_indices: Vec::with_capacity(geometry.dynamic_opaque_index_capacity),
                dynamic_transparent_triangles: Vec::with_capacity(
                    geometry
                        .transparent_triangle_capacity
                        .saturating_sub(geometry.static_transparent_triangles.len()),
                ),
                sort_keys: Vec::with_capacity(geometry.transparent_triangle_capacity),
                sorted_transparent_indices: Vec::with_capacity(geometry.transparent_index_capacity),
            }
        }
    }

    struct MirrorCpuGeometry {
        static_vertices: Vec<MirrorWorldVertex>,
        background_vertices: Vec<MirrorWorldVertex>,
        static_opaque_indices: Vec<u32>,
        background_indices: Vec<u32>,
        static_transparent_triangles: Vec<MirrorTriangle>,
        background_extent: f32,
        static_triangle_count: u32,
        dynamic_vertex_capacity: usize,
        dynamic_opaque_index_capacity: usize,
        transparent_triangle_capacity: usize,
        animated_vertices: Vec<AnimatedVertex>,
        animated_materials: Vec<AnimatedMaterialState>,
    }

    fn checked_add(total: usize, value: usize, label: &str) -> Result<usize, String> {
        total
            .checked_add(value)
            .ok_or_else(|| format!("rear-view {label} count overflow"))
    }

    fn checked_mul(left: usize, right: usize, label: &str) -> Result<usize, String> {
        left.checked_mul(right)
            .ok_or_else(|| format!("rear-view {label} size overflow"))
    }

    fn scene_material(scene: &Scene3d, index: u32) -> Result<&Material, String> {
        let material = scene
            .materials
            .get(index as usize)
            .ok_or_else(|| "rear-view mesh references a missing material".to_string())?;
        if material.texture >= scene.textures.layers.len()
            || !material.frames_reference_layers(scene.textures.layers.len())
            || !material.atlas_uv_is_well_formed()
            || material.tint.iter().any(|component| !component.is_finite())
        {
            return Err("rear-view material references invalid texture data".into());
        }
        Ok(material)
    }

    fn triangle_is_transparent(
        scene: &Scene3d,
        mesh: &IndexedMesh,
        tri: [u32; 3],
    ) -> Result<bool, String> {
        let mut transparent = 0;
        for index in tri {
            let vertex = mesh
                .vertices
                .get(index as usize)
                .ok_or_else(|| "rear-view triangle references a missing vertex".to_string())?;
            if scene_material(scene, vertex.material)?.alpha_mode
                == MaterialAlphaMode::StraightAlpha
            {
                transparent += 1;
            }
        }
        if transparent != 0 && transparent != 3 {
            return Err("a triangle cannot mix opaque and straight-alpha materials".into());
        }
        Ok(transparent == 3)
    }

    fn mirror_world_vertex(
        scene: &Scene3d,
        source: &MeshVertex,
        world_position: Vec3,
        world_normal: Vec3,
        tick: Option<u64>,
    ) -> Result<MirrorWorldVertex, String> {
        if !world_position.x.is_finite()
            || !world_position.y.is_finite()
            || !world_position.z.is_finite()
            || !world_normal.x.is_finite()
            || !world_normal.y.is_finite()
            || !world_normal.z.is_finite()
        {
            return Err("rear-view world transform produced non-finite geometry".into());
        }
        let material = scene_material(scene, source.material)?;
        let frame = tick.map_or(
            MaterialFrame {
                texture: material.texture,
                uv_offset: material.uv_offset,
                uv_scale: material.uv_scale,
            },
            |tick| material.frame_at(tick),
        );
        Ok(MirrorWorldVertex {
            world_position: [world_position.x, world_position.y, world_position.z, 1.0],
            normal: [world_normal.x, world_normal.y, world_normal.z],
            uv: source.uv,
            tint: material.tint,
            texture_layer: frame.texture as u32,
            uv_scale: frame.uv_scale,
            uv_offset: frame.uv_offset,
            uv_wrap: [
                match material.wrap_u {
                    UvWrap::Clamp => 0,
                    UvWrap::Repeat => 1,
                },
                match material.wrap_v {
                    UvWrap::Clamp => 0,
                    UvWrap::Repeat => 1,
                },
            ],
        })
    }

    fn mirror_triangle_centroid(points: [Vec3; 3]) -> Result<Vec3, String> {
        let third = 1.0 / 3.0;
        let centroid = points[0] * third + points[1] * third + points[2] * third;
        if !centroid.x.is_finite() || !centroid.y.is_finite() || !centroid.z.is_finite() {
            return Err("rear-view triangle centroid is non-finite".into());
        }
        Ok(centroid)
    }

    fn mirror_instance_counts(
        scene: &Scene3d,
        instances: &[MeshInstance],
    ) -> Result<(usize, usize, usize, u32), String> {
        let mut vertices = 0usize;
        let mut indices = 0usize;
        let mut transparent_triangles = 0usize;
        let mut triangle_count = 0u32;
        for instance in instances {
            let mesh = scene
                .meshes
                .get(instance.mesh_index as usize)
                .ok_or_else(|| "rear-view instance references a missing mesh".to_string())?;
            if !mesh.is_well_formed()
                || !instance.translation.x.is_finite()
                || !instance.translation.y.is_finite()
                || !instance.translation.z.is_finite()
                || !instance.heading.is_finite()
                || !instance.scale.is_finite()
                || instance.scale <= 0.0
            {
                return Err("rear-view instance geometry is malformed".into());
            }
            vertices = checked_add(vertices, mesh.vertices.len(), "vertex")?;
            indices = checked_add(indices, mesh.indices.len(), "index")?;
            let mesh_triangle_count = u32::try_from(mesh.indices.len() / 3)
                .map_err(|_| "rear-view triangle count overflow".to_string())?;
            triangle_count = triangle_count
                .checked_add(mesh_triangle_count)
                .ok_or_else(|| "rear-view triangle count overflow".to_string())?;
            for triangle in mesh.indices.chunks_exact(3) {
                let triangle = [triangle[0], triangle[1], triangle[2]];
                if triangle_is_transparent(scene, mesh, triangle)? {
                    transparent_triangles =
                        checked_add(transparent_triangles, 1, "transparent triangle")?;
                }
            }
        }
        Ok((vertices, indices, transparent_triangles, triangle_count))
    }

    fn build_mirror_cpu_geometry(
        scene: &Scene3d,
        static_instances: &[MeshInstance],
        vehicle_templates: &[MeshInstance],
        device_max_buffer_size: u64,
        dynamic_slots: usize,
        animate: bool,
    ) -> Result<MirrorCpuGeometry, String> {
        if scene.materials.is_empty() || !scene.textures.is_well_formed() {
            return Err("rear-view scene has no materials or malformed textures".into());
        }
        validate_background_meshes(scene, &[])?;
        for mesh in &scene.meshes {
            if !mesh.is_well_formed() {
                return Err("rear-view scene mesh is malformed".into());
            }
            for vertex in &mesh.vertices {
                scene_material(scene, vertex.material)?;
            }
        }
        let (static_vertex_count, static_index_count, static_alpha_count, static_triangles) =
            mirror_instance_counts(scene, static_instances)?;
        if vehicle_templates.is_empty() {
            return Err("rear-view cache requires at least one vehicle profile".into());
        }
        let mut vehicle_vertex_count = 0usize;
        let mut vehicle_index_count = 0usize;
        let mut vehicle_alpha_count = 0usize;
        for template in vehicle_templates {
            let (vertices, indices, alpha, _) = mirror_instance_counts(scene, &[*template])?;
            vehicle_vertex_count = vehicle_vertex_count.max(vertices);
            vehicle_index_count = vehicle_index_count.max(indices);
            vehicle_alpha_count = vehicle_alpha_count.max(alpha);
        }
        let remote_count = dynamic_slots;
        let dynamic_vertex_capacity =
            checked_mul(vehicle_vertex_count, remote_count, "dynamic vertex")?;
        let dynamic_opaque_index_capacity = checked_mul(
            vehicle_index_count.saturating_sub(vehicle_alpha_count * 3),
            remote_count,
            "dynamic opaque index",
        )?;
        let dynamic_transparent_triangle_capacity = checked_mul(
            vehicle_alpha_count,
            remote_count,
            "dynamic transparent triangle",
        )?;
        let transparent_triangle_capacity = checked_add(
            static_alpha_count,
            dynamic_transparent_triangle_capacity,
            "transparent triangle",
        )?;
        let expanded_vertex_capacity = checked_add(
            static_vertex_count,
            dynamic_vertex_capacity,
            "expanded vertex",
        )?;
        let expanded_index_capacity = checked_add(
            static_index_count,
            checked_mul(vehicle_index_count, remote_count, "dynamic index")?,
            "expanded index",
        )?;
        if expanded_vertex_capacity > MAX_MIRROR_EXPANDED_VERTICES
            || expanded_index_capacity > MAX_MIRROR_EXPANDED_INDICES
            || expanded_vertex_capacity > u32::MAX as usize
        {
            return Err("rear-view expanded geometry exceeds supported bounds".into());
        }

        let mut background_vertex_capacity = 0usize;
        let mut background_index_capacity = 0usize;
        for &mesh_index in &scene.background_mesh_indices {
            let mesh = scene
                .meshes
                .get(mesh_index as usize)
                .ok_or_else(|| "rear-view background references a missing mesh".to_string())?;
            background_vertex_capacity = checked_add(
                background_vertex_capacity,
                mesh.vertices.len(),
                "background vertex",
            )?;
            background_index_capacity = checked_add(
                background_index_capacity,
                mesh.indices.len(),
                "background index",
            )?;
        }
        let transparent_index_capacity =
            checked_mul(transparent_triangle_capacity, 3, "transparent index")?;
        let byte_capacity = [
            (
                expanded_vertex_capacity,
                std::mem::size_of::<MirrorWorldVertex>(),
            ),
            (
                background_vertex_capacity,
                std::mem::size_of::<MirrorWorldVertex>(),
            ),
            (expanded_index_capacity, 4),
            (
                transparent_triangle_capacity,
                std::mem::size_of::<MirrorTriangle>(),
            ),
            (transparent_index_capacity, 4),
            (background_index_capacity, 4),
        ]
        .into_iter()
        .try_fold(0u64, |total, (count, stride)| {
            let bytes = count
                .checked_mul(stride)
                .and_then(|value| u64::try_from(value).ok())
                .ok_or_else(|| "rear-view geometry cache size overflow".to_string())?;
            total
                .checked_add(bytes)
                .ok_or_else(|| "rear-view geometry cache size overflow".to_string())
        })?;
        if byte_capacity > MAX_MIRROR_CACHE_BYTES {
            return Err("rear-view geometry cache exceeds its memory budget".into());
        }
        let device_buffer_capacities = [
            (
                expanded_vertex_capacity,
                std::mem::size_of::<MirrorWorldVertex>(),
                "scene vertex buffer",
            ),
            (
                background_vertex_capacity,
                std::mem::size_of::<MirrorWorldVertex>(),
                "background vertex buffer",
            ),
            (static_index_count, 4, "static index buffer"),
            (
                dynamic_opaque_index_capacity,
                4,
                "dynamic opaque index buffer",
            ),
            (transparent_index_capacity, 4, "transparent index buffer"),
            (background_index_capacity, 4, "background index buffer"),
        ];
        for (count, stride, label) in device_buffer_capacities {
            let requested_bytes = count
                .checked_mul(stride)
                .and_then(|size| u64::try_from(size).ok())
                .ok_or_else(|| "rear-view geometry cache size overflow".to_string())?
                .max(4);
            if requested_bytes > device_max_buffer_size {
                return Err(format!("rear-view {label} exceeds the device buffer limit"));
            }
        }

        let mut static_vertices = Vec::with_capacity(static_vertex_count);
        let mut static_opaque_indices = Vec::with_capacity(static_index_count);
        let mut static_transparent_triangles = Vec::with_capacity(static_alpha_count);
        let mut animated_vertices = Vec::new();
        let mut animated_materials: Vec<AnimatedMaterialState> = Vec::new();
        let mut source_order = 0u32;
        for instance in static_instances {
            let mesh = &scene.meshes[instance.mesh_index as usize];
            let base = u32::try_from(static_vertices.len())
                .map_err(|_| "rear-view vertex offset exceeds supported bounds".to_string())?;
            let transform = instance.transform_for_frame();
            let mut world_positions = Vec::with_capacity(mesh.vertices.len());
            for vertex in &mesh.vertices {
                let world_position = transform.transform_point(vertex.position);
                world_positions.push(world_position);
                let world_vertex = mirror_world_vertex(
                    scene,
                    vertex,
                    world_position,
                    transform.transform_normal(vertex.normal),
                    None,
                )?;
                if animate
                    && scene.materials[vertex.material as usize]
                        .animation
                        .is_some()
                {
                    let slot = match animated_materials
                        .iter()
                        .position(|state| state.material == vertex.material)
                    {
                        Some(slot) => slot,
                        None => {
                            animated_materials.push(AnimatedMaterialState {
                                material: vertex.material,
                                applied: None,
                            });
                            animated_materials.len() - 1
                        }
                    };
                    animated_vertices.push(AnimatedVertex {
                        index: u32::try_from(static_vertices.len()).map_err(|_| {
                            "rear-view vertex offset exceeds supported bounds".to_string()
                        })?,
                        slot: slot as u32,
                        vertex: world_vertex,
                    });
                }
                static_vertices.push(world_vertex);
            }
            for triangle in mesh.indices.chunks_exact(3) {
                let tri = [triangle[0], triangle[1], triangle[2]];
                if triangle_is_transparent(scene, mesh, tri)? {
                    let points = tri.map(|index| world_positions[index as usize]);
                    static_transparent_triangles.push(MirrorTriangle {
                        centroid: mirror_triangle_centroid(points)?,
                        source_order,
                        indices: tri.map(|index| base + index),
                    });
                } else {
                    static_opaque_indices.extend(tri.map(|index| base + index));
                }
                source_order = source_order
                    .checked_add(1)
                    .ok_or_else(|| "rear-view source order overflow".to_string())?;
            }
        }
        let mut background_vertices = Vec::new();
        let mut background_indices = Vec::new();
        for &mesh_index in &scene.background_mesh_indices {
            let mesh = &scene.meshes[mesh_index as usize];
            let base = u32::try_from(background_vertices.len())
                .map_err(|_| "rear-view background vertex offset overflow".to_string())?;
            for vertex in &mesh.vertices {
                background_vertices.push(mirror_world_vertex(
                    scene,
                    vertex,
                    vertex.position,
                    Vec3::UP,
                    None,
                )?);
            }
            background_indices.extend(mesh.indices.iter().map(|index| base + *index));
        }
        Ok(MirrorCpuGeometry {
            static_vertices,
            background_vertices,
            static_opaque_indices,
            background_indices,
            static_transparent_triangles,
            background_extent: validate_background_meshes(scene, &[])?,
            static_triangle_count: static_triangles,
            dynamic_vertex_capacity,
            dynamic_opaque_index_capacity,
            transparent_triangle_capacity,
            animated_vertices,
            animated_materials,
        })
    }

    fn create_mirror_buffer(
        device: &wgpu::Device,
        label: &str,
        requested_bytes: u64,
        usage: wgpu::BufferUsages,
    ) -> Result<wgpu::Buffer, String> {
        let size = requested_bytes.max(4);
        if size > device.limits().max_buffer_size {
            return Err(format!("rear-view {label} exceeds the device buffer limit"));
        }
        Ok(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        }))
    }

    fn create_mirror_geometry_cache(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene3d,
        static_instances: &[MeshInstance],
        vehicle_templates: &[MeshInstance],
    ) -> Result<MirrorGeometryCache, String> {
        create_geometry_cache(
            device,
            queue,
            scene,
            static_instances,
            vehicle_templates,
            MAX_RENDER_VEHICLES.saturating_sub(1),
            false,
        )
    }

    /// Main-view cache: static track and prop geometry is uploaded once; up to
    /// every protocol vehicle is rewritten per frame and animated materials are
    /// patched when their frame changes.
    fn create_main_geometry_cache(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene3d,
        static_instances: &[MeshInstance],
        vehicle_templates: &[MeshInstance],
    ) -> Result<MirrorGeometryCache, String> {
        create_geometry_cache(
            device,
            queue,
            scene,
            static_instances,
            vehicle_templates,
            MAX_RENDER_VEHICLES,
            true,
        )
    }

    fn create_geometry_cache(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene3d,
        static_instances: &[MeshInstance],
        vehicle_templates: &[MeshInstance],
        dynamic_slots: usize,
        animate: bool,
    ) -> Result<MirrorGeometryCache, String> {
        let cpu = build_mirror_cpu_geometry(
            scene,
            static_instances,
            vehicle_templates,
            device.limits().max_buffer_size,
            dynamic_slots,
            animate,
        )?;
        let static_vertex_count = cpu.static_vertices.len();
        let expanded_vertex_capacity = static_vertex_count + cpu.dynamic_vertex_capacity;
        let transparent_index_capacity =
            checked_mul(cpu.transparent_triangle_capacity, 3, "transparent index")?;
        let bytes = |count: usize, stride: usize| -> Result<u64, String> {
            let size = count
                .checked_mul(stride)
                .ok_or_else(|| "rear-view GPU buffer size overflow".to_string())?;
            u64::try_from(size).map_err(|_| "rear-view GPU buffer size overflow".into())
        };
        let scene_vertex_bytes = bytes(
            expanded_vertex_capacity,
            std::mem::size_of::<MirrorWorldVertex>(),
        )?;
        let background_vertex_bytes = bytes(
            cpu.background_vertices.len(),
            std::mem::size_of::<MirrorWorldVertex>(),
        )?;
        let static_opaque_bytes = bytes(cpu.static_opaque_indices.len(), 4)?;
        let dynamic_opaque_bytes = bytes(cpu.dynamic_opaque_index_capacity, 4)?;
        let transparent_bytes = bytes(transparent_index_capacity, 4)?;
        let background_index_bytes = bytes(cpu.background_indices.len(), 4)?;
        let device_limit = device.limits().max_buffer_size;
        for (label, size) in [
            ("scene vertex buffer", scene_vertex_bytes),
            ("background vertex buffer", background_vertex_bytes),
            ("opaque index buffer", static_opaque_bytes),
            ("dynamic opaque index buffer", dynamic_opaque_bytes),
            ("transparent index buffer", transparent_bytes),
            ("background index buffer", background_index_bytes),
        ] {
            if size.max(4) > device_limit {
                return Err(format!("rear-view {label} exceeds the device buffer limit"));
            }
        }
        let scene_vertex_buffer = create_mirror_buffer(
            device,
            "rear-view static and dynamic world vertices",
            scene_vertex_bytes,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        )?;
        let background_vertex_buffer = create_mirror_buffer(
            device,
            "rear-view cached background vertices",
            background_vertex_bytes,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        )?;
        let static_opaque_index_buffer = create_mirror_buffer(
            device,
            "rear-view cached opaque indices",
            static_opaque_bytes,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        )?;
        let dynamic_opaque_index_buffer = create_mirror_buffer(
            device,
            "rear-view dynamic opaque indices",
            dynamic_opaque_bytes,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        )?;
        let transparent_index_buffer = create_mirror_buffer(
            device,
            "rear-view sorted transparent indices",
            transparent_bytes,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        )?;
        let background_index_buffer = create_mirror_buffer(
            device,
            "rear-view cached background indices",
            background_index_bytes,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        )?;
        if !cpu.static_vertices.is_empty() {
            queue.write_buffer(
                &scene_vertex_buffer,
                0,
                bytemuck::cast_slice(&cpu.static_vertices),
            );
        }
        if !cpu.background_vertices.is_empty() {
            queue.write_buffer(
                &background_vertex_buffer,
                0,
                bytemuck::cast_slice(&cpu.background_vertices),
            );
        }
        if !cpu.static_opaque_indices.is_empty() {
            queue.write_buffer(
                &static_opaque_index_buffer,
                0,
                bytemuck::cast_slice(&cpu.static_opaque_indices),
            );
        }
        if !cpu.background_indices.is_empty() {
            queue.write_buffer(
                &background_index_buffer,
                0,
                bytemuck::cast_slice(&cpu.background_indices),
            );
        }
        Ok(MirrorGeometryCache {
            scene_vertex_buffer,
            background_vertex_buffer,
            static_opaque_index_buffer,
            dynamic_opaque_index_buffer,
            transparent_index_buffer,
            background_index_buffer,
            scene_vertex_count: static_vertex_count as u32,
            background_index_count: cpu.background_indices.len() as u32,
            static_opaque_index_count: cpu.static_opaque_indices.len() as u32,
            background_extent: cpu.background_extent,
            dynamic_vertex_capacity: cpu.dynamic_vertex_capacity,
            dynamic_opaque_index_capacity: cpu.dynamic_opaque_index_capacity,
            transparent_index_capacity,
            transparent_triangle_capacity: cpu.transparent_triangle_capacity,
            static_transparent_triangles: cpu.static_transparent_triangles,
            static_triangle_count: cpu.static_triangle_count,
            animated_vertices: cpu.animated_vertices,
            animated_materials: cpu.animated_materials,
        })
    }

    fn mirror_camera_bind_group_layout_for(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rear-view camera uniform layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<MirrorCameraUniform>() as u64,
                    ),
                },
                count: None,
            }],
        })
    }

    fn create_mirror_camera_binding(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        label: &str,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::bytes_of(&MirrorCameraUniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        (buffer, bind_group)
    }

    fn mirror_camera_uniform(
        camera: Camera3d,
        aspect: f32,
        camera_up: Vec3,
    ) -> MirrorCameraUniform {
        let projection = CameraFrameProjection::new(camera, aspect, camera_up);
        MirrorCameraUniform {
            position: [
                projection.position.x,
                projection.position.y,
                projection.position.z,
                0.0,
            ],
            forward: [
                projection.forward.x,
                projection.forward.y,
                projection.forward.z,
                0.0,
            ],
            right: [
                projection.right.x,
                projection.right.y,
                projection.right.z,
                0.0,
            ],
            up: [projection.up.x, projection.up.y, projection.up.z, 0.0],
            projection: [
                projection.focal_scale,
                projection.aspect,
                projection.depth_scale,
                projection.depth_offset,
            ],
            fog: [0.0; 4],
            fog_range: [0.0; 4],
        }
    }

    fn with_fog(mut uniform: MirrorCameraUniform, fog: Option<SceneFog>) -> MirrorCameraUniform {
        if let Some(f) = fog {
            uniform.fog = [f.color[0], f.color[1], f.color[2], f.amount.clamp(0.0, 1.0)];
            uniform.fog_range = [f.start, f.end.max(f.start + 1.0), 0.0, 0.0];
        }
        uniform
    }

    #[derive(Clone, Copy)]
    struct CachedPipelines<'a> {
        opaque: &'a wgpu::RenderPipeline,
        alpha: &'a wgpu::RenderPipeline,
        background: &'a wgpu::RenderPipeline,
    }

    /// Rebuild the per-frame vehicle geometry and the depth-sorted transparent
    /// index list for a cached scene. Static geometry is never touched.
    fn build_dynamic_frame(
        scene: &Scene3d,
        geometry: &MirrorGeometryCache,
        scratch: &mut MirrorFrameScratch,
        instances: &[MeshInstance],
        camera_position: Vec3,
        forward: Vec3,
        tick: Option<u64>,
    ) -> Result<(), String> {
        let static_vertex_count = geometry.scene_vertex_count as usize;
        let mut source_order = geometry.static_triangle_count;
        {
            scratch.dynamic_vertices.clear();
            scratch.dynamic_opaque_indices.clear();
            scratch.dynamic_transparent_triangles.clear();
            scratch.sort_keys.clear();
            scratch.sorted_transparent_indices.clear();
            scratch.world_positions.clear();
            for &instance in instances {
                if !instance.translation.x.is_finite()
                    || !instance.translation.y.is_finite()
                    || !instance.translation.z.is_finite()
                    || !instance.heading.is_finite()
                    || !instance.ground_normal.x.is_finite()
                    || !instance.ground_normal.y.is_finite()
                    || !instance.ground_normal.z.is_finite()
                {
                    return Err("rear-view remote pose contains a non-finite value".into());
                }
                let transform = instance.transform_for_frame();
                let mesh = scene
                    .meshes
                    .get(instance.mesh_index as usize)
                    .ok_or_else(|| "rear-view vehicle references a missing mesh".to_string())?;
                let base = u32::try_from(static_vertex_count + scratch.dynamic_vertices.len())
                    .map_err(|_| "rear-view vehicle vertex offset overflow".to_string())?;
                scratch.world_positions.clear();
                if scratch.world_positions.capacity() < mesh.vertices.len() {
                    scratch.world_positions.reserve(mesh.vertices.len());
                }
                for vertex in &mesh.vertices {
                    let world_position = transform.transform_point(vertex.position);
                    scratch.world_positions.push(world_position);
                    scratch.dynamic_vertices.push(mirror_world_vertex(
                        scene,
                        vertex,
                        world_position,
                        transform.transform_normal(vertex.normal),
                        tick,
                    )?);
                }
                for triangle in mesh.indices.chunks_exact(3) {
                    let tri = [triangle[0], triangle[1], triangle[2]];
                    if triangle_is_transparent(scene, mesh, tri)? {
                        let points = tri.map(|index| scratch.world_positions[index as usize]);
                        scratch.dynamic_transparent_triangles.push(MirrorTriangle {
                            centroid: mirror_triangle_centroid(points)?,
                            source_order,
                            indices: tri.map(|index| base + index),
                        });
                    } else {
                        scratch
                            .dynamic_opaque_indices
                            .extend(tri.map(|index| base + index));
                    }
                    source_order = source_order
                        .checked_add(1)
                        .ok_or_else(|| "rear-view source order overflow".to_string())?;
                }
            }
            if scratch.dynamic_vertices.len() > geometry.dynamic_vertex_capacity
                || scratch.dynamic_opaque_indices.len() > geometry.dynamic_opaque_index_capacity
                || geometry.static_transparent_triangles.len()
                    + scratch.dynamic_transparent_triangles.len()
                    > geometry.transparent_triangle_capacity
            {
                return Err("rear-view dynamic geometry exceeds its validated cache".into());
            }
            scratch.sort_keys.extend(
                geometry
                    .static_transparent_triangles
                    .iter()
                    .enumerate()
                    .map(|(triangle_index, triangle)| MirrorTriangleSortKey {
                        depth: (triangle.centroid - camera_position).dot(forward),
                        source_order: triangle.source_order,
                        dynamic: false,
                        triangle_index,
                    }),
            );
            scratch.sort_keys.extend(
                scratch
                    .dynamic_transparent_triangles
                    .iter()
                    .enumerate()
                    .map(|(triangle_index, triangle)| MirrorTriangleSortKey {
                        depth: (triangle.centroid - camera_position).dot(forward),
                        source_order: triangle.source_order,
                        dynamic: true,
                        triangle_index,
                    }),
            );
            if scratch.sort_keys.iter().any(|key| !key.depth.is_finite()) {
                return Err("rear-view transparent triangle depth is non-finite".into());
            }
            scratch.sort_keys.sort_by(|a, b| {
                b.depth
                    .total_cmp(&a.depth)
                    .then_with(|| a.source_order.cmp(&b.source_order))
            });
            for key in &scratch.sort_keys {
                let triangle = if key.dynamic {
                    &scratch.dynamic_transparent_triangles[key.triangle_index]
                } else {
                    &geometry.static_transparent_triangles[key.triangle_index]
                };
                scratch
                    .sorted_transparent_indices
                    .extend_from_slice(&triangle.indices);
            }
        }

        Ok(())
    }

    fn upload_dynamic_frame(
        queue: &wgpu::Queue,
        geometry: &MirrorGeometryCache,
        scratch: &MirrorFrameScratch,
    ) {
        let static_vertex_count = geometry.scene_vertex_count as usize;
        if !scratch.dynamic_vertices.is_empty() {
            let offset = (static_vertex_count * std::mem::size_of::<MirrorWorldVertex>()) as u64;
            queue.write_buffer(
                &geometry.scene_vertex_buffer,
                offset,
                bytemuck::cast_slice(&scratch.dynamic_vertices),
            );
        }
        if !scratch.dynamic_opaque_indices.is_empty() {
            queue.write_buffer(
                &geometry.dynamic_opaque_index_buffer,
                0,
                bytemuck::cast_slice(&scratch.dynamic_opaque_indices),
            );
        }
        if !scratch.sorted_transparent_indices.is_empty() {
            queue.write_buffer(
                &geometry.transparent_index_buffer,
                0,
                bytemuck::cast_slice(&scratch.sorted_transparent_indices),
            );
        }
    }

    /// Patch only the static vertices of materials whose animation frame
    /// changed since the last upload.
    fn sync_animated_materials(
        queue: &wgpu::Queue,
        geometry: &mut MirrorGeometryCache,
        scene: &Scene3d,
        tick: u64,
    ) {
        if geometry.animated_materials.is_empty() {
            return;
        }
        let mut changed = vec![false; geometry.animated_materials.len()];
        let mut any = false;
        for (slot, state) in geometry.animated_materials.iter_mut().enumerate() {
            let Some(material) = scene.materials.get(state.material as usize) else {
                continue;
            };
            let frame = material.frame_at(tick);
            if state.applied != Some(frame) {
                state.applied = Some(frame);
                changed[slot] = true;
                any = true;
            }
        }
        if !any {
            return;
        }
        let stride = std::mem::size_of::<MirrorWorldVertex>() as u64;
        let mut run: Vec<MirrorWorldVertex> = Vec::new();
        let mut run_start = 0u32;
        for animated in geometry.animated_vertices.iter_mut() {
            let slot = animated.slot as usize;
            if !changed[slot] {
                continue;
            }
            if let Some(frame) = geometry.animated_materials[slot].applied {
                animated.vertex.texture_layer = frame.texture as u32;
                animated.vertex.uv_scale = frame.uv_scale;
                animated.vertex.uv_offset = frame.uv_offset;
            }
            if !run.is_empty() && animated.index != run_start + run.len() as u32 {
                queue.write_buffer(
                    &geometry.scene_vertex_buffer,
                    u64::from(run_start) * stride,
                    bytemuck::cast_slice(&run),
                );
                run.clear();
            }
            if run.is_empty() {
                run_start = animated.index;
            }
            run.push(animated.vertex);
        }
        if !run.is_empty() {
            queue.write_buffer(
                &geometry.scene_vertex_buffer,
                u64::from(run_start) * stride,
                bytemuck::cast_slice(&run),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_cached_scene(
        encoder: &mut wgpu::CommandEncoder,
        geometry: &MirrorGeometryCache,
        scratch: &MirrorFrameScratch,
        material_bind_group: &wgpu::BindGroup,
        scene_camera: &wgpu::BindGroup,
        background_camera: &wgpu::BindGroup,
        pipelines: CachedPipelines<'_>,
        color_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
    ) {
        let static_vertex_count = geometry.scene_vertex_count as usize;
        if geometry.background_index_count > 0 {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rear-view cached background"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.48,
                            g: 0.68,
                            b: 0.84,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &material_bind_group, &[]);
            pass.set_bind_group(1, &background_camera, &[]);
            pass.set_pipeline(&pipelines.background);
            pass.set_vertex_buffer(0, geometry.background_vertex_buffer.slice(..));
            pass.set_index_buffer(
                geometry.background_index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            pass.draw_indexed(0..geometry.background_index_count, 0, 0..1);
        }

        let dynamic_vertex_count = scratch.dynamic_vertices.len();
        if static_vertex_count > 0 || dynamic_vertex_count > 0 {
            let has_background = geometry.background_index_count > 0;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rear-view cached scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if has_background {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.48,
                                g: 0.68,
                                b: 0.84,
                                a: 1.0,
                            })
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &material_bind_group, &[]);
            pass.set_bind_group(1, &scene_camera, &[]);
            pass.set_vertex_buffer(0, geometry.scene_vertex_buffer.slice(..));
            if geometry.static_opaque_index_count > 0 || !scratch.dynamic_opaque_indices.is_empty()
            {
                pass.set_pipeline(&pipelines.opaque);
                if geometry.static_opaque_index_count > 0 {
                    pass.set_index_buffer(
                        geometry.static_opaque_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    pass.draw_indexed(0..geometry.static_opaque_index_count, 0, 0..1);
                }
                if !scratch.dynamic_opaque_indices.is_empty() {
                    pass.set_index_buffer(
                        geometry.dynamic_opaque_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    pass.draw_indexed(0..scratch.dynamic_opaque_indices.len() as u32, 0, 0..1);
                }
            }
            if !scratch.sorted_transparent_indices.is_empty() {
                pass.set_pipeline(&pipelines.alpha);
                pass.set_index_buffer(
                    geometry.transparent_index_buffer.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                pass.draw_indexed(0..scratch.sorted_transparent_indices.len() as u32, 0, 0..1);
            }
        } else if geometry.background_index_count == 0 {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear empty rear view"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.48,
                            g: 0.68,
                            b: 0.84,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    struct MirrorCompositeVertex {
        position: [f32; 2],
        uv: [f32; 2],
    }

    struct MirrorTarget {
        color: wgpu::Texture,
        depth: wgpu::Texture,
        bind_group: wgpu::BindGroup,
        width: u32,
        height: u32,
    }

    struct PendingMirror {
        camera: Camera3d,
        up: Vec3,
        remote_instances: Vec<MeshInstance>,
    }

    #[derive(Clone, Copy)]
    struct MirrorView {
        camera: Camera3d,
        up: Vec3,
    }

    pub struct WgpuRenderer {
        surface: wgpu::Surface<'static>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
        pipeline: wgpu::RenderPipeline,
        straight_alpha_pipeline: wgpu::RenderPipeline,
        background_pipeline: wgpu::RenderPipeline,
        mirror_frame_pipeline: wgpu::RenderPipeline,
        mirror_image_pipeline: wgpu::RenderPipeline,
        mirror_opaque_pipeline: wgpu::RenderPipeline,
        mirror_alpha_pipeline: wgpu::RenderPipeline,
        mirror_background_pipeline: wgpu::RenderPipeline,
        mirror_bind_group_layout: wgpu::BindGroupLayout,
        mirror_target: MirrorTarget,
        mirror_scene_camera_buffer: wgpu::Buffer,
        mirror_scene_camera_bind_group: wgpu::BindGroup,
        mirror_background_camera_buffer: wgpu::Buffer,
        mirror_background_camera_bind_group: wgpu::BindGroup,
        mirror_geometry: MirrorGeometryCache,
        mirror_scratch: MirrorFrameScratch,
        main_scene_camera_buffer: wgpu::Buffer,
        main_scene_camera_bind_group: wgpu::BindGroup,
        main_background_camera_buffer: wgpu::Buffer,
        main_background_camera_bind_group: wgpu::BindGroup,
        /// Once-per-scene GPU copy of track, prop and horizon geometry for the
        /// main view, with room for every protocol vehicle.
        main_geometry: MirrorGeometryCache,
        main_scratch: MirrorFrameScratch,
        /// False whenever the GPU texture array may not match `scene`.
        cached_scene_textures_current: bool,
        mirror_frame_vertices: wgpu::Buffer,
        mirror_image_vertices: wgpu::Buffer,
        pending_mirror: Option<PendingMirror>,
        bind_group_layout: wgpu::BindGroupLayout,
        bind_group: wgpu::BindGroup,
        material_texture: wgpu::Texture,
        texture_cache: RgbaTextureArray,
        depth: wgpu::Texture,
        scene: Scene3d,
        /// Distance fog and grading of the installed scene (project tracks).
        fog: Option<SceneFog>,
        vehicle_template: MeshInstance,
        static_instances: Vec<MeshInstance>,
        vehicle_track_source_fingerprint: Option<String>,
        vehicle_profile_templates: HashMap<VehicleProfileIdentity, MeshInstance>,
        vehicle_profile_geometry_bytes: usize,
        vehicle_min_z_scaled: f32,
        track_surface: Option<TrackSurface>,
        track_surface_states: Vec<TrackSurfaceState>,
        vehicle_roster_signature: Option<String>,
        preview_camera_mode: PreviewCameraMode,
        preview_look_behind: bool,
        /// Smoothed chase-camera yaw supplied by the host (see
        /// tests/feel); `None` follows the body.
        chase_camera_heading: Option<f32>,
        preview_mirror_visible: bool,
        backend_name: &'static str,
    }
    impl WgpuRenderer {
        pub async fn new(canvas: web_sys::HtmlCanvasElement) -> Result<Self, String> {
            let (b, n) = if browser_webgpu_adapter_available().await {
                (wgpu::Backends::BROWSER_WEBGPU, "WebGPU")
            } else {
                (wgpu::Backends::GL, "WebGL2")
            };
            Self::with_backend(canvas, b, n).await
        }
        pub async fn new_webgl(canvas: web_sys::HtmlCanvasElement) -> Result<Self, String> {
            Self::with_backend(canvas, wgpu::Backends::GL, "WebGL2").await
        }
        async fn with_backend(
            canvas: web_sys::HtmlCanvasElement,
            backends: wgpu::Backends,
            backend_name: &'static str,
        ) -> Result<Self, String> {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends,
                ..Default::default()
            });
            let surface = instance
                .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
                .map_err(|e| e.to_string())?;
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
                .or(instance
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::LowPower,
                        compatible_surface: Some(&surface),
                        force_fallback_adapter: true,
                    })
                    .await)
                .ok_or_else(|| "No WebGPU/WebGL adapter is available".to_string())?;
            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("racer device"),
                        required_features: wgpu::Features::empty(),
                        required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                    },
                    None,
                )
                .await
                .map_err(|e| e.to_string())?;
            let caps = surface.get_capabilities(&adapter);
            let format = *caps
                .formats
                .first()
                .ok_or_else(|| "Surface exposes no color formats".to_string())?;
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: canvas.width().max(1),
                height: canvas.height().max(1),
                present_mode: caps
                    .present_modes
                    .first()
                    .copied()
                    .unwrap_or(wgpu::PresentMode::Fifo),
                alpha_mode: caps
                    .alpha_modes
                    .first()
                    .copied()
                    .unwrap_or(wgpu::CompositeAlphaMode::Auto),
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            surface.configure(&device, &config);
            let bind_layout = bind_group_layout_for(&device);
            let texture_cache = synthetic_textures();
            let (material_texture, bind_group) = create_texture_binding(
                &device,
                &queue,
                &bind_layout,
                &texture_cache,
                texture_array_storage_layers(
                    texture_cache.width,
                    texture_cache.height,
                    texture_cache.layers.len(),
                    device.limits().max_texture_array_layers,
                )
                .expect("synthetic texture array layer count is valid"),
            );
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("3D material shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/racer.wgsl").into()),
            });
            let pipeline = create_pipeline(
                &device,
                &shader,
                &bind_layout,
                format,
                ScenePipelineKind::Opaque,
            );
            let straight_alpha_pipeline = create_pipeline(
                &device,
                &shader,
                &bind_layout,
                format,
                ScenePipelineKind::StraightAlpha,
            );
            let background_pipeline = create_pipeline(
                &device,
                &shader,
                &bind_layout,
                format,
                ScenePipelineKind::Background,
            );
            let mirror_camera_bind_group_layout = mirror_camera_bind_group_layout_for(&device);
            let (mirror_scene_camera_buffer, mirror_scene_camera_bind_group) =
                create_mirror_camera_binding(
                    &device,
                    &mirror_camera_bind_group_layout,
                    "rear-view scene camera",
                );
            let (mirror_background_camera_buffer, mirror_background_camera_bind_group) =
                create_mirror_camera_binding(
                    &device,
                    &mirror_camera_bind_group_layout,
                    "rear-view background camera",
                );
            let (main_scene_camera_buffer, main_scene_camera_bind_group) =
                create_mirror_camera_binding(
                    &device,
                    &mirror_camera_bind_group_layout,
                    "main scene camera",
                );
            let (main_background_camera_buffer, main_background_camera_bind_group) =
                create_mirror_camera_binding(
                    &device,
                    &mirror_camera_bind_group_layout,
                    "main background camera",
                );
            let mirror_opaque_pipeline = create_mirror_scene_pipeline(
                &device,
                &shader,
                &bind_layout,
                &mirror_camera_bind_group_layout,
                format,
                ScenePipelineKind::Opaque,
            );
            let mirror_alpha_pipeline = create_mirror_scene_pipeline(
                &device,
                &shader,
                &bind_layout,
                &mirror_camera_bind_group_layout,
                format,
                ScenePipelineKind::StraightAlpha,
            );
            let mirror_background_pipeline = create_mirror_scene_pipeline(
                &device,
                &shader,
                &bind_layout,
                &mirror_camera_bind_group_layout,
                format,
                ScenePipelineKind::Background,
            );
            let mirror_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("rear-view inset compositor shader"),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!("shaders/mirror_composite.wgsl").into(),
                ),
            });
            let mirror_bind_group_layout = mirror_bind_group_layout_for(&device);
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let mirror_frame_pipeline = create_mirror_composite_pipeline(
                &device,
                &mirror_shader,
                &mirror_bind_group_layout,
                format,
                "fs_frame",
                "rear-view inset frame pipeline",
            );
            if let Some(error) = device.pop_error_scope().await {
                return Err(format!("rear-view frame pipeline is invalid: {error}"));
            }
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let mirror_image_pipeline = create_mirror_composite_pipeline(
                &device,
                &mirror_shader,
                &mirror_bind_group_layout,
                format,
                "fs_image",
                "rear-view inset image pipeline",
            );
            if let Some(error) = device.pop_error_scope().await {
                return Err(format!("rear-view image pipeline is invalid: {error}"));
            }
            let (mirror_width, mirror_height) =
                mirror_target_dimensions(config.width, config.height);
            let mirror_target = create_mirror_target(
                &device,
                &mirror_bind_group_layout,
                format,
                mirror_width,
                mirror_height,
            );
            let (mirror_frame_vertices, mirror_image_vertices) = create_mirror_composite_vertices(
                &device,
                config.width,
                config.height,
                mirror_width,
                mirror_height,
            );
            let depth = create_depth(&device, config.width, config.height);
            let track = TrackRuntime::synthetic_circuit();
            let scene = synthetic_scene(&track, Vec2::new(0.0, 0.0), 0.0);
            let vehicle_template = scene.instances[2];
            let static_instances = scene
                .instances
                .iter()
                .enumerate()
                .filter_map(|(index, instance)| (index != 2).then_some(*instance))
                .collect::<Vec<_>>();
            let mirror_geometry = create_mirror_geometry_cache(
                &device,
                &queue,
                &scene,
                &static_instances,
                &[vehicle_template],
            )?;
            let mirror_scratch = MirrorFrameScratch::for_geometry(&mirror_geometry);
            let main_geometry = create_main_geometry_cache(
                &device,
                &queue,
                &scene,
                &static_instances,
                &[vehicle_template],
            )?;
            let main_scratch = MirrorFrameScratch::for_geometry(&main_geometry);
            Ok(Self {
                surface,
                device,
                queue,
                config,
                pipeline,
                straight_alpha_pipeline,
                background_pipeline,
                mirror_frame_pipeline,
                mirror_image_pipeline,
                mirror_opaque_pipeline,
                mirror_alpha_pipeline,
                mirror_background_pipeline,
                mirror_bind_group_layout,
                mirror_target,
                mirror_scene_camera_buffer,
                mirror_scene_camera_bind_group,
                mirror_background_camera_buffer,
                mirror_background_camera_bind_group,
                mirror_geometry,
                mirror_scratch,
                main_scene_camera_buffer,
                main_scene_camera_bind_group,
                main_background_camera_buffer,
                main_background_camera_bind_group,
                main_geometry,
                main_scratch,
                cached_scene_textures_current: false,
                mirror_frame_vertices,
                mirror_image_vertices,
                pending_mirror: None,
                bind_group_layout: bind_layout,
                bind_group,
                material_texture,
                texture_cache,
                depth,
                scene,
                fog: None,
                vehicle_template,
                static_instances,
                vehicle_track_source_fingerprint: None,
                vehicle_profile_templates: HashMap::new(),
                vehicle_profile_geometry_bytes: 0,
                vehicle_min_z_scaled: 0.0,
                track_surface: None,
                track_surface_states: Vec::new(),
                vehicle_roster_signature: None,
                preview_camera_mode: PreviewCameraMode::Chase,
                preview_look_behind: false,
                chase_camera_heading: None,
                preview_mirror_visible: true,
                backend_name,
            })
        }
        pub fn backend_name(&self) -> &'static str {
            self.backend_name
        }

        /// Fog and scene grading for the installed scene; `set_scene` clears it.
        pub fn set_fog(&mut self, fog: Option<SceneFog>) {
            self.fog = fog;
        }

        pub fn set_preview_camera_mode(&mut self, mode: PreviewCameraMode) {
            self.preview_camera_mode = mode;
        }

        pub fn set_preview_look_behind(&mut self, enabled: bool) {
            self.preview_look_behind = enabled;
        }

        /// Set (or clear with `None`) the yaw the external chase and look-behind
        /// views use instead of the local vehicle's body heading. The rear
        /// mirror and the hood view stay attached to the body.
        pub fn set_chase_camera_heading(&mut self, heading: Option<f32>) {
            self.chase_camera_heading = heading.filter(|value| value.is_finite());
        }

        /// Show or hide the project-owned rear-view inset in every preview path.
        pub fn set_preview_mirror_visible(&mut self, visible: bool) {
            self.preview_mirror_visible = visible;
        }

        /// Replace the generic scene with a prevalidated decoded scene. The
        /// selected instance remains in local mesh space and is transformed
        /// from current simulation pose by `render` each frame.
        pub fn set_scene(
            &mut self,
            scene: Scene3d,
            vehicle_instance: usize,
            track_mesh_indices: &[u32],
        ) -> Result<(), String> {
            validate_background_meshes(&scene, track_mesh_indices)?;
            let Some(vehicle) = scene.instances.get(vehicle_instance).copied() else {
                return Err("loaded scene has no designated vehicle instance".into());
            };
            let vehicle_mesh = scene
                .meshes
                .get(vehicle.mesh_index as usize)
                .ok_or_else(|| "loaded vehicle references a missing mesh".to_string())?;
            let vehicle_min_z = vehicle_mesh
                .vertices
                .iter()
                .map(|vertex| vertex.position.z)
                .reduce(f32::min)
                .ok_or_else(|| "loaded vehicle mesh has no vertices".to_string())?;
            let vehicle_min_z_scaled = vehicle_min_z * vehicle.scale;
            let static_instances = scene
                .instances
                .iter()
                .enumerate()
                .filter_map(|(index, instance)| (index != vehicle_instance).then_some(*instance))
                .collect::<Vec<_>>();
            let track_surface = TrackSurface::build(&scene, track_mesh_indices)
                .map_err(|error| format!("could not prepare track height sampler: {error}"))?;
            let mirror_geometry = create_mirror_geometry_cache(
                &self.device,
                &self.queue,
                &scene,
                &static_instances,
                &[vehicle],
            )?;
            let mirror_scratch = MirrorFrameScratch::for_geometry(&mirror_geometry);
            let main_geometry = create_main_geometry_cache(
                &self.device,
                &self.queue,
                &scene,
                &static_instances,
                &[vehicle],
            )?;
            let main_scratch = MirrorFrameScratch::for_geometry(&main_geometry);
            self.scene = scene;
            self.fog = None;
            self.vehicle_template = vehicle;
            self.static_instances = static_instances;
            self.mirror_geometry = mirror_geometry;
            self.mirror_scratch = mirror_scratch;
            self.main_geometry = main_geometry;
            self.main_scratch = main_scratch;
            self.cached_scene_textures_current = false;
            self.vehicle_min_z_scaled = vehicle_min_z_scaled;
            self.track_surface = Some(track_surface);
            self.track_surface_states.clear();
            self.vehicle_roster_signature = None;
            self.vehicle_track_source_fingerprint = None;
            self.vehicle_profile_templates.clear();
            self.vehicle_profile_geometry_bytes = 0;
            self.pending_mirror = None;
            Ok(())
        }

        /// Install a multiplayer scene with a fingerprint-bound local vehicle
        /// template. Replacing the scene clears every registered profile.
        pub fn set_scene_for_vehicle_profile(
            &mut self,
            scene: Scene3d,
            vehicle_instance: usize,
            track_mesh_indices: &[u32],
            track_source_fingerprint: &str,
            active_vehicle: VehicleProfileIdentity,
        ) -> Result<(), String> {
            if !valid_source_fingerprint(track_source_fingerprint) {
                return Err("track source fingerprint must be lowercase SHA-256 hex".into());
            }
            let active_vehicle = VehicleProfileIdentity::new(
                &active_vehicle.vehicle_profile_id,
                &active_vehicle.vehicle_source_fingerprint,
            )?;
            let vehicle_template = scene
                .instances
                .get(vehicle_instance)
                .copied()
                .ok_or_else(|| "loaded scene has no designated vehicle instance".to_string())?;
            let geometry_bytes = vehicle_mesh_geometry_bytes(&scene, vehicle_template)?;
            if geometry_bytes > MAX_PROFILE_GEOMETRY_BYTES {
                return Err("active vehicle profile geometry exceeds its aggregate budget".into());
            }
            self.set_scene(scene, vehicle_instance, track_mesh_indices)?;
            self.vehicle_track_source_fingerprint = Some(track_source_fingerprint.to_owned());
            self.vehicle_profile_geometry_bytes = geometry_bytes;
            self.vehicle_profile_templates
                .insert(active_vehicle, self.vehicle_template);
            Ok(())
        }

        /// Add one validated local car template without rebuilding track
        /// support data. The profile is committed only after its combined
        /// scene and mirror buffers pass all bounds.
        pub fn register_vehicle_profile(
            &mut self,
            track_source_fingerprint: &str,
            vehicle: VehicleProfileIdentity,
            source_scene: &Scene3d,
            source_vehicle_instance: usize,
        ) -> Result<(), String> {
            if !valid_source_fingerprint(track_source_fingerprint)
                || self.vehicle_track_source_fingerprint.as_deref()
                    != Some(track_source_fingerprint)
            {
                return Err(
                    "vehicle profile track fingerprint does not match the active scene".into(),
                );
            }
            let vehicle = VehicleProfileIdentity::new(
                &vehicle.vehicle_profile_id,
                &vehicle.vehicle_source_fingerprint,
            )?;
            if self.vehicle_profile_templates.contains_key(&vehicle) {
                return Ok(());
            }
            if self.vehicle_profile_templates.len() >= MAX_RENDER_VEHICLE_PROFILES {
                return Err(
                    "registered vehicle profile count exceeds the eight-profile limit".into(),
                );
            }
            let source_template = source_scene
                .instances
                .get(source_vehicle_instance)
                .copied()
                .ok_or_else(|| "vehicle profile has no designated vehicle instance".to_string())?;
            let source_bytes = vehicle_mesh_geometry_bytes(source_scene, source_template)?;
            let aggregate_geometry_bytes = self
                .vehicle_profile_geometry_bytes
                .checked_add(source_bytes)
                .ok_or_else(|| "registered vehicle profile geometry size overflow".to_string())?;
            if aggregate_geometry_bytes > MAX_PROFILE_GEOMETRY_BYTES {
                return Err(
                    "registered vehicle profile geometry exceeds its aggregate budget".into(),
                );
            }

            let mut candidate_scene = self.scene.clone();
            let template = append_vehicle_profile_mesh_in_place(
                &mut candidate_scene,
                source_scene,
                source_vehicle_instance,
            )?;
            let texture_layers = texture_array_storage_layers(
                candidate_scene.textures.width,
                candidate_scene.textures.height,
                candidate_scene.textures.layers.len(),
                self.device.limits().max_texture_array_layers,
            )
            .ok_or_else(|| "vehicle profile textures exceed the active GPU limit".to_string())?;
            if candidate_scene.textures.width > self.device.limits().max_texture_dimension_2d
                || candidate_scene.textures.height > self.device.limits().max_texture_dimension_2d
            {
                return Err(
                    "vehicle profile texture dimensions exceed the active GPU limit".into(),
                );
            }
            let mut templates = self.vehicle_profile_templates.clone();
            templates.insert(vehicle, template);
            let mut ordered_templates = templates.iter().collect::<Vec<_>>();
            ordered_templates.sort_by(|(left, _), (right, _)| {
                left.vehicle_profile_id
                    .cmp(&right.vehicle_profile_id)
                    .then_with(|| {
                        left.vehicle_source_fingerprint
                            .cmp(&right.vehicle_source_fingerprint)
                    })
            });
            let mirror_templates = ordered_templates
                .into_iter()
                .map(|(_, template)| *template)
                .collect::<Vec<_>>();
            let mirror_geometry = create_mirror_geometry_cache(
                &self.device,
                &self.queue,
                &candidate_scene,
                &self.static_instances,
                &mirror_templates,
            )?;
            let mirror_scratch = MirrorFrameScratch::for_geometry(&mirror_geometry);
            let main_geometry = create_main_geometry_cache(
                &self.device,
                &self.queue,
                &candidate_scene,
                &self.static_instances,
                &mirror_templates,
            )?;
            let main_scratch = MirrorFrameScratch::for_geometry(&main_geometry);
            let (material_texture, bind_group) = create_texture_binding(
                &self.device,
                &self.queue,
                &self.bind_group_layout,
                &candidate_scene.textures,
                texture_layers,
            );

            self.scene = candidate_scene;
            self.material_texture = material_texture;
            self.bind_group = bind_group;
            self.texture_cache = self.scene.textures.clone();
            self.vehicle_profile_templates = templates;
            self.vehicle_profile_geometry_bytes = aggregate_geometry_bytes;
            self.mirror_geometry = mirror_geometry;
            self.mirror_scratch = mirror_scratch;
            self.main_geometry = main_geometry;
            self.main_scratch = main_scratch;
            self.pending_mirror = None;
            Ok(())
        }

        /// Reset per-car surface continuity when the stable participant set
        /// changes, so a new occupant never inherits a departed car's height.
        pub fn set_vehicle_roster(&mut self, signature: &str) -> Result<(), String> {
            if signature.len() > 512 {
                return Err("vehicle roster signature exceeds its bound".into());
            }
            if vehicle_roster_changed(self.vehicle_roster_signature.as_deref(), signature) {
                self.track_surface_states.clear();
                self.vehicle_roster_signature = Some(signature.to_owned());
            }
            Ok(())
        }
        pub fn resize(&mut self, width: u32, height: u32) {
            let (w, h) = (width.max(1), height.max(1));
            if self.config.width == w && self.config.height == h {
                return;
            }
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(&self.device, &self.config);
            self.depth = create_depth(&self.device, w, h);
            let (mirror_width, mirror_height) = mirror_target_dimensions(w, h);
            self.mirror_target = create_mirror_target(
                &self.device,
                &self.mirror_bind_group_layout,
                self.config.format,
                mirror_width,
                mirror_height,
            );
            (self.mirror_frame_vertices, self.mirror_image_vertices) =
                create_mirror_composite_vertices(&self.device, w, h, mirror_width, mirror_height);
        }
        pub fn render(
            &mut self,
            position: Vec2,
            heading: f64,
            _speed: f64,
            _next_checkpoint: usize,
        ) -> Result<(), String> {
            self.render_world_pose(
                Vec3::new(position.x as f32, position.y as f32, 0.0),
                heading as f32,
            )
            .map(|_| ())
        }

        /// Render at an explicit world pose, suitable for a validated
        /// simulation-to-asset transform supplied by a content importer.
        pub fn render_world_pose(&mut self, position: Vec3, heading: f32) -> Result<f32, String> {
            self.render_world_vehicles(position, heading, &[(position, heading)], Some(0))
        }

        /// Render up to the protocol's eight authoritative vehicles. Each
        /// uses the designated local-space car mesh; static track and prop
        /// instances are retained exactly once.
        pub fn render_world_vehicles(
            &mut self,
            fallback_camera_position: Vec3,
            fallback_camera_heading: f32,
            poses: &[(Vec3, f32)],
            local_vehicle_index: Option<usize>,
        ) -> Result<f32, String> {
            self.render_world_vehicles_with_surface(
                fallback_camera_position,
                fallback_camera_heading,
                poses,
                local_vehicle_index,
                true,
            )
        }

        /// Render supplied vehicle heights without renderer-side surface
        /// snapping when the local simulation already owns grounded Z.
        pub fn render_world_vehicles_with_surface(
            &mut self,
            fallback_camera_position: Vec3,
            fallback_camera_heading: f32,
            poses: &[(Vec3, f32)],
            local_vehicle_index: Option<usize>,
            sample_track_surface: bool,
        ) -> Result<f32, String> {
            self.render_world_vehicles_with_attitude(
                fallback_camera_position,
                fallback_camera_heading,
                poses,
                local_vehicle_index,
                sample_track_surface,
                None,
            )
        }

        /// Apply a selected support normal to the local preview vehicle only.
        /// Remote/server poses remain planar and use the world-up default.
        pub fn render_world_vehicles_with_attitude(
            &mut self,
            fallback_camera_position: Vec3,
            fallback_camera_heading: f32,
            poses: &[(Vec3, f32)],
            local_vehicle_index: Option<usize>,
            sample_track_surface: bool,
            local_ground_normal: Option<Vec3>,
        ) -> Result<f32, String> {
            let mut profiled_poses = poses
                .iter()
                .map(|(position, heading)| (*position, *heading, Vec3::UP))
                .collect::<Vec<_>>();
            if let (Some(index), Some(normal)) = (local_vehicle_index, local_ground_normal) {
                let Some(pose) = profiled_poses.get_mut(index) else {
                    return Err("local render vehicle index is outside the pose list".into());
                };
                pose.2 = normal;
            }
            let templates = vec![self.vehicle_template; poses.len()];
            self.render_world_vehicles_with_templates(
                fallback_camera_position,
                fallback_camera_heading,
                &profiled_poses,
                &templates,
                local_vehicle_index,
                sample_track_surface,
            )
        }

        /// Render poses with exact locally registered profile identities. Unknown
        /// or mismatched source fingerprints fail closed instead of borrowing a
        /// different participant's template.
        pub fn render_world_vehicles_with_profiles(
            &mut self,
            fallback_camera_position: Vec3,
            fallback_camera_heading: f32,
            poses: &[(Vec3, f32, Vec3)],
            profiles: &[VehicleProfileIdentity],
            local_vehicle_index: Option<usize>,
        ) -> Result<f32, String> {
            self.render_world_vehicles_with_profiles_surface(
                fallback_camera_position,
                fallback_camera_heading,
                poses,
                profiles,
                local_vehicle_index,
                false,
            )
        }

        /// Like [`Self::render_world_vehicles_with_profiles`], optionally
        /// resting every vehicle on the loaded track surface (project scenes,
        /// whose poses carry no height).
        pub fn render_world_vehicles_with_profiles_surface(
            &mut self,
            fallback_camera_position: Vec3,
            fallback_camera_heading: f32,
            poses: &[(Vec3, f32, Vec3)],
            profiles: &[VehicleProfileIdentity],
            local_vehicle_index: Option<usize>,
            sample_track_surface: bool,
        ) -> Result<f32, String> {
            if profiles.len() != poses.len() {
                return Err("vehicle profile count does not match the pose list".into());
            }
            let mut templates = Vec::with_capacity(profiles.len());
            for profile in profiles {
                let profile = VehicleProfileIdentity::new(
                    &profile.vehicle_profile_id,
                    &profile.vehicle_source_fingerprint,
                )?;
                templates.push(
                    *self
                        .vehicle_profile_templates
                        .get(&profile)
                        .ok_or_else(|| {
                            "vehicle render profile is not registered locally".to_string()
                        })?,
                );
            }
            self.render_world_vehicles_with_templates(
                fallback_camera_position,
                fallback_camera_heading,
                poses,
                &templates,
                local_vehicle_index,
                sample_track_surface,
            )
        }

        fn render_world_vehicles_with_templates(
            &mut self,
            fallback_camera_position: Vec3,
            fallback_camera_heading: f32,
            poses: &[(Vec3, f32, Vec3)],
            templates: &[MeshInstance],
            local_vehicle_index: Option<usize>,
            sample_track_surface: bool,
        ) -> Result<f32, String> {
            if poses.len() > MAX_RENDER_VEHICLES {
                return Err("render vehicle count exceeds the protocol limit".into());
            }
            if templates.len() != poses.len() {
                return Err("vehicle profile count does not match the pose list".into());
            }
            let fallback_camera = (fallback_camera_position, fallback_camera_heading);
            let camera_input_poses = poses
                .iter()
                .map(|(position, heading, _)| (*position, *heading))
                .collect::<Vec<_>>();
            vehicle_camera_pose(&camera_input_poses, local_vehicle_index, fallback_camera)
                .map_err(str::to_string)?;
            if !fallback_camera_position.x.is_finite()
                || !fallback_camera_position.y.is_finite()
                || !fallback_camera_position.z.is_finite()
                || !fallback_camera_heading.is_finite()
                || poses.iter().any(|(position, heading, normal)| {
                    !position.x.is_finite()
                        || !position.y.is_finite()
                        || !position.z.is_finite()
                        || !heading.is_finite()
                        || !normal.x.is_finite()
                        || !normal.y.is_finite()
                        || !normal.z.is_finite()
                        || normal.length() <= 1.0e-6
                        || normal.z <= 0.0
                })
            {
                return Err(
                    "render vehicle pose contains an invalid ground normal or non-finite value"
                        .into(),
                );
            }

            self.track_surface_states
                .resize_with(poses.len(), TrackSurfaceState::default);
            let mut sampled_poses = Vec::with_capacity(poses.len());
            for (index, (mut position, heading, normal)) in poses.iter().copied().enumerate() {
                if sample_track_surface {
                    if let Some(track_surface) = &self.track_surface {
                        let state = &mut self.track_surface_states[index];
                        let previous_ground =
                            state.previous_ground_height(position.z, self.vehicle_min_z_scaled);
                        let lookup = track_surface.sample(position.x, position.y, previous_ground);
                        position.z =
                            state.apply_lookup(lookup, position.z, self.vehicle_min_z_scaled);
                    }
                }
                sampled_poses.push((position, heading, normal.normalized()));
            }
            let hidden_local = (self.preview_look_behind
                || self.preview_camera_mode == PreviewCameraMode::Road)
                .then_some(local_vehicle_index)
                .flatten();
            let car_instances =
                compose_profiled_render_instances(&[], templates, &sampled_poses, hidden_local)
                    .map_err(str::to_string)?;
            let scene = std::mem::take(&mut self.scene);
            let camera_input_poses = sampled_poses
                .iter()
                .map(|(position, heading, _)| (*position, *heading))
                .collect::<Vec<_>>();
            let (camera_position, camera_heading) =
                vehicle_camera_pose(&camera_input_poses, local_vehicle_index, fallback_camera)
                    .map_err(str::to_string)?;
            self.pending_mirror = if let Some(local_index) =
                active_mirror_local_index(self.preview_mirror_visible, local_vehicle_index)
            {
                let remote_poses = sampled_poses
                    .iter()
                    .copied()
                    .zip(templates.iter().copied())
                    .enumerate()
                    .filter_map(|(index, ((position, heading, normal), template))| {
                        (index != local_index).then_some(MeshInstance {
                            translation: position,
                            heading,
                            ground_normal: normal,
                            ..template
                        })
                    })
                    .collect();
                let camera_up = sampled_poses[local_index].2;
                Some(PendingMirror {
                    camera: Camera3d::rear_mirror_world(camera_position, camera_heading, camera_up),
                    up: camera_up,
                    remote_instances: remote_poses,
                })
            } else {
                None
            };
            let view_heading = if self.preview_look_behind
                || self.preview_camera_mode != PreviewCameraMode::Road
            {
                self.chase_camera_heading.unwrap_or(camera_heading)
            } else {
                camera_heading
            };
            let result = self.render_cached_scene(
                &scene,
                if self.preview_look_behind {
                    Camera3d::look_behind_world(camera_position, view_heading)
                } else {
                    Camera3d::preview_world(camera_position, view_heading, self.preview_camera_mode)
                },
                &car_instances,
            );
            self.scene = scene;
            result.map(|()| camera_position.z)
        }

        /// Explicit GPU-only depth regression scene for acceptance tooling.
        /// The far blue plane is submitted after a near red plane; correct
        /// depth testing leaves red visible in their overlap.
        pub fn render_depth_order_fixture(&mut self) -> Result<(), String> {
            let scene = depth_order_fixture();
            self.render_scene(&scene, depth_order_camera())
        }

        /// Explicit GPU fixture proving that separate material slots sample
        /// separate layers of the scene's RGBA texture array.
        pub fn render_texture_array_fixture(&mut self) -> Result<(), String> {
            let scene = texture_array_fixture();
            self.render_scene(&scene, depth_order_camera())
        }

        /// Explicit cutout fixture; transparent texels must reveal the
        /// farther surface and leave its depth value intact.
        pub fn render_alpha_cutout_fixture(&mut self) -> Result<(), String> {
            let scene = alpha_cutout_fixture();
            self.render_scene(&scene, depth_order_camera())
        }

        /// Render project-owned partially transparent quads. The fixture
        /// reverses source order to verify camera-space sorting and can add an
        /// opaque depth occluder between the two transparent surfaces.
        pub fn render_straight_alpha_fixture(
            &mut self,
            width: u32,
            height: u32,
            reverse_order: bool,
            opaque_occluder: bool,
        ) -> Result<(), String> {
            self.resize(width, height);
            self.texture_cache = RgbaTextureArray::default();
            self.render_scene(
                &straight_alpha_fixture(reverse_order, opaque_occluder),
                depth_order_camera(),
            )
        }

        /// Project-owned synthetic camera-centered panels exercise background
        /// translation invariance, yaw selection, and foreground occlusion.
        pub fn render_background_fixture(
            &mut self,
            width: u32,
            height: u32,
            yaw: f32,
            camera_x: f32,
            camera_y: f32,
            foreground: bool,
            enabled: bool,
        ) -> Result<(), String> {
            self.resize(width, height);
            let scene = background_fixture(foreground, enabled);
            let position = Vec3::new(camera_x, camera_y, 0.0);
            let forward = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
            self.render_scene(
                &scene,
                Camera3d {
                    position,
                    target: position + forward,
                    vertical_fov_radians: 0.9,
                    near: 0.1,
                    far: 5.0,
                },
            )
        }

        /// Render a fully enclosed project-owned synthetic horizon to verify
        /// pitch coverage and camera-translation invariance of the background pass.
        pub fn render_horizon_fixture(
            &mut self,
            width: u32,
            height: u32,
            yaw: f32,
            pitch: f32,
            camera_x: f32,
            camera_y: f32,
        ) -> Result<(), String> {
            self.resize(width, height);
            let scene = closed_horizon_fixture();
            let position = Vec3::new(camera_x, camera_y, 0.0);
            let horizontal = pitch.cos();
            let forward = Vec3::new(yaw.cos() * horizontal, yaw.sin() * horizontal, pitch.sin());
            self.render_scene(
                &scene,
                Camera3d {
                    position,
                    target: position + forward,
                    vertical_fov_radians: 0.9,
                    near: 0.1,
                    far: 100.0,
                },
            )
        }

        /// Render a project-owned asymmetric mirror fixture for browser pixel
        /// checks. The front and rear panels differ; `occluder` inserts a
        /// nearer opaque panel, and `reverse_alpha` reverses two rear layers.
        pub fn render_rear_mirror_fixture(
            &mut self,
            width: u32,
            height: u32,
            up_x: f32,
            up_y: f32,
            occluder: bool,
            transparent: bool,
            reverse_alpha: bool,
        ) -> Result<(), String> {
            if !up_x.is_finite() || !up_y.is_finite() || up_x * up_x + up_y * up_y > 0.64 {
                return Err("rear-view fixture up axis is invalid".into());
            }
            self.resize(width, height);
            let up = Vec3::new(up_x, up_y, (1.0 - up_x * up_x - up_y * up_y).sqrt());
            let scene = rear_mirror_fixture(occluder, transparent, reverse_alpha);
            let mirror_instances = scene
                .instances
                .iter()
                .enumerate()
                .filter_map(|(index, instance)| (index != 3).then_some(*instance))
                .collect::<Vec<_>>();
            let vehicle_template = *scene
                .instances
                .get(3)
                .ok_or_else(|| "rear-view fixture has no local vehicle".to_string())?;
            let fixture_geometry = create_mirror_geometry_cache(
                &self.device,
                &self.queue,
                &scene,
                &mirror_instances,
                &[vehicle_template],
            )?;
            let fixture_scratch = MirrorFrameScratch::for_geometry(&fixture_geometry);
            let previous_geometry = std::mem::replace(&mut self.mirror_geometry, fixture_geometry);
            let previous_scratch = std::mem::replace(&mut self.mirror_scratch, fixture_scratch);
            self.pending_mirror = Some(PendingMirror {
                camera: Camera3d::rear_mirror_world(Vec3::ZERO, 0.0, up),
                up,
                remote_instances: Vec::new(),
            });
            let result = self.render_scene(
                &scene,
                Camera3d {
                    position: Vec3::new(-3.0, 0.0, 3.2),
                    target: Vec3::new(20.0, 0.0, 3.2),
                    vertical_fov_radians: 0.82,
                    near: 0.1,
                    far: 100.0,
                },
            );
            self.mirror_geometry = previous_geometry;
            self.mirror_scratch = previous_scratch;
            result
        }

        /// Synthetic browser-only seam for cache lifecycle checks. Stage 0
        /// installs a scene, stages 1/2 reject bounded invalid replacements,
        /// and stage 3 installs a smaller scene to expose stale cache content.
        pub fn render_mirror_cache_lifecycle_fixture(
            &mut self,
            width: u32,
            height: u32,
            stage: u8,
        ) -> Result<(), String> {
            self.resize(width, height);
            match stage {
                0 => {
                    let mut scene = rear_mirror_fixture(false, false, false);
                    let track_mesh = add_mirror_cache_track(&mut scene);
                    self.set_scene(scene, 3, &[track_mesh])?;
                }
                1 => {
                    let error = self
                        .set_scene(oversized_mirror_cache_fixture(), 0, &[0])
                        .err()
                        .ok_or_else(|| "oversized mirror cache fixture was accepted".to_string())?;
                    if !error.contains("exceeds supported bounds") {
                        return Err(format!("unexpected oversized-cache rejection: {error}"));
                    }
                }
                2 => {
                    let mut scene = nonfinite_mirror_transform_fixture();
                    let track_mesh = add_mirror_cache_track(&mut scene);
                    let error = self
                        .set_scene(scene, 3, &[track_mesh])
                        .err()
                        .ok_or_else(|| {
                            "overflowing mirror transform fixture was accepted".to_string()
                        })?;
                    if !error.contains("non-finite geometry") {
                        return Err(format!(
                            "unexpected non-finite-transform rejection: {error}"
                        ));
                    }
                }
                3 => {
                    let mut scene = rear_mirror_fixture(false, false, false);
                    let local_vehicle = scene.instances[3];
                    scene.instances = vec![local_vehicle];
                    let track_mesh = add_mirror_cache_track(&mut scene);
                    self.set_scene(scene, 0, &[track_mesh])?;
                }
                _ => return Err("unknown mirror cache lifecycle stage".into()),
            }
            self.set_preview_camera_mode(PreviewCameraMode::Chase);
            self.set_preview_look_behind(false);
            self.render_world_vehicles_with_surface(
                Vec3::ZERO,
                0.0,
                &[(Vec3::ZERO, 0.0)],
                Some(0),
                false,
            )?;
            Ok(())
        }

        fn render_scene(&mut self, scene: &Scene3d, camera: Camera3d) -> Result<(), String> {
            let mirror = self.pending_mirror.take();
            // This generic path may rebind textures for a fixture scene.
            self.cached_scene_textures_current = false;
            if scene.materials.is_empty() || !scene.textures.is_well_formed() {
                return Err("3D scene has no materials or malformed RGBA8 textures".into());
            }
            let limits = self.device.limits();
            let storage_layers = texture_array_storage_layers(
                scene.textures.width,
                scene.textures.height,
                scene.textures.layers.len(),
                limits.max_texture_array_layers,
            );
            if scene.textures.width > limits.max_texture_dimension_2d
                || scene.textures.height > limits.max_texture_dimension_2d
                || storage_layers.map_or(true, |layers| layers > limits.max_texture_array_layers)
            {
                return Err(format!(
                    "3D texture array {}x{}x{} exceeds device limit {}x{}x{}",
                    scene.textures.width,
                    scene.textures.height,
                    scene.textures.layers.len(),
                    limits.max_texture_dimension_2d,
                    limits.max_texture_dimension_2d,
                    limits.max_texture_array_layers
                ));
            }
            if scene.materials.iter().any(|material| {
                material.texture >= scene.textures.layers.len()
                    || !material.frames_reference_layers(scene.textures.layers.len())
                    || !material.atlas_uv_is_well_formed()
                    || material.tint.iter().any(|value| !value.is_finite())
            }) {
                return Err("3D scene has malformed material texture coordinates".into());
            }
            for mesh in &scene.meshes {
                if !mesh.is_well_formed()
                    || mesh.vertices.iter().any(|vertex| {
                        vertex.material as usize >= scene.materials.len()
                            || scene.materials[vertex.material as usize].texture
                                >= scene.textures.layers.len()
                    })
                {
                    return Err("3D scene has malformed indices or material references".into());
                }
            }
            if scene.instances.iter().any(|instance| {
                instance.mesh_index as usize >= scene.meshes.len()
                    || !instance.translation.x.is_finite()
                    || !instance.translation.y.is_finite()
                    || !instance.translation.z.is_finite()
                    || !instance.heading.is_finite()
                    || !instance.scale.is_finite()
                    || instance.scale <= 0.0
            }) {
                return Err("3D scene has a malformed mesh instance".into());
            }
            let bg_extent = validate_background_meshes(scene, &[])?;
            if self.texture_cache != scene.textures {
                let (texture, bind_group) = create_texture_binding(
                    &self.device,
                    &self.queue,
                    &self.bind_group_layout,
                    &scene.textures,
                    storage_layers.expect("texture array storage was validated"),
                );
                self.material_texture = texture;
                self.bind_group = bind_group;
                self.texture_cache = scene.textures.clone();
            }
            let frame = match self.surface.get_current_texture() {
                Ok(frame) => frame,
                Err(SurfaceError::Lost | SurfaceError::Outdated) => {
                    self.surface.configure(&self.device, &self.config);
                    return Ok(());
                }
                Err(SurfaceError::Timeout) => return Ok(()),
                Err(e) => return Err(e.to_string()),
            };
            let aspect = self.config.width as f32 / self.config.height.max(1) as f32;
            let mut bg_verts = Vec::new();
            let mut bg_indices = Vec::new();
            let forward = (camera.target - camera.position).normalized();
            let background_camera = Camera3d {
                position: Vec3::new(0.0, 0.0, 0.0),
                target: forward,
                near: 0.001,
                far: (bg_extent * 2.0 + 0.01).max(1.0),
                ..camera
            };
            for &mesh_index in &scene.background_mesh_indices {
                let mesh = &scene.meshes[mesh_index as usize];
                let base = bg_verts.len() as u32;
                for v in &mesh.vertices {
                    let mat = &scene.materials[v.material as usize];
                    bg_verts.push(GpuVertex {
                        clip: [0.0; 4],
                        normal: [0.0, 0.0, 1.0],
                        uv: v.uv,
                        tint: mat.tint,
                        texture_layer: mat.texture as u32,
                        uv_scale: mat.uv_scale,
                        uv_offset: mat.uv_offset,
                        uv_wrap: [
                            match mat.wrap_u {
                                UvWrap::Clamp => 0,
                                UvWrap::Repeat => 1,
                            },
                            match mat.wrap_v {
                                UvWrap::Clamp => 0,
                                UvWrap::Repeat => 1,
                            },
                        ],
                    });
                }
                bg_indices.extend(mesh.indices.iter().map(|i| base + *i));
            }
            if !bg_verts.is_empty() {
                for (vertex, source) in bg_verts.iter_mut().zip(
                    scene
                        .background_mesh_indices
                        .iter()
                        .flat_map(|i| scene.meshes[*i as usize].vertices.iter()),
                ) {
                    vertex.clip = background_camera.project(source.position, aspect);
                }
            }
            let animation_tick = (js_sys::Date::now() / 1000.0
                * MATERIAL_ANIMATION_TICKS_PER_SECOND)
                .max(0.0) as u64;
            let mut verts = Vec::new();
            let mut indices = Vec::new();
            let mut transparent_triangles = Vec::new();
            let mut source_order = 0usize;
            for instance in &scene.instances {
                let mesh = &scene.meshes[instance.mesh_index as usize];
                let base = verts.len() as u32;
                for v in &mesh.vertices {
                    let mat = scene
                        .materials
                        .get(v.material as usize)
                        .unwrap_or(&scene.materials[0]);
                    let frame = mat.frame_at(animation_tick);
                    verts.push(GpuVertex {
                        clip: camera.project(instance.transform_point(v.position), aspect),
                        normal: {
                            let n = instance.transform_normal(v.normal);
                            [n.x, n.y, n.z]
                        },
                        uv: v.uv,
                        tint: mat.tint,
                        texture_layer: frame.texture as u32,
                        uv_scale: frame.uv_scale,
                        uv_offset: frame.uv_offset,
                        uv_wrap: [
                            match mat.wrap_u {
                                UvWrap::Clamp => 0,
                                UvWrap::Repeat => 1,
                            },
                            match mat.wrap_v {
                                UvWrap::Clamp => 0,
                                UvWrap::Repeat => 1,
                            },
                        ],
                    });
                }
                for triangle in mesh.indices.chunks_exact(3) {
                    let triangle: [u32; 3] = triangle
                        .try_into()
                        .expect("validated triangle index chunks have three entries");
                    let materials = triangle.map(|index| {
                        scene.materials[mesh.vertices[index as usize].material as usize].alpha_mode
                    });
                    let transparent_count = materials
                        .iter()
                        .filter(|mode| **mode == MaterialAlphaMode::StraightAlpha)
                        .count();
                    if transparent_count != 0 && transparent_count != 3 {
                        return Err(
                            "a triangle cannot mix opaque and straight-alpha materials".into()
                        );
                    }
                    if transparent_count == 3 {
                        let world = triangle.map(|index| {
                            instance.transform_point(mesh.vertices[index as usize].position)
                        });
                        let depth = world
                            .iter()
                            .map(|point| (*point - camera.position).dot(forward))
                            .sum::<f32>()
                            / 3.0;
                        if !depth.is_finite() {
                            return Err("straight-alpha triangle depth is non-finite".into());
                        }
                        transparent_triangles.push((
                            depth,
                            source_order,
                            triangle.map(|index| base + index),
                        ));
                    } else {
                        indices.extend(triangle.map(|index| base + index));
                    }
                    source_order = source_order.saturating_add(1);
                }
            }
            transparent_triangles.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let transparent_indices = transparent_triangles
                .into_iter()
                .flat_map(|(_, _, triangle)| triangle)
                .collect::<Vec<_>>();
            let vb = (!verts.is_empty()).then(|| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("indexed scene vertices"),
                        contents: bytemuck::cast_slice(&verts),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
            });
            let ib = (!indices.is_empty()).then(|| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("opaque scene indices"),
                        contents: bytemuck::cast_slice(&indices),
                        usage: wgpu::BufferUsages::INDEX,
                    })
            });
            let transparent_ib = (!transparent_indices.is_empty()).then(|| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("sorted straight-alpha scene indices"),
                        contents: bytemuck::cast_slice(&transparent_indices),
                        usage: wgpu::BufferUsages::INDEX,
                    })
            });
            if vb.is_none() && (ib.is_some() || transparent_ib.is_some()) {
                return Err("3D scene indices have no vertex buffer".into());
            }
            let view = frame.texture.create_view(&Default::default());
            let depth_view = self.depth.create_view(&Default::default());
            let mut encoder = self.device.create_command_encoder(&Default::default());
            if !bg_indices.is_empty() {
                let bg_vb = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("camera-centered background vertices"),
                        contents: bytemuck::cast_slice(&bg_verts),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                let bg_ib = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("camera-centered background indices"),
                        contents: bytemuck::cast_slice(&bg_indices),
                        usage: wgpu::BufferUsages::INDEX,
                    });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("camera-centered background meshes"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.48,
                                g: 0.68,
                                b: 0.84,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_pipeline(&self.background_pipeline);
                pass.set_vertex_buffer(0, bg_vb.slice(..));
                pass.set_index_buffer(bg_ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..bg_indices.len() as u32, 0, 0..1);
            }
            if vb.is_some() {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("3D track and car"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: if bg_indices.is_empty() {
                                wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.48,
                                    g: 0.68,
                                    b: 0.84,
                                    a: 1.0,
                                })
                            } else {
                                wgpu::LoadOp::Load
                            },
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(
                    0,
                    vb.as_ref().expect("foreground vertices checked").slice(..),
                );
                if let Some(ib) = &ib {
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.set_pipeline(&self.pipeline);
                    pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
                }
                if let Some(ib) = &transparent_ib {
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.set_pipeline(&self.straight_alpha_pipeline);
                    pass.draw_indexed(0..transparent_indices.len() as u32, 0, 0..1);
                }
            } else if bg_indices.is_empty() {
                let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("clear-only scene frame"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.48,
                                g: 0.68,
                                b: 0.84,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            }
            if let Some(mirror) = mirror {
                let mirror_color = self
                    .mirror_target
                    .color
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mirror_depth = self
                    .mirror_target
                    .depth
                    .create_view(&wgpu::TextureViewDescriptor::default());
                self.encode_mirror_scene_view(
                    scene,
                    &mirror.remote_instances,
                    MirrorView {
                        camera: mirror.camera,
                        up: mirror.up,
                    },
                    &mut encoder,
                    &mirror_color,
                    &mirror_depth,
                )?;
                self.encode_mirror_composite(&mut encoder, &view);
            }
            self.queue.submit(Some(encoder.finish()));
            frame.present();
            Ok(())
        }

        fn encode_mirror_scene_view(
            &mut self,
            scene: &Scene3d,
            remote_instances: &[MeshInstance],
            view: MirrorView,
            encoder: &mut wgpu::CommandEncoder,
            color_view: &wgpu::TextureView,
            depth_view: &wgpu::TextureView,
        ) -> Result<(), String> {
            if remote_instances.len() > MAX_RENDER_VEHICLES.saturating_sub(1) {
                return Err("rear-view remote vehicle count exceeds its cache budget".into());
            }
            let aspect = self.mirror_target.width as f32 / self.mirror_target.height.max(1) as f32;
            let projection = CameraFrameProjection::new(view.camera, aspect, view.up);
            let background_camera = Camera3d {
                position: Vec3::ZERO,
                target: projection.forward,
                near: 0.001,
                far: (self.mirror_geometry.background_extent * 2.0 + 0.01).max(1.0),
                ..view.camera
            };
            let scene_uniform = with_fog(
                mirror_camera_uniform(view.camera, aspect, view.up),
                self.fog,
            );
            let background_uniform = mirror_camera_uniform(background_camera, aspect, view.up);
            self.queue.write_buffer(
                &self.mirror_scene_camera_buffer,
                0,
                bytemuck::bytes_of(&scene_uniform),
            );
            self.queue.write_buffer(
                &self.mirror_background_camera_buffer,
                0,
                bytemuck::bytes_of(&background_uniform),
            );
            build_dynamic_frame(
                scene,
                &self.mirror_geometry,
                &mut self.mirror_scratch,
                remote_instances,
                view.camera.position,
                projection.forward,
                None,
            )?;
            upload_dynamic_frame(&self.queue, &self.mirror_geometry, &self.mirror_scratch);
            encode_cached_scene(
                encoder,
                &self.mirror_geometry,
                &self.mirror_scratch,
                &self.bind_group,
                &self.mirror_scene_camera_bind_group,
                &self.mirror_background_camera_bind_group,
                CachedPipelines {
                    opaque: &self.mirror_opaque_pipeline,
                    alpha: &self.mirror_alpha_pipeline,
                    background: &self.mirror_background_pipeline,
                },
                color_view,
                depth_view,
            );
            Ok(())
        }

        /// Validate the scene's textures against device limits and (re)bind the
        /// texture array when it differs from what the GPU currently holds.
        fn bind_scene_textures(&mut self, scene: &Scene3d) -> Result<(), String> {
            if scene.materials.is_empty() || !scene.textures.is_well_formed() {
                return Err("3D scene has no materials or malformed RGBA8 textures".into());
            }
            let limits = self.device.limits();
            let storage_layers = texture_array_storage_layers(
                scene.textures.width,
                scene.textures.height,
                scene.textures.layers.len(),
                limits.max_texture_array_layers,
            );
            if scene.textures.width > limits.max_texture_dimension_2d
                || scene.textures.height > limits.max_texture_dimension_2d
                || storage_layers.map_or(true, |layers| layers > limits.max_texture_array_layers)
            {
                return Err(format!(
                    "3D texture array {}x{}x{} exceeds device limit {}x{}x{}",
                    scene.textures.width,
                    scene.textures.height,
                    scene.textures.layers.len(),
                    limits.max_texture_dimension_2d,
                    limits.max_texture_dimension_2d,
                    limits.max_texture_array_layers
                ));
            }
            if self.texture_cache != scene.textures {
                let (texture, bind_group) = create_texture_binding(
                    &self.device,
                    &self.queue,
                    &self.bind_group_layout,
                    &scene.textures,
                    storage_layers.expect("texture array storage was validated"),
                );
                self.material_texture = texture;
                self.bind_group = bind_group;
                self.texture_cache = scene.textures.clone();
            }
            Ok(())
        }

        /// Draw the installed scene from its once-uploaded GPU buffers. Only the
        /// vehicles, animated-material patches, the sorted transparent index
        /// list and the camera uniforms change per frame.
        fn render_cached_scene(
            &mut self,
            scene: &Scene3d,
            camera: Camera3d,
            car_instances: &[MeshInstance],
        ) -> Result<(), String> {
            let mirror = self.pending_mirror.take();
            if !self.cached_scene_textures_current {
                self.bind_scene_textures(scene)?;
                self.cached_scene_textures_current = true;
            }
            let frame = match self.surface.get_current_texture() {
                Ok(frame) => frame,
                Err(SurfaceError::Lost | SurfaceError::Outdated) => {
                    self.surface.configure(&self.device, &self.config);
                    return Ok(());
                }
                Err(SurfaceError::Timeout) => return Ok(()),
                Err(e) => return Err(e.to_string()),
            };
            let aspect = self.config.width as f32 / self.config.height.max(1) as f32;
            let forward = (camera.target - camera.position).normalized();
            let background_camera = Camera3d {
                position: Vec3::ZERO,
                target: forward,
                near: 0.001,
                far: (self.main_geometry.background_extent * 2.0 + 0.01).max(1.0),
                ..camera
            };
            self.queue.write_buffer(
                &self.main_scene_camera_buffer,
                0,
                bytemuck::bytes_of(&with_fog(
                    mirror_camera_uniform(camera, aspect, Vec3::UP),
                    self.fog,
                )),
            );
            self.queue.write_buffer(
                &self.main_background_camera_buffer,
                0,
                bytemuck::bytes_of(&mirror_camera_uniform(background_camera, aspect, Vec3::UP)),
            );
            let tick = (js_sys::Date::now() / 1000.0 * MATERIAL_ANIMATION_TICKS_PER_SECOND).max(0.0)
                as u64;
            sync_animated_materials(&self.queue, &mut self.main_geometry, scene, tick);
            build_dynamic_frame(
                scene,
                &self.main_geometry,
                &mut self.main_scratch,
                car_instances,
                camera.position,
                forward,
                Some(tick),
            )?;
            upload_dynamic_frame(&self.queue, &self.main_geometry, &self.main_scratch);
            let view = frame.texture.create_view(&Default::default());
            let depth_view = self.depth.create_view(&Default::default());
            let mut encoder = self.device.create_command_encoder(&Default::default());
            encode_cached_scene(
                &mut encoder,
                &self.main_geometry,
                &self.main_scratch,
                &self.bind_group,
                &self.main_scene_camera_bind_group,
                &self.main_background_camera_bind_group,
                CachedPipelines {
                    opaque: &self.mirror_opaque_pipeline,
                    alpha: &self.mirror_alpha_pipeline,
                    background: &self.mirror_background_pipeline,
                },
                &view,
                &depth_view,
            );
            if let Some(mirror) = mirror {
                let mirror_color = self
                    .mirror_target
                    .color
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mirror_depth = self
                    .mirror_target
                    .depth
                    .create_view(&wgpu::TextureViewDescriptor::default());
                self.encode_mirror_scene_view(
                    scene,
                    &mirror.remote_instances,
                    MirrorView {
                        camera: mirror.camera,
                        up: mirror.up,
                    },
                    &mut encoder,
                    &mirror_color,
                    &mirror_depth,
                )?;
                self.encode_mirror_composite(&mut encoder, &view);
            }
            self.queue.submit(Some(encoder.finish()));
            frame.present();
            Ok(())
        }

        fn encode_mirror_composite(
            &self,
            encoder: &mut wgpu::CommandEncoder,
            surface_view: &wgpu::TextureView,
        ) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composite rear-view inset"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.mirror_target.bind_group, &[]);
            pass.set_vertex_buffer(0, self.mirror_frame_vertices.slice(..));
            pass.set_pipeline(&self.mirror_frame_pipeline);
            pass.draw(0..6, 0..1);
            pass.set_vertex_buffer(0, self.mirror_image_vertices.slice(..));
            pass.set_pipeline(&self.mirror_image_pipeline);
            pass.draw(0..6, 0..1);
        }
    }

    fn depth_order_camera() -> Camera3d {
        Camera3d {
            position: Vec3::new(0.0, -6.0, 4.0),
            target: Vec3::new(0.0, 0.0, 0.0),
            vertical_fov_radians: 0.9,
            near: 0.1,
            far: 100.0,
        }
    }

    fn rear_mirror_fixture(occluder: bool, transparent: bool, reverse_alpha: bool) -> Scene3d {
        let mut rear_far = Material::whole_layer(
            "synthetic rear far transparent",
            [1.0, 0.05, 0.05, 0.55],
            0,
            UvWrap::Clamp,
        );
        rear_far.alpha_mode = MaterialAlphaMode::StraightAlpha;
        let mut rear_near = Material::whole_layer(
            "synthetic rear near transparent",
            [0.05, 1.0, 0.05, 0.55],
            0,
            UvWrap::Clamp,
        );
        rear_near.alpha_mode = MaterialAlphaMode::StraightAlpha;
        let materials = vec![
            Material::whole_layer(
                "synthetic front green",
                [0.05, 1.0, 0.05, 1.0],
                0,
                UvWrap::Clamp,
            ),
            Material::whole_layer(
                "synthetic rear blue",
                [0.05, 0.05, 1.0, 1.0],
                0,
                UvWrap::Clamp,
            ),
            Material::whole_layer(
                "synthetic local vehicle",
                [1.0, 0.0, 1.0, 1.0],
                0,
                UvWrap::Clamp,
            ),
            Material::whole_layer(
                "synthetic rear occluder",
                [1.0, 0.85, 0.05, 1.0],
                0,
                UvWrap::Clamp,
            ),
            rear_far,
            rear_near,
            Material::whole_layer(
                "synthetic asymmetric rear right",
                [1.0, 0.05, 0.05, 1.0],
                0,
                UvWrap::Clamp,
            ),
            Material::whole_layer(
                "synthetic other vehicle",
                [1.0, 0.55, 0.05, 1.0],
                0,
                UvWrap::Clamp,
            ),
        ];
        let make_panel = |x: f32, half_width: f32, z_min: f32, z_max: f32, material: u32| {
            let mut mesh = IndexedMesh::default();
            mesh.quad(
                [
                    Vec3::new(x, -half_width, z_min),
                    Vec3::new(x, half_width, z_min),
                    Vec3::new(x, half_width, z_max),
                    Vec3::new(x, -half_width, z_max),
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                material,
            );
            mesh
        };
        let mut meshes = vec![
            make_panel(18.0, 12.0, -8.0, 10.0, 0),
            make_panel(-32.0, 10.0, -8.0, 10.0, 1),
            make_panel(-32.0, 10.0, -8.0, 10.0, 6),
            make_panel(0.0, 2.2, -0.6, 2.6, 2),
            make_panel(-9.0, 1.1, -0.4, 3.4, 7),
        ];
        let mut instances = (0..5)
            .map(|mesh_index| MeshInstance {
                mesh_index: match mesh_index {
                    1 => 1,
                    2 => 2,
                    _ => mesh_index,
                },
                translation: match mesh_index {
                    1 => Vec3::new(0.0, -10.0, 0.0),
                    2 => Vec3::new(0.0, 10.0, 0.0),
                    4 => Vec3::new(0.0, 3.0, 0.0),
                    _ => Vec3::ZERO,
                },
                heading: 0.0,
                ground_normal: Vec3::UP,
                scale: 1.0,
            })
            .collect::<Vec<_>>();
        if transparent {
            meshes.extend([
                make_panel(-20.0, 10.0, -8.0, 10.0, 4),
                make_panel(-10.0, 10.0, -8.0, 10.0, 5),
            ]);
            let far_alpha_instance = MeshInstance {
                mesh_index: 5,
                translation: Vec3::ZERO,
                heading: 0.0,
                ground_normal: Vec3::UP,
                scale: 1.0,
            };
            let near_alpha_instance = MeshInstance {
                mesh_index: 6,
                ..far_alpha_instance
            };
            if reverse_alpha {
                instances.extend([near_alpha_instance, far_alpha_instance]);
            } else {
                instances.extend([far_alpha_instance, near_alpha_instance]);
            }
        }
        if occluder {
            meshes.push(make_panel(-8.0, 3.0, -0.5, 2.3, 3));
            instances.push(MeshInstance {
                mesh_index: if transparent { 7 } else { 5 },
                translation: Vec3::ZERO,
                heading: 0.0,
                ground_normal: Vec3::UP,
                scale: 1.0,
            });
        }
        Scene3d {
            meshes,
            instances,
            materials,
            textures: synthetic_textures(),
            ..Scene3d::default()
        }
    }

    fn oversized_mirror_cache_fixture() -> Scene3d {
        const REPEATED_VERTICES: usize = 4_000;
        const INSTANCE_COUNT: usize = 256;
        let mut track = IndexedMesh::default();
        track.quad(
            [
                Vec3::new(-20.0, -20.0, -1000.0),
                Vec3::new(20.0, -20.0, -1000.0),
                Vec3::new(20.0, 20.0, -1000.0),
                Vec3::new(-20.0, 20.0, -1000.0),
            ],
            [[0.0; 2]; 4],
            0,
        );
        let mut oversized = IndexedMesh::default();
        let vertex = MeshVertex {
            position: Vec3::ZERO,
            normal: Vec3::UP,
            uv: [0.0, 0.0],
            material: 0,
        };
        oversized.vertices = vec![vertex; REPEATED_VERTICES];
        oversized.indices = vec![0, 1, 2];
        let mut vehicle = IndexedMesh::default();
        vehicle.triangle(vertex, vertex, vertex);
        let vehicle_instance = MeshInstance {
            mesh_index: 2,
            translation: Vec3::ZERO,
            heading: 0.0,
            ground_normal: Vec3::UP,
            scale: 1.0,
        };
        let mut instances = vec![
            vehicle_instance,
            MeshInstance {
                mesh_index: 0,
                translation: Vec3::ZERO,
                heading: 0.0,
                ground_normal: Vec3::UP,
                scale: 1.0,
            },
        ];
        instances.extend((0..INSTANCE_COUNT).map(|_| MeshInstance {
            mesh_index: 1,
            translation: Vec3::ZERO,
            heading: 0.0,
            ground_normal: Vec3::UP,
            scale: 1.0,
        }));
        Scene3d {
            meshes: vec![track, oversized, vehicle],
            instances,
            materials: vec![Material::whole_layer(
                "oversized mirror fixture",
                [1.0; 4],
                0,
                UvWrap::Clamp,
            )],
            textures: synthetic_textures(),
            ..Scene3d::default()
        }
    }

    fn nonfinite_mirror_transform_fixture() -> Scene3d {
        let mut scene = rear_mirror_fixture(false, false, false);
        let mesh_index = scene.instances[0].mesh_index as usize;
        for vertex in &mut scene.meshes[mesh_index].vertices {
            vertex.position.x = f32::MAX;
        }
        scene.instances[0].translation.x = f32::MAX;
        scene
    }

    fn add_mirror_cache_track(scene: &mut Scene3d) -> u32 {
        let mut track = IndexedMesh::default();
        track.quad(
            [
                Vec3::new(-20.0, -20.0, -1000.0),
                Vec3::new(20.0, -20.0, -1000.0),
                Vec3::new(20.0, 20.0, -1000.0),
                Vec3::new(-20.0, 20.0, -1000.0),
            ],
            [[0.0; 2]; 4],
            0,
        );
        let mesh_index = scene.meshes.len() as u32;
        scene.meshes.push(track);
        scene.instances.push(MeshInstance {
            mesh_index,
            translation: Vec3::ZERO,
            heading: 0.0,
            ground_normal: Vec3::UP,
            scale: 1.0,
        });
        mesh_index
    }

    fn background_fixture(foreground: bool, enabled: bool) -> Scene3d {
        let colors = [
            [1.0, 0.05, 0.05, 1.0],
            [0.05, 1.0, 0.05, 1.0],
            [0.05, 0.05, 1.0, 1.0],
            [1.0, 1.0, 0.05, 1.0],
        ];
        let mut scene = Scene3d {
            materials: colors
                .iter()
                .enumerate()
                .map(|(i, tint)| {
                    Material::whole_layer(
                        format!("synthetic-background-{i}"),
                        *tint,
                        0,
                        UvWrap::Clamp,
                    )
                })
                .collect(),
            textures: synthetic_textures(),
            ..Scene3d::default()
        };
        let dirs = [(1.0_f32, 0.0_f32), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)];
        for (i, dir) in dirs.into_iter().enumerate() {
            let center = Vec3::new(dir.0 * 20.0, dir.1 * 20.0, 0.0);
            let right = Vec3::new(-dir.1, dir.0, 0.0);
            let point = |x: f32, z: f32| center + right * x + Vec3::new(0.0, 0.0, z);
            let mut mesh = IndexedMesh::default();
            mesh.quad(
                [
                    point(-9.0, -20.0),
                    point(9.0, -20.0),
                    point(9.0, 20.0),
                    point(-9.0, 20.0),
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                i as u32,
            );
            scene.meshes.push(mesh);
            if enabled {
                scene.background_mesh_indices.push(i as u32);
            }
        }
        if foreground {
            scene.materials.push(Material::whole_layer(
                "synthetic-foreground",
                [1.0, 0.0, 1.0, 1.0],
                0,
                UvWrap::Clamp,
            ));
            let forward = Vec3::new(1.0, 0.0, 0.0);
            let right = Vec3::new(0.0, 1.0, 0.0);
            let center = forward * 3.0;
            let mut mesh = IndexedMesh::default();
            mesh.quad(
                [
                    center - right * 1.0 - Vec3::UP,
                    center + right * 1.0 - Vec3::UP,
                    center + right * 1.0 + Vec3::UP,
                    center - right * 1.0 + Vec3::UP,
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                4,
            );
            scene.meshes.push(mesh);
            scene.instances.push(MeshInstance {
                mesh_index: 4,
                translation: Vec3::ZERO,
                heading: 0.0,
                ground_normal: Vec3::UP,
                scale: 1.0,
            });
        }
        scene
    }

    fn closed_horizon_fixture() -> Scene3d {
        const SLOTS: usize = 16;
        const RADIUS: f32 = 20.0;
        const BOTTOM: f32 = -2.0;
        const TOP: f32 = 2.0;
        let colors = [
            [1.0, 0.04, 0.03, 1.0],
            [0.04, 1.0, 0.03, 1.0],
            [0.03, 0.04, 1.0, 1.0],
            [1.0, 0.8, 0.03, 1.0],
        ];
        let sky = [1.0, 0.8, 0.03, 1.0];
        let earth = [0.8, 0.03, 0.8, 1.0];
        let mut materials = colors
            .iter()
            .enumerate()
            .map(|(i, tint)| {
                Material::whole_layer(format!("fixture-panel-{i}"), *tint, 0, UvWrap::Clamp)
            })
            .collect::<Vec<_>>();
        materials.push(Material::whole_layer(
            "fixture-sky-fill",
            sky,
            0,
            UvWrap::Clamp,
        ));
        materials.push(Material::whole_layer(
            "fixture-earth-fill",
            earth,
            0,
            UvWrap::Clamp,
        ));
        let mut mesh = IndexedMesh::default();
        let point = |index: usize, z: f32| {
            let angle = std::f32::consts::TAU * (index % SLOTS) as f32 / SLOTS as f32;
            Vec3::new(RADIUS * angle.cos(), RADIUS * angle.sin(), z)
        };
        for slot in 0..SLOTS {
            let next = slot + 1;
            let material = (slot / 4) as u32;
            mesh.quad(
                [
                    point(slot, BOTTOM),
                    point(next, BOTTOM),
                    point(next, TOP),
                    point(slot, TOP),
                ],
                [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                material,
            );
            mesh.quad(
                [
                    point(slot, TOP),
                    point(next, TOP),
                    point(next, RADIUS),
                    point(slot, RADIUS),
                ],
                [[0.0; 2]; 4],
                4,
            );
            mesh.quad(
                [
                    point(slot, -RADIUS),
                    point(next, -RADIUS),
                    point(next, BOTTOM),
                    point(slot, BOTTOM),
                ],
                [[0.0; 2]; 4],
                5,
            );
            mesh.triangle(
                MeshVertex {
                    position: Vec3::new(0.0, 0.0, RADIUS),
                    normal: Vec3::UP,
                    uv: [0.0, 0.0],
                    material: 4,
                },
                MeshVertex {
                    position: point(next, RADIUS),
                    normal: Vec3::UP,
                    uv: [0.0, 0.0],
                    material: 4,
                },
                MeshVertex {
                    position: point(slot, RADIUS),
                    normal: Vec3::UP,
                    uv: [0.0, 0.0],
                    material: 4,
                },
            );
            mesh.triangle(
                MeshVertex {
                    position: Vec3::new(0.0, 0.0, -RADIUS),
                    normal: Vec3::UP,
                    uv: [0.0, 0.0],
                    material: 5,
                },
                MeshVertex {
                    position: point(slot, -RADIUS),
                    normal: Vec3::UP,
                    uv: [0.0, 0.0],
                    material: 5,
                },
                MeshVertex {
                    position: point(next, -RADIUS),
                    normal: Vec3::UP,
                    uv: [0.0, 0.0],
                    material: 5,
                },
            );
        }
        Scene3d {
            meshes: vec![mesh],
            background_mesh_indices: vec![0],
            materials,
            textures: synthetic_textures(),
            ..Scene3d::default()
        }
    }

    fn depth_order_fixture() -> Scene3d {
        let camera = depth_order_camera();
        let forward = (camera.target - camera.position).normalized();
        let right = forward.cross(Vec3::UP).normalized();
        let up = right.cross(forward).normalized();
        let mut meshes = Vec::new();
        // Draw the far blue surface after the near red surface. Correct depth
        // testing must preserve red in the overlapping center of the image.
        for (distance, half_extent, material) in
            [(8.0, 5.0, 0u32), (4.0, 1.5, 1u32), (6.0, 2.5, 2u32)]
        {
            let center = camera.position + forward * distance;
            let point = |x: f32, y: f32| center + right * x + up * y;
            let mut mesh = IndexedMesh::default();
            mesh.quad(
                [
                    point(-half_extent, -half_extent),
                    point(half_extent, -half_extent),
                    point(half_extent, half_extent),
                    point(-half_extent, half_extent),
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                material,
            );
            meshes.push(mesh);
        }
        Scene3d {
            instances: identity_instances(meshes.len()),
            meshes,
            background_mesh_indices: Vec::new(),
            materials: vec![
                Material::whole_layer("far green", [0.1, 0.8, 0.2, 1.0], 0, UvWrap::Clamp),
                Material::whole_layer("near red", [0.95, 0.08, 0.04, 1.0], 0, UvWrap::Clamp),
                Material::whole_layer("middle blue", [0.05, 0.12, 0.95, 1.0], 0, UvWrap::Clamp),
            ],
            textures: synthetic_textures(),
        }
    }

    fn texture_array_fixture() -> Scene3d {
        let camera = depth_order_camera();
        let forward = (camera.target - camera.position).normalized();
        let right = forward.cross(Vec3::UP).normalized();
        let up = right.cross(forward).normalized();
        let center = camera.position + forward * 8.0;
        let point = |x: f32, y: f32| center + right * x + up * y;
        let mut mesh = IndexedMesh::default();
        mesh.quad(
            [
                point(-3.0, -1.8),
                point(3.0, -1.8),
                point(3.0, 1.8),
                point(-3.0, 1.8),
            ],
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            0,
        );
        Scene3d {
            instances: identity_instances(1),
            meshes: vec![mesh],
            background_mesh_indices: Vec::new(),
            materials: vec![Material::whole_layer(
                "last logical texture layer",
                [1.0; 4],
                5,
                UvWrap::Clamp,
            )],
            textures: RgbaTextureArray {
                width: 16,
                height: 16,
                layers: (0..6)
                    .map(|layer| {
                        let color = if layer == 5 {
                            [240, 16, 220, 255]
                        } else {
                            [32 + layer as u8 * 20, 180, 24, 255]
                        };
                        color.repeat(16 * 16)
                    })
                    .collect(),
            },
        }
    }

    fn alpha_cutout_fixture() -> Scene3d {
        let camera = depth_order_camera();
        let forward = (camera.target - camera.position).normalized();
        let right = forward.cross(Vec3::UP).normalized();
        let up = right.cross(forward).normalized();
        let plane = |distance: f32, extent: f32, material: u32| {
            let center = camera.position + forward * distance;
            let point = |x: f32, y: f32| center + right * x + up * y;
            let mut mesh = IndexedMesh::default();
            mesh.quad(
                [
                    point(-extent, -extent),
                    point(extent, -extent),
                    point(extent, extent),
                    point(-extent, extent),
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                material,
            );
            mesh
        };
        let meshes = vec![plane(8.0, 5.0, 0), plane(4.0, 3.5, 1)];
        Scene3d {
            instances: identity_instances(meshes.len()),
            meshes,
            background_mesh_indices: Vec::new(),
            materials: vec![
                Material::whole_layer("far green", [0.1, 0.8, 0.2, 1.0], 0, UvWrap::Clamp),
                Material::whole_layer(
                    "transparent red cutout",
                    [0.95, 0.08, 0.04, 1.0],
                    2,
                    UvWrap::Clamp,
                ),
            ],
            textures: synthetic_textures(),
        }
    }

    fn straight_alpha_fixture(reverse_order: bool, opaque_occluder: bool) -> Scene3d {
        let camera = depth_order_camera();
        let forward = (camera.target - camera.position).normalized();
        let right = forward.cross(Vec3::UP).normalized();
        let up = right.cross(forward).normalized();
        let normal = Vec3::new(-0.35, -0.45, 0.82).normalized();
        let plane = |distance: f32, extent: f32, material: u32| {
            let center = camera.position + forward * distance;
            let point = |x: f32, y: f32| center + right * x + up * y;
            let mut mesh = IndexedMesh::default();
            mesh.quad(
                [
                    point(-extent, -extent),
                    point(extent, -extent),
                    point(extent, extent),
                    point(-extent, extent),
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                material,
            );
            for vertex in &mut mesh.vertices {
                vertex.normal = normal;
            }
            mesh
        };
        let far = plane(8.0, 5.0, 0);
        let near = plane(4.0, 3.5, 1);
        let occluder = plane(2.0, 3.5, 2);
        let mut meshes = if reverse_order {
            vec![near, far]
        } else {
            vec![far, near]
        };
        let mut instances = identity_instances(meshes.len());
        if opaque_occluder {
            let index = meshes.len() as u32;
            meshes.push(occluder);
            instances.push(MeshInstance {
                mesh_index: index,
                translation: Vec3::ZERO,
                heading: 0.0,
                ground_normal: Vec3::UP,
                scale: 1.0,
            });
        }
        let mut materials = vec![
            Material::whole_layer(
                "far translucent red",
                [1.0, 0.0, 0.0, 1.0],
                0,
                UvWrap::Clamp,
            ),
            Material::whole_layer(
                "near translucent green",
                [0.0, 1.0, 0.0, 1.0],
                0,
                UvWrap::Clamp,
            ),
        ];
        materials[0].alpha_mode = MaterialAlphaMode::StraightAlpha;
        materials[1].alpha_mode = MaterialAlphaMode::StraightAlpha;
        if opaque_occluder {
            materials.push(Material::whole_layer(
                "opaque blue depth occluder",
                [0.0, 0.0, 1.0, 1.0],
                1,
                UvWrap::Clamp,
            ));
        }
        let mut layers = vec![
            vec![[255, 255, 255, 96]; 16 * 16]
                .into_iter()
                .flatten()
                .collect(),
        ];
        if opaque_occluder {
            for _ in 0..2 {
                layers.push(
                    vec![[255, 255, 255, 255]; 16 * 16]
                        .into_iter()
                        .flatten()
                        .collect(),
                );
            }
        }
        Scene3d {
            meshes,
            background_mesh_indices: Vec::new(),
            instances,
            materials,
            textures: RgbaTextureArray {
                width: 16,
                height: 16,
                layers,
            },
        }
    }

    fn identity_instances(mesh_count: usize) -> Vec<MeshInstance> {
        (0..mesh_count)
            .map(|mesh_index| MeshInstance {
                mesh_index: mesh_index as u32,
                translation: Vec3::ZERO,
                heading: 0.0,
                ground_normal: Vec3::UP,
                scale: 1.0,
            })
            .collect()
    }
    fn bind_group_layout_for(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        })
    }

    fn create_texture_binding(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        images: &RgbaTextureArray,
        storage_layers: u32,
    ) -> (wgpu::Texture, wgpu::BindGroup) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scene material texture array"),
            size: wgpu::Extent3d {
                width: images.width,
                height: images.height,
                depth_or_array_layers: storage_layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let layer_bytes = (images.width * images.height * 4) as usize;
        let mut pixels = images.layers.iter().flatten().copied().collect::<Vec<_>>();
        pixels.resize(layer_bytes * storage_layers as usize, 255);
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(images.width * 4),
                rows_per_image: Some(images.height),
            },
            wgpu::Extent3d {
                width: images.width,
                height: images.height,
                depth_or_array_layers: storage_layers,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("scene material texture array view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            base_array_layer: 0,
            array_layer_count: Some(storage_layers),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene material texture bind group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        (texture, bind_group)
    }
    fn create_depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scene depth buffer"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth24Plus,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }

    fn mirror_target_dimensions(width: u32, height: u32) -> (u32, u32) {
        let mut mirror_width = (width / 3).clamp(1, 512);
        let mut mirror_height = (mirror_width * 9 / 16).max(1);
        let max_height = (height / 3).clamp(1, 256);
        if mirror_height > max_height {
            mirror_height = max_height;
            mirror_width = (mirror_height * 16 / 9).max(1);
        }
        (mirror_width.min(width), mirror_height.min(height))
    }

    fn mirror_bind_group_layout_for(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rear-view compositor texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        })
    }

    fn create_mirror_target(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> MirrorTarget {
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rear-view preview color target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rear-view preview compositor sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rear-view preview compositor bindings"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        MirrorTarget {
            color,
            depth: create_depth(device, width, height),
            bind_group,
            width,
            height,
        }
    }

    fn create_mirror_composite_pipeline(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        bind_group_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        fragment_entry: &str,
        label: &str,
    ) -> wgpu::RenderPipeline {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[bind_group_layout],
            push_constant_ranges: &[],
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<MirrorCompositeVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: fragment_entry,
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
        })
    }

    fn mirror_quad_vertices(
        surface_width: u32,
        surface_height: u32,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
    ) -> [MirrorCompositeVertex; 6] {
        let right = left + width;
        let bottom = top + height;
        let to_ndc = |x: f32, y: f32| {
            [
                2.0 * x / surface_width as f32 - 1.0,
                1.0 - 2.0 * y / surface_height as f32,
            ]
        };
        let vertex = |x, y, u, v| MirrorCompositeVertex {
            position: to_ndc(x, y),
            uv: [u, v],
        };
        [
            vertex(left, top, 0.0, 0.0),
            vertex(right, top, 1.0, 0.0),
            vertex(right, bottom, 1.0, 1.0),
            vertex(left, top, 0.0, 0.0),
            vertex(right, bottom, 1.0, 1.0),
            vertex(left, bottom, 0.0, 1.0),
        ]
    }

    fn create_mirror_composite_vertices(
        device: &wgpu::Device,
        surface_width: u32,
        surface_height: u32,
        width: u32,
        height: u32,
    ) -> (wgpu::Buffer, wgpu::Buffer) {
        let margin = (surface_width.min(surface_height) as f32 * 0.025).clamp(4.0, 18.0);
        let border = (surface_width.min(surface_height) as f32 * 0.006).clamp(2.0, 5.0);
        let left = surface_width as f32 - margin - width as f32;
        // The HTML speed/time/status band spans the canvas near its top edge.
        // Keep the provisional mirror below that overlay instead of painting
        // behind it; this is layout clearance, not a claimed reference mount.
        // A proportional offset keeps CSS clearance stable when the render
        // surface is backed at devicePixelRatio 1 or 2.
        let top = (surface_height as f32 * 0.22).max(margin + border);
        let frame = mirror_quad_vertices(
            surface_width,
            surface_height,
            left - border,
            top - border,
            width as f32 + border * 2.0,
            height as f32 + border * 2.0,
        );
        let image = mirror_quad_vertices(
            surface_width,
            surface_height,
            left,
            top,
            width as f32,
            height as f32,
        );
        let frame_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rear-view inset frame vertices"),
            contents: bytemuck::cast_slice(&frame),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let image_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rear-view inset image vertices"),
            contents: bytemuck::cast_slice(&image),
            usage: wgpu::BufferUsages::VERTEX,
        });
        (frame_buffer, image_buffer)
    }

    #[derive(Clone, Copy)]
    enum ScenePipelineKind {
        Opaque,
        StraightAlpha,
        Background,
    }

    fn create_pipeline(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        bind_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        kind: ScenePipelineKind,
    ) -> wgpu::RenderPipeline {
        let (label, fragment_entry, blend, depth_stencil) = match kind {
            ScenePipelineKind::Opaque => (
                "depth-tested opaque scene pipeline",
                "fs_main",
                wgpu::BlendState::REPLACE,
                Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth24Plus,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
            ),
            ScenePipelineKind::StraightAlpha => (
                "depth-tested straight-alpha scene pipeline",
                "fs_straight_alpha",
                wgpu::BlendState::ALPHA_BLENDING,
                Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth24Plus,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
            ),
            ScenePipelineKind::Background => (
                "background pipeline without depth writes",
                "fs_background",
                wgpu::BlendState::REPLACE,
                None,
            ),
        };
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3D renderer layout"),
            bind_group_layouts: &[bind_layout],
            push_constant_ranges: &[],
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x4,
                        1 => Float32x3,
                        2 => Float32x2,
                        3 => Float32x4,
                        4 => Uint32,
                        5 => Float32x2,
                        6 => Float32x2,
                        7 => Uint32x2
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: fragment_entry,
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil,
            multisample: Default::default(),
            multiview: None,
        })
    }

    fn create_mirror_scene_pipeline(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        material_layout: &wgpu::BindGroupLayout,
        camera_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        kind: ScenePipelineKind,
    ) -> wgpu::RenderPipeline {
        let (label, fragment_entry, blend, depth_stencil) = match kind {
            ScenePipelineKind::Opaque => (
                "cached rear-view opaque pipeline",
                "fs_main",
                wgpu::BlendState::REPLACE,
                Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth24Plus,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
            ),
            ScenePipelineKind::StraightAlpha => (
                "cached rear-view straight-alpha pipeline",
                "fs_straight_alpha",
                wgpu::BlendState::ALPHA_BLENDING,
                Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth24Plus,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
            ),
            ScenePipelineKind::Background => (
                "cached rear-view background pipeline",
                "fs_background",
                wgpu::BlendState::REPLACE,
                None,
            ),
        };
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[material_layout, camera_layout],
            push_constant_ranges: &[],
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: "vs_mirror",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<MirrorWorldVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x4,
                        1 => Float32x3,
                        2 => Float32x2,
                        3 => Float32x4,
                        4 => Uint32,
                        5 => Float32x2,
                        6 => Float32x2,
                        7 => Uint32x2
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: fragment_entry,
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil,
            multisample: Default::default(),
            multiview: None,
        })
    }

    async fn browser_webgpu_adapter_available() -> bool {
        let Some(window) = web_sys::window() else {
            return false;
        };
        let nav = window.navigator();
        let Ok(gpu) = Reflect::get(&nav, &JsValue::from_str("gpu")) else {
            return false;
        };
        if gpu.is_null() || gpu.is_undefined() {
            return false;
        }
        let Ok(method) = Reflect::get(&gpu, &JsValue::from_str("requestAdapter")) else {
            return false;
        };
        let Some(method) = method.dyn_ref::<Function>() else {
            return false;
        };
        let Ok(promise) = method.call0(&gpu).and_then(JsValue::dyn_into::<Promise>) else {
            return false;
        };
        JsFuture::from(promise)
            .await
            .is_ok_and(|v| !v.is_null() && !v.is_undefined())
    }
}
