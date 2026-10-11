//! Low-poly prop prototypes (local space: +Y up, base at the origin, the
//! "front" of signs, stands and lamps faces +Z). All geometry is generated.

use crate::materials::Mats;
use crate::math::{Rng, V3, cos, sin, v3};
use crate::mesh::MeshData;
use crate::spec::Theme;
use std::f64::consts::TAU;

/// Indices into the track's prop mesh list; empty vectors mean "unused".
#[derive(Clone, Debug, Default)]
pub struct ProtoIds {
    pub pine: Vec<u32>,
    /// Cheap two-cone pines for the distant forest.
    pub far_pine: Vec<u32>,
    pub cabin: Vec<u32>,
    /// Roadside warning signs (one per board design).
    pub sign: Vec<u32>,
    /// Rock-cutting faces beside the road.
    pub cliff: Vec<u32>,
    pub broad: Vec<u32>,
    pub palm: Vec<u32>,
    pub bush: Vec<u32>,
    pub boulder: Vec<u32>,
    pub mesa: Vec<u32>,
    pub cactus: Vec<u32>,
    pub chevron_left: u32,
    pub chevron_right: u32,
    pub lamp: u32,
    pub stand: u32,
    pub house: Vec<u32>,
    pub building: Vec<u32>,
    pub warehouse: u32,
    pub containers: Vec<u32>,
    pub crane: u32,
    pub tank: u32,
    pub lighthouse: u32,
    pub gantry: u32,
}

pub struct Protos {
    pub meshes: Vec<(String, MeshData)>,
    pub ids: ProtoIds,
}

impl Protos {
    fn push(&mut self, name: &str, m: MeshData) -> u32 {
        self.meshes.push((name.to_string(), m));
        (self.meshes.len() - 1) as u32
    }
}

fn pine(rng: &mut Rng, m: &Mats, layers: usize, narrow: f64) -> MeshData {
    let mut d = MeshData::new();
    d.add_cylinder(V3::ZERO, 0.30, 0.16, 2.6, 6, m.bark, false, 2.0);
    for i in 0..layers {
        let t = i as f64 / layers as f64;
        let r = (2.5 - 1.5 * t) * narrow * (0.92 + 0.16 * rng.f());
        let y0 = 1.6 + i as f64 * (7.5 / layers as f64);
        d.add_cone(v3(0.0, y0, 0.0), r, 3.3 - 0.5 * t, 7, m.pine, 4.0);
    }
    d
}

fn broadleaf(rng: &mut Rng, m: &Mats, mat: u32) -> MeshData {
    let mut d = MeshData::new();
    let h = 3.2 + rng.f();
    d.add_cylinder(V3::ZERO, 0.32, 0.20, h, 6, m.bark, false, 2.0);
    d.add_blob(
        v3(0.0, h + 1.8, 0.0),
        v3(2.7, 2.2, 2.7),
        4,
        8,
        0.2,
        rng,
        mat,
    );
    d.add_blob(
        v3(1.3, h + 1.0, 0.7),
        v3(1.7, 1.5, 1.7),
        3,
        7,
        0.2,
        rng,
        mat,
    );
    d
}

fn palm(rng: &mut Rng, m: &Mats) -> MeshData {
    let mut d = MeshData::new();
    let h = 6.5 + rng.f() * 1.5;
    d.add_cylinder(V3::ZERO, 0.32, 0.18, h, 6, m.bark, false, 1.5);
    let crown = v3(0.0, h, 0.0);
    for i in 0..8 {
        let a = TAU * i as f64 / 8.0 + rng.f() * 0.3;
        let dir = v3(cos(a), 0.0, sin(a));
        let side = v3(-dir.z, 0.0, dir.x);
        let p1 = crown + dir * 1.9 + v3(0.0, 0.7, 0.0);
        let p2 = crown + dir * 3.6 + v3(0.0, -0.5, 0.0);
        let w = 0.55;
        d.flat_tri(
            [crown, p1 + side * w, p1 - side * w],
            [[0.5, 0.0], [1.0, 0.5], [0.0, 0.5]],
            m.palm,
            0,
            Some(V3::UP),
        );
        d.flat_tri(
            [p1 + side * w, p1 - side * w, p2],
            [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]],
            m.palm,
            0,
            Some(V3::UP),
        );
    }
    d
}

