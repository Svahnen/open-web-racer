//! RCP v1 documents (car + vehicle) for each design.
//!
//! The car body uses one palette texture (8x1, nearest) addressed by per-vertex
//! UVs, so a paint option is just a recoloured palette. The shared wheel mesh
//! is stored in `Car::wheel_mesh`.

use crate::design::{DESIGNS, Design};
use crate::mesh::{Geometry, Tri, to_rcp, triangulate};
use crate::palette::{PAINTS, default_palette, palette};
use racer_pack_schema::buffer::{BufferBuilder, ElementKind};
use racer_pack_schema::car::{
    CameraAnchor, Car, CarLod, CollisionBox, LightAnchor, LightKind, MaterialSlot, PaintOption,
    PaintScheme, PaintSet, PaletteEntry, WheelAnchor,
};
use racer_pack_schema::common::{AlphaMode, Material, Mesh, Primitive, Texture, WrapMode};
use racer_pack_schema::vehicle::{
    ArcadeVehicleParams, ClassicVehicleParams, EngineSynthParams, VehicleDef,
};
use racer_pack_schema::{BufferBytes, Manifest, Pack, RCP_SCHEMA_ID};

fn add_mesh(b: &mut BufferBuilder, name: &str, tris: &[Tri]) -> Mesh {
    let g = Geometry::from_tris(tris);
    let uv: Vec<f32> = g
        .roles
        .iter()
        .flat_map(|r| {
            let u = (*r as usize as f32 + 0.5) / 8.0;
            [u, 0.5, u, 0.5, u, 0.5]
        })
        .collect();
    let positions = b.add_f32(ElementKind::Vec3, &g.positions);
    let normals = b.add_f32(ElementKind::Vec3, &g.normals);
    let uvs = b.add_f32(ElementKind::Vec2, &uv);
    let indices = b.add_u16(&g.indices);
    Mesh {
        name: name.to_string(),
        positions,
        normals: Some(normals),
        uv: Some(uvs),
        colors: None,
        indices,
        primitives: vec![Primitive {
            material: 0,
            first_index: 0,
            index_count: g.indices.len() as u32,
        }],
        triangle_surface: None,
    }
}

