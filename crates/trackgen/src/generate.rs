//! Assembles a complete RCP pack for a [`TrackSpec`].

use crate::ai;
use crate::materials::Mats;
use crate::math::{V3, atan2, cos, fbm2, lerp, sin, v3};
use crate::mesh::MeshData;
use crate::props::{self, Protos};
use crate::road::{KERB_WIDTH_M, Road};
use crate::scenery::{self, Placement};
use crate::spec::{GENERATOR_VERSION, TrackSpec};
use crate::structures;
use crate::style::{ShoulderKind, Style, style_for};
use crate::textures::{
    self, BARRIER_TEX_M, GROUND_TEX_M, KERB_TEX_LENGTH_M, ROAD_TEX_LENGTH_M, id as tex,
};
use crate::world::{World, edge_lat, surface_point};
use racer_pack_schema::buffer::BufferBuilder;
use racer_pack_schema::common::{Texture, Transform, WrapMode};
use racer_pack_schema::track::{
    CenterlineSegment, FrictionClass, Gate, HorizonParams, PropInstance, PropKind, Sky,
    SkyBackdrop, SpawnSlot, SurfaceType, TrackChunk, VariantFlags, WallPolyline, WallSide,
};
use racer_pack_schema::{DocumentIndex, Manifest, Pack, RCP_SCHEMA_ID, Track};
use std::collections::BTreeMap;

/// Surface ids used by generated tracks.
pub mod surf {
    pub const ASPHALT: u8 = 0;
    pub const KERB: u8 = 1;
    pub const SHOULDER: u8 = 2;
    pub const GRASS: u8 = 3;
    pub const SAND: u8 = 4;
    pub const ROCK: u8 = 5;
    pub const CONCRETE: u8 = 6;
    pub const WATER: u8 = 7;
}

const CHUNK_SECTIONS: usize = 32;
const TILE: f64 = 192.0;
const FINE_CELL: f64 = 8.0;
const COARSE_CELL: f64 = 32.0;
const FINE_RANGE: f64 = 80.0;
const BARRIER_HEIGHT: f64 = 1.0;
const BARRIER_THICKNESS: f64 = 0.6;
const FOOTING: f64 = 0.9;
const GRID_SLOTS: usize = 8;

