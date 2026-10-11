//! Native ingest of an RCP v1 track into the engine: render scene, simulation
//! runtime and preview collision.
//!
//! Subset implemented here (see `docs/TRACKGEN.md`):
//!
//! * chunks and instanced props become scene meshes/instances (yaw-only,
//!   uniformly scaled props are instanced, anything else is baked);
//! * every texture is resampled to one layer size; untextured materials use a
//!   white layer; `cutout` materials rely on the renderer's alpha discard and
//!   `blend` on straight alpha; `unlit` is approximated by pointing vertex
//!   normals at the light so the diffuse term is 1;
//! * the panorama sky becomes a camera-centred dome of background meshes;
//! * drivable ground is every triangle close to the centerline surface;
//!   centerline widths become the road-edge wall rows; wall polylines become
//!   wall segments; gates become circular checkpoints;
//! * surface types, texture animation, triggered/animated props, gradient
//!   skies, mirror/reverse (generator level) are not interpreted by the engine.

use racer_pack_schema::common::{AlphaMode, Mesh, WrapMode};
use racer_pack_schema::track::{SkyBackdrop, Track};
use racer_pack_schema::{BufferBytes, Pack};
use racer_render::{
    IndexedMesh, Material, MaterialAlphaMode, MeshInstance, MeshVertex, RgbaTextureArray, Scene3d,
    SceneFog, UvWrap, Vec3, VehicleMesh, project_car_mesh,
};
use racer_sim::{
    Checkpoint, PreviewCollisionWorld, PreviewGroundTriangle, PreviewWallSegment, TrackEdgeRow,
    TrackRuntime, Vec2,
};
use std::collections::HashMap;

const SKY_RADIUS: f32 = 800.0;
const SKY_SPAN_DEG: f64 = 100.0;
const LIGHT_DIRECTION: Vec3 = Vec3::new(-0.35, -0.45, 0.82);
const GROUND_HEIGHT_TOLERANCE: f64 = 0.6;
const CORRIDOR_MARGIN: f64 = 0.5;
const MIN_WALKABLE_NORMAL_Z: f64 = 0.65;

/// A start-grid slot in simulation coordinates (slot 0 is the origin).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RcpSpawnSlot {
    pub x: f64,
    pub y: f64,
    pub heading: f64,
}

/// A track ready for rendering and simulation.
#[derive(Clone, Debug)]
pub struct RcpTrackScene {
    pub track_id: String,
    pub name: String,
    pub scene: Scene3d,
    pub vehicle_instance_index: usize,
    pub track_mesh_indices: Vec<u32>,
    pub runtime: TrackRuntime,
    pub collision: PreviewCollisionWorld,
    /// Scene-space position of the simulation origin (spawn slot 0) and its
    /// forward direction in the scene XY plane.
    pub anchor_scene_position: Vec3,
    pub anchor_scene_forward: Vec3,
    pub spawn_slots: Vec<RcpSpawnSlot>,
    /// The track document without geometry (centerline lanes, `ai`, gates),
    /// for computer drivers; its frame is the pack's, not the simulation's.
    pub pack_track: Track,
    pub laps: u32,
    pub ground_triangles: usize,
    /// Distance fog from the pack's horizon haze (colour and distance), also
    /// switching on the renderer's warm/cool scene grading.
    pub fog: Option<SceneFog>,
}

impl RcpTrackScene {
    /// Replace the built-in placeholder car (the designated vehicle instance)
    /// with `vehicle`: its materials are appended and tint the scene's white
    /// page.
    pub fn set_vehicle(&mut self, vehicle: &VehicleMesh) -> Result<(), String> {
        let white = self
            .scene
            .materials
            .iter()
            .find(|m| m.name == "car-paint")
            .map(|m| m.texture)
            .ok_or("track scene has no vehicle material")?;
        let instance = self
            .scene
            .instances
            .get(self.vehicle_instance_index)
            .ok_or("track scene has no vehicle instance")?;
        let mesh_index = instance.mesh_index as usize;
        let base = self.scene.materials.len() as u32;
        let mut mesh = vehicle.mesh.clone();
        for vertex in &mut mesh.vertices {
            vertex.material += base;
        }
        if !mesh.is_well_formed() || mesh_index >= self.scene.meshes.len() {
            return Err("vehicle mesh is malformed".into());
        }
        for material in &vehicle.materials {
            let mut material = material.clone();
            material.texture = white;
            self.scene.materials.push(material);
        }
        self.scene.meshes[mesh_index] = mesh;
        Ok(())
    }
}