fn car_document(d: &Design) -> (Car, String, Vec<u8>) {
    let uri = format!("buffers/{}.bin", d.id);
    let mut b = BufferBuilder::new(&uri);
    let body = add_mesh(&mut b, "body", &triangulate(&d.body_faces()));
    let wheel = add_mesh(&mut b, "wheel", &triangulate(&d.wheel_faces()));
    let pal = default_palette(d);
    let pixels: Vec<u8> = pal.iter().flatten().copied().collect();
    let pixel_accessor = b.add_rgba8(&pixels);
    let indexed = b.add_u8(&[0, 1, 2, 3, 4, 5, 6, 7]);
    let (table, uri, bytes) = b.finish();

    let wheels = d
        .wheel_centres()
        .iter()
        .map(|(name, c)| {
            let p = to_rcp(*c);
            WheelAnchor {
                name: (*name).to_string(),
                position: [p[0] as f32, p[1] as f32, p[2] as f32],
                radius_m: d.wheel.radius as f32,
                width_m: d.wheel.width as f32,
                steered: name.starts_with("wheel_f"),
                driven: if name.starts_with("wheel_f") {
                    d.wheel.front_driven
                } else {
                    d.wheel.rear_driven
                },
            }
        })
        .collect();

    let nose = d.stations[1];
    let tail = d.stations[d.stations.len() - 2];
    let lamp = |u: f64, w: f64, h: f64, kind: LightKind, dir: [f32; 3], color: [f32; 4]| {
        let p = to_rcp([u, w, h]);
        LightAnchor {
            kind,
            position: [p[0] as f32, p[1] as f32, p[2] as f32],
            direction: dir,
            color,
        }
    };
    let lamp_h = |s: &crate::design::Station| s.y_bottom + (s.y_top - s.y_bottom) * 0.68;
    let lights = vec![
        lamp(
            nose.u,
            -nose.w_shoulder * 0.62,
            lamp_h(&nose),
            LightKind::Head,
            [0.0, 0.0, -1.0],
            [1.0, 0.96, 0.8, 1.0],
        ),
        lamp(
            nose.u,
            nose.w_shoulder * 0.62,
            lamp_h(&nose),
            LightKind::Head,
            [0.0, 0.0, -1.0],
            [1.0, 0.96, 0.8, 1.0],
        ),
        lamp(
            tail.u,
            -tail.w_shoulder * 0.66,
            lamp_h(&tail),
            LightKind::Brake,
            [0.0, 0.0, 1.0],
            [1.0, 0.1, 0.08, 1.0],
        ),
        lamp(
            tail.u,
            tail.w_shoulder * 0.66,
            lamp_h(&tail),
            LightKind::Brake,
            [0.0, 0.0, 1.0],
            [1.0, 0.1, 0.08, 1.0],
        ),
    ];

    let roof = d.roof_height();
    let chase_dir = {
        let v = [0.0_f32, -0.12, -1.0];
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let options = PAINTS
        .iter()
        .map(|s| PaintOption {
            name: s.id.to_string(),
            scheme: PaintScheme::Recolour {
                entries: vec![
                    PaletteEntry {
                        index: 0,
                        color: s.body,
                    },
                    PaletteEntry {
                        index: 1,
                        color: s.accent,
                    },
                ],
            },
        })
        .collect();

    let car = Car {
        id: d.id.to_string(),
        name: d.name.to_string(),
        data: table,
        lods: vec![CarLod {
            mesh: body,
            max_distance_m: None,
        }],
        material_slots: vec![MaterialSlot {
            name: "palette".into(),
            material: Material {
                name: "palette".into(),
                texture: Some(0),
                base_color: [1.0; 4],
                alpha_mode: AlphaMode::Opaque,
                alpha_cutoff: 0.5,
                double_sided: false,
                unlit: false,
                animation: None,
            },
            paintable: true,
        }],
        textures: vec![Texture {
            name: "palette".into(),
            width: 8,
            height: 1,
            pixels: pixel_accessor,
            wrap_u: WrapMode::Clamp,
            wrap_v: WrapMode::Clamp,
            smooth: false,
        }],
        wheel_mesh: Some(wheel),
        paint: Some(PaintSet {
            texture: 0,
            indexed_pixels: indexed,
            base_palette: pal.to_vec(),
            options,
        }),
        wheels,
        lights,
        cameras: vec![
            CameraAnchor {
                name: "chase".into(),
                position: [0.0, (roof + 0.9) as f32, (d.length() * 0.5 + 3.6) as f32],
                forward: chase_dir,
                fov_deg: 70.0,
            },
            CameraAnchor {
                name: "driver".into(),
                position: [-0.35, (roof - 0.22) as f32, -0.1],
                forward: [0.0, 0.0, -1.0],
                fov_deg: 80.0,
            },
        ],
        mirrors: vec![],
        collision: CollisionBox {
            half_extents_m: [
                d.max_half_width() as f32,
                (roof * 0.5) as f32,
                (d.length() * 0.5) as f32,
            ],
            center: [0.0, (roof * 0.5) as f32, 0.0],
        },
    };
    (car, uri, bytes)
}

fn vehicle_document(d: &Design) -> VehicleDef {
    let a = &d.arcade;
    let c = &d.classic;
    VehicleDef {
        id: d.id.to_string(),
        name: d.name.to_string(),
        car: Some(d.id.to_string()),
        arcade: Some(ArcadeVehicleParams {
            mass_kg: a.mass_kg,
            wheelbase_m: d.wheelbase as f32,
            track_width_m: d.wheel.track as f32,
            wheel_radius_m: d.wheel.radius as f32,
            peak_torque_nm: a.peak_torque_nm,
            idle_rpm: a.idle_rpm,
            redline_rpm: a.redline_rpm,
            gear_ratios: a.gear_ratios.to_vec(),
            reverse_ratio: a.reverse_ratio,
            final_drive: a.final_drive,
            drivetrain_efficiency: 0.88,
            max_brake_force_n: a.max_brake_force_n,
            drag_area_m2: a.drag_area_m2,
            downforce_area_m2: a.downforce_area_m2,
            rolling_resistance: a.rolling_resistance,
            tyre_grip: a.tyre_grip,
            max_steer_angle_rad: a.max_steer_angle_rad,
            steer_falloff_speed_mps: a.steer_falloff_speed_mps,
            max_speed_mps: a.max_speed_mps,
            front_drive_bias: a.front_drive_bias,
            steer_rate_per_s: a.steer_rate_per_s,
            steer_return_per_s: a.steer_return_per_s,
            lateral_grip_per_s: a.lateral_grip_per_s,
        }),
        classic: Some(ClassicVehicleParams {
            upshift_mps: c.upshift_mps.to_vec(),
            downshift_mps: c.downshift_mps.to_vec(),
            top_speed_mps: c.top_speed_mps,
            accel_scale: c.accel_scale,
            coast_scale: c.coast_scale,
            brake_scale: c.brake_scale,
            shift_seconds: c.shift_seconds,
            reverse_speed_mps: c.reverse_speed_mps,
            steer_gain: c.steer_gain,
            grip: c.grip,
            low_speed_yaw_rate: c.low_speed_yaw_rate,
            yaw_boost_speed_mps: c.yaw_boost_speed_mps,
        }),
        model: None,
        ai: None,
        engine_sound: Some(EngineSynthParams {
            idle_rpm: a.idle_rpm,
            redline_rpm: a.redline_rpm,
            cylinders: a.sound.cylinders,
            harmonics: a.sound.harmonics.to_vec(),
            filter_hz_idle: a.sound.filter_hz_idle,
            filter_hz_redline: a.sound.filter_hz_redline,
            noise_gain: a.sound.noise_gain,
            gain_idle: a.sound.gain_idle,
            gain_redline: a.sound.gain_redline,
        }),
    }
}

/// The complete pack of demo cars and vehicles.
pub fn pack() -> Pack {
    let mut buffers = BufferBytes::new();
    let mut cars = Vec::new();
    let mut vehicles = Vec::new();
    for d in DESIGNS {
        let (car, uri, bytes) = car_document(d);
        buffers.insert(uri, bytes);
        cars.push(car);
        vehicles.push(vehicle_document(d));
    }
    Pack {
        manifest: Some(Manifest {
            schema: RCP_SCHEMA_ID.to_string(),
            id: "project-demo-cars".into(),
            name: "Project demo cars".into(),
            version: Some("1.0.0".into()),
            license: "Apache-2.0".into(),
            author: "racer contributors".into(),
            description: Some(
                "Four low-poly demo cars generated by racer-cargen with tuned classic and generic arcade handling parameters."
                    .into(),
            ),
            documents: Default::default(),
        }),
        cars,
        vehicles,
        buffers,
        ..Pack::default()
    }
}

/// Palette of paint scheme `index` applied to a design (for tests and tools).
pub fn scheme_palette(index: usize) -> [[u8; 4]; 8] {
    palette(&PAINTS[index])
}
