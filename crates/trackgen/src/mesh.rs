//! Triangle mesh accumulation and conversion into RCP meshes.

use crate::math::{Rng, V3, cos, sin, v3};
use racer_pack_schema::buffer::BufferBuilder;
use racer_pack_schema::buffer::ElementKind;
use racer_pack_schema::common::{Mesh, Primitive};
use std::f64::consts::TAU;

#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    /// (a, b, c, material, surface)
    pub tris: Vec<([u32; 3], u32, u8)>,
}

impl MeshData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    pub fn vertex(&mut self, p: V3, n: V3, uv: [f64; 2]) -> u32 {
        self.pos.push(p.arr());
        self.nrm.push(n.arr());
        self.uv.push([uv[0] as f32, uv[1] as f32]);
        (self.pos.len() - 1) as u32
    }

    pub fn tri(&mut self, a: u32, b: u32, c: u32, material: u32, surface: u8) {
        self.tris.push(([a, b, c], material, surface));
    }

    /// Quad `a b c d` in order (two triangles a-b-c and a-c-d).
    pub fn quad(&mut self, v: [u32; 4], material: u32, surface: u8) {
        self.tri(v[0], v[1], v[2], material, surface);
        self.tri(v[0], v[2], v[3], material, surface);
    }

    /// Flat-shaded quad from four corners; the normal is taken from the winding
    /// and flipped to face `toward` when given.
    pub fn flat_quad(
        &mut self,
        p: [V3; 4],
        uv: [[f64; 2]; 4],
        material: u32,
        surface: u8,
        toward: Option<V3>,
    ) {
        let mut n = (p[1] - p[0]).cross(p[2] - p[0]).norm();
        if let Some(t) = toward {
            if n.dot(t) < 0.0 {
                n = -n;
            }
        }
        let ids = [
            self.vertex(p[0], n, uv[0]),
            self.vertex(p[1], n, uv[1]),
            self.vertex(p[2], n, uv[2]),
            self.vertex(p[3], n, uv[3]),
        ];
        self.quad(ids, material, surface);
    }

    pub fn flat_tri(
        &mut self,
        p: [V3; 3],
        uv: [[f64; 2]; 3],
        material: u32,
        surface: u8,
        toward: Option<V3>,
    ) {
        let mut n = (p[1] - p[0]).cross(p[2] - p[0]).norm();
        if let Some(t) = toward {
            if n.dot(t) < 0.0 {
                n = -n;
            }
        }
        let a = self.vertex(p[0], n, uv[0]);
        let b = self.vertex(p[1], n, uv[1]);
        let c = self.vertex(p[2], n, uv[2]);
        self.tri(a, b, c, material, surface);
    }

    /// Axis-aligned box with per-face UVs at `uv_m` metres per texture repeat.
    pub fn add_box(&mut self, center: V3, half: V3, material: u32, uv_m: f64) {
        let (c, h) = (center, half);
        let corner = |sx: f64, sy: f64, sz: f64| v3(c.x + sx * h.x, c.y + sy * h.y, c.z + sz * h.z);
        let faces: [([V3; 4], V3, f64, f64); 6] = [
            (
                [
                    corner(-1.0, -1.0, 1.0),
                    corner(1.0, -1.0, 1.0),
                    corner(1.0, 1.0, 1.0),
                    corner(-1.0, 1.0, 1.0),
                ],
                v3(0.0, 0.0, 1.0),
                h.x,
                h.y,
            ),
            (
                [
                    corner(1.0, -1.0, -1.0),
                    corner(-1.0, -1.0, -1.0),
                    corner(-1.0, 1.0, -1.0),
                    corner(1.0, 1.0, -1.0),
                ],
                v3(0.0, 0.0, -1.0),
                h.x,
                h.y,
            ),
            (
                [
                    corner(1.0, -1.0, 1.0),
                    corner(1.0, -1.0, -1.0),
                    corner(1.0, 1.0, -1.0),
                    corner(1.0, 1.0, 1.0),
                ],
                v3(1.0, 0.0, 0.0),
                h.z,
                h.y,
            ),
            (
                [
                    corner(-1.0, -1.0, -1.0),
                    corner(-1.0, -1.0, 1.0),
                    corner(-1.0, 1.0, 1.0),
                    corner(-1.0, 1.0, -1.0),
                ],
                v3(-1.0, 0.0, 0.0),
                h.z,
                h.y,
            ),
            (
                [
                    corner(-1.0, 1.0, 1.0),
                    corner(1.0, 1.0, 1.0),
                    corner(1.0, 1.0, -1.0),
                    corner(-1.0, 1.0, -1.0),
                ],
                v3(0.0, 1.0, 0.0),
                h.x,
                h.z,
            ),
            (
                [
                    corner(-1.0, -1.0, -1.0),
                    corner(1.0, -1.0, -1.0),
                    corner(1.0, -1.0, 1.0),
                    corner(-1.0, -1.0, 1.0),
                ],
                v3(0.0, -1.0, 0.0),
                h.x,
                h.z,
            ),
        ];
        for (p, n, w, hh) in faces {
            let (uw, uh) = (2.0 * w / uv_m, 2.0 * hh / uv_m);
            let ids = [
                self.vertex(p[0], n, [0.0, 0.0]),
                self.vertex(p[1], n, [uw, 0.0]),
                self.vertex(p[2], n, [uw, uh]),
                self.vertex(p[3], n, [0.0, uh]),
            ];
            self.quad(ids, material, 0);
        }
    }

    /// Vertical (optionally tapered) cylinder with `sides` facets, base at
    /// `base`. `cap_top` closes the upper end.
    #[allow(clippy::too_many_arguments)]
    pub fn add_cylinder(
        &mut self,
        base: V3,
        r0: f64,
        r1: f64,
        height: f64,
        sides: usize,
        material: u32,
        cap_top: bool,
        uv_m: f64,
    ) {
        let slope = (r0 - r1) / height.max(1e-6);
        for i in 0..sides {
            let a0 = TAU * i as f64 / sides as f64;
            let a1 = TAU * (i + 1) as f64 / sides as f64;
            let n0 = v3(cos(a0), slope, sin(a0)).norm();
            let n1 = v3(cos(a1), slope, sin(a1)).norm();
            let u0 = i as f64 / sides as f64 * TAU * r0.max(r1) / uv_m;
            let u1 = (i + 1) as f64 / sides as f64 * TAU * r0.max(r1) / uv_m;
            let vh = height / uv_m;
            let a = self.vertex(base + v3(cos(a0) * r0, 0.0, sin(a0) * r0), n0, [u0, 0.0]);
            let b = self.vertex(base + v3(cos(a1) * r0, 0.0, sin(a1) * r0), n1, [u1, 0.0]);
            let c = self.vertex(base + v3(cos(a1) * r1, height, sin(a1) * r1), n1, [u1, vh]);
            let d = self.vertex(base + v3(cos(a0) * r1, height, sin(a0) * r1), n0, [u0, vh]);
            self.quad([a, b, c, d], material, 0);
            if cap_top && r1 > 1e-6 {
                let t = base + v3(0.0, height, 0.0);
                let ct = self.vertex(t, V3::UP, [0.5, 0.5]);
                let p0 = self.vertex(
                    base + v3(cos(a0) * r1, height, sin(a0) * r1),
                    V3::UP,
                    [0.5 + 0.5 * cos(a0), 0.5 + 0.5 * sin(a0)],
                );
                let p1 = self.vertex(
                    base + v3(cos(a1) * r1, height, sin(a1) * r1),
                    V3::UP,
                    [0.5 + 0.5 * cos(a1), 0.5 + 0.5 * sin(a1)],
                );
                self.tri(ct, p1, p0, material, 0);
            }
        }
    }

    /// Cone from `base` radius `r` to a point `height` above.
    pub fn add_cone(
        &mut self,
        base: V3,
        r: f64,
        height: f64,
        sides: usize,
        material: u32,
        uv_m: f64,
    ) {
        let slope = r / height.max(1e-6);
        for i in 0..sides {
            let a0 = TAU * i as f64 / sides as f64;
            let a1 = TAU * (i + 1) as f64 / sides as f64;
            let am = 0.5 * (a0 + a1);
            let nm = v3(cos(am), slope, sin(am)).norm();
            let n0 = v3(cos(a0), slope, sin(a0)).norm();
            let n1 = v3(cos(a1), slope, sin(a1)).norm();
            let a = self.vertex(
                base + v3(cos(a0) * r, 0.0, sin(a0) * r),
                n0,
                [i as f64 * 0.5, 0.0],
            );
            let b = self.vertex(
                base + v3(cos(a1) * r, 0.0, sin(a1) * r),
                n1,
                [i as f64 * 0.5 + 0.5, 0.0],
            );
            let t = self.vertex(
                base + v3(0.0, height, 0.0),
                nm,
                [i as f64 * 0.5 + 0.25, height / uv_m],
            );
            self.tri(a, b, t, material, 0);
        }
    }

    /// Rough ellipsoid ("blob") with jittered vertices, smooth normals.
    #[allow(clippy::too_many_arguments)]
    pub fn add_blob(
        &mut self,
        center: V3,
        radii: V3,
        rings: usize,
        segs: usize,
        jitter: f64,
        rng: &mut Rng,
        material: u32,
    ) {
        let mut grid: Vec<Vec<V3>> = Vec::new();
        let top = rings;
        for r in 0..=top {
            let lat = std::f64::consts::PI * (r as f64 / top as f64) - std::f64::consts::FRAC_PI_2;
            let mut row = Vec::new();
            let count = if r == 0 || r == top { 1 } else { segs };
            for s in 0..count {
                let lon = TAU * s as f64 / segs as f64;
                let k = 1.0 + jitter * (rng.f() * 2.0 - 1.0);
                row.push(v3(
                    cos(lat) * cos(lon) * radii.x * k,
                    sin(lat) * radii.y * k,
                    cos(lat) * sin(lon) * radii.z * k,
                ));
            }
            grid.push(row);
        }
        let mut ids: Vec<Vec<u32>> = Vec::new();
        for row in &grid {
            let mut r = Vec::new();
            for p in row {
                let n = v3(
                    p.x / (radii.x * radii.x),
                    p.y / (radii.y * radii.y),
                    p.z / (radii.z * radii.z),
                )
                .norm();
                r.push(self.vertex(
                    center + *p,
                    n,
                    [0.5 + p.x * 0.2, 0.5 + p.z * 0.2 + p.y * 0.1],
                ));
            }
            ids.push(r);
        }
        for r in 0..top {
            let (a, b) = (&ids[r], &ids[r + 1]);
            for s in 0..segs {
                let s1 = (s + 1) % segs;
                if r == 0 {
                    self.tri(a[0], b[s1], b[s], material, 0);
                } else if r + 1 == top {
                    self.tri(a[s], a[s1], b[0], material, 0);
                } else {
                    self.quad([a[s], a[s1], b[s1], b[s]], material, 0);
                }
            }
        }
    }

    /// Append `other` (e.g. a transformed prop) into this mesh.
    pub fn append(&mut self, other: &MeshData) {
        let base = self.pos.len() as u32;
        self.pos.extend(&other.pos);
        self.nrm.extend(&other.nrm);
        self.uv.extend(&other.uv);
        for (t, m, s) in &other.tris {
            self.tris
                .push(([t[0] + base, t[1] + base, t[2] + base], *m, *s));
        }
    }

    /// Write into `bb` as an RCP mesh. Triangles are grouped by material.
    pub fn finish(&self, bb: &mut BufferBuilder, name: &str, with_surface: bool) -> Mesh {
        let mut order: Vec<usize> = (0..self.tris.len()).collect();
        order.sort_by_key(|&i| (self.tris[i].1, i));
        let flat_pos: Vec<f32> = self.pos.iter().flatten().copied().collect();
        let flat_nrm: Vec<f32> = self.nrm.iter().flatten().copied().collect();
        let flat_uv: Vec<f32> = self.uv.iter().flatten().copied().collect();
        let positions = bb.add_f32(ElementKind::Vec3, &flat_pos);
        let normals = bb.add_f32(ElementKind::Vec3, &flat_nrm);
        let uv = bb.add_f32(ElementKind::Vec2, &flat_uv);
        let mut prims: Vec<Primitive> = Vec::new();
        let mut idx32: Vec<u32> = Vec::with_capacity(order.len() * 3);
        let mut surf: Vec<u8> = Vec::with_capacity(order.len());
        for (k, &i) in order.iter().enumerate() {
            let (t, m, s) = self.tris[i];
            idx32.extend(t);
            surf.push(s);
            match prims.last_mut() {
                Some(p) if p.material == m => p.index_count += 3,
                _ => prims.push(Primitive {
                    material: m,
                    first_index: (k * 3) as u32,
                    index_count: 3,
                }),
            }
        }
        let indices = if self.pos.len() <= 65_535 {
            let v16: Vec<u16> = idx32.iter().map(|&i| i as u16).collect();
            bb.add_u16(&v16)
        } else {
            bb.add_u32(&idx32)
        };
        let triangle_surface = with_surface.then(|| bb.add_u8(&surf));
        Mesh {
            name: name.to_string(),
            positions,
            normals: Some(normals),
            uv: Some(uv),
            colors: None,
            indices,
            primitives: prims,
            triangle_surface,
        }
    }
}
