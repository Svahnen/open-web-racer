//! Shared project-owned collision derivation for bounded imported previews.
//!
//! This geometric policy is provisional and does not claim reference collision
//! fidelity. Keeping it in `racer-content` lets local prediction and an
//! owner-configured server derive the same bounded open-drive collision data.

use crate::LoadedScene;
use racer_render::{MaterialAlphaMode, Vec3};
use racer_sim::{
    PreviewCollisionWorld, PreviewGroundTriangle, PreviewWallSegment, TrackEdgeRow, Vec2,
};
use std::collections::BTreeSet;

pub fn derive_preview_collision(
    loaded: &LoadedScene,
    units_per_sim_unit: f64,
) -> Result<PreviewCollisionWorld, String> {
    const MAX_SOURCE_TRIANGLES: usize =
        PreviewCollisionWorld::MAX_GROUND_TRIANGLES + PreviewCollisionWorld::MAX_WALL_SEGMENTS;
    const WALKABLE_NORMAL_Z: f64 = 0.65;
    const WALL_NORMAL_Z: f64 = 0.25;
    const MIN_WALL_HEIGHT_FRACTION: f64 = 0.25;

    if !units_per_sim_unit.is_finite()
        || !(0.0..=100.0).contains(&units_per_sim_unit)
        || units_per_sim_unit == 0.0
    {
        return Err("preview collision scale is outside supported bounds".into());
    }
    let scene = &loaded.scene;
    let track_meshes = loaded
        .track_mesh_indices
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if track_meshes.is_empty() || track_meshes.len() != loaded.track_mesh_indices.len() {
        return Err("preview track mesh selection is empty or duplicated".into());
    }
    let track_instances = scene
        .instances
        .iter()
        .copied()
        .filter(|instance| track_meshes.contains(&instance.mesh_index))
        .collect::<Vec<_>>();
    if track_instances.is_empty() {
        return Err("preview scene has no instance for its designated track mesh".into());
    }
    let source_triangles = track_instances
        .iter()
        .try_fold(0usize, |total, instance| {
            let mesh = scene.meshes.get(instance.mesh_index as usize)?;
            total.checked_add(mesh.indices.len() / 3)
        })
        .ok_or_else(|| "preview track triangle count overflow".to_string())?;
    if source_triangles == 0 || source_triangles > MAX_SOURCE_TRIANGLES {
        return Err("preview track triangles exceed the collision derivation limit".into());
    }

    let vehicle_instance = scene
        .instances
        .get(loaded.vehicle_instance_index)
        .ok_or_else(|| "preview vehicle instance is outside the scene".to_string())?;
    let vehicle_mesh = scene
        .meshes
        .get(vehicle_instance.mesh_index as usize)
        .ok_or_else(|| "preview vehicle mesh is outside the scene".to_string())?;
    let mut vehicle_radius_scene = 0.0_f64;
    let mut vehicle_min_z = f64::INFINITY;
    let mut vehicle_max_z = f64::NEG_INFINITY;
    for vertex in &vehicle_mesh.vertices {
        let point = vertex.position * vehicle_instance.scale;
        vehicle_radius_scene =
            vehicle_radius_scene.max(f64::from(point.x).hypot(f64::from(point.y)));
        vehicle_min_z = vehicle_min_z.min(f64::from(point.z));
        vehicle_max_z = vehicle_max_z.max(f64::from(point.z));
    }
    let vehicle_height = vehicle_max_z - vehicle_min_z;
    let vehicle_radius = vehicle_radius_scene / units_per_sim_unit;
    if !vehicle_radius.is_finite()
        || vehicle_radius <= 0.0
        || !vehicle_height.is_finite()
        || vehicle_height <= 1.0e-4
    {
        return Err("preview vehicle has no usable finite contact bounds".into());
    }

    let anchor = loaded.spawn_pose_mapping.anchor_scene_position_m;
    let forward = loaded.spawn_pose_mapping.anchor_scene_forward;
    let heading_offset = f64::from(forward.y.atan2(forward.x));
    if !anchor.x.is_finite()
        || !anchor.y.is_finite()
        || !anchor.z.is_finite()
        || !heading_offset.is_finite()
    {
        return Err("preview scene anchor is non-finite".into());
    }
    let (sin, cos) = heading_offset.sin_cos();
    let scene_to_sim =
        |point: Vec3| preview_scene_point_to_sim(point, anchor, sin, cos, units_per_sim_unit);
    let mut ground = Vec::new();
    let mut walls = Vec::new();
    let mut wall_keys = BTreeSet::new();
    for instance in track_instances {
        let mesh = &scene.meshes[instance.mesh_index as usize];
        for indices in mesh.indices.chunks_exact(3) {
            let mut points = [Vec3::ZERO; 3];
            let mut materials = [None; 3];
            for corner in 0..3 {
                let vertex = mesh.vertices.get(indices[corner] as usize).ok_or_else(|| {
                    "preview track triangle has an invalid vertex index".to_string()
                })?;
                points[corner] = instance.transform_point(vertex.position);
                materials[corner] = scene.materials.get(vertex.material as usize);
            }
            if materials.iter().all(|material| {
                material.is_some_and(|material| {
                    material.alpha_mode == MaterialAlphaMode::StraightAlpha
                        || material.tint[3] < 0.5
                })
            }) {
                continue;
            }
            let sim_points = points
                .map(&scene_to_sim)
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
            let sim_vertices = [
                [sim_points[0].x, sim_points[0].y, f64::from(points[0].z)],
                [sim_points[1].x, sim_points[1].y, f64::from(points[1].z)],
                [sim_points[2].x, sim_points[2].y, f64::from(points[2].z)],
            ];
            let Some(normal) = geometric_normal_sim(sim_vertices) else {
                continue;
            };
            let world_triangle = PreviewGroundTriangle {
                vertices: sim_vertices,
            };
            if normal[2].abs() >= WALKABLE_NORMAL_Z {
                if ground.len() >= PreviewCollisionWorld::MAX_GROUND_TRIANGLES {
                    return Err("preview walkable triangles exceed the collision limit".into());
                }
                ground.push(world_triangle);
            } else if normal[2].abs() <= WALL_NORMAL_Z {
                let z_min = points
                    .iter()
                    .map(|point| f64::from(point.z))
                    .fold(f64::INFINITY, f64::min);
                let z_max = points
                    .iter()
                    .map(|point| f64::from(point.z))
                    .fold(f64::NEG_INFINITY, f64::max);
                if z_max - z_min < vehicle_height * MIN_WALL_HEIGHT_FRACTION {
                    continue;
                }
                let mut order = [0usize, 1, 2];
                order.sort_by(|a, b| points[*a].z.total_cmp(&points[*b].z));
                let start = scene_to_sim(points[order[0]])?;
                let end = scene_to_sim(points[order[1]])?;
                if (start - end).length() <= 1.0e-4 {
                    continue;
                }
                let key = canonical_wall_key(start, end, z_min, z_max);
                if wall_keys.insert(key) {
                    if walls.len() >= PreviewCollisionWorld::MAX_WALL_SEGMENTS {
                        return Err("preview wall segments exceed the collision limit".into());
                    }
                    walls.push(PreviewWallSegment {
                        start,
                        end,
                        min_z: z_min,
                        max_z: z_max,
                    });
                }
            }
        }
    }
    let edge_rows = loaded
        .track_edge_rows_scene
        .iter()
        .map(|row| {
            let origin = scene_to_sim(row.position)?;
            let rotate = |v: [f32; 2]| {
                let (x, y) = (f64::from(v[0]), f64::from(v[1]));
                Vec2::new(x * cos + y * sin, -x * sin + y * cos)
            };
            Ok(TrackEdgeRow {
                x: origin.x,
                y: origin.y,
                z: f64::from(row.position.z),
                forward: rotate(row.forward),
                right: rotate(row.right),
                left_distance: f64::from(row.left_distance) / units_per_sim_unit,
                right_distance: f64::from(row.right_distance) / units_per_sim_unit,
                block: row.block,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let collision = PreviewCollisionWorld {
        ground,
        walls,
        vehicle_radius,
        vehicle_min_z,
        vehicle_max_z,
        initial_ground_z: f64::from(anchor.z),
        units_per_sim_unit,
        edge_rows,
    };
    collision.validate().map_err(str::to_string)?;
    collision
        .validate_initial_support()
        .map_err(str::to_string)?;
    Ok(collision)
}

fn preview_scene_point_to_sim(
    point: Vec3,
    anchor: Vec3,
    sin: f64,
    cos: f64,
    units_per_sim_unit: f64,
) -> Result<Vec2, String> {
    let dx = (f64::from(point.x) - f64::from(anchor.x)) / units_per_sim_unit;
    let dy = (f64::from(point.y) - f64::from(anchor.y)) / units_per_sim_unit;
    let transformed = Vec2::new(dx * cos + dy * sin, -dx * sin + dy * cos);
    if !transformed.x.is_finite()
        || !transformed.y.is_finite()
        || transformed.x.abs() > 1.0e6
        || transformed.y.abs() > 1.0e6
    {
        return Err("preview collision coordinate exceeds its world bound".into());
    }
    Ok(transformed)
}

fn geometric_normal_sim(points: [[f64; 3]; 3]) -> Option<[f64; 3]> {
    let a = [
        points[1][0] - points[0][0],
        points[1][1] - points[0][1],
        points[1][2] - points[0][2],
    ];
    let b = [
        points[2][0] - points[0][0],
        points[2][1] - points[0][1],
        points[2][2] - points[0][2],
    ];
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    (length.is_finite() && length > 1.0e-10).then(|| cross.map(|value| value / length))
}

fn canonical_wall_key(start: Vec2, end: Vec2, min_z: f64, max_z: f64) -> [i64; 6] {
    let quantize = |value: f64| (value * 1000.0).round() as i64;
    let start_key = [quantize(start.x), quantize(start.y)];
    let end_key = [quantize(end.x), quantize(end.y)];
    let (a, b) = if start_key <= end_key {
        (start_key, end_key)
    } else {
        (end_key, start_key)
    };
    [a[0], a[1], b[0], b[1], quantize(min_z), quantize(max_z)]
}