fn bush(rng: &mut Rng, m: &Mats, mat: u32) -> MeshData {
    let mut d = MeshData::new();
    d.add_blob(v3(0.0, 0.6, 0.0), v3(1.3, 0.9, 1.3), 3, 7, 0.25, rng, mat);
    let _ = m;
    d
}

fn boulder(rng: &mut Rng, m: &Mats) -> MeshData {
    let mut d = MeshData::new();
    let rx = 1.4 + rng.f();
    d.add_blob(
        v3(0.0, 0.7, 0.0),
        v3(rx, 0.9 + 0.5 * rng.f(), rx * 0.85),
        3,
        7,
        0.32,
        rng,
        m.boulder,
    );
    d
}

fn mesa(rng: &mut Rng, m: &Mats, spire: bool) -> MeshData {
    let mut d = MeshData::new();
    if spire {
        d.add_cylinder(v3(0.0, -2.0, 0.0), 6.0, 3.4, 24.0, 7, m.boulder, true, 10.0);
        d.add_cylinder(v3(0.0, 22.0, 0.0), 3.4, 2.6, 5.0, 7, m.boulder, true, 10.0);
        return d;
    }
    let mut r = 26.0 + 8.0 * rng.f();
    let mut y = -4.0;
    for i in 0..4 {
        let h = 6.0 + 2.0 * rng.f();
        let r_top = if i % 2 == 0 { r * 0.86 } else { r * 0.98 };
        d.add_cylinder(v3(0.0, y, 0.0), r, r_top, h, 10, m.boulder, i == 3, 12.0);
        y += h;
        r = r_top * 0.93;
    }
    d
}

fn cactus(rng: &mut Rng, m: &Mats) -> MeshData {
    let mut d = MeshData::new();
    let h = 3.6 + rng.f() * 1.2;
    d.add_cylinder(V3::ZERO, 0.38, 0.32, h, 6, m.cactus, true, 1.0);
    for (side, y) in [(1.0, 1.6), (-1.0, 2.2)] {
        d.add_box(v3(side * 0.7, y, 0.0), v3(0.5, 0.17, 0.17), m.cactus, 1.0);
        d.add_box(
            v3(side * 1.1, y + 0.7, 0.0),
            v3(0.17, 0.75, 0.17),
            m.cactus,
            1.0,
        );
    }
    d
}

fn chevron(m: &Mats, mirror: bool) -> MeshData {
    let mut d = MeshData::new();
    d.add_cylinder(v3(-1.0, 0.0, 0.0), 0.07, 0.07, 2.6, 5, m.pole, true, 1.0);
    d.add_cylinder(v3(1.0, 0.0, 0.0), 0.07, 0.07, 2.6, 5, m.pole, true, 1.0);
    let (u0, u1) = if mirror { (1.0, 0.0) } else { (0.0, 1.0) };
    let p = [
        v3(-1.5, 1.6, 0.06),
        v3(1.5, 1.6, 0.06),
        v3(1.5, 2.9, 0.06),
        v3(-1.5, 2.9, 0.06),
    ];
    d.flat_quad(
        p,
        [[u0, 0.0], [u1, 0.0], [u1, 1.0], [u0, 1.0]],
        m.chevron,
        0,
        Some(v3(0.0, 0.0, 1.0)),
    );
    let q = [
        v3(-1.5, 1.6, 0.0),
        v3(1.5, 1.6, 0.0),
        v3(1.5, 2.9, 0.0),
        v3(-1.5, 2.9, 0.0),
    ];
    d.flat_quad(
        q,
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        m.metal,
        0,
        Some(v3(0.0, 0.0, -1.0)),
    );
    d
}