fn err<T>(m: impl Into<String>) -> Result<T, String> {
    Err(m.into())
}

/// RCP (x, y up, z) to scene (x, -z, up).
fn to_scene(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], -p[2], p[1])
}

fn to_scene64(p: [f32; 3]) -> [f64; 3] {
    [f64::from(p[0]), -f64::from(p[2]), f64::from(p[1])]
}

/// Bilinear resample with wrap-around.
fn resample(src: &[u8], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u8> {
    if sw == dw && sh == dh {
        return src.to_vec();
    }
    let mut out = vec![0u8; dw * dh * 4];
    for y in 0..dh {
        let fy = (y as f32 + 0.5) * sh as f32 / dh as f32 - 0.5;
        let y0 = fy.floor();
        let ty = fy - y0;
        let (y0, y1) = (
            (y0 as isize).rem_euclid(sh as isize) as usize,
            (y0 as isize + 1).rem_euclid(sh as isize) as usize,
        );
        for x in 0..dw {
            let fx = (x as f32 + 0.5) * sw as f32 / dw as f32 - 0.5;
            let x0 = fx.floor();
            let tx = fx - x0;
            let (x0, x1) = (
                (x0 as isize).rem_euclid(sw as isize) as usize,
                (x0 as isize + 1).rem_euclid(sw as isize) as usize,
            );
            for c in 0..4 {
                let p = |xx: usize, yy: usize| f32::from(src[(yy * sw + xx) * 4 + c]);
                let top = p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx;
                let bottom = p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx;
                out[(y * dw + x) * 4 + c] = (top * (1.0 - ty) + bottom * ty + 0.5) as u8;
            }
        }
    }
    out
}

fn read_texture_pixels(
    track: &Track,
    index: usize,
    bytes: &BufferBytes,
) -> Result<Vec<u8>, String> {
    let t = &track.textures[index];
    let raw = track.data.read_u32(t.pixels, bytes)?;
    Ok(raw.into_iter().map(|v| v as u8).collect())
}

fn wrap_of(w: WrapMode) -> UvWrap {
    match w {
        WrapMode::Clamp => UvWrap::Clamp,
        WrapMode::Repeat | WrapMode::Mirror => UvWrap::Repeat,
    }
}

struct Converted {
    mesh: IndexedMesh,
}

/// Convert an RCP mesh: duplicates vertices per material, converts axes, and
/// points normals of unlit materials at the light.
fn convert_mesh(
    track: &Track,
    mesh: &Mesh,
    bytes: &BufferBytes,
    xform: &dyn Fn(Vec3) -> Vec3,
    xform_normal: &dyn Fn(Vec3) -> Vec3,
) -> Result<Converted, String> {
    let positions = track.data.read_f32(mesh.positions, bytes)?;
    let normals = match mesh.normals {
        Some(n) => Some(track.data.read_f32(n, bytes)?),
        None => None,
    };
    let uvs = match mesh.uv {
        Some(u) => Some(track.data.read_f32(u, bytes)?),
        None => None,
    };
    let indices = track.data.read_u32(mesh.indices, bytes)?;
    let light = LIGHT_DIRECTION.normalized();
    let mut out = IndexedMesh::default();
    let mut remap: HashMap<(u32, u32), u32> = HashMap::new();
    for prim in &mesh.primitives {
        let unlit = track
            .materials
            .get(prim.material as usize)
            .is_some_and(|m| m.unlit);
        let range = prim.first_index as usize..(prim.first_index + prim.index_count) as usize;
        let tri_indices = indices
            .get(range)
            .ok_or_else(|| format!("mesh {:?} primitive exceeds its indices", mesh.name))?;
        for tri in tri_indices.chunks_exact(3) {
            let mut face_normal = None;
            let mut ids = [0u32; 3];
            for (k, &vi) in tri.iter().enumerate() {
                let key = (vi, prim.material);
                let id = match remap.get(&key) {
                    Some(&id) => id,
                    None => {
                        let i = vi as usize;
                        let p =
                            Vec3::new(positions[i * 3], positions[i * 3 + 1], positions[i * 3 + 2]);
                        let n = match &normals {
                            Some(n) => Vec3::new(n[i * 3], n[i * 3 + 1], n[i * 3 + 2]),
                            None => *face_normal.get_or_insert_with(|| {
                                let v = |j: usize| {
                                    let t = tri[j] as usize;
                                    Vec3::new(
                                        positions[t * 3],
                                        positions[t * 3 + 1],
                                        positions[t * 3 + 2],
                                    )
                                };
                                (v(1) - v(0)).cross(v(2) - v(0)).normalized()
                            }),
                        };
                        let normal = if unlit { light } else { xform_normal(n) };
                        let uv = match &uvs {
                            Some(u) => [u[i * 2], u[i * 2 + 1]],
                            None => [0.0, 0.0],
                        };
                        out.vertices.push(MeshVertex {
                            position: xform(p),
                            normal,
                            uv,
                            material: prim.material,
                        });
                        let id = (out.vertices.len() - 1) as u32;
                        remap.insert(key, id);
                        id
                    }
                };
                ids[k] = id;
            }
            out.indices.extend(ids);
        }
    }
    Ok(Converted { mesh: out })
}

fn quat_yaw(q: [f32; 4]) -> Option<f32> {
    // pure rotation about +Y: x and z components vanish
    (q[0].abs() < 1e-4 && q[2].abs() < 1e-4).then(|| 2.0 * q[1].atan2(q[3]))
}

fn rotate_quat(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let t = [
        2.0 * (y * v[2] - z * v[1]),
        2.0 * (z * v[0] - x * v[2]),
        2.0 * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

/// Dome of background meshes for a panorama sky. Returns mesh indices.
fn push_sky(
    scene: &mut Scene3d,
    track: &Track,
    strip_materials: &[u32],
    fill_above: u32,
    fill_below: u32,
) -> Result<Vec<u32>, String> {
    let sky = track.sky.as_ref().ok_or("track has no sky")?;
    let fraction = f64::from(sky.horizon.height_fraction);
    let top = fraction * SKY_SPAN_DEG;
    let bottom = -(1.0 - fraction) * SKY_SPAN_DEG;
    let strips = strip_materials.len();
    let (cols, rows) = (10usize, 12usize);
    let point = |az: f64, el: f64| {
        let (ce, se) = (el.to_radians().cos(), el.to_radians().sin());
        Vec3::new(
            (f64::from(SKY_RADIUS) * ce * az.cos()) as f32,
            (-f64::from(SKY_RADIUS) * ce * az.sin()) as f32,
            (f64::from(SKY_RADIUS) * se) as f32,
        )
    };
    let vertex = |p: Vec3, uv: [f32; 2], material: u32| MeshVertex {
        position: p,
        normal: Vec3::new(-p.x, -p.y, -p.z).normalized(),
        uv,
        material,
    };
    let mut indices = Vec::new();
    for (strip, &material) in strip_materials.iter().enumerate() {
        let mut mesh = IndexedMesh::default();
        let a0 = std::f64::consts::TAU * strip as f64 / strips as f64;
        let a1 = std::f64::consts::TAU * (strip + 1) as f64 / strips as f64;
        for r in 0..rows {
            for c in 0..cols {
                let (u0, u1) = (c as f64 / cols as f64, (c + 1) as f64 / cols as f64);
                let (v0, v1) = (r as f64 / rows as f64, (r + 1) as f64 / rows as f64);
                let az = |u: f64| a0 + (a1 - a0) * u;
                let el = |v: f64| top + (bottom - top) * v;
                mesh.quad(
                    [
                        point(az(u0), el(v0)),
                        point(az(u1), el(v0)),
                        point(az(u1), el(v1)),
                        point(az(u0), el(v1)),
                    ],
                    [
                        [u0 as f32, v0 as f32],
                        [u1 as f32, v0 as f32],
                        [u1 as f32, v1 as f32],
                        [u0 as f32, v1 as f32],
                    ],
                    material,
                );
                let base = mesh.vertices.len() - 4;
                for k in 0..4 {
                    let p = mesh.vertices[base + k].position;
                    mesh.vertices[base + k].normal = Vec3::new(-p.x, -p.y, -p.z).normalized();
                }
            }
        }
        scene.meshes.push(mesh);
        indices.push((scene.meshes.len() - 1) as u32);
    }
    // caps above the first and below the last ring
    let mut caps = IndexedMesh::default();
    let segs = 24usize;
    for s in 0..segs {
        let a = std::f64::consts::TAU * s as f64 / segs as f64;
        let b = std::f64::consts::TAU * (s + 1) as f64 / segs as f64;
        let tri =
            |p0: Vec3, p1: Vec3, p2: Vec3, m: u32| [p0, p1, p2].map(|p| vertex(p, [0.5, 0.5], m));
        let up = tri(point(a, top), point(b, top), point(0.0, 90.0), fill_above);
        caps.triangle(up[0], up[1], up[2]);
        let down = tri(
            point(a, bottom),
            point(b, bottom),
            point(0.0, -90.0),
            fill_below,
        );
        caps.triangle(down[0], down[1], down[2]);
    }
    scene.meshes.push(caps);
    indices.push((scene.meshes.len() - 1) as u32);
    Ok(indices)
}

pub fn build_track_scene(pack: &Pack, track_index: usize) -> Result<RcpTrackScene, String> {
    let track = pack
        .tracks
        .get(track_index)
        .ok_or_else(|| format!("pack has no track {track_index}"))?;
    let bytes = &pack.buffers;
    if track.centerline.len() < 3 || track.spawn_slots.is_empty() || track.checkpoints.is_empty() {
        return err("track needs a centerline, a spawn slot and a checkpoint");
    }

    // Textures: one array with a common layer size plus a white layer.
    let (mut lw, mut lh) = (1usize, 1usize);
    for t in &track.textures {
        lw = lw.max(t.width as usize);
        lh = lh.max(t.height as usize);
    }
    let mut layers: Vec<Vec<u8>> = Vec::with_capacity(track.textures.len() + 1);
    for (i, t) in track.textures.iter().enumerate() {
        let px = read_texture_pixels(track, i, bytes)?;
        layers.push(resample(&px, t.width as usize, t.height as usize, lw, lh));
    }
    let white_layer = layers.len();
    layers.push(vec![255u8; lw * lh * 4]);

    // Materials.
    let mut materials: Vec<Material> = Vec::new();
    for m in &track.materials {
        let (layer, wrap_u, wrap_v) = match m.texture {
            Some(t) => {
                let tex = &track.textures[t as usize];
                (t as usize, wrap_of(tex.wrap_u), wrap_of(tex.wrap_v))
            }
            None => (white_layer, UvWrap::Repeat, UvWrap::Repeat),
        };
        let mut material = Material::atlas_layer(
            m.name.clone(),
            m.base_color,
            layer,
            [0.0, 0.0],
            [1.0, 1.0],
            wrap_u,
            wrap_v,
        );
        if m.alpha_mode == AlphaMode::Blend {
            material.alpha_mode = MaterialAlphaMode::StraightAlpha;
        }
        materials.push(material);
    }
    let car_paint = materials.len() as u32;
    materials.push(Material::whole_layer(
        "car-paint",
        [0.83, 0.23, 0.10, 1.0],
        white_layer,
        UvWrap::Clamp,
    ));
    materials.push(Material::whole_layer(
        "rubber",
        [0.07, 0.08, 0.09, 1.0],
        white_layer,
        UvWrap::Clamp,
    ));
    materials.push(Material::whole_layer(
        "glass",
        [0.12, 0.48, 0.62, 1.0],
        white_layer,
        UvWrap::Clamp,
    ));

    let mut scene = Scene3d {
        meshes: Vec::new(),
        background_mesh_indices: Vec::new(),
        instances: Vec::new(),
        materials,
        textures: RgbaTextureArray {
            width: lw as u32,
            height: lh as u32,
            layers,
        },
    };

    // Geometry chunks.
    let axis = |p: Vec3| Vec3::new(p.x, -p.z, p.y);
    let mut chunk_mesh_ids: Vec<Option<u32>> = Vec::new();
    for chunk in &track.chunks {
        let converted = convert_mesh(track, &chunk.mesh, bytes, &axis, &axis)?;
        if converted.mesh.vertices.is_empty() {
            chunk_mesh_ids.push(None);
            continue;
        }
        scene.meshes.push(converted.mesh);
        let id = (scene.meshes.len() - 1) as u32;
        scene.instances.push(identity_instance(id));
        chunk_mesh_ids.push(Some(id));
    }

    // Props: instanced when yaw-only and uniformly scaled, otherwise baked.
    let mut proto_ids: Vec<Option<u32>> = vec![None; track.prop_meshes.len()];
    let mut baked = IndexedMesh::default();
    for prop in &track.props {
        let t = &prop.transform;
        let uniform =
            (t.scale[0] - t.scale[1]).abs() < 1e-4 && (t.scale[1] - t.scale[2]).abs() < 1e-4;
        let proto = prop.mesh as usize;
        match (quat_yaw(t.rotation), uniform) {
            (Some(yaw), true) => {
                if proto_ids[proto].is_none() {
                    let c = convert_mesh(track, &track.prop_meshes[proto], bytes, &axis, &axis)?;
                    if c.mesh.vertices.is_empty() {
                        continue;
                    }
                    scene.meshes.push(c.mesh);
                    proto_ids[proto] = Some((scene.meshes.len() - 1) as u32);
                }
                scene.instances.push(MeshInstance {
                    mesh_index: proto_ids[proto].unwrap_or(0),
                    translation: to_scene(t.translation),
                    heading: yaw,
                    ground_normal: Vec3::UP,
                    scale: t.scale[0],
                });
            }
            _ => {
                let (q, s, tr) = (t.rotation, t.scale, t.translation);
                let place = move |p: Vec3| {
                    let scaled = [p.x * s[0], p.y * s[1], p.z * s[2]];
                    let r = rotate_quat(q, scaled);
                    to_scene([r[0] + tr[0], r[1] + tr[1], r[2] + tr[2]])
                };
                let place_n = move |n: Vec3| {
                    let r = rotate_quat(q, [n.x, n.y, n.z]);
                    to_scene(r).normalized()
                };
                let c = convert_mesh(track, &track.prop_meshes[proto], bytes, &place, &place_n)?;
                let base = baked.vertices.len() as u32;
                baked.vertices.extend(c.mesh.vertices);
                baked
                    .indices
                    .extend(c.mesh.indices.iter().map(|i| i + base));
            }
        }
    }
    if !baked.vertices.is_empty() {
        scene.meshes.push(baked);
        let id = (scene.meshes.len() - 1) as u32;
        scene.instances.push(identity_instance(id));
    }

    // Sky dome.
    if let Some(sky) = &track.sky {
        match &sky.backdrop {
            SkyBackdrop::Panorama { strips } => {
                let mut strip_materials = Vec::new();
                for &t in strips {
                    let tex = &track.textures[t as usize];
                    scene.materials.push(Material::atlas_layer(
                        format!("sky-{t}"),
                        [1.0, 1.0, 1.0, 1.0],
                        t as usize,
                        [0.0, 0.0],
                        [1.0, 1.0],
                        wrap_of(tex.wrap_u),
                        wrap_of(tex.wrap_v),
                    ));
                    strip_materials.push((scene.materials.len() - 1) as u32);
                }
                let fill = |scene: &mut Scene3d, name: &str, c: [f32; 4]| {
                    scene.materials.push(Material::whole_layer(
                        name,
                        c,
                        white_layer,
                        UvWrap::Clamp,
                    ));
                    (scene.materials.len() - 1) as u32
                };
                let above = fill(&mut scene, "sky-fill-above", sky.fill_above);
                let below = fill(&mut scene, "sky-fill-below", sky.fill_below);
                let ids = push_sky(&mut scene, track, &strip_materials, above, below)?;
                scene.background_mesh_indices = ids;
            }
            SkyBackdrop::Gradient { .. } => {}
        }
    }

    // Vehicle template (project-owned placeholder car), last instance.
    scene
        .meshes
        .push(project_car_mesh(car_paint, car_paint + 1, car_paint + 2));
    let car_mesh = (scene.meshes.len() - 1) as u32;
    scene.instances.push(identity_instance(car_mesh));
    let vehicle_instance_index = scene.instances.len() - 1;
    for mesh in &scene.meshes {
        if !mesh.is_well_formed() {
            return err("generated scene mesh is malformed");
        }
        if mesh
            .vertices
            .iter()
            .any(|v| v.material as usize >= scene.materials.len())
        {
            return err("scene mesh references a missing material");
        }
    }

    // Simulation frame: origin at spawn slot 0, heading 0 along the road.
    let slot0 = &track.spawn_slots[0];
    let anchor = to_scene64(slot0.position);
    let heading = f64::from(slot0.heading_rad);
    // heading is CCW from +X seen from above in RCP: forward = (cos h, 0, -sin h)
    let fwd_scene = [heading.cos(), heading.sin()];
    let hoff = fwd_scene[1].atan2(fwd_scene[0]);
    let (sin_h, cos_h) = hoff.sin_cos();
    let to_sim = |p: [f64; 3]| -> Vec2 {
        let (dx, dy) = (p[0] - anchor[0], p[1] - anchor[1]);
        Vec2::new(dx * cos_h + dy * sin_h, -dx * sin_h + dy * cos_h)
    };
    let dir_to_sim = |d: [f64; 2]| -> Vec2 {
        Vec2::new(d[0] * cos_h + d[1] * sin_h, -d[0] * sin_h + d[1] * cos_h)
    };

    let spawn_slots = track
        .spawn_slots
        .iter()
        .map(|s| {
            let p = to_sim(to_scene64(s.position));
            RcpSpawnSlot {
                x: p.x,
                y: p.y,
                heading: f64::from(s.heading_rad) - heading,
            }
        })
        .collect::<Vec<_>>();

    let runtime = TrackRuntime {
        min: Vec2::new(-1.0e7, -1.0e7),
        max: Vec2::new(1.0e7, 1.0e7),
        checkpoints: track
            .checkpoints
            .iter()
            .map(|g| Checkpoint {
                center: to_sim(to_scene64(g.position)),
                radius: f64::from(g.width_m) * 0.5,
            })
            .collect(),
        laps_to_finish: track.default_laps,
    };

    // Edge rows from the centerline.
    let mut edge_rows = Vec::with_capacity(track.centerline.len());
    for seg in &track.centerline {
        let p = to_scene64(seg.position);
        let f = to_scene64(seg.forward);
        let fl = (f[0] * f[0] + f[1] * f[1]).sqrt().max(1e-9);
        let fwd = dir_to_sim([f[0] / fl, f[1] / fl]);
        let sim = to_sim(p);
        edge_rows.push(TrackEdgeRow {
            x: sim.x,
            y: sim.y,
            z: p[2],
            forward: fwd,
            right: Vec2::new(fwd.y, -fwd.x),
            left_distance: f64::from(seg.left_width_m).max(0.5),
            right_distance: f64::from(seg.right_width_m).max(0.5),
            block: 0,
        });
    }

    // Walls.
    let mut walls = Vec::new();
    for w in &track.walls {
        let pts: Vec<[f64; 3]> = w.points.iter().map(|p| to_scene64(*p)).collect();
        let count = if w.closed {
            pts.len()
        } else {
            pts.len().saturating_sub(1)
        };
        for i in 0..count {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            let (sa, sb) = (to_sim(a), to_sim(b));
            if (sa.x - sb.x).hypot(sa.y - sb.y) <= 1e-6 {
                continue;
            }
            let lo = a[2].min(b[2]);
            walls.push(PreviewWallSegment {
                start: sa,
                end: sb,
                min_z: lo - 0.3,
                max_z: a[2].max(b[2]) + f64::from(w.height_m),
            });
        }
    }

    // Ground: triangles close to the centerline surface.
    let corridor = Corridor::new(track);
    let mut ground = Vec::new();
    let mut track_meshes = Vec::new();
    for (ci, chunk) in track.chunks.iter().enumerate() {
        let before = ground.len();
        let m = &chunk.mesh;
        let positions = track.data.read_f32(m.positions, bytes)?;
        let indices = track.data.read_u32(m.indices, bytes)?;
        for tri in indices.chunks_exact(3) {
            let p = |k: usize| {
                let i = tri[k] as usize;
                to_scene64([positions[i * 3], positions[i * 3 + 1], positions[i * 3 + 2]])
            };
            let (a, b, c) = (p(0), p(1), p(2));
            let centroid = [
                (a[0] + b[0] + c[0]) / 3.0,
                (a[1] + b[1] + c[1]) / 3.0,
                (a[2] + b[2] + c[2]) / 3.0,
            ];
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len < 1e-9 || (n[2] / len).abs() < MIN_WALKABLE_NORMAL_Z {
                continue;
            }
            if !corridor.contains(centroid) {
                continue;
            }
            let sa = to_sim(a);
            let sb = to_sim(b);
            let sc = to_sim(c);
            ground.push(PreviewGroundTriangle {
                vertices: [[sa.x, sa.y, a[2]], [sb.x, sb.y, b[2]], [sc.x, sc.y, c[2]]],
            });
        }
        if ground.len() > before {
            if let Some(Some(id)) = chunk_mesh_ids.get(ci) {
                track_meshes.push(*id);
            }
        }
    }
    if ground.is_empty() {
        return err("track has no drivable ground near its centerline");
    }
    let ground_triangles = ground.len();

    // Vehicle contact bounds from the placeholder car.
    let car = &scene.meshes[car_mesh as usize];
    let (mut radius, mut min_z, mut max_z) = (0.0_f64, f64::INFINITY, f64::NEG_INFINITY);
    for v in &car.vertices {
        radius = radius.max(f64::from(v.position.x).hypot(f64::from(v.position.y)));
        min_z = min_z.min(f64::from(v.position.z));
        max_z = max_z.max(f64::from(v.position.z));
    }
    let initial_ground_z = anchor[2];
    let collision = PreviewCollisionWorld {
        ground,
        walls,
        vehicle_radius: radius,
        vehicle_min_z: min_z,
        vehicle_max_z: max_z,
        initial_ground_z,
        units_per_sim_unit: 1.0,
        edge_rows,
    };
    collision.validate().map_err(|e| e.to_string())?;

    let fog = track.sky.as_ref().and_then(|sky| {
        let color = sky.horizon.haze_color?;
        let distance = sky.horizon.haze_distance_m?;
        (distance > 20.0).then_some(SceneFog {
            color: [color[0], color[1], color[2]],
            start: distance * 0.22,
            end: distance,
            amount: 0.94,
        })
    });

    // Computer-driver source: the track document without geometry.
    let mut pack_track = track.clone();
    pack_track.chunks.clear();
    pack_track.props.clear();
    pack_track.prop_meshes.clear();
    pack_track.walls.clear();
    pack_track.textures.clear();
    pack_track.materials.clear();
    pack_track.sky = None;

    Ok(RcpTrackScene {
        track_id: track.id.clone(),
        name: track.name.clone(),
        scene,
        vehicle_instance_index,
        track_mesh_indices: track_meshes,
        runtime,
        collision,
        anchor_scene_position: Vec3::new(anchor[0] as f32, anchor[1] as f32, anchor[2] as f32),
        anchor_scene_forward: Vec3::new(fwd_scene[0] as f32, fwd_scene[1] as f32, 0.0),
        spawn_slots,
        pack_track,
        laps: track.default_laps,
        ground_triangles,
        fog,
    })
}

fn identity_instance(mesh_index: u32) -> MeshInstance {
    MeshInstance {
        mesh_index,
        translation: Vec3::ZERO,
        heading: 0.0,
        ground_normal: Vec3::UP,
        scale: 1.0,
    }
}

/// Spatial index over the centerline used to find drivable ground.
struct Corridor {
    cell: f64,
    x0: f64,
    y0: f64,
    nx: i64,
    ny: i64,
    cells: Vec<Vec<u32>>,
    pos: Vec<[f64; 3]>,
    right: Vec<[f64; 2]>,
    up: Vec<[f64; 3]>,
    left_w: Vec<f64>,
    right_w: Vec<f64>,
}

impl Corridor {
    fn new(track: &Track) -> Self {
        let pos: Vec<[f64; 3]> = track
            .centerline
            .iter()
            .map(|s| to_scene64(s.position))
            .collect();
        let up: Vec<[f64; 3]> = track.centerline.iter().map(|s| to_scene64(s.up)).collect();
        let right: Vec<[f64; 2]> = track
            .centerline
            .iter()
            .map(|s| {
                let f = to_scene64(s.forward);
                let l = (f[0] * f[0] + f[1] * f[1]).sqrt().max(1e-9);
                [f[1] / l, -f[0] / l]
            })
            .collect();
        let max_w = track
            .centerline
            .iter()
            .map(|s| f64::from(s.left_width_m.max(s.right_width_m)))
            .fold(1.0, f64::max);
        let cell = (max_w + 8.0).max(16.0);
        let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for p in &pos {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            y0 = y0.min(p[1]);
            y1 = y1.max(p[1]);
        }
        let nx = ((x1 - x0) / cell).floor() as i64 + 1;
        let ny = ((y1 - y0) / cell).floor() as i64 + 1;
        let mut cells = vec![Vec::new(); (nx * ny) as usize];
        for (i, p) in pos.iter().enumerate() {
            let cx = ((p[0] - x0) / cell).floor() as i64;
            let cy = ((p[1] - y0) / cell).floor() as i64;
            cells[(cy * nx + cx) as usize].push(i as u32);
        }
        Corridor {
            cell,
            x0,
            y0,
            nx,
            ny,
            cells,
            pos,
            right,
            up,
            left_w: track
                .centerline
                .iter()
                .map(|s| f64::from(s.left_width_m))
                .collect(),
            right_w: track
                .centerline
                .iter()
                .map(|s| f64::from(s.right_width_m))
                .collect(),
        }
    }

    /// True when `p` (scene space) lies within the road corridor and close to
    /// the road surface.
    fn contains(&self, p: [f64; 3]) -> bool {
        let cx = ((p[0] - self.x0) / self.cell).floor() as i64;
        let cy = ((p[1] - self.y0) / self.cell).floor() as i64;
        let mut best: Option<(f64, usize)> = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (ix, iy) = (cx + dx, cy + dy);
                if ix < 0 || iy < 0 || ix >= self.nx || iy >= self.ny {
                    continue;
                }
                for &i in &self.cells[(iy * self.nx + ix) as usize] {
                    let s = self.pos[i as usize];
                    let d = (s[0] - p[0]).powi(2) + (s[1] - p[1]).powi(2);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, i as usize));
                    }
                }
            }
        }
        let Some((_, i)) = best else { return false };
        let s = self.pos[i];
        let r = self.right[i];
        let lateral = (p[0] - s[0]) * r[0] + (p[1] - s[1]) * r[1];
        let limit = if lateral >= 0.0 {
            self.right_w[i]
        } else {
            self.left_w[i]
        } + CORRIDOR_MARGIN;
        if lateral.abs() > limit {
            return false;
        }
        let u = self.up[i];
        let dev = ((p[0] - s[0]) * u[0] + (p[1] - s[1]) * u[1] + (p[2] - s[2]) * u[2])
            / u[2].abs().max(0.2);
        dev.abs() <= GROUND_HEIGHT_TOLERANCE
    }
}
