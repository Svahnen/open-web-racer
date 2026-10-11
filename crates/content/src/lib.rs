//! Bounded, format-neutral scene descriptor validation.
//!
//! The JSON descriptor contains mesh/material metadata and opaque texture IDs.
//! Callers resolve those IDs through an explicit asset manifest, verify the
//! bytes, decode images to RGBA8, then pass the layers here. This crate never
//! opens files or constructs paths from descriptor data.

use racer_render::{
    IndexedMesh, Material, MaterialAlphaMode, MaterialAnimation, MaterialFrame, MeshInstance,
    MeshVertex, RgbaTextureArray, Scene3d, UvWrap, Vec3,
};
use serde::Deserialize;
use std::fmt;

pub mod arcade_params;
pub mod classic_params;
mod preview_collision;
pub mod project_cars;
pub mod project_tracks;
pub mod provider;
pub mod rcp_track;
pub use preview_collision::derive_preview_collision;

/// Highest car colour index a descriptor or room selection may carry.
pub const MAX_VEHICLE_PAINT_COLOUR: u8 = 87;

/// The painted car colour recorded by the importer (v14 and later), if any.
/// Older or unpainted descriptors yield `None`.
pub fn descriptor_vehicle_paint_colour(json: &str) -> Result<Option<u8>, SceneError> {
    if json.len() > MAX_DESCRIPTOR_BYTES {
        return err("scene descriptor exceeds the byte limit");
    }
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| SceneError(format!("invalid scene descriptor: {e}")))?;
    match value.get("vehicle_paint") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(paint) => {
            let colour = paint
                .get("colour_index")
                .and_then(serde_json::Value::as_u64)
                .filter(|c| *c <= u64::from(MAX_VEHICLE_PAINT_COLOUR))
                .ok_or_else(|| SceneError("scene vehicle paint colour is invalid".into()))?;
            Ok(Some(colour as u8))
        }
    }
}

