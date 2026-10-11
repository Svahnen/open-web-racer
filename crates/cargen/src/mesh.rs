//! Triangle soup in the RCP frame (+Y up, -Z forward), flat shaded.

use crate::geom::{Face, P, Role, cross, len, q, scale, sub};

/// One flat-shaded triangle.
#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub pos: [[f32; 3]; 3],
    pub normal: [f32; 3],
    pub role: Role,
}

/// Car frame `[u forward, w right, h up]` to RCP `[x right, y up, z back]`.
pub fn to_rcp(p: P) -> [f64; 3] {
    [p[1], p[2], -p[0]]
}

/// Triangulate faces (fan) into RCP-frame triangles. The mapping is a
/// reflection of the numeric triple, so the winding is reversed to keep
/// counter-clockwise-outward triangles.
pub fn triangulate(faces: &[Face]) -> Vec<Tri> {
    let mut out = Vec::new();
    for f in faces {
        let pts: Vec<[f64; 3]> = f.pts.iter().rev().map(|p| to_rcp(*p)).collect();
        for i in 1..pts.len() - 1 {
            let (a, b, c) = (pts[0], pts[i], pts[i + 1]);
            let n = cross(sub(b, a), sub(c, a));
            let l = len(n);
            if l < 1e-9 {
                continue;
            }
            let n = scale(n, 1.0 / l);
            let to32 = |v: [f64; 3]| [q(v[0]) as f32, q(v[1]) as f32, q(v[2]) as f32];
            out.push(Tri {
                pos: [to32(a), to32(b), to32(c)],
                normal: to32(n),
                role: f.role,
            });
        }
    }
    out
}

/// Indexed geometry with one vertex per triangle corner.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u16>,
    pub roles: Vec<Role>,
}

impl Geometry {
    /// Build from triangles (kept in the given order). Fails if the vertex
    /// count would not fit 16-bit indices.
    pub fn from_tris(tris: &[Tri]) -> Geometry {
        let mut g = Geometry::default();
        for t in tris {
            for k in 0..3 {
                let base = (g.positions.len() / 3) as u16;
                g.positions.extend_from_slice(&t.pos[k]);
                g.normals.extend_from_slice(&t.normal);
                g.indices.push(base);
            }
            g.roles.push(t.role);
        }
        assert!(g.positions.len() / 3 <= u16::MAX as usize, "mesh too large");
        g
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }

    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for v in self.positions.chunks_exact(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        (lo, hi)
    }
}

/// Stable-sort triangles by role (keeps authored order inside a role).
pub fn sorted_by_role(tris: &[Tri]) -> Vec<Tri> {
    let mut v = tris.to_vec();
    v.sort_by_key(|t| t.role);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::DESIGNS;

    #[test]
    fn normals_point_away_from_the_body_centre() {
        for d in DESIGNS {
            let tris = triangulate(&d.body_faces());
            let centre = [0.0_f32, 0.6, 0.0];
            // Only the hull (paint and glass): the add-on boxes have faces
            // that legitimately point back into the body.
            let hull: Vec<&Tri> = tris
                .iter()
                .filter(|t| matches!(t.role, Role::Paint | Role::Glass))
                .collect();
            let outward = hull
                .iter()
                .filter(|t| {
                    let c: Vec<f32> = (0..3)
                        .map(|k| (t.pos[0][k] + t.pos[1][k] + t.pos[2][k]) / 3.0 - centre[k])
                        .collect();
                    c[0] * t.normal[0] + c[1] * t.normal[1] + c[2] * t.normal[2] > 0.0
                })
                .count();
            let ratio = outward as f32 / hull.len() as f32;
            assert!(ratio > 0.99, "{} outward ratio {ratio}", d.id);
        }
    }

    #[test]
    fn stored_normals_match_triangle_winding() {
        let tris = triangulate(&DESIGNS[0].body_faces());
        for t in &tris {
            let a = t.pos[0];
            let e1 = [t.pos[1][0] - a[0], t.pos[1][1] - a[1], t.pos[1][2] - a[2]];
            let e2 = [t.pos[2][0] - a[0], t.pos[2][1] - a[1], t.pos[2][2] - a[2]];
            let c = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            assert!(c[0] * t.normal[0] + c[1] * t.normal[1] + c[2] * t.normal[2] > 0.0);
        }
    }
}