fn lamp(m: &Mats) -> MeshData {
    let mut d = MeshData::new();
    d.add_cylinder(V3::ZERO, 0.16, 0.09, 9.0, 6, m.pole, true, 2.0);
    d.add_box(v3(0.0, 9.0, 1.3), v3(0.07, 0.07, 1.4), m.pole, 1.0);
    d.add_box(v3(0.0, 8.85, 2.5), v3(0.28, 0.08, 0.55), m.glow, 1.0);
    d
}

fn stand(m: &Mats) -> MeshData {
    let mut d = MeshData::new();
    for i in 0..5 {
        let hy = 0.6 * (i as f64 + 1.0);
        d.add_box(
            v3(0.0, hy, 6.0 - 2.4 * i as f64),
            v3(20.0, hy, 1.2),
            m.crowd,
            4.0,
        );
    }
    for x in [-19.0, -6.5, 6.5, 19.0] {
        d.add_cylinder(v3(x, 0.0, -4.0), 0.2, 0.2, 7.2, 6, m.pole, false, 2.0);
    }
    d.add_box(v3(0.0, 7.4, 0.5), v3(21.0, 0.18, 8.0), m.metal, 6.0);
    d.add_box(v3(0.0, 3.4, -6.0), v3(20.0, 3.4, 0.25), m.stand, 4.0);
    d
}

fn house(rng: &mut Rng, m: &Mats) -> MeshData {
    let mut d = MeshData::new();
    let (w, depth, h) = (4.5 + 2.0 * rng.f(), 3.8 + rng.f(), 3.0 + rng.f());
    d.add_box(v3(0.0, h, 0.0), v3(w, h, depth), m.plaster, 4.0);
    let r = 1.9;
    let (ew, ed) = (w + 0.6, depth + 0.5);
    let up = v3(0.0, 1.0, 0.7);
    d.flat_quad(
        [
            v3(-ew, 2.0 * h, ed),
            v3(ew, 2.0 * h, ed),
            v3(ew, 2.0 * h + r, 0.0),
            v3(-ew, 2.0 * h + r, 0.0),
        ],
        [[0.0, 0.0], [ew / 2.0, 0.0], [ew / 2.0, 1.0], [0.0, 1.0]],
        m.roof,
        0,
        Some(up),
    );
    d.flat_quad(
        [
            v3(ew, 2.0 * h, -ed),
            v3(-ew, 2.0 * h, -ed),
            v3(-ew, 2.0 * h + r, 0.0),
            v3(ew, 2.0 * h + r, 0.0),
        ],
        [[0.0, 0.0], [ew / 2.0, 0.0], [ew / 2.0, 1.0], [0.0, 1.0]],
        m.roof,
        0,
        Some(v3(0.0, 1.0, -0.7)),
    );
    d.flat_tri(
        [
            v3(-w, 2.0 * h, depth),
            v3(w, 2.0 * h, depth),
            v3(0.0, 2.0 * h + r, depth),
        ],
        [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]],
        m.plaster,
        0,
        Some(v3(0.0, 0.0, 1.0)),
    );
    d
}

fn building(m: &Mats, w: f64, depth: f64, h: f64) -> MeshData {
    let mut d = MeshData::new();
    d.add_box(
        v3(0.0, h * 0.5, 0.0),
        v3(w * 0.5, h * 0.5, depth * 0.5),
        m.windows,
        12.0,
    );
    d.add_box(
        v3(0.0, h + 0.4, 0.0),
        v3(w * 0.5 + 0.3, 0.4, depth * 0.5 + 0.3),
        m.pole,
        6.0,
    );
    d
}

fn warehouse(m: &Mats) -> MeshData {
    let mut d = MeshData::new();
    d.add_box(v3(0.0, 5.0, 0.0), v3(22.0, 5.0, 12.0), m.metal, 6.0);
    d.add_box(v3(0.0, 10.4, 0.0), v3(22.4, 0.4, 12.4), m.pole, 6.0);
    d.add_box(v3(0.0, 3.2, 12.05), v3(4.0, 3.2, 0.1), m.pole, 3.0);
    d
}