/// Renderer identity fingerprint for one painted variant of a car. The
/// unpainted fingerprint (which the server matches) is returned unchanged for
/// `None`; each colour gets its own derived SHA-256 so one renderer can hold
/// several colour variants of the same car profile.
pub fn vehicle_render_fingerprint(vehicle_source_fingerprint: &str, colour: Option<u8>) -> String {
    use sha2::{Digest, Sha256};
    let Some(colour) = colour else {
        return vehicle_source_fingerprint.to_owned();
    };
    let mut hash = Sha256::new();
    hash.update(b"racer/vehicle-paint-variant-v1\0");
    hash.update((vehicle_source_fingerprint.len() as u64).to_le_bytes());
    hash.update(vehicle_source_fingerprint.as_bytes());
    hash.update([colour]);
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Decode an in-memory scene descriptor together with its decoded texture
/// pages (`(page id, RGBA8 bytes)`), resolving the descriptor's layer list
/// against the pages by id. No paths or source bytes are opened by this helper.
pub fn decode_scene_pages(
    descriptor_json: &str,
    pages: &[(&str, &[u8])],
) -> Result<LoadedScene, SceneError> {
    if descriptor_json.len() > MAX_DESCRIPTOR_BYTES || pages.len() > MAX_TEXTURE_LAYERS {
        return err("local render scene exceeds descriptor or page limits");
    }
    let total_page_bytes = pages
        .iter()
        .try_fold(0usize, |total, page| total.checked_add(page.1.len()));
    if total_page_bytes.is_none_or(|total| total > MAX_TEXTURE_BYTES) {
        return err("local render scene exceeds decoded texture byte limit");
    }
    let descriptor: serde_json::Value = serde_json::from_str(descriptor_json)
        .map_err(|error| SceneError(format!("scene descriptor is invalid: {error}")))?;
    let layers = descriptor
        .get("textures")
        .and_then(|textures| textures.get("layers"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| SceneError("scene descriptor has no texture layer list".into()))?;
    let mut rgba_layers = Vec::with_capacity(layers.len());
    for layer in layers {
        let id = layer
            .as_str()
            .ok_or_else(|| SceneError("scene texture layer ID is invalid".into()))?;
        let page = pages.iter().find(|page| page.0 == id).ok_or_else(|| {
            SceneError("scene import is missing a referenced texture page".into())
        })?;
        rgba_layers.push(page.1.to_vec());
    }
    decode_scene_descriptor(descriptor_json, rgba_layers)
}

const MAX_DESCRIPTOR_BYTES: usize = 48 * 1024 * 1024;
const MAX_MESHES: usize = 4096;
const MAX_INSTANCES: usize = 4096;
const MAX_VERTICES: usize = 500_000;
const MAX_INDICES: usize = 1_500_000;
const MAX_EXPANDED_VERTICES: usize = 1_000_000;
const MAX_EXPANDED_INDICES: usize = 3_000_000;
const MAX_MATERIALS: usize = 1024;
const MAX_TEXTURE_LAYERS: usize = 256;
const MAX_TEXTURE_DIMENSION: u32 = 512;
const MAX_TEXTURE_BYTES: usize = 64 * 1024 * 1024;
const MAX_NAME_BYTES: usize = 256;
const MAX_BACKGROUND_MESHES: usize = 64;
const MAX_PREVIEW_REFERENCE_POINTS: usize = 65_535;
const MAX_TRACK_EDGE_ROWS: usize = 65_535;
const MAX_BACKGROUND_VERTICES: usize = 65_536;
const MAX_BACKGROUND_INDICES: usize = 196_608;
const MAX_BACKGROUND_COORDINATE: f32 = 1_000_000.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneError(pub String);

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SceneError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds3 {
    pub min: Vec3,
    pub max: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceToScene {
    /// Row-major basis conversion, applied before `source_scale_m`.
    pub matrix: [[f32; 3]; 3],
    pub source_scale_m: f32,
}

impl SourceToScene {
    pub fn position(self, source: Vec3) -> Vec3 {
        let scaled = [source.x, source.y, source.z].map(|v| v * self.source_scale_m);
        Vec3::new(
            dot(self.matrix[0], scaled),
            dot(self.matrix[1], scaled),
            dot(self.matrix[2], scaled),
        )
    }

    pub fn direction(self, source: Vec3) -> Vec3 {
        Vec3::new(
            dot(self.matrix[0], [source.x, source.y, source.z]),
            dot(self.matrix[1], [source.x, source.y, source.z]),
            dot(self.matrix[2], [source.x, source.y, source.z]),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpawnPoseMapping {
    pub anchor_source_position_m: Vec3,
    pub anchor_scene_position_m: Vec3,
    pub anchor_source_forward: Vec3,
    pub anchor_scene_forward: Vec3,
    pub confidence: String,
}

/// Explicit placeholder mapping from the current simulation's XY plane into
/// imported scene space. The caller supplies units-per-simulation-unit; this
/// does not infer game physics or reinterpret the source-axis transform.
#[derive(Clone, Debug, PartialEq)]
pub struct SimSceneBinding {
    pub sim_origin_xy: [f64; 2],
    pub scene_origin: Vec3,
    pub units_per_sim_unit: f32,
    pub heading_offset: f32,
    pub confidence: String,
}

impl SimSceneBinding {
    pub fn from_spawn(
        sim_origin_xy: [f64; 2],
        sim_origin_heading: f64,
        scene_origin: Vec3,
        scene_forward: Vec3,
        units_per_sim_unit: f32,
        confidence: impl Into<String>,
    ) -> Result<Self, SceneError> {
        let confidence = confidence.into();
        let forward_length = scene_forward.x.hypot(scene_forward.y);
        if sim_origin_xy.iter().any(|v| !v.is_finite())
            || !sim_origin_heading.is_finite()
            || !finite_vec3(scene_origin)
            || !finite_vec3(scene_forward)
            || !units_per_sim_unit.is_finite()
            || units_per_sim_unit <= 0.0
            || !forward_length.is_finite()
            || forward_length <= 1.0e-6
            || confidence.is_empty()
            || confidence.len() > MAX_NAME_BYTES
        {
            return err("simulation-to-scene binding is invalid");
        }
        Ok(Self {
            sim_origin_xy,
            scene_origin,
            units_per_sim_unit,
            heading_offset: scene_forward.y.atan2(scene_forward.x) - sim_origin_heading as f32,
            confidence,
        })
    }

    pub fn map_pose(
        &self,
        sim_x: f64,
        sim_y: f64,
        sim_heading: f64,
    ) -> Result<(Vec3, f32), SceneError> {
        if !sim_x.is_finite() || !sim_y.is_finite() || !sim_heading.is_finite() {
            return err("simulation pose contains a non-finite value");
        }
        let x = ((sim_x - self.sim_origin_xy[0]) as f32) * self.units_per_sim_unit;
        let y = ((sim_y - self.sim_origin_xy[1]) as f32) * self.units_per_sim_unit;
        let (sin, cos) = self.heading_offset.sin_cos();
        let position = Vec3::new(
            self.scene_origin.x + x * cos - y * sin,
            self.scene_origin.y + x * sin + y * cos,
            self.scene_origin.z,
        );
        let heading = sim_heading as f32 + self.heading_offset;
        if !finite_vec3(position) || !heading.is_finite() {
            return err("mapped scene pose is outside finite range");
        }
        Ok((position, heading))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedScene {
    pub scene: Scene3d,
    pub vehicle_instance_index: usize,
    /// Profile and source identities embedded by current local imports. Older
    /// saved descriptors remain readable but have no identity metadata.
    pub source_identity: Option<SceneSourceIdentity>,
    /// Validated descriptor mesh IDs designated as track geometry.
    pub track_mesh_indices: Vec<u32>,
    /// Ordered one-to-one with `scene.textures.layers`; IDs are opaque and are
    /// resolved by the caller against the separately verified asset manifest.
    pub texture_asset_ids: Vec<String>,
    pub track_bounds: Bounds3,
    pub source_to_scene: SourceToScene,
    pub spawn_pose_mapping: SpawnPoseMapping,
    /// Optional owner-local geometric path metadata. Older saved descriptors
    /// leave this absent, making the coverage diagnostic unavailable.
    pub preview_reference_path_scene: Option<Vec<Vec3>>,
    /// Road-edge rows in scene space, empty for
    /// older saved descriptors (importer v15 and earlier), which then have no
    /// road-edge wall.
    pub track_edge_rows_scene: Vec<SceneEdgeRow>,
}

/// One road-edge row in scene space. Distances are scene units (metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneEdgeRow {
    pub position: Vec3,
    /// Unit track direction in the scene XY plane.
    pub forward: [f32; 2],
    /// Unit right vector in the scene XY plane.
    pub right: [f32; 2],
    pub left_distance: f32,
    pub right_distance: f32,
    pub block: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneSourceIdentity {
    pub track_profile_id: String,
    pub track_source_fingerprint: String,
    pub vehicle_profile_id: String,
    pub vehicle_source_fingerprint: String,
}

/// Decode and validate a render scene descriptor and already-decoded
/// RGBA8 texture pages. Descriptor texture IDs must exactly match the supplied
/// layer count and order. This function performs no filesystem or network I/O.
pub fn decode_scene_descriptor(
    json: &str,
    rgba_layers: Vec<Vec<u8>>,
) -> Result<LoadedScene, SceneError> {
    if json.len() > MAX_DESCRIPTOR_BYTES {
        return err("scene descriptor exceeds the byte limit");
    }
    let wire: SceneWire = serde_json::from_str(json)
        .map_err(|e| SceneError(format!("invalid scene descriptor: {e}")))?;
    if !(0..=1).contains(&wire.schema_version) {
        return err("unsupported scene descriptor version");
    }
    let source_identity = match wire.source_identity {
        Some(identity) => {
            if wire.schema_version != 1
                || !valid_profile_id(&identity.track_profile_id)
                || !valid_profile_id(&identity.vehicle_profile_id)
                || !valid_sha256(&identity.track_source_fingerprint)
                || !valid_sha256(&identity.vehicle_source_fingerprint)
            {
                return err("scene source identity is malformed or unsupported");
            }
            Some(SceneSourceIdentity {
                track_profile_id: identity.track_profile_id,
                track_source_fingerprint: identity.track_source_fingerprint,
                vehicle_profile_id: identity.vehicle_profile_id,
                vehicle_source_fingerprint: identity.vehicle_source_fingerprint,
            })
        }
        None if wire.schema_version == 1 => {
            return err("version-1 scene descriptor is missing source identity");
        }
        None => None,
    };
    if wire.coordinate_system.source_scale_m <= 0.0
        || !wire.coordinate_system.source_scale_m.is_finite()
        || wire
            .coordinate_system
            .source_to_scene_matrix
            .iter()
            .flatten()
            .any(|v| !v.is_finite())
    {
        return err("invalid source coordinate transform");
    }
    let source_to_scene = SourceToScene {
        matrix: wire.coordinate_system.source_to_scene_matrix,
        source_scale_m: wire.coordinate_system.source_scale_m,
    };
    let spawn_pose_mapping = parse_spawn_mapping(wire.group_metadata.spawn_pose_mapping)?;
    let preview_reference_path_scene = match wire.preview_reference_path_scene {
        Some(points) => {
            if !(2..=MAX_PREVIEW_REFERENCE_POINTS).contains(&points.len())
                || points
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v.abs() > MAX_BACKGROUND_COORDINATE)
            {
                return err("preview reference path exceeds its point or coordinate bounds");
            }
            Some(points.into_iter().map(vec3).collect())
        }
        None => None,
    };
    let track_edge_rows_scene = match wire.track_edge_rows_scene {
        Some(rows) => decode_edge_rows(rows)?,
        None => Vec::new(),
    };
    let track_mesh_indices = wire.group_metadata.track_mesh_indices;

    let texture_ids = wire.textures.layers;
    if texture_ids.is_empty() || texture_ids.len() > MAX_TEXTURE_LAYERS {
        return err("texture layer count is outside supported bounds");
    }
    if texture_ids.iter().any(|id| !valid_opaque_id(id)) {
        return err("texture layer has an invalid opaque asset ID");
    }
    if rgba_layers.len() != texture_ids.len() {
        return err("decoded texture count does not match descriptor");
    }
    let width = wire.textures.width;
    let height = wire.textures.height;
    if width == 0 || height == 0 || width > MAX_TEXTURE_DIMENSION || height > MAX_TEXTURE_DIMENSION
    {
        return err("texture dimensions are outside supported bounds");
    }
    let layer_size = (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4))
        .ok_or_else(|| SceneError("texture dimensions overflow".into()))?;
    let total_texture_size = layer_size
        .checked_mul(rgba_layers.len())
        .ok_or_else(|| SceneError("texture allocation size overflow".into()))?;
    if total_texture_size > MAX_TEXTURE_BYTES
        || rgba_layers.iter().any(|layer| layer.len() != layer_size)
    {
        return err("decoded texture data is malformed or exceeds its byte limit");
    }

    if wire.meshes.is_empty() || wire.meshes.len() > MAX_MESHES {
        return err("mesh count is outside supported bounds");
    }
    if wire.materials.is_empty() || wire.materials.len() > MAX_MATERIALS {
        return err("material count is outside supported bounds");
    }
    let total_vertices: usize = wire.meshes.iter().map(|m| m.vertices.len()).sum();
    let total_indices: usize = wire.meshes.iter().map(|m| m.indices.len()).sum();
    if total_vertices == 0
        || total_vertices > MAX_VERTICES
        || total_indices == 0
        || total_indices > MAX_INDICES
    {
        return err("scene geometry exceeds supported bounds");
    }

    let materials = wire
        .materials
        .into_iter()
        .map(|m| {
            if m.name.is_empty()
                || m.name.len() > MAX_NAME_BYTES
                || m.texture as usize >= texture_ids.len()
                || m.animation.as_ref().is_some_and(|animation| {
                    animation.frames.iter().any(|frame| frame.texture as usize >= texture_ids.len())
                })
                || m.tint
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                || m.uv_offset
                    .iter()
                    .chain(m.uv_scale.iter())
                    .any(|v| !v.is_finite())
                || (m.alpha_mode == MaterialAlphaModeWire::StraightAlpha && m.tint[3] != 1.0)
            {
                return err("material contains an invalid name, texture, alpha policy, tint, or UV transform");
            }
            let wrap = parse_wrap(&m.wrap)?;
            let mut material = Material::atlas_layer(
                m.name,
                m.tint,
                m.texture as usize,
                m.uv_offset,
                m.uv_scale,
                wrap,
                wrap,
            );
            if let Some(animation) = m.animation {
                material.animation = Some(MaterialAnimation {
                    period_ticks: animation.period_ticks,
                    frames: animation
                        .frames
                        .into_iter()
                        .map(|frame| MaterialFrame {
                            texture: frame.texture as usize,
                            uv_offset: frame.uv_offset,
                            uv_scale: frame.uv_scale,
                        })
                        .collect(),
                });
            }
            material.alpha_mode = match m.alpha_mode {
                MaterialAlphaModeWire::Opaque => MaterialAlphaMode::Opaque,
                MaterialAlphaModeWire::StraightAlpha => MaterialAlphaMode::StraightAlpha,
            };
            if !material.atlas_uv_is_well_formed() {
                return err("material UV atlas range is invalid");
            }
            Ok(material)
        })
        .collect::<Result<Vec<_>, SceneError>>()?;

    let meshes = wire
        .meshes
        .into_iter()
        .map(|mesh| {
            let vertices = mesh
                .vertices
                .into_iter()
                .map(|v| MeshVertex {
                    position: Vec3::new(v.position[0], v.position[1], v.position[2]),
                    normal: Vec3::new(v.normal[0], v.normal[1], v.normal[2]),
                    uv: v.uv,
                    material: v.material,
                })
                .collect();
            let result = IndexedMesh {
                vertices,
                indices: mesh.indices,
            };
            if !result.is_well_formed()
                || result
                    .vertices
                    .iter()
                    .any(|v| v.material as usize >= materials.len())
            {
                return err("mesh has invalid indices, non-finite attributes, or material refs");
            }
            for triangle in result.indices.chunks_exact(3) {
                let alpha_count = triangle
                    .iter()
                    .filter(|index| {
                        materials[result.vertices[**index as usize].material as usize].alpha_mode
                            == MaterialAlphaMode::StraightAlpha
                    })
                    .count();
                if alpha_count != 0 && alpha_count != 3 {
                    return err("triangle mixes opaque and straight-alpha materials");
                }
            }
            Ok(result)
        })
        .collect::<Result<Vec<_>, SceneError>>()?;

    if wire.instances.is_empty() || wire.instances.len() > MAX_INSTANCES {
        return err("instance count is outside supported bounds");
    }
    let mut vehicle_instance_index = None;
    let mut instances = Vec::with_capacity(wire.instances.len());
    for (index, instance) in wire.instances.into_iter().enumerate() {
        if instance.mesh_index as usize >= meshes.len()
            || instance.role.is_empty()
            || instance.role.len() > MAX_NAME_BYTES
            || instance.translation.iter().any(|v| !v.is_finite())
            || !instance.heading.is_finite()
            || instance.scale.iter().any(|v| !v.is_finite() || *v <= 0.0)
        {
            return err("scene instance has invalid transform or mesh reference");
        }
        if (instance.role == "vehicle" || instance.role == "vehicle-local-dynamic")
            && vehicle_instance_index.replace(index).is_some()
        {
            return err("scene must contain exactly one vehicle instance");
        }
        if (instance.scale[0] - instance.scale[1]).abs() > 1.0e-6
            || (instance.scale[0] - instance.scale[2]).abs() > 1.0e-6
        {
            return err("non-uniform mesh instance scale is not supported");
        }
        instances.push(MeshInstance {
            mesh_index: instance.mesh_index,
            translation: Vec3::new(
                instance.translation[0],
                instance.translation[1],
                instance.translation[2],
            ),
            heading: instance.heading,
            ground_normal: Vec3::UP,
            scale: instance.scale[0],
        });
    }
    let vehicle_instance_index = vehicle_instance_index
        .ok_or_else(|| SceneError("scene is missing its vehicle instance".into()))?;
    validate_background_meshes(
        &wire.background_mesh_indices,
        &meshes,
        &instances,
        &track_mesh_indices,
    )?;
    let (expanded_vertices, expanded_indices) = instances
        .iter()
        .try_fold((0usize, 0usize), |(vertices, indices), instance| {
            let mesh = &meshes[instance.mesh_index as usize];
            Some((
                vertices.checked_add(mesh.vertices.len())?,
                indices.checked_add(mesh.indices.len())?,
            ))
        })
        .ok_or_else(|| SceneError("expanded scene geometry size overflow".into()))?;
    if expanded_vertices > MAX_EXPANDED_VERTICES || expanded_indices > MAX_EXPANDED_INDICES {
        return err("expanded scene geometry exceeds supported bounds");
    }
    let scene = Scene3d {
        meshes,
        background_mesh_indices: wire.background_mesh_indices,
        instances,
        materials,
        textures: RgbaTextureArray {
            width,
            height,
            layers: rgba_layers,
        },
    };
    let track_bounds = compute_track_bounds(&scene, &track_mesh_indices)?;

    Ok(LoadedScene {
        scene,
        vehicle_instance_index,
        source_identity,
        track_mesh_indices,
        texture_asset_ids: texture_ids,
        track_bounds,
        source_to_scene,
        spawn_pose_mapping,
        preview_reference_path_scene,
        track_edge_rows_scene,
    })
}

/// Wire row: `[x, y, z, forward_x, forward_y, right_x, right_y, left, right, block]`.
fn decode_edge_rows(rows: Vec<[f32; 10]>) -> Result<Vec<SceneEdgeRow>, SceneError> {
    if rows.len() > MAX_TRACK_EDGE_ROWS {
        return err("track edge rows exceed their count limit");
    }
    rows.into_iter()
        .map(|row| {
            let unit = |x: f32, y: f32| {
                let n = x.hypot(y);
                (n.is_finite() && (0.5..=1.5).contains(&n)).then(|| [x / n, y / n])
            };
            let valid = row[..9]
                .iter()
                .all(|v| v.is_finite() && v.abs() <= MAX_BACKGROUND_COORDINATE)
                && row[7] > 0.0
                && row[8] > 0.0
                && row[7] <= 1000.0
                && row[8] <= 1000.0
                && row[9].is_finite()
                && (0.0..=65_535.0).contains(&row[9])
                && row[9].fract() == 0.0;
            match (valid, unit(row[3], row[4]), unit(row[5], row[6])) {
                (true, Some(forward), Some(right)) => Ok(SceneEdgeRow {
                    position: Vec3::new(row[0], row[1], row[2]),
                    forward,
                    right,
                    left_distance: row[7],
                    right_distance: row[8],
                    block: row[9] as u16,
                }),
                _ => err("track edge row is invalid"),
            }
        })
        .collect()
}

fn validate_background_meshes(
    background: &[u32],
    meshes: &[IndexedMesh],
    instances: &[MeshInstance],
    track_meshes: &[u32],
) -> Result<(), SceneError> {
    if background.len() > MAX_BACKGROUND_MESHES {
        return err("background mesh count exceeds supported bounds");
    }
    let mut refs = std::collections::HashSet::new();
    let mut vertices = 0usize;
    let mut indices = 0usize;
    for &mesh_index in background {
        if !refs.insert(mesh_index) {
            return err("background mesh references contain duplicates");
        }
        let Some(mesh) = meshes.get(mesh_index as usize) else {
            return err("background mesh reference is outside the mesh list");
        };
        if instances.iter().any(|i| i.mesh_index == mesh_index)
            || track_meshes.contains(&mesh_index)
        {
            return err("background meshes must be disjoint from scene and track meshes");
        }
        vertices = vertices.saturating_add(mesh.vertices.len());
        indices = indices.saturating_add(mesh.indices.len());
        if mesh.vertices.iter().any(|v| {
            [v.position.x, v.position.y, v.position.z]
                .iter()
                .any(|p| p.abs() > MAX_BACKGROUND_COORDINATE)
        }) {
            return err("background mesh coordinates exceed supported bounds");
        }
    }
    if vertices > MAX_BACKGROUND_VERTICES || indices > MAX_BACKGROUND_INDICES {
        return err("background geometry exceeds supported bounds");
    }
    Ok(())
}

fn compute_track_bounds(scene: &Scene3d, track_meshes: &[u32]) -> Result<Bounds3, SceneError> {
    if track_meshes.is_empty() || track_meshes.len() > MAX_MESHES {
        return err("track mesh count is outside supported bounds");
    }
    let track_set: std::collections::HashSet<u32> = track_meshes.iter().copied().collect();
    if track_set.len() != track_meshes.len()
        || track_meshes
            .iter()
            .any(|i| *i as usize >= scene.meshes.len())
    {
        return err("track mesh references are invalid");
    }
    let mut min = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut found = false;
    for instance in &scene.instances {
        if !track_set.contains(&instance.mesh_index) {
            continue;
        }
        for vertex in &scene.meshes[instance.mesh_index as usize].vertices {
            let p = instance.transform_point(vertex.position);
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            min.z = min.z.min(p.z);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
            max.z = max.z.max(p.z);
            found = true;
        }
    }
    if !found {
        return err("track meshes have no instances");
    }
    if !finite_vec3(min) || !finite_vec3(max) {
        return err("track bounds exceed finite scene coordinates");
    }
    Ok(Bounds3 { min, max })
}

fn parse_spawn_mapping(value: SpawnWire) -> Result<SpawnPoseMapping, SceneError> {
    let anchor_source_position_m = vec3(value.anchor_source_position_m);
    let anchor_scene_position_m = vec3(value.anchor_scene_position_m);
    let anchor_source_forward = vec3(value.anchor_source_forward);
    let anchor_scene_forward = vec3(value.anchor_scene_forward);
    if value.confidence.is_empty() || value.confidence.len() > MAX_NAME_BYTES {
        return err("spawn mapping confidence label is invalid");
    }
    if !finite_vec3(anchor_source_position_m)
        || !finite_vec3(anchor_scene_position_m)
        || !finite_vec3(anchor_source_forward)
        || !finite_vec3(anchor_scene_forward)
    {
        return err("spawn mapping contains non-finite values");
    }
    Ok(SpawnPoseMapping {
        anchor_source_position_m,
        anchor_scene_position_m,
        anchor_source_forward,
        anchor_scene_forward,
        confidence: value.confidence,
    })
}

fn parse_wrap(value: &str) -> Result<UvWrap, SceneError> {
    match value {
        "clamp" => Ok(UvWrap::Clamp),
        "repeat" => Ok(UvWrap::Repeat),
        _ => err("unsupported texture wrap mode"),
    }
}

fn valid_opaque_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_NAME_BYTES
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn vec3(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}

fn finite_vec3(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_profile_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn err<T>(message: &str) -> Result<T, SceneError> {
    Err(SceneError(message.to_owned()))
}

#[derive(Deserialize)]
struct SceneWire {
    schema_version: u32,
    coordinate_system: CoordinateWire,
    meshes: Vec<MeshWire>,
    materials: Vec<MaterialWire>,
    textures: TextureWire,
    instances: Vec<InstanceWire>,
    #[serde(default)]
    background_mesh_indices: Vec<u32>,
    #[serde(default)]
    preview_reference_path_scene: Option<Vec<[f32; 3]>>,
    #[serde(default)]
    track_edge_rows_scene: Option<Vec<[f32; 10]>>,
    #[serde(default)]
    source_identity: Option<SceneSourceIdentityWire>,
    group_metadata: GroupWire,
}

#[derive(Deserialize)]
struct SceneSourceIdentityWire {
    track_profile_id: String,
    track_source_fingerprint: String,
    vehicle_profile_id: String,
    vehicle_source_fingerprint: String,
}

#[derive(Deserialize)]
struct CoordinateWire {
    source_scale_m: f32,
    source_to_scene_matrix: [[f32; 3]; 3],
}

#[derive(Deserialize)]
struct MeshWire {
    vertices: Vec<VertexWire>,
    indices: Vec<u32>,
}

#[derive(Deserialize)]
struct VertexWire {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    material: u32,
}

#[derive(Deserialize)]
struct MaterialWire {
    name: String,
    tint: [f32; 4],
    texture: u32,
    uv_offset: [f32; 2],
    uv_scale: [f32; 2],
    wrap: String,
    #[serde(default)]
    alpha_mode: MaterialAlphaModeWire,
    #[serde(default)]
    animation: Option<MaterialAnimationWire>,
}

#[derive(Deserialize)]
struct MaterialAnimationWire {
    period_ticks: u32,
    frames: Vec<MaterialFrameWire>,
}

#[derive(Deserialize)]
struct MaterialFrameWire {
    texture: u32,
    uv_offset: [f32; 2],
    uv_scale: [f32; 2],
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum MaterialAlphaModeWire {
    #[default]
    Opaque,
    StraightAlpha,
}

#[derive(Deserialize)]
struct TextureWire {
    width: u32,
    height: u32,
    layers: Vec<String>,
}

#[derive(Deserialize)]
struct InstanceWire {
    mesh_index: u32,
    translation: [f32; 3],
    heading: f32,
    scale: [f32; 3],
    role: String,
}

#[derive(Deserialize)]
struct GroupWire {
    track_mesh_indices: Vec<u32>,
    spawn_pose_mapping: SpawnWire,
}

#[derive(Deserialize)]
struct SpawnWire {
    anchor_source_position_m: [f32; 3],
    anchor_scene_position_m: [f32; 3],
    anchor_source_forward: [f32; 3],
    anchor_scene_forward: [f32; 3],
    confidence: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vehicle_paint_descriptor_field_and_render_fingerprint() {
        assert_eq!(descriptor_vehicle_paint_colour("{}").unwrap(), None);
        let painted =
            r#"{"vehicle_paint":{"colour_index":87,"table_source":"external","archive":"a"}}"#;
        assert_eq!(descriptor_vehicle_paint_colour(painted).unwrap(), Some(87));
        let too_big = r#"{"vehicle_paint":{"colour_index":88}}"#;
        assert!(descriptor_vehicle_paint_colour(too_big).is_err());
        assert!(descriptor_vehicle_paint_colour(r#"{"vehicle_paint":{}}"#).is_err());
        let fp = "ab".repeat(32);
        assert_eq!(vehicle_render_fingerprint(&fp, None), fp);
        let red = vehicle_render_fingerprint(&fp, Some(2));
        let blue = vehicle_render_fingerprint(&fp, Some(6));
        assert_eq!(red.len(), 64);
        assert!(
            red.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert_ne!(red, blue);
        assert_ne!(red, fp);
        assert_eq!(red, vehicle_render_fingerprint(&fp, Some(2)));
    }

    const SCENE: &str = r#"{
      "schema_version":0,
      "coordinate_system":{"source_scale_m":1.0,"source_to_scene_matrix":[[1,0,0],[0,1,0],[0,0,1]]},
      "meshes":[{"vertices":[
        {"position":[-1,0,0],"normal":[0,0,1],"uv":[0,0],"material":0},
        {"position":[1,0,0],"normal":[0,0,1],"uv":[1,0],"material":0},
        {"position":[0,2,0],"normal":[0,0,1],"uv":[0.5,1],"material":0}],"indices":[0,1,2]}],
      "materials":[{"name":"track","tint":[1,1,1,1],"texture":0,"uv_offset":[0,0],"uv_scale":[1,1],"wrap":"clamp"}],
      "textures":{"width":1,"height":1,"layers":["tex_0"]},
      "instances":[
        {"mesh_index":0,"translation":[0,0,0],"heading":0,"scale":[1,1,1],"role":"static"},
        {"mesh_index":0,"translation":[0,0,0],"heading":0,"scale":[1,1,1],"role":"vehicle-local-dynamic"}],
      "group_metadata":{"track_mesh_indices":[0],"spawn_pose_mapping":{
        "anchor_source_position_m":[0,0,0],"anchor_scene_position_m":[0,0,0],
        "anchor_source_forward":[1,0,0],"anchor_scene_forward":[1,0,0],"confidence":"low"}}
    }"#;

    fn scene_with_counts(mesh_count: usize, instance_count: usize) -> String {
        let mesh = r#"{"vertices":[{"position":[-1,0,0],"normal":[0,0,1],"uv":[0,0],"material":0},{"position":[1,0,0],"normal":[0,0,1],"uv":[1,0],"material":0},{"position":[0,2,0],"normal":[0,0,1],"uv":[0.5,1],"material":0}],"indices":[0,1,2]}"#;
        let meshes = std::iter::repeat_n(mesh, mesh_count)
            .collect::<Vec<_>>()
            .join(",");
        let mut instances = Vec::with_capacity(instance_count);
        for index in 0..instance_count {
            let role = if index == 0 {
                "vehicle-local-dynamic"
            } else {
                "static"
            };
            instances.push(format!(
                "{{\"mesh_index\":0,\"translation\":[0,0,0],\"heading\":0,\"scale\":[1,1,1],\"role\":\"{role}\"}}"
            ));
        }
        let instances = instances.join(",");
        let mut json = SCENE.to_owned();
        let meshes_marker = "\"meshes\":[";
        let meshes_start = json.find(meshes_marker).unwrap() + meshes_marker.len();
        let meshes_end = json.find("],\n      \"materials\"").unwrap();
        json.replace_range(meshes_start..meshes_end, &meshes);
        let instances_marker = "\"instances\":[";
        let instances_start = json.find(instances_marker).unwrap() + instances_marker.len();
        let instances_end = json.find("],\n      \"group_metadata\"").unwrap();
        json.replace_range(instances_start..instances_end, &instances);
        json
    }

    #[test]
    fn decodes_bounded_scene_and_computes_track_bounds() {
        let loaded = decode_scene_descriptor(SCENE, vec![vec![255, 255, 255, 255]]).unwrap();
        assert_eq!(loaded.vehicle_instance_index, 1);
        assert_eq!(loaded.track_mesh_indices, [0]);
        assert_eq!(loaded.texture_asset_ids, ["tex_0"]);
        assert_eq!(loaded.track_bounds.min, Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(loaded.track_bounds.max, Vec3::new(1.0, 2.0, 0.0));
        assert_eq!(
            loaded.scene.materials[0].alpha_mode,
            MaterialAlphaMode::Opaque
        );
        assert_eq!(
            loaded.source_to_scene.position(Vec3::new(2.0, 3.0, 4.0)),
            Vec3::new(2.0, 3.0, 4.0)
        );
        assert!(loaded.preview_reference_path_scene.is_none());
    }

    #[test]
    fn source_identity_v1_is_typed_and_legacy_v0_remains_identityless() {
        let legacy = decode_scene_descriptor(SCENE, vec![vec![255; 4]]).unwrap();
        assert!(legacy.source_identity.is_none());

        let identity = serde_json::json!({
            "track_profile_id": "track-profile-se",
            "track_source_fingerprint": "a".repeat(64),
            "vehicle_profile_id": "vehicle-profile-synth-b",
            "vehicle_source_fingerprint": "b".repeat(64),
        });
        let v1 = SCENE
            .replace("\"schema_version\":0", "\"schema_version\":1")
            .replace(
                "\"coordinate_system\"",
                &format!("\"source_identity\":{identity},\"coordinate_system\""),
            );
        let loaded = decode_scene_descriptor(&v1, vec![vec![255; 4]]).unwrap();
        let source = loaded.source_identity.unwrap();
        assert_eq!(source.track_profile_id, "track-profile-se");
        assert_eq!(source.track_source_fingerprint, "a".repeat(64));
        assert_eq!(source.vehicle_profile_id, "vehicle-profile-synth-b");
        assert_eq!(source.vehicle_source_fingerprint, "b".repeat(64));

        let missing = SCENE.replace("\"schema_version\":0", "\"schema_version\":1");
        assert!(decode_scene_descriptor(&missing, vec![vec![255; 4]]).is_err());
        let uppercase_hash = v1.replace(&"a".repeat(64), &"A".repeat(64));
        assert!(decode_scene_descriptor(&uppercase_hash, vec![vec![255; 4]]).is_err());
        let malformed_profile = v1.replace("track-profile-se", "../track");
        assert!(decode_scene_descriptor(&malformed_profile, vec![vec![255; 4]]).is_err());
    }

    #[test]
    fn scene_page_count_is_bounded_before_texture_cloning() {
        let ids: Vec<String> = (0..=MAX_TEXTURE_LAYERS)
            .map(|index| format!("page-{index}"))
            .collect();
        let pages: Vec<(&str, &[u8])> = ids.iter().map(|id| (id.as_str(), &[][..])).collect();
        assert!(decode_scene_pages(SCENE, &pages).is_err());
    }

    #[test]
    fn optional_reference_path_is_bounded_and_survives_private_scene_cache_json() {
        let with_path = SCENE.replace(
            "\"group_metadata\"",
            "\"preview_reference_path_scene\":[[-1,0,0],[1,0,0]],\"group_metadata\"",
        );
        let loaded = decode_scene_descriptor(&with_path, vec![vec![255; 4]]).unwrap();
        assert_eq!(
            loaded.preview_reference_path_scene,
            Some(vec![Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)])
        );

        let unbounded = SCENE.replace(
            "\"group_metadata\"",
            "\"preview_reference_path_scene\":[[1000001,0,0],[1,0,0]],\"group_metadata\"",
        );
        assert!(decode_scene_descriptor(&unbounded, vec![vec![255; 4]]).is_err());

        let too_short = SCENE.replace(
            "\"group_metadata\"",
            "\"preview_reference_path_scene\":[[0,0,0]],\"group_metadata\"",
        );
        assert!(decode_scene_descriptor(&too_short, vec![vec![255; 4]]).is_err());
    }

    #[test]
    fn optional_track_edge_rows_are_validated_and_absent_in_older_descriptors() {
        assert!(
            decode_scene_descriptor(SCENE, vec![vec![255; 4]])
                .unwrap()
                .track_edge_rows_scene
                .is_empty()
        );
        let with_rows = |rows: &str| {
            SCENE.replace(
                "\"group_metadata\"",
                &format!("\"track_edge_rows_scene\":{rows},\"group_metadata\""),
            )
        };
        let loaded = decode_scene_descriptor(
            &with_rows("[[1,2,3,0,1.2,1.3,0,2.5,4,9]]"),
            vec![vec![255; 4]],
        )
        .unwrap();
        let row = loaded.track_edge_rows_scene[0];
        assert_eq!(row.position, Vec3::new(1.0, 2.0, 3.0));
        // Frame vectors are re-normalised.
        assert_eq!(row.forward, [0.0, 1.0]);
        assert_eq!(row.right, [1.0, 0.0]);
        assert_eq!(
            (row.left_distance, row.right_distance, row.block),
            (2.5, 4.0, 9)
        );
        for bad in [
            "[[0,0,0,0,1,1,0,0,4,0]]",       // zero left distance
            "[[0,0,0,0,0,1,0,2,4,0]]",       // degenerate forward
            "[[0,0,0,0,1,5,0,2,4,0]]",       // right vector far from unit
            "[[0,0,0,0,1,1,0,2,4,1.5]]",     // fractional block
            "[[2000000,0,0,0,1,1,0,2,4,0]]", // coordinate bound
        ] {
            assert!(
                decode_scene_descriptor(&with_rows(bad), vec![vec![255; 4]]).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn optional_alpha_mode_defaults_to_opaque_and_rejects_unknown_values() {
        let straight = SCENE
            .replace("\"name\":\"track\"", "\"name\":\"track-0083\"")
            .replace(
                "\"wrap\":\"clamp\"",
                "\"wrap\":\"clamp\",\"alpha_mode\":\"straight_alpha\"",
            );
        let loaded = decode_scene_descriptor(&straight, vec![vec![255, 255, 255, 96]]).unwrap();
        assert_eq!(
            loaded.scene.materials[0].alpha_mode,
            MaterialAlphaMode::StraightAlpha
        );
        let invalid = SCENE.replace(
            "\"wrap\":\"clamp\"",
            "\"wrap\":\"clamp\",\"alpha_mode\":\"blend_whatever\"",
        );
        assert!(decode_scene_descriptor(&invalid, vec![vec![255, 255, 255, 96]]).is_err());
        // Alpha policy is per material (row flags / 32-bit images), not per name.
        let any_material = SCENE.replace(
            "\"wrap\":\"clamp\"",
            "\"wrap\":\"clamp\",\"alpha_mode\":\"straight_alpha\"",
        );
        assert!(decode_scene_descriptor(&any_material, vec![vec![255, 255, 255, 96]]).is_ok());
        let tinted = SCENE
            .replace("\"name\":\"track\"", "\"name\":\"track-0083\"")
            .replace("\"tint\":[1,1,1,1]", "\"tint\":[1,1,1,0.5]")
            .replace(
                "\"wrap\":\"clamp\"",
                "\"wrap\":\"clamp\",\"alpha_mode\":\"straight_alpha\"",
            );
        assert!(decode_scene_descriptor(&tinted, vec![vec![255, 255, 255, 96]]).is_err());
    }

    #[test]
    fn optional_material_animation_decodes_and_validates_frames() {
        let animated = SCENE.replace(
            "\"wrap\":\"clamp\"",
            "\"wrap\":\"clamp\",\"animation\":{\"period_ticks\":7,\"frames\":[{\"texture\":0,\"uv_offset\":[0,0],\"uv_scale\":[0.5,0.5]},{\"texture\":0,\"uv_offset\":[0.5,0],\"uv_scale\":[0.25,0.25]}]}",
        );
        let loaded = decode_scene_descriptor(&animated, vec![vec![255; 4]]).unwrap();
        let animation = loaded.scene.materials[0].animation.as_ref().unwrap();
        assert_eq!(animation.period_ticks, 7);
        assert_eq!(animation.frames.len(), 2);
        assert_eq!(loaded.scene.materials[0].frame_at(6).uv_offset, [0.0, 0.0]);
        assert_eq!(loaded.scene.materials[0].frame_at(7).uv_offset, [0.5, 0.0]);
        assert_eq!(loaded.scene.materials[0].frame_at(14).uv_offset, [0.0, 0.0]);

        let zero_period = animated.replace("\"period_ticks\":7", "\"period_ticks\":0");
        assert!(decode_scene_descriptor(&zero_period, vec![vec![255; 4]]).is_err());
        let bad_layer = animated.replacen(
            "\"texture\":0,\"uv_offset\":[0.5",
            "\"texture\":3,\"uv_offset\":[0.5",
            1,
        );
        assert!(decode_scene_descriptor(&bad_layer, vec![vec![255; 4]]).is_err());
        let out_of_atlas = animated.replace("[0.5,0]", "[0.9,0]");
        assert!(decode_scene_descriptor(&out_of_atlas, vec![vec![255; 4]]).is_err());
        let empty = animated.replacen(
            "\"frames\":[{\"texture\":0,\"uv_offset\":[0,0],\"uv_scale\":[0.5,0.5]},{\"texture\":0,\"uv_offset\":[0.5,0],\"uv_scale\":[0.25,0.25]}]",
            "\"frames\":[]",
            1,
        );
        assert!(decode_scene_descriptor(&empty, vec![vec![255; 4]]).is_err());
    }

    #[test]
    fn rejects_mixed_alpha_modes_within_a_triangle_before_installation() {
        let two_materials = SCENE
            .replace("\"materials\":[{", "\"materials\":[{")
            .replace(
                "\"wrap\":\"clamp\"}],",
                "\"wrap\":\"clamp\"},{\"name\":\"track-0083\",\"tint\":[1,1,1,1],\"texture\":0,\"uv_offset\":[0,0],\"uv_scale\":[1,1],\"wrap\":\"clamp\",\"alpha_mode\":\"straight_alpha\"}],",
            )
            .replace("\"material\":0}],\"indices\":[0,1,2]", "\"material\":1}],\"indices\":[0,1,2]");
        let error =
            decode_scene_descriptor(&two_materials, vec![vec![255, 255, 255, 96]]).unwrap_err();
        assert!(error.0.contains("triangle mixes opaque and straight-alpha"));
    }

    #[test]
    fn rejects_wrong_texture_size_and_bad_references() {
        assert!(decode_scene_descriptor(SCENE, vec![vec![0; 3]]).is_err());
        let bad = SCENE.replace("\"texture\":0", "\"texture\":1");
        let padding_layer_reference = decode_scene_descriptor(&bad, vec![vec![0; 4]]).unwrap_err();
        assert!(padding_layer_reference.0.contains("invalid name, texture"));
        let bad_id = SCENE.replace("tex_0", "../asset");
        assert!(decode_scene_descriptor(&bad_id, vec![vec![0; 4]]).is_err());
    }

    #[test]
    fn rejects_nonfinite_geometry_and_duplicate_vehicle() {
        let bad = SCENE.replace("[-1,0,0]", "[1e999,0,0]");
        assert!(decode_scene_descriptor(&bad, vec![vec![0; 4]]).is_err());
        let duplicate = SCENE.replace(
            "{\"mesh_index\":0,\"translation\":[0,0,0],\"heading\":0,\"scale\":[1,1,1],\"role\":\"vehicle-local-dynamic\"}",
            "{\"mesh_index\":0,\"translation\":[0,0,0],\"heading\":0,\"scale\":[1,1,1],\"role\":\"vehicle-local-dynamic\"},{\"mesh_index\":0,\"translation\":[0,0,0],\"heading\":0,\"scale\":[1,1,1],\"role\":\"vehicle-local-dynamic\"}",
        );
        assert!(decode_scene_descriptor(&duplicate, vec![vec![0; 4]]).is_err());
        let nonuniform = SCENE.replace("\"scale\":[1,1,1]", "\"scale\":[1,2,1]");
        assert!(decode_scene_descriptor(&nonuniform, vec![vec![0; 4]]).is_err());
        let bad_spawn = SCENE.replace(
            "\"anchor_scene_forward\":[1,0,0]",
            "\"anchor_scene_forward\":[1e999,0,0]",
        );
        assert!(decode_scene_descriptor(&bad_spawn, vec![vec![0; 4]]).is_err());
        let bad_tint = SCENE.replace("\"tint\":[1,1,1,1]", "\"tint\":[2,1,1,1]");
        assert!(decode_scene_descriptor(&bad_tint, vec![vec![0; 4]]).is_err());
    }

    #[test]
    fn enforces_descriptor_and_version_limits() {
        assert!(decode_scene_descriptor(&" ".repeat(MAX_DESCRIPTOR_BYTES + 1), vec![]).is_err());
        let bad = SCENE.replace("\"schema_version\":0", "\"schema_version\":1");
        assert!(decode_scene_descriptor(&bad, vec![vec![0; 4]]).is_err());
    }

    #[test]
    fn optional_background_meshes_are_bounded_and_disjoint() {
        let old = decode_scene_descriptor(SCENE, vec![vec![255; 4]]).unwrap();
        assert!(old.scene.background_mesh_indices.is_empty());

        let mut valid: serde_json::Value = serde_json::from_str(SCENE).unwrap();
        let extra_mesh = valid["meshes"][0].clone();
        valid["meshes"].as_array_mut().unwrap().push(extra_mesh);
        valid["background_mesh_indices"] = serde_json::json!([1]);
        let valid = serde_json::to_string(&valid).unwrap();
        let loaded = decode_scene_descriptor(&valid, vec![vec![255; 4]]).unwrap();
        assert_eq!(loaded.scene.background_mesh_indices, [1]);

        for invalid_refs in [
            serde_json::json!([0]),    // track and foreground instance overlap
            serde_json::json!([2]),    // out of range
            serde_json::json!([1, 1]), // duplicate
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(&valid).unwrap();
            invalid["background_mesh_indices"] = invalid_refs;
            let invalid = serde_json::to_string(&invalid).unwrap();
            assert!(decode_scene_descriptor(&invalid, vec![vec![255; 4]]).is_err());
        }

        let mut bad_material: serde_json::Value = serde_json::from_str(&valid).unwrap();
        bad_material["meshes"][1]["vertices"][0]["material"] = serde_json::json!(1);
        let bad_material = serde_json::to_string(&bad_material).unwrap();
        assert!(decode_scene_descriptor(&bad_material, vec![vec![255; 4]]).is_err());

        let mut unbounded_coord: serde_json::Value = serde_json::from_str(&valid).unwrap();
        unbounded_coord["meshes"][1]["vertices"][0]["position"][0] =
            serde_json::json!(MAX_BACKGROUND_COORDINATE as f64 + 1.0);
        let unbounded_coord = serde_json::to_string(&unbounded_coord).unwrap();
        assert!(decode_scene_descriptor(&unbounded_coord, vec![vec![255; 4]]).is_err());

        let mut too_many: serde_json::Value = serde_json::from_str(&valid).unwrap();
        too_many["background_mesh_indices"] = serde_json::Value::Array(
            (0..=MAX_BACKGROUND_MESHES)
                .map(|i| serde_json::json!(i))
                .collect(),
        );
        let too_many = serde_json::to_string(&too_many).unwrap();
        assert!(decode_scene_descriptor(&too_many, vec![vec![255; 4]]).is_err());
    }

    #[test]
    fn accepts_measured_prop_scene_caps_and_rejects_over_limit_counts() {
        let measured = scene_with_counts(467, 507);
        let loaded = decode_scene_descriptor(&measured, vec![vec![255; 4]]).unwrap();
        assert_eq!(loaded.scene.meshes.len(), 467);
        assert_eq!(loaded.scene.instances.len(), 507);

        let too_many_meshes = scene_with_counts(MAX_MESHES + 1, 2);
        assert!(decode_scene_descriptor(&too_many_meshes, vec![vec![255; 4]]).is_err());

        let too_many_instances = scene_with_counts(1, MAX_INSTANCES + 1);
        assert!(decode_scene_descriptor(&too_many_instances, vec![vec![255; 4]]).is_err());
    }

    #[test]
    fn simulation_binding_is_explicit_and_maps_heading_from_scene_forward() {
        let binding = SimSceneBinding::from_spawn(
            [10.0, 20.0],
            std::f64::consts::FRAC_PI_4,
            Vec3::new(5.0, 7.0, 2.0),
            Vec3::new(0.0, 1.0, 0.0),
            2.0,
            "provisional visual alignment",
        )
        .unwrap();
        let (position, heading) = binding
            .map_pose(11.0, 22.0, std::f64::consts::FRAC_PI_4)
            .unwrap();
        let root_two = 2.0_f32.sqrt();
        assert!((position.x - (5.0 - root_two)).abs() < 1.0e-6);
        assert!((position.y - (3.0 * root_two + 7.0)).abs() < 1.0e-6);
        assert_eq!(position.z, 2.0);
        assert!((heading - std::f32::consts::FRAC_PI_2).abs() < 1.0e-6);
        assert!(
            SimSceneBinding::from_spawn(
                [0.0, 0.0],
                f64::NAN,
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                1.0,
                "invalid anchor",
            )
            .is_err()
        );
        assert!(binding.map_pose(f64::NAN, 0.0, 0.0).is_err());
    }
}