/// Generate the full pack (one track plus its buffer file).
pub fn generate_pack(spec: &TrackSpec) -> Pack {
    let style = style_for(spec.theme);
    let road = Road::build(spec, style.bank_gain, style.max_bank_deg);
    let world = World::new(&road, &style, spec.stream("terrain"));
    let mean_width = 2.0 * road.secs.iter().map(|s| s.hw).sum::<f64>() / road.len() as f64;
    let texs = textures::generate(&style, spec.stream("textures"), mean_width);
    let mats = Mats::build(&style);
    let uri = format!("buffers/{}.bin", spec.id);
    let mut bb = BufferBuilder::new(&uri);

    let textures: Vec<Texture> = texs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let sky = i as u32 >= tex::SKY0;
            let wrap = if sky {
                WrapMode::Clamp
            } else {
                WrapMode::Repeat
            };
            Texture {
                name: t.name.clone(),
                width: t.w,
                height: t.h,
                pixels: bb.add_rgba8(&t.rgba),
                wrap_u: wrap,
                wrap_v: wrap,
                smooth: true,
            }
        })
        .collect();

    let rails = guard_rails(&road, &world, &style);

    // Road, kerbs, shoulders and barriers in chunks of consecutive sections.
    let mut chunks: Vec<TrackChunk> = Vec::new();
    let n = road.len();
    let mut a = 0usize;
    let mut chunk_index = 0;
    while a < n {
        let b = (a + CHUNK_SECTIONS).min(n);
        let mut m = MeshData::new();
        for i in a..b {
            road_strip(&mut m, &road, &style, &mats, i);
            barrier_strip(&mut m, &road, &style, &mats, i, &rails);
        }
        if a == 0 && spec.start_fraction == 0.0 {
            start_line(&mut m, &road, &mats);
        }
        let bounds = bounds_of(&m);
        let mesh = m.finish(&mut bb, &format!("road_{chunk_index:03}"), true);
        chunks.push(TrackChunk {
            mesh,
            bounds: Some(bounds),
        });
        chunk_index += 1;
        a = b;
    }

    // Terrain skirt.
    for (name, m) in terrain_meshes(&world, &style, &mats) {
        let bounds = bounds_of(&m);
        let mesh = m.finish(&mut bb, &name, true);
        chunks.push(TrackChunk {
            mesh,
            bounds: Some(bounds),
        });
    }
    if let Some(level) = style.sea_level {
        let (x0, z0, x1, z1) = world.bounds();
        let (cx, cz) = ((x0 + x1) * 0.5, (z0 + z1) * 0.5);
        let r = 2400.0;
        let mut m = MeshData::new();
        let p = [
            v3(cx - r, level, cz - r),
            v3(cx + r, level, cz - r),
            v3(cx + r, level, cz + r),
            v3(cx - r, level, cz + r),
        ];
        let uvs = |q: V3| [q.x / 22.0, q.z / 22.0];
        let ids = [
            m.vertex(p[0], V3::UP, uvs(p[0])),
            m.vertex(p[1], V3::UP, uvs(p[1])),
            m.vertex(p[2], V3::UP, uvs(p[2])),
            m.vertex(p[3], V3::UP, uvs(p[3])),
        ];
        m.quad(ids, mats.water, surf::WATER);
        let bounds = bounds_of(&m);
        let mesh = m.finish(&mut bb, "water", true);
        chunks.push(TrackChunk {
            mesh,
            bounds: Some(bounds),
        });
    }

    // Props.
    let mut protos: Protos =
        props::build_protos(spec.theme, spec.stream("props"), &mats, mean_width);
    let built = structures::build(&road, start_shift(spec, &road), &world, &style, &mats);
    let mut structure_ids = Vec::new();
    for (name, mesh) in built.meshes {
        protos.meshes.push((name, mesh));
        structure_ids.push((protos.meshes.len() - 1) as u32);
    }
    let mut placements: Vec<Placement> = scenery::place(
        &road,
        &world,
        &style,
        &protos.ids,
        spec.stream("scenery"),
        spec.start_fraction == 0.0,
        &built.keep_clear,
    );
    // structures are modelled in world coordinates: identity placements
    for id in structure_ids {
        placements.push(Placement {
            proto: id,
            pos: V3::ZERO,
            yaw: 0.0,
            scale: 1.0,
        });
    }
    let prop_meshes = protos
        .meshes
        .iter()
        .map(|(name, m)| m.finish(&mut bb, name, false))
        .collect::<Vec<_>>();
    let props = placements
        .iter()
        .map(|p| {
            let half = p.yaw * 0.5;
            PropInstance {
                mesh: p.proto,
                transform: Transform {
                    translation: p.pos.arr(),
                    rotation: [0.0, sin(half) as f32, 0.0, cos(half) as f32],
                    scale: [p.scale as f32; 3],
                },
                kind: PropKind::Static,
                animation_rate_hz: None,
                trigger_radius_m: None,
            }
        })
        .collect();

    // Centreline, walls, grid, gates.
    let centerline: Vec<CenterlineSegment> = road
        .secs
        .iter()
        .zip(ai::segment_lanes(&road))
        .map(|(s, lanes)| {
            let e = edge_lat(&style, s);
            CenterlineSegment {
                position: s.pos.arr(),
                forward: s.fwd.arr(),
                up: s.up.arr(),
                length_m: road.ds as f32,
                left_width_m: e as f32,
                right_width_m: e as f32,
                surface: u32::from(surf::ASPHALT),
                lanes: Some(lanes),
            }
        })
        .collect();
    let walls = [(-1.0, WallSide::Left), (1.0, WallSide::Right)]
        .into_iter()
        .map(|(sign, side)| WallPolyline {
            points: (0..n)
                .step_by(2)
                .map(|i| surface_point(&road.secs[i], sign * edge_lat(&style, &road.secs[i])).arr())
                .collect(),
            height_m: BARRIER_HEIGHT as f32,
            side,
            closed: true,
        })
        .collect();
    let spawn_slots = spawn_grid(&road, &style);
    let checkpoints = gates(&road, &style);

    // Computer-driver data: speed profile from curvature, grid.
    let track_ai = ai::track_ai(&road);

    let sky = Sky {
        backdrop: SkyBackdrop::Panorama {
            strips: (0..textures::SKY_STRIPS as u32)
                .map(|i| tex::SKY0 + i)
                .collect(),
        },
        horizon: HorizonParams {
            height_fraction: textures::SKY_HORIZON_FRACTION as f32,
            parallax: 1.0,
            haze_color: Some([
                style.sky.horizon[0] as f32,
                style.sky.horizon[1] as f32,
                style.sky.horizon[2] as f32,
                1.0,
            ]),
            haze_distance_m: Some(450.0),
        },
        fill_above: rgba(style.sky.zenith),
        fill_below: rgba(style.sky.hill_near),
    };

    let (table, buffer_uri, bytes) = bb.finish();
    let track = Track {
        id: spec.id.clone(),
        name: spec.name.clone(),
        data: table,
        chunks,
        materials: mats.list.clone(),
        textures,
        prop_meshes,
        props,
        sky: Some(sky),
        centerline,
        closed: true,
        walls,
        surfaces: surfaces(&style),
        spawn_slots,
        checkpoints,
        variants: VariantFlags {
            allow_mirror: true,
            allow_reverse: true,
        },
        default_laps: spec.laps,
        ai: Some(track_ai),
    };
    let mut buffers = BTreeMap::new();
    buffers.insert(buffer_uri, bytes);
    let manifest = Manifest {
        schema: RCP_SCHEMA_ID.to_string(),
        id: spec.id.clone(),
        name: spec.name.clone(),
        version: Some(format!("gen{GENERATOR_VERSION}")),
        license: "CC0-1.0".to_string(),
        author: "racer project (procedural)".to_string(),
        description: Some(format!(
            "{} Generated by racer-trackgen ({} km lap).",
            spec.description,
            (road.length / 100.0).round() / 10.0
        )),
        documents: DocumentIndex::default(),
    };
    Pack {
        manifest: Some(manifest),
        tracks: vec![track],
        buffers,
        ..Pack::default()
    }
}