fn containers(rng: &mut Rng, m: &Mats) -> MeshData {
    let mut d = MeshData::new();
    let cols = 1 + rng.below(3) as usize;
    for cx in 0..cols {
        let high = 1 + rng.below(3) as usize;
        for cy in 0..high {
            let mat = m.container[rng.below(4) as usize];
            d.add_box(
                v3(
                    cx as f64 * 2.6 - cols as f64 * 1.3 + 1.3,
                    1.3 + cy as f64 * 2.62,
                    0.0,
                ),
                v3(1.25, 1.3, 6.0),
                mat,
                4.0,
            );
        }
    }
    d
}

fn crane(m: &Mats) -> MeshData {
    let mut d = MeshData::new();
    for (x, z) in [(-12.0, -5.0), (12.0, -5.0), (-12.0, 5.0), (12.0, 5.0)] {
        d.add_box(v3(x, 12.0, z), v3(0.7, 12.0, 0.7), m.pole, 4.0);
    }
    d.add_box(v3(0.0, 24.5, 0.0), v3(13.0, 0.9, 6.0), m.container[0], 4.0);
    d.add_box(v3(0.0, 26.8, 0.0), v3(0.8, 1.5, 28.0), m.metal, 4.0);
    d.add_box(v3(0.0, 24.0, 20.0), v3(1.6, 1.2, 1.6), m.pole, 2.0);
    d.add_box(v3(0.0, 12.0, 30.0), v3(0.15, 12.0, 0.15), m.pole, 2.0);
    d
}

fn tank(m: &Mats) -> MeshData {
    let mut d = MeshData::new();
    d.add_cylinder(V3::ZERO, 9.0, 9.0, 14.0, 14, m.metal, true, 6.0);
    d
}

fn lighthouse(m: &Mats) -> MeshData {
    let mut d = MeshData::new();
    let mut y = 0.0;
    for i in 0..4 {
        let (r0, r1) = (3.4 - 0.25 * i as f64, 3.4 - 0.25 * (i + 1) as f64);
        let mat = if i % 2 == 0 {
            m.plaster
        } else {
            m.container[0]
        };
        d.add_cylinder(v3(0.0, y, 0.0), r0, r1, 5.5, 10, mat, false, 5.0);
        y += 5.5;
    }
    d.add_cylinder(v3(0.0, y, 0.0), 2.9, 2.9, 0.6, 10, m.pole, true, 4.0);
    d.add_cylinder(v3(0.0, y + 0.6, 0.0), 2.1, 2.1, 2.2, 10, m.glow, false, 4.0);
    d.add_cone(v3(0.0, y + 2.8, 0.0), 2.6, 2.4, 10, m.container[0], 4.0);
    d
}

fn far_pine(rng: &mut Rng, m: &Mats, narrow: f64) -> MeshData {
    let mut d = MeshData::new();
    let r = 2.5 * narrow * (0.9 + 0.2 * rng.f());
    d.add_cone(v3(0.0, 0.3, 0.0), r, 5.2, 6, m.pine, 4.0);
    d.add_cone(v3(0.0, 3.2, 0.0), r * 0.72, 4.6, 6, m.pine, 4.0);
    d.add_cone(v3(0.0, 5.9, 0.0), r * 0.46, 3.4, 6, m.pine, 4.0);
    d
}

