//! Faceted solid-modelling helpers.
//!
//! Cars are authored in a "car frame": `u` forward, `w` right, `h` up, metres,
//! origin on the ground below the middle of the wheelbase. Faces are convex
//! polygons tagged with a material [`Role`]; output stages triangulate them
//! with flat normals and convert to the RCP frame (+Y up, -Z forward).

/// A point in the car frame: `[u, w, h]`.
pub type P = [f64; 3];

/// Material role of a face. The discriminant is the palette index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Paint = 0,
    Accent = 1,
    Glass = 2,
    Tyre = 3,
    Rim = 4,
    Trim = 5,
    LampFront = 6,
    LampRear = 7,
}

impl Role {
    pub const ALL: [Role; 8] = [
        Role::Paint,
        Role::Accent,
        Role::Glass,
        Role::Tyre,
        Role::Rim,
        Role::Trim,
        Role::LampFront,
        Role::LampRear,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Role::Paint => "paint",
            Role::Accent => "accent",
            Role::Glass => "glass",
            Role::Tyre => "tyre",
            Role::Rim => "rim",
            Role::Trim => "trim",
            Role::LampFront => "lamp_front",
            Role::LampRear => "lamp_rear",
        }
    }
}

/// Convex polygon with outward winding already resolved.
#[derive(Clone, Debug)]
pub struct Face {
    pub pts: Vec<P>,
    pub role: Role,
}

pub fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn scale(a: P, s: f64) -> P {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: P, b: P) -> P {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn len(a: P) -> f64 {
    dot(a, a).sqrt()
}

pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn centroid(pts: &[P]) -> P {
    let mut c = [0.0; 3];
    for p in pts {
        c = add(c, *p);
    }
    scale(c, 1.0 / pts.len() as f64)
}

/// Newell normal (not normalised) of a polygon.
fn newell(pts: &[P]) -> P {
    let mut n = [0.0; 3];
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

/// Build a face whose winding makes the normal point along `outward`.
/// Returns `None` for degenerate polygons.
pub fn face(mut pts: Vec<P>, role: Role, outward: P) -> Option<Face> {
    // Drop consecutive duplicates so collapsed loft edges become triangles.
    pts.dedup_by(|a, b| len(sub(*a, *b)) < 1e-9);
    while pts.len() > 1 && len(sub(pts[0], pts[pts.len() - 1])) < 1e-9 {
        pts.pop();
    }
    if pts.len() < 3 {
        return None;
    }
    let n = newell(&pts);
    if len(n) < 1e-9 {
        return None;
    }
    if dot(n, outward) < 0.0 {
        pts.reverse();
    }
    Some(Face { pts, role })
}

/// Face oriented away from the interior point `inside`.
pub fn face_away(pts: Vec<P>, role: Role, inside: P) -> Option<Face> {
    let c = centroid(&pts);
    face(pts, role, sub(c, inside))
}

/// Axis-aligned box.
pub fn boxp(center: P, half: P, role: Role, out: &mut Vec<Face>) {
    let corner = |sx: f64, sy: f64, sz: f64| {
        [
            center[0] + sx * half[0],
            center[1] + sy * half[1],
            center[2] + sz * half[2],
        ]
    };
    let quads: [([P; 4], P); 6] = [
        (
            [
                corner(1.0, -1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(1.0, 1.0, 1.0),
                corner(1.0, -1.0, 1.0),
            ],
            [1.0, 0.0, 0.0],
        ),
        (
            [
                corner(-1.0, -1.0, -1.0),
                corner(-1.0, 1.0, -1.0),
                corner(-1.0, 1.0, 1.0),
                corner(-1.0, -1.0, 1.0),
            ],
            [-1.0, 0.0, 0.0],
        ),
        (
            [
                corner(-1.0, 1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(1.0, 1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
            ],
            [0.0, 1.0, 0.0],
        ),
        (
            [
                corner(-1.0, -1.0, -1.0),
                corner(1.0, -1.0, -1.0),
                corner(1.0, -1.0, 1.0),
                corner(-1.0, -1.0, 1.0),
            ],
            [0.0, -1.0, 0.0],
        ),
        (
            [
                corner(-1.0, -1.0, 1.0),
                corner(1.0, -1.0, 1.0),
                corner(1.0, 1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
            ],
            [0.0, 0.0, 1.0],
        ),
        (
            [
                corner(-1.0, -1.0, -1.0),
                corner(1.0, -1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(-1.0, 1.0, -1.0),
            ],
            [0.0, 0.0, -1.0],
        ),
    ];
    for (pts, n) in quads {
        if let Some(f) = face(pts.to_vec(), role, n) {
            out.push(f);
        }
    }
}

/// Connect consecutive rings with quads. `closed` joins the last point of a
/// ring to its first. `role(segment, edge)` picks the material of each quad.
/// Faces point away from the axis through the ring centres.
pub fn loft(
    rings: &[Vec<P>],
    closed: bool,
    role: &dyn Fn(usize, usize) -> Role,
    out: &mut Vec<Face>,
) {
    for s in 0..rings.len() - 1 {
        let (a, b) = (&rings[s], &rings[s + 1]);
        let edges = if closed { a.len() } else { a.len() - 1 };
        for e in 0..edges {
            let e2 = (e + 1) % a.len();
            let quad = vec![a[e], b[e], b[e2], a[e2]];
            // Interior reference: centre of both rings' bounding middle.
            let mid_h = |r: &Vec<P>| {
                let lo = r.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
                let hi = r.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
                0.5 * (lo + hi)
            };
            let c = centroid(&quad);
            let inside = [c[0], 0.0, 0.5 * (mid_h(a) + mid_h(b))];
            if let Some(f) = face_away(quad, role(s, e), inside) {
                out.push(f);
            }
        }
    }
}

/// Cap a ring with one fan polygon facing along `outward`.
pub fn cap(ring: &[P], role: Role, outward: P, out: &mut Vec<Face>) {
    if let Some(f) = face(ring.to_vec(), role, outward) {
        out.push(f);
    }
}

/// Round to 1e-4 so platform libm differences cannot change output bytes.
pub fn q(v: f64) -> f64 {
    let r = (v * 10_000.0).round() / 10_000.0;
    if r == 0.0 { 0.0 } else { r }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_faces_point_outward_and_are_closed() {
        let mut faces = Vec::new();
        boxp([1.0, 2.0, 3.0], [0.5, 0.25, 0.1], Role::Paint, &mut faces);
        assert_eq!(faces.len(), 6);
        let c = [1.0, 2.0, 3.0];
        for f in &faces {
            let n = newell(&f.pts);
            assert!(dot(n, sub(centroid(&f.pts), c)) > 0.0);
        }
    }

    #[test]
    fn degenerate_polygons_are_dropped() {
        assert!(
            face(
                vec![[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
                Role::Paint,
                [0.0, 0.0, 1.0]
            )
            .is_none()
        );
    }
}