/// Sections the start line was moved along the lap (see `Road::build`).
fn start_shift(spec: &TrackSpec, road: &Road) -> usize {
    let count = road.len();
    ((spec.start_fraction.rem_euclid(1.0)) * count as f64) as usize % count
}

/// The pack as `path -> bytes` (manifest, track document, buffer).
pub fn generate_files(spec: &TrackSpec) -> BTreeMap<String, Vec<u8>> {
    generate_pack(spec)
        .to_files()
        .expect("generated pack serializes")
}

fn rgba(c: [f64; 3]) -> [f32; 4] {
    [c[0] as f32, c[1] as f32, c[2] as f32, 1.0]
}

fn bounds_of(m: &MeshData) -> [[f32; 3]; 2] {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in &m.pos {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    [lo, hi]
}

fn surfaces(style: &Style) -> Vec<SurfaceType> {
    let shoulder = match style.shoulder {
        ShoulderKind::Concrete => SurfaceType {
            name: "shoulder-concrete".into(),
            friction_class: FrictionClass::Paved,
            friction: 0.9,
            rolling_resistance: 0.02,
        },
        ShoulderKind::Gravel => SurfaceType {
            name: "shoulder-gravel".into(),
            friction_class: FrictionClass::Loose,
            friction: 0.62,
            rolling_resistance: 0.06,
        },
        ShoulderKind::Sand | ShoulderKind::RedDirt => SurfaceType {
            name: "shoulder-sand".into(),
            friction_class: FrictionClass::Loose,
            friction: 0.55,
            rolling_resistance: 0.09,
        },
    };
    vec![
        SurfaceType {
            name: "asphalt".into(),
            friction_class: FrictionClass::Paved,
            friction: 1.0,
            rolling_resistance: 0.012,
        },
        SurfaceType {
            name: "kerb".into(),
            friction_class: FrictionClass::Paved,
            friction: 0.95,
            rolling_resistance: 0.016,
        },
        shoulder,
        SurfaceType {
            name: "grass".into(),
            friction_class: FrictionClass::Grass,
            friction: 0.5,
            rolling_resistance: 0.1,
        },
        SurfaceType {
            name: "sand".into(),
            friction_class: FrictionClass::Loose,
            friction: 0.45,
            rolling_resistance: 0.14,
        },
        SurfaceType {
            name: "rock".into(),
            friction_class: FrictionClass::Rough,
            friction: 0.7,
            rolling_resistance: 0.05,
        },
        SurfaceType {
            name: "concrete".into(),
            friction_class: FrictionClass::Paved,
            friction: 0.9,
            rolling_resistance: 0.02,
        },
        SurfaceType {
            name: "water".into(),
            friction_class: FrictionClass::Wet,
            friction: 0.3,
            rolling_resistance: 0.4,
        },
    ]
}

/// Cross-section rows: shoulder|kerb|road|kerb|shoulder edge points.
fn row(style: &Style, sec: &crate::road::Section) -> [V3; 6] {
    let e = edge_lat(style, sec);
    let kw = KERB_WIDTH_M;
    [
        surface_point(sec, -e),
        surface_point(sec, -(sec.hw + kw)),
        surface_point(sec, -sec.hw),
        surface_point(sec, sec.hw),
        surface_point(sec, sec.hw + kw),
        surface_point(sec, e),
    ]
}

fn road_strip(m: &mut MeshData, road: &Road, style: &Style, mats: &Mats, i: usize) {
    let (s0, s1) = (road.at(i as isize), road.at(i as isize + 1));
    let (d0, d1) = (i as f64 * road.ds, (i + 1) as f64 * road.ds);
    let (r0, r1) = (row(style, s0), row(style, s1));
    let (n0, n1) = (s0.up, s1.up);
    let mut quad =
        |a: usize, b: usize, mat: u32, surface: u8, uv: &dyn Fn(V3, f64, f64) -> [f64; 2]| {
            let ids = [
                m.vertex(r0[a], n0, uv(r0[a], d0, 0.0)),
                m.vertex(r0[b], n0, uv(r0[b], d0, 1.0)),
                m.vertex(r1[b], n1, uv(r1[b], d1, 1.0)),
                m.vertex(r1[a], n1, uv(r1[a], d1, 0.0)),
            ];
            m.quad(ids, mat, surface);
        };
    let ground = |p: V3, _d: f64, _u: f64| [p.x / GROUND_TEX_M, p.z / GROUND_TEX_M];
    quad(0, 1, mats.shoulder, surf::SHOULDER, &ground);
    quad(1, 2, mats.kerb, surf::KERB, &|_, d, u| {
        [u, d / KERB_TEX_LENGTH_M]
    });
    quad(2, 3, mats.road, surf::ASPHALT, &|_, d, u| {
        [u, d / ROAD_TEX_LENGTH_M]
    });
    quad(3, 4, mats.kerb, surf::KERB, &|_, d, u| {
        [1.0 - u, d / KERB_TEX_LENGTH_M]
    });
    quad(4, 5, mats.shoulder, surf::SHOULDER, &ground);
}

/// Per section and side (left, right): steel guard rail instead of the
/// theme's fence where the ground falls away or the road turns hard.
fn guard_rails(road: &Road, world: &World, style: &Style) -> Vec<[bool; 2]> {
    use crate::spec::Theme;
    let n = road.len();
    let mut raw = vec![[false; 2]; n];
    if style.theme != Theme::PineRidge {
        return raw;
    }
    for (i, sec) in road.secs.iter().enumerate() {
        let e = edge_lat(style, sec);
        let rh = v3(sec.right.x, 0.0, sec.right.z).norm();
        for (k, sign) in [-1.0f64, 1.0].into_iter().enumerate() {
            let lat = sign * (e + 9.0);
            let (x, z) = (sec.pos.x + rh.x * lat, sec.pos.z + rh.z * lat);
            let drop = sec.pos.y - world.height(x, z);
            // outside of a turn: left side for right turns (kappa > 0)
            let outer = sec.kappa * sign < -1.0 / 150.0;
            raw[i][k] = drop > 2.2 || outer;
        }
    }
    // close short gaps and drop one-section islands so rails come in runs
    let mut out = raw.clone();
    for i in 0..n {
        for k in 0..2 {
            let near = |d: isize| raw[(i as isize + d).rem_euclid(n as isize) as usize][k];
            let window = (-5..=5).filter(|&d| near(d)).count();
            out[i][k] = if raw[i][k] { window >= 3 } else { window >= 8 };
        }
    }
    out
}

fn barrier_strip(
    m: &mut MeshData,
    road: &Road,
    style: &Style,
    mats: &Mats,
    i: usize,
    rails: &[[bool; 2]],
) {
    let (s0, s1) = (road.at(i as isize), road.at(i as isize + 1));
    let (d0, d1) = (
        i as f64 * road.ds / BARRIER_TEX_M,
        (i + 1) as f64 * road.ds / BARRIER_TEX_M,
    );
    for (side, sign) in [-1.0, 1.0].into_iter().enumerate() {
        let barrier_mat = if rails[i % rails.len()][side] {
            mats.armco
        } else {
            mats.barrier
        };
        let frame = |s: &crate::road::Section| {
            let e = edge_lat(style, s);
            let inner = surface_point(s, sign * e);
            let outer = surface_point(s, sign * (e + BARRIER_THICKNESS));
            let rh = v3(s.right.x, 0.0, s.right.z).norm();
            (inner, outer, rh * sign)
        };
        let (i0, o0, out0) = frame(s0);
        let (i1, o1, out1) = frame(s1);
        let up = v3(0.0, BARRIER_HEIGHT, 0.0);
        let drop = v3(0.0, FOOTING, 0.0);
        let top0 = i0 + up;
        let top1 = i1 + up;
        let otop0 = v3(o0.x, i0.y + BARRIER_HEIGHT, o0.z);
        let otop1 = v3(o1.x, i1.y + BARRIER_HEIGHT, o1.z);
        // face toward the road
        let toward = -out0;
        let a = m.vertex(i0, toward, [d0, 0.0]);
        let b = m.vertex(i1, toward, [d1, 0.0]);
        let c = m.vertex(top1, toward, [d1, 0.5]);
        let d = m.vertex(top0, toward, [d0, 0.5]);
        m.quad([a, b, c, d], barrier_mat, surf::SHOULDER);
        // footing below the face
        let a = m.vertex(i0 - drop, toward, [d0, 0.75]);
        let b = m.vertex(i1 - drop, toward, [d1, 0.75]);
        let c = m.vertex(i1, toward, [d1, 0.75]);
        let d = m.vertex(i0, toward, [d0, 0.75]);
        m.quad([a, b, c, d], barrier_mat, surf::SHOULDER);
        // top
        let a = m.vertex(top0, V3::UP, [d0, 0.46]);
        let b = m.vertex(top1, V3::UP, [d1, 0.46]);
        let c = m.vertex(otop1, V3::UP, [d1, 0.5]);
        let d = m.vertex(otop0, V3::UP, [d0, 0.5]);
        m.quad([a, b, c, d], barrier_mat, surf::SHOULDER);
        // outer face and footing
        let a = m.vertex(otop0, out0, [d0, 0.5]);
        let b = m.vertex(otop1, out1, [d1, 0.5]);
        let c = m.vertex(o1 - drop, out1, [d1, 0.75]);
        let d = m.vertex(o0 - drop, out0, [d0, 0.75]);
        m.quad([a, b, c, d], barrier_mat, surf::SHOULDER);
    }
}

fn start_line(m: &mut MeshData, road: &Road, mats: &Mats) {
    let f0 = road.frame_at(0.0);
    let f1 = road.frame_at(1.8);
    let lift = v3(0.0, 0.012, 0.0);
    let p = [
        surface_point(&f0, -f0.hw) + lift,
        surface_point(&f0, f0.hw) + lift,
        surface_point(&f1, f1.hw) + lift,
        surface_point(&f1, -f1.hw) + lift,
    ];
    let w = 2.0 * f0.hw / 7.2;
    let ids = [
        m.vertex(p[0], f0.up, [0.0, 0.0]),
        m.vertex(p[1], f0.up, [w, 0.0]),
        m.vertex(p[2], f1.up, [w, 0.25]),
        m.vertex(p[3], f1.up, [0.0, 0.25]),
    ];
    m.quad(ids, mats.startline, surf::ASPHALT);
}

fn spawn_grid(road: &Road, style: &Style) -> Vec<SpawnSlot> {
    (0..GRID_SLOTS)
        .map(|k| {
            let rowi = (k / 2) as f64;
            let col = if k % 2 == 0 { -1.0 } else { 1.0 };
            // pole position first, staggered columns, each row 8 m further back
            let back = 7.0 + 8.0 * rowi + if col > 0.0 { 4.0 } else { 0.0 };
            let f = road.frame_at(-back);
            let lat = col * (f.hw * 0.45);
            let p = surface_point(&f, lat);
            let _ = style;
            SpawnSlot {
                position: p.arr(),
                heading_rad: atan2(-f.fwd.z, f.fwd.x) as f32,
            }
        })
        .collect()
}

fn gates(road: &Road, style: &Style) -> Vec<Gate> {
    let count = ((road.length / 380.0).round() as usize).max(6);
    (0..count)
        .map(|k| {
            let f = road.frame_at(road.length * k as f64 / count as f64);
            let e = edge_lat(style, &f);
            Gate {
                position: f.pos.arr(),
                normal: v3(f.fwd.x, 0.0, f.fwd.z).norm().arr(),
                width_m: (2.0 * e) as f32,
            }
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TileKind {
    None,
    Fine,
    Coarse,
}

fn terrain_meshes(world: &World, style: &Style, mats: &Mats) -> Vec<(String, MeshData)> {
    let (bx0, bz0, bx1, bz1) = world.bounds();
    let tx0 = (bx0 / TILE).floor() as i64;
    let tx1 = (bx1 / TILE).ceil() as i64;
    let tz0 = (bz0 / TILE).floor() as i64;
    let tz1 = (bz1 / TILE).ceil() as i64;
    let kind_of = |tx: i64, tz: i64| -> TileKind {
        let mut best = f64::MAX;
        for a in 0..7 {
            for b in 0..7 {
                let x = (tx as f64 + a as f64 / 6.0) * TILE;
                let z = (tz as f64 + b as f64 / 6.0) * TILE;
                if let Some(o) = world.outside_distance(x, z) {
                    best = best.min(o);
                }
            }
        }
        if best > crate::world::TERRAIN_REACH_M {
            TileKind::None
        } else if best < FINE_RANGE {
            TileKind::Fine
        } else {
            TileKind::Coarse
        }
    };
    let width = (tx1 - tx0 + 1) as usize;
    let mut kinds = Vec::new();
    for tz in tz0..=tz1 {
        for tx in tx0..=tx1 {
            kinds.push(kind_of(tx, tz));
        }
    }
    let kind_at = |tx: i64, tz: i64| -> TileKind {
        if tx < tx0 || tx > tx1 || tz < tz0 || tz > tz1 {
            TileKind::None
        } else {
            kinds[(tz - tz0) as usize * width + (tx - tx0) as usize]
        }
    };
    let mut out = Vec::new();
    for tz in tz0..=tz1 {
        for tx in tx0..=tx1 {
            let kind = kind_at(tx, tz);
            if kind == TileKind::None {
                continue;
            }
            let mesh = terrain_tile(
                world,
                style,
                mats,
                tx,
                tz,
                kind,
                [
                    kind_at(tx - 1, tz),
                    kind_at(tx + 1, tz),
                    kind_at(tx, tz - 1),
                    kind_at(tx, tz + 1),
                ],
            );
            out.push((format!("terrain_{tx}_{tz}"), mesh));
        }
    }
    out
}

fn terrain_tile(
    world: &World,
    style: &Style,
    mats: &Mats,
    tx: i64,
    tz: i64,
    kind: TileKind,
    // west, east, north (-z), south (+z)
    neighbours: [TileKind; 4],
) -> MeshData {
    let cell = if kind == TileKind::Fine {
        FINE_CELL
    } else {
        COARSE_CELL
    };
    let cells = (TILE / cell).round() as usize;
    let (ox, oz) = (tx as f64 * TILE, tz as f64 * TILE);
    let side = cells + 3; // one margin vertex on each side
    let at = |i: isize, j: isize| (ox + i as f64 * cell, oz + j as f64 * cell);
    let mut h = vec![0.0f64; side * side];
    let idx = |i: isize, j: isize| ((j + 1) as usize) * side + (i + 1) as usize;
    for j in -1..=(cells as isize + 1) {
        for i in -1..=(cells as isize + 1) {
            let (x, z) = at(i, j);
            h[idx(i, j)] = world.height(x, z);
        }
    }
    if kind == TileKind::Fine {
        let snap = |along: f64, fixed: f64, vertical_edge: bool| {
            let c0 = (along / COARSE_CELL).floor() * COARSE_CELL;
            let t = (along - c0) / COARSE_CELL;
            let (a, b) = if vertical_edge {
                (
                    world.height(fixed, c0),
                    world.height(fixed, c0 + COARSE_CELL),
                )
            } else {
                (
                    world.height(c0, fixed),
                    world.height(c0 + COARSE_CELL, fixed),
                )
            };
            lerp(a, b, t)
        };
        let n = cells as isize;
        for k in 0..=n {
            if neighbours[0] == TileKind::Coarse {
                let (x, z) = at(0, k);
                h[idx(0, k)] = snap(z, x, true);
            }
            if neighbours[1] == TileKind::Coarse {
                let (x, z) = at(n, k);
                h[idx(n, k)] = snap(z, x, true);
            }
            if neighbours[2] == TileKind::Coarse {
                let (x, z) = at(k, 0);
                h[idx(k, 0)] = snap(x, z, false);
            }
            if neighbours[3] == TileKind::Coarse {
                let (x, z) = at(k, n);
                h[idx(k, n)] = snap(x, z, false);
            }
        }
    }
    let normal_at = |i: isize, j: isize| {
        let hx = h[idx(i + 1, j)] - h[idx(i - 1, j)];
        let hz = h[idx(i, j + 1)] - h[idx(i, j - 1)];
        v3(-hx / (2.0 * cell), 1.0, -hz / (2.0 * cell)).norm()
    };
    let mut m = MeshData::new();
    let seed = world.seed;
    for j in 0..cells as isize {
        for i in 0..cells as isize {
            let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            let pts: Vec<V3> = corners
                .iter()
                .map(|&(ci, cj)| {
                    let (x, z) = at(ci, cj);
                    v3(x, h[idx(ci, cj)], z)
                })
                .collect();
            let centre = (pts[0] + pts[1] + pts[2] + pts[3]) * 0.25;
            let ny = ((normal_at(i, j) + normal_at(i + 1, j + 1)) * 0.5).norm().y;
            let outside = world.outside_distance(centre.x, centre.z).unwrap_or(1e9);
            let (mat, surface) = terrain_material(style, mats, seed, centre, ny, outside);
            let ids: Vec<u32> = corners
                .iter()
                .zip(&pts)
                .map(|(&(ci, cj), p)| {
                    m.vertex(
                        *p,
                        normal_at(ci, cj),
                        [p.x / GROUND_TEX_M, p.z / GROUND_TEX_M],
                    )
                })
                .collect();
            m.quad([ids[0], ids[3], ids[2], ids[1]], mat, surface);
        }
    }
    m
}

fn terrain_material(
    style: &Style,
    mats: &Mats,
    seed: u64,
    c: V3,
    ny: f64,
    outside: f64,
) -> (u32, u8) {
    use crate::spec::Theme;
    let patch = fbm2(seed ^ 0xA7, c.x / 45.0, c.z / 45.0, 3, 0, 0);
    if let Some(sea) = style.sea_level {
        if c.y < sea + 0.3 {
            return if style.theme == Theme::NightHarbour {
                (mats.rock, surf::ROCK)
            } else {
                (mats.sand, surf::SAND)
            };
        }
    }
    if outside < 7.0 {
        return (mats.verge, surf::GRASS);
    }
    let steep = match style.theme {
        Theme::RedMesa => 0.80,
        Theme::PineRidge => 0.74,
        Theme::Coastline => 0.78,
        Theme::NightHarbour => 0.85,
    };
    if ny < steep {
        if style.theme == Theme::PineRidge && c.y > 100.0 + 14.0 * (patch - 0.5) && ny > 0.5 {
            return (mats.snow, surf::ROCK);
        }
        return (mats.rock, surf::ROCK);
    }
    match style.theme {
        Theme::Coastline => {
            if let Some(sea) = style.sea_level {
                if c.y < sea + 2.2 {
                    return (mats.sand, surf::SAND);
                }
            }
            if patch > 0.66 {
                (mats.verge, surf::GRASS)
            } else {
                (mats.grass, surf::GRASS)
            }
        }
        Theme::PineRidge => {
            let snowline = 92.0 + 18.0 * (patch - 0.5);
            let flecks = fbm2(seed ^ 0x5E, c.x / 21.0, c.z / 21.0, 2, 0, 0);
            if c.y > snowline + 8.0 * flecks && ny > 0.62 {
                (mats.snow, surf::ROCK)
            } else if c.y > 74.0 && ny < 0.93 {
                (mats.rock, surf::ROCK)
            } else if c.y > 74.0 {
                (mats.dirt, surf::GRASS)
            } else if patch > 0.74 {
                (mats.dirt, surf::GRASS)
            } else if patch > 0.62 {
                (mats.verge, surf::GRASS)
            } else {
                (mats.grass, surf::GRASS)
            }
        }
        Theme::RedMesa => {
            if patch > 0.58 {
                (mats.sand, surf::SAND)
            } else if patch < 0.32 {
                (mats.verge, surf::GRASS)
            } else {
                (mats.grass, surf::GRASS)
            }
        }
        Theme::NightHarbour => {
            if c.y > -0.8 && ny > 0.97 {
                (mats.shoulder, surf::CONCRETE)
            } else if patch > 0.6 {
                (mats.grass, surf::GRASS)
            } else {
                (mats.verge, surf::GRASS)
            }
        }
    }
}