fn cabin(rng: &mut Rng, m: &Mats) -> MeshData {
    let mut d = MeshData::new();
    let (w, depth) = (3.0 + 1.4 * rng.f(), 2.4 + 0.8 * rng.f());
    let h = 2.5;
    d.add_box(v3(0.0, h * 0.5, 0.0), v3(w, h * 0.5, depth), m.log, 2.4);
    let (ew, ed, r) = (w + 0.5, depth + 0.6, 1.9);
    d.flat_quad(
        [
            v3(-ew, h, ed),
            v3(ew, h, ed),
            v3(ew, h + r, 0.0),
            v3(-ew, h + r, 0.0),
        ],
        [[0.0, 0.0], [ew / 2.0, 0.0], [ew / 2.0, 1.0], [0.0, 1.0]],
        m.shingle,
        0,
        Some(v3(0.0, 1.0, 0.8)),
    );
    d.flat_quad(
        [
            v3(ew, h, -ed),
            v3(-ew, h, -ed),
            v3(-ew, h + r, 0.0),
            v3(ew, h + r, 0.0),
        ],
        [[0.0, 0.0], [ew / 2.0, 0.0], [ew / 2.0, 1.0], [0.0, 1.0]],
        m.shingle,
        0,
        Some(v3(0.0, 1.0, -0.8)),
    );
    for z in [depth, -depth] {
        d.flat_tri(
            [v3(-w, h, z), v3(w, h, z), v3(0.0, h + r - 0.35, z)],
            [[0.0, 0.0], [1.0, 0.0], [0.5, 0.7]],
            m.log,
            0,
            Some(v3(0.0, 0.0, z)),
        );
    }
    d.add_box(
        v3(w * 0.55, h + 1.7, -depth * 0.35),
        v3(0.38, 1.1, 0.38),
        m.stone,
        1.2,
    );
    d.add_box(
        v3(0.0, 0.95, depth + 0.03),
        v3(0.5, 0.95, 0.04),
        m.shingle,
        1.5,
    );
    d.add_box(
        v3(w * 0.55, 1.55, depth + 0.03),
        v3(0.45, 0.4, 0.04),
        m.windows,
        1.4,
    );
    d.add_box(
        v3(-w * 0.55, 1.55, depth + 0.03),
        v3(0.45, 0.4, 0.04),
        m.windows,
        1.4,
    );
    d
}

fn warning_sign(m: &Mats, quadrant: usize) -> MeshData {
    let mut d = MeshData::new();
    d.add_cylinder(V3::ZERO, 0.05, 0.05, 2.45, 5, m.pole, true, 1.0);
    let (u0, v0) = ((quadrant % 2) as f64 * 0.5, (quadrant / 2) as f64 * 0.5);
    let p = [
        v3(-0.55, 1.45, 0.06),
        v3(0.55, 1.45, 0.06),
        v3(0.55, 2.55, 0.06),
        v3(-0.55, 2.55, 0.06),
    ];
    d.flat_quad(
        p,
        [
            [u0, v0 + 0.5],
            [u0 + 0.5, v0 + 0.5],
            [u0 + 0.5, v0],
            [u0, v0],
        ],
        m.signs,
        0,
        Some(v3(0.0, 0.0, 1.0)),
    );
    let q = [
        v3(-0.55, 1.45, 0.0),
        v3(0.55, 1.45, 0.0),
        v3(0.55, 2.55, 0.0),
        v3(-0.55, 2.55, 0.0),
    ];
    d.flat_quad(
        q,
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        m.metal,
        0,
        Some(v3(0.0, 0.0, -1.0)),
    );
    d
}

/// Jagged rock-cutting face. Local +Z faces the road, the base sits on y = 0.
fn cliff(rng: &mut Rng, m: &Mats) -> MeshData {
    let mut d = MeshData::new();
    let (cols, rows) = (6usize, 4usize);
    let width = 10.0 + 4.0 * rng.f();
    let height = 8.0 + 5.0 * rng.f();
    let mut grid: Vec<Vec<V3>> = Vec::new();
    for r in 0..=rows {
        let t = r as f64 / rows as f64;
        let mut row = Vec::new();
        for c in 0..=cols {
            let x = (c as f64 / cols as f64 - 0.5) * 2.0 * width;
            let taper = 1.0 - 0.18 * t;
            let jx = if c == 0 || c == cols {
                0.0
            } else {
                rng.range(-1.2, 1.2)
            };
            let jy = if r == 0 {
                -1.5
            } else {
                rng.range(-0.9, 0.9) * (1.0 - 0.3 * t)
            };
            let lean = -3.2 * t * t.sqrt() + rng.range(-0.9, 0.9) * t;
            row.push(v3(x * taper + jx, t * height + jy, lean));
        }
        grid.push(row);
    }
    for r in 0..rows {
        for c in 0..cols {
            let p = [
                grid[r][c],
                grid[r][c + 1],
                grid[r + 1][c + 1],
                grid[r + 1][c],
            ];
            let (u0, u1) = (c as f64 * 0.45, (c + 1) as f64 * 0.45);
            let (v0, v1) = (r as f64 * 0.4, (r + 1) as f64 * 0.4);
            d.flat_quad(
                p,
                [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
                m.cliff,
                0,
                Some(v3(0.0, 0.2, 1.0)),
            );
        }
    }
    // flat-ish top sloping back into the hill, closed ends
    let top = &grid[rows];
    for c in 0..cols {
        let back = |p: V3| v3(p.x, p.y - 1.2, p.z - 7.0);
        let p = [top[c], top[c + 1], back(top[c + 1]), back(top[c])];
        d.flat_quad(
            p,
            [[0.0, 0.0], [0.5, 0.0], [0.5, 1.5], [0.0, 1.5]],
            m.cliff,
            0,
            Some(V3::UP),
        );
    }
    for c in [0usize, cols] {
        for r in 0..rows {
            let (a, b) = (grid[r][c], grid[r + 1][c]);
            let (a2, b2) = (v3(a.x, a.y, a.z - 7.0), v3(b.x, b.y, b.z - 7.0));
            let out = if c == 0 {
                v3(-1.0, 0.0, 0.0)
            } else {
                v3(1.0, 0.0, 0.0)
            };
            d.flat_quad(
                [a, b, b2, a2],
                [[0.0, 0.0], [0.0, 0.4], [1.5, 0.4], [1.5, 0.0]],
                m.cliff,
                0,
                Some(out),
            );
        }
    }
    d
}

/// Start/finish gantry spanning `road_width` metres.
pub fn gantry(m: &Mats, road_width: f64) -> MeshData {
    let mut d = MeshData::new();
    let half = road_width * 0.5 + 2.4;
    for sx in [-1.0, 1.0] {
        d.add_box(v3(sx * half, 3.3, 0.0), v3(0.55, 3.3, 0.55), m.pole, 3.0);
    }
    d.add_box(v3(0.0, 6.9, 0.0), v3(half + 0.55, 0.45, 0.45), m.metal, 3.0);
    for z in [-0.5, 0.5] {
        let p = [
            v3(-half, 6.0, z),
            v3(half, 6.0, z),
            v3(half, 7.8, z),
            v3(-half, 7.8, z),
        ];
        let reps = half * 2.0 / 3.0;
        d.flat_quad(
            p,
            [[0.0, 0.0], [reps, 0.0], [reps, 0.6], [0.0, 0.6]],
            m.checker,
            0,
            Some(v3(0.0, 0.0, z * 4.0)),
        );
    }
    d
}

/// Generate every prop prototype the `theme` uses.
pub fn build_protos(theme: Theme, seed: u64, m: &Mats, road_width: f64) -> Protos {
    let mut p = Protos {
        meshes: Vec::new(),
        ids: ProtoIds::default(),
    };
    let mut rng = Rng::new(seed ^ 0x9201);
    p.ids.gantry = p.push("gantry", gantry(m, road_width));
    p.ids.chevron_left = p.push("chevron-left", chevron(m, false));
    p.ids.chevron_right = p.push("chevron-right", chevron(m, true));
    p.ids.stand = p.push("grandstand", stand(m));
    if theme != Theme::NightHarbour {
        for q in 0..4 {
            let id = p.push(&format!("sign-{q}"), warning_sign(m, q));
            p.ids.sign.push(id);
        }
        for i in 0..3 {
            let mesh = cliff(&mut rng, m);
            let id = p.push(&format!("cliff-{i}"), mesh);
            p.ids.cliff.push(id);
        }
    }
    match theme {
        Theme::PineRidge => {
            for (i, (layers, narrow)) in [(4usize, 1.0), (5, 0.8), (3, 1.15)].iter().enumerate() {
                let mesh = pine(&mut rng, m, *layers, *narrow);
                let id = p.push(&format!("pine-{i}"), mesh);
                p.ids.pine.push(id);
            }
            for (i, narrow) in [0.9, 1.1, 1.3].iter().enumerate() {
                let mesh = far_pine(&mut rng, m, *narrow);
                let id = p.push(&format!("far-pine-{i}"), mesh);
                p.ids.far_pine.push(id);
            }
            for i in 0..3 {
                let mesh = cabin(&mut rng, m);
                let id = p.push(&format!("cabin-{i}"), mesh);
                p.ids.cabin.push(id);
            }

            for i in 0..2 {
                let mesh = broadleaf(&mut rng, m, if i == 0 { m.leaf } else { m.leaf_olive });
                let id = p.push(&format!("broadleaf-{i}"), mesh);
                p.ids.broad.push(id);
            }
            for i in 0..3 {
                let mesh = boulder(&mut rng, m);
                let id = p.push(&format!("boulder-{i}"), mesh);
                p.ids.boulder.push(id);
            }
            let mesh = bush(&mut rng, m, m.leaf_olive);
            let id = p.push("bush", mesh);
            p.ids.bush.push(id);
        }
        Theme::Coastline => {
            for i in 0..3 {
                let mesh = palm(&mut rng, m);
                let id = p.push(&format!("palm-{i}"), mesh);
                p.ids.palm.push(id);
            }
            for i in 0..2 {
                let mesh = pine(&mut rng, m, 3 + i, 0.9);
                let id = p.push(&format!("pine-{i}"), mesh);
                p.ids.pine.push(id);
            }
            for i in 0..2 {
                let mesh = bush(&mut rng, m, if i == 0 { m.leaf } else { m.leaf_olive });
                let id = p.push(&format!("bush-{i}"), mesh);
                p.ids.bush.push(id);
            }
            for i in 0..2 {
                let mesh = boulder(&mut rng, m);
                let id = p.push(&format!("boulder-{i}"), mesh);
                p.ids.boulder.push(id);
            }
            for i in 0..3 {
                let mesh = house(&mut rng, m);
                let id = p.push(&format!("house-{i}"), mesh);
                p.ids.house.push(id);
            }
            p.ids.lighthouse = p.push("lighthouse", lighthouse(m));
        }
        Theme::RedMesa => {
            for i in 0..3 {
                let mesh = mesa(&mut rng, m, false);
                let id = p.push(&format!("mesa-{i}"), mesh);
                p.ids.mesa.push(id);
            }
            let mesh = mesa(&mut rng, m, true);
            let id = p.push("spire", mesh);
            p.ids.mesa.push(id);
            for i in 0..3 {
                let mesh = cactus(&mut rng, m);
                let id = p.push(&format!("cactus-{i}"), mesh);
                p.ids.cactus.push(id);
            }
            for i in 0..3 {
                let mesh = boulder(&mut rng, m);
                let id = p.push(&format!("boulder-{i}"), mesh);
                p.ids.boulder.push(id);
            }
            let mesh = bush(&mut rng, m, m.leaf_olive);
            let id = p.push("scrub", mesh);
            p.ids.bush.push(id);
        }
        Theme::NightHarbour => {
            p.ids.lamp = p.push("lamp", lamp(m));
            for (i, (w, dpt, h)) in [
                (22.0, 22.0, 26.0),
                (30.0, 18.0, 44.0),
                (18.0, 18.0, 70.0),
                (36.0, 24.0, 18.0),
            ]
            .iter()
            .enumerate()
            {
                let id = p.push(&format!("building-{i}"), building(m, *w, *dpt, *h));
                p.ids.building.push(id);
            }
            p.ids.warehouse = p.push("warehouse", warehouse(m));
            for i in 0..4 {
                let mesh = containers(&mut rng, m);
                let id = p.push(&format!("containers-{i}"), mesh);
                p.ids.containers.push(id);
            }
            p.ids.crane = p.push("crane", crane(m));
            p.ids.tank = p.push("tank", tank(m));
            for i in 0..2 {
                let mesh = boulder(&mut rng, m);
                let id = p.push(&format!("rubble-{i}"), mesh);
                p.ids.boulder.push(id);
            }
        }
    }
    let _ = (sin(0.0), road_width);
    p
}
