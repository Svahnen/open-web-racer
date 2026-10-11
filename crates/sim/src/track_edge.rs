//! Road-edge wall for imported tracks (tests/feel).
//!
//! The observed part is the contact law: a hard wall at `-left` / `+right`
//! along each row's right vector, an oriented-box car, restitution 0, a
//! tangential friction loss `mu * |v_n|` and a heading rotation toward the wall
//! tangent per contact tick. Everything about row lookup, interpolation, and
//! reset is project semantics (provisional, documented in the spec).
//!
//! All lengths are simulation units; the car box is defined in metres and
//! converted with `units_per_sim_unit`. The law is specified per 1/32 s tick
//! and time-converted for other step sizes (`ticks = dt * 32`).

use std::collections::HashMap;

use crate::Vec2;

/// Oriented-box half-width (m).
pub const BOX_HALF_WIDTH_M: f64 = 1.05;
/// Oriented-box half-length (m).
pub const BOX_HALF_LENGTH_M: f64 = 2.2;
/// Tangential speed lost per unit of normal speed removed per 1/32 s contact tick.
pub const WALL_FRICTION: f64 = 0.73;
/// Fraction of the angle to the wall tangent the heading rotates per contact tick.
pub const WALL_HEADING_GAIN: f64 = 0.19;
/// Tick rate the law was measured at.
pub const OBSERVED_TICKS_PER_SECOND: f64 = 32.0;
/// Revision of the edge-wall equations and project semantics; part of the
/// simulation identity whenever a world carries edge rows.
pub const TRACK_EDGE_MODEL_REVISION: &str = "track-edge-wall-v1";

/// Project semantics: seconds without ground support before a reset.
pub const NO_GROUND_RESET_SECONDS: f64 = 0.5;
/// Project semantics (metres): a car whose centre is further outside the edge
/// than this after the wall pass (tunnelled or spawned outside) is reset.
pub const OUTSIDE_RESET_MARGIN_M: f64 = 4.0;
/// Project semantics (metres): no row within this distance means "outside all rows".
pub const MAX_ROW_LOOKUP_M: f64 = 300.0;
/// Project semantics (metres): rows further apart than this are not neighbours.
pub const MAX_ROW_SPACING_M: f64 = 300.0;
/// Mesh walls closer than this to a road edge (metres, inside) are redundant
/// with the edge wall and dropped when edge rows are installed.
pub const MESH_WALL_EDGE_INSET_M: f64 = 3.0;

pub const MAX_EDGE_ROWS: usize = 65_535;
const GRID_CELL: f64 = 64.0;

/// One type-15 reference row in simulation coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackEdgeRow {
    pub x: f64,
    pub y: f64,
    /// Scene-space height of the row.
    pub z: f64,
    /// Unit track direction in simulation XY.
    pub forward: Vec2,
    /// Unit right vector in simulation XY (the lateral axis).
    pub right: Vec2,
    pub left_distance: f64,
    pub right_distance: f64,
    pub block: u16,
}

/// Interpolated road frame at a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeFrame {
    pub origin: Vec2,
    pub z: f64,
    pub forward: Vec2,
    pub right: Vec2,
    pub left_distance: f64,
    pub right_distance: f64,
    pub row: usize,
}

impl EdgeFrame {
    /// Signed lateral coordinate of `p` (positive = right).
    pub fn lateral(&self, p: Vec2) -> f64 {
        (p - self.origin).dot(self.right)
    }

    /// Centre of the road at this frame.
    pub fn road_centre(&self) -> Vec2 {
        self.origin + self.right * ((self.right_distance - self.left_distance) * 0.5)
    }
}

/// Car box and tuning, in simulation units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeBody {
    pub half_width: f64,
    pub half_length: f64,
    pub mu: f64,
    pub heading_gain: f64,
    /// Fixed step in seconds.
    pub dt: f64,
}

impl EdgeBody {
    pub fn observed(units_per_sim_unit: f64, dt: f64) -> Self {
        Self {
            half_width: BOX_HALF_WIDTH_M / units_per_sim_unit,
            half_length: BOX_HALF_LENGTH_M / units_per_sim_unit,
            mu: WALL_FRICTION,
            heading_gain: WALL_HEADING_GAIN,
            dt,
        }
    }
}

/// Extent of the box along the wall normal for heading angle `theta` (radians)
/// between heading and wall tangent: `w cos(theta) + l sin(theta)`.
pub fn box_extent(half_width: f64, half_length: f64, theta: f64) -> f64 {
    half_width * theta.cos().abs() + half_length * theta.sin().abs()
}

/// Result of one tick of the contact law.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactResponse {
    pub velocity: Vec2,
    pub heading: f64,
}

/// One contact tick. `normal` is the outward wall normal (unit). Only an
/// outward-moving car is changed; resting or departing cars keep their
/// velocity and heading. Time conversion: `k = dt * 32` scales the friction
/// loss and compounds the heading gain, so `dt = 1/32` is the measured tick.
pub fn contact_response(
    velocity: Vec2,
    heading: f64,
    normal: Vec2,
    body: EdgeBody,
) -> ContactResponse {
    let tangent = Vec2::new(-normal.y, normal.x);
    let v_n = velocity.dot(normal);
    if v_n <= 0.0 {
        return ContactResponse { velocity, heading };
    }
    let k = body.dt * OBSERVED_TICKS_PER_SECOND;
    let v_t = velocity.dot(tangent);
    let loss = body.mu * k * v_n;
    let v_t_new = v_t.signum() * (v_t.abs() - loss).max(0.0);
    let new_velocity = if v_t == 0.0 {
        Vec2::default()
    } else {
        tangent * v_t_new
    };

    // Heading rotates toward the nearer tangent direction.
    let h = Vec2::new(heading.cos(), heading.sin());
    let t_dir = if h.dot(tangent) >= 0.0 {
        tangent
    } else {
        tangent * -1.0
    };
    let signed_angle = h.cross(t_dir).atan2(h.dot(t_dir));
    let fraction = 1.0 - (1.0 - body.heading_gain).powf(k);
    let new_heading = (heading + fraction * signed_angle).rem_euclid(std::f64::consts::TAU);
    ContactResponse {
        velocity: new_velocity,
        heading: new_heading,
    }
}

/// Corners of the oriented box.
pub fn box_corners(position: Vec2, heading: f64, half_width: f64, half_length: f64) -> [Vec2; 4] {
    let f = Vec2::new(heading.cos(), heading.sin());
    let r = Vec2::new(-f.y, f.x);
    [
        position + f * half_length + r * half_width,
        position + f * half_length - r * half_width,
        position - f * half_length + r * half_width,
        position - f * half_length - r * half_width,
    ]
}

/// What the edge pass decided for one vehicle this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdgeOutcome {
    /// Inside the edges, nothing changed.
    Clear,
    /// The box touched an edge; position (and, if moving outward, velocity and
    /// heading) were corrected.
    Contact {
        position: Vec2,
        velocity: Vec2,
        heading: f64,
        /// Penetration depth that was removed.
        depth: f64,
    },
    /// Outside all rows or beyond the reset margin: reset to this frame.
    Reset(EdgeFrame),
}

/// Rows plus a uniform grid for nearest-row queries.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackEdge {
    rows: Vec<TrackEdgeRow>,
    cells: HashMap<(i32, i32), Vec<u32>>,
    units_per_sim_unit: f64,
}

impl TrackEdge {
    /// Validate rows: finite values, unit vectors within tolerance, positive
    /// distances.
    pub fn validate_rows(rows: &[TrackEdgeRow]) -> Result<(), &'static str> {
        if rows.len() > MAX_EDGE_ROWS {
            return Err("track edge rows exceed their count limit");
        }
        for row in rows {
            let finite = [
                row.x,
                row.y,
                row.z,
                row.forward.x,
                row.forward.y,
                row.right.x,
                row.right.y,
                row.left_distance,
                row.right_distance,
            ]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1.0e6);
            if !finite
                || (row.right.length() - 1.0).abs() > 1.0e-3
                || (row.forward.length() - 1.0).abs() > 1.0e-3
                || row.left_distance <= 0.0
                || row.right_distance <= 0.0
                || row.left_distance > 1000.0
                || row.right_distance > 1000.0
            {
                return Err("track edge row is invalid");
            }
        }
        Ok(())
    }

    pub fn new(rows: Vec<TrackEdgeRow>, units_per_sim_unit: f64) -> Result<Self, &'static str> {
        Self::validate_rows(&rows)?;
        if !units_per_sim_unit.is_finite() || units_per_sim_unit <= 0.0 {
            return Err("track edge scale is invalid");
        }
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (index, row) in rows.iter().enumerate() {
            cells
                .entry(cell_of(row.x, row.y))
                .or_default()
                .push(index as u32);
        }
        Ok(Self {
            rows,
            cells,
            units_per_sim_unit,
        })
    }

    pub fn rows(&self) -> &[TrackEdgeRow] {
        &self.rows
    }

    fn lookup_limit(&self) -> f64 {
        MAX_ROW_LOOKUP_M / self.units_per_sim_unit
    }

    fn spacing_limit(&self) -> f64 {
        MAX_ROW_SPACING_M / self.units_per_sim_unit
    }

    /// Nearest row to `p` by 3D distance (`z` is the reference height; ties go
    /// to the lowest index).
    pub fn nearest_row(&self, p: Vec2, z: f64) -> Option<usize> {
        let limit = self.lookup_limit();
        let (cx, cy) = cell_of(p.x, p.y);
        let max_ring = (limit / GRID_CELL).ceil() as i32 + 1;
        let mut best: Option<(f64, usize)> = None;
        for ring in 0..=max_ring {
            for dx in -ring..=ring {
                for dy in -ring..=ring {
                    if dx.abs().max(dy.abs()) != ring {
                        continue;
                    }
                    let Some(bucket) = self.cells.get(&(cx + dx, cy + dy)) else {
                        continue;
                    };
                    for &index in bucket {
                        let row = &self.rows[index as usize];
                        let d2 =
                            (row.x - p.x).powi(2) + (row.y - p.y).powi(2) + (row.z - z).powi(2);
                        let better = match best {
                            None => true,
                            Some((b, bi)) => d2 < b || (d2 == b && (index as usize) < bi),
                        };
                        if better {
                            best = Some((d2, index as usize));
                        }
                    }
                }
            }
            if let Some((d2, _)) = best {
                if d2.sqrt() <= f64::from(ring) * GRID_CELL {
                    break;
                }
            }
        }
        best.filter(|(d2, _)| d2.sqrt() <= limit).map(|(_, i)| i)
    }

    fn neighbour(&self, index: usize, step: isize) -> Option<usize> {
        let n = self.rows.len();
        if n < 2 {
            return None;
        }
        let j = index as isize + step;
        let j = if j < 0 {
            n - 1
        } else if j as usize >= n {
            0
        } else {
            j as usize
        };
        if j == index {
            return None;
        }
        let a = &self.rows[index];
        let b = &self.rows[j];
        let horizontal = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        // Rows on another level (bridge over road) or far away are not neighbours.
        (horizontal <= self.spacing_limit() && (b.z - a.z).abs() <= 0.5 * horizontal + 1.0)
            .then_some(j)
    }

    /// Road frame at `p`: nearest row, interpolated toward the neighbour the
    /// point has advanced towards (clamped to the segment).
    pub fn frame_at(&self, p: Vec2, z: f64) -> Option<EdgeFrame> {
        let i = self.nearest_row(p, z)?;
        let a = self.rows[i];
        let mut best: Option<(f64, usize)> = None;
        for step in [-1isize, 1] {
            let Some(j) = self.neighbour(i, step) else {
                continue;
            };
            let b = &self.rows[j];
            let seg = Vec2::new(b.x - a.x, b.y - a.y);
            let len2 = seg.dot(seg);
            if len2 <= 1.0e-12 {
                continue;
            }
            let t = (p - Vec2::new(a.x, a.y)).dot(seg) / len2;
            if t > 0.0 && best.is_none_or(|(bt, _)| t > bt) {
                best = Some((t.min(1.0), j));
            }
        }
        let frame = match best {
            None => EdgeFrame {
                origin: Vec2::new(a.x, a.y),
                z: a.z,
                forward: a.forward,
                right: a.right,
                left_distance: a.left_distance,
                right_distance: a.right_distance,
                row: i,
            },
            Some((t, j)) => {
                let b = self.rows[j];
                let lerp = |u: f64, v: f64| u + (v - u) * t;
                let unit = |v: Vec2, fallback: Vec2| v.normalized().unwrap_or(fallback);
                EdgeFrame {
                    origin: Vec2::new(lerp(a.x, b.x), lerp(a.y, b.y)),
                    z: lerp(a.z, b.z),
                    forward: unit(
                        Vec2::new(
                            lerp(a.forward.x, b.forward.x),
                            lerp(a.forward.y, b.forward.y),
                        ),
                        a.forward,
                    ),
                    right: unit(
                        Vec2::new(lerp(a.right.x, b.right.x), lerp(a.right.y, b.right.y)),
                        a.right,
                    ),
                    left_distance: lerp(a.left_distance, b.left_distance),
                    right_distance: lerp(a.right_distance, b.right_distance),
                    row: i,
                }
            }
        };
        Some(frame)
    }

    /// Resolve the oriented box against both road edges for one tick.
    /// `ground_z` is the reference height for row lookup.
    pub fn resolve(
        &self,
        mut position: Vec2,
        heading: f64,
        velocity: Vec2,
        ground_z: f64,
        body: EdgeBody,
    ) -> EdgeOutcome {
        let Some(centre_frame) = self.frame_at(position, ground_z) else {
            return match self.nearest_any(position, ground_z) {
                Some(frame) => EdgeOutcome::Reset(frame),
                None => EdgeOutcome::Clear,
            };
        };
        let margin = OUTSIDE_RESET_MARGIN_M / self.units_per_sim_unit;
        let s = centre_frame.lateral(position);
        if s > centre_frame.right_distance + margin || s < -(centre_frame.left_distance + margin) {
            return EdgeOutcome::Reset(centre_frame);
        }

        let mut first: Option<(Vec2, f64)> = None; // (outward normal, depth)
        let mut total_depth = 0.0;
        for _ in 0..3 {
            let mut deepest: Option<(Vec2, f64)> = None;
            for corner in box_corners(position, heading, body.half_width, body.half_length) {
                let Some(frame) = self.frame_at(corner, ground_z) else {
                    continue;
                };
                let s = frame.lateral(corner);
                for (normal, depth) in [
                    (frame.right, s - frame.right_distance),
                    (frame.right * -1.0, -s - frame.left_distance),
                ] {
                    if depth > 1.0e-9 && deepest.is_none_or(|(_, d)| depth > d) {
                        deepest = Some((normal, depth));
                    }
                }
            }
            let Some((normal, depth)) = deepest else {
                break;
            };
            position = position - normal * depth;
            total_depth += depth;
            if first.is_none() {
                first = Some((normal, depth));
            }
        }
        match first {
            None => EdgeOutcome::Clear,
            Some((normal, _)) => {
                let response = contact_response(velocity, heading, normal, body);
                EdgeOutcome::Contact {
                    position,
                    velocity: response.velocity,
                    heading: response.heading,
                    depth: total_depth,
                }
            }
        }
    }

    /// Nearest frame ignoring the lookup limit (used for resets of far-away cars).
    fn nearest_any(&self, p: Vec2, z: f64) -> Option<EdgeFrame> {
        let mut best: Option<(f64, usize)> = None;
        for (index, row) in self.rows.iter().enumerate() {
            let d2 = (row.x - p.x).powi(2) + (row.y - p.y).powi(2) + (row.z - z).powi(2);
            if best.is_none_or(|(b, _)| d2 < b) {
                best = Some((d2, index));
            }
        }
        let row = self.rows[best?.1];
        Some(EdgeFrame {
            origin: Vec2::new(row.x, row.y),
            z: row.z,
            forward: row.forward,
            right: row.right,
            left_distance: row.left_distance,
            right_distance: row.right_distance,
            row: best?.1,
        })
    }

    /// Whether a mesh wall segment is redundant with the edge wall: its
    /// midpoint lies outside the road or within the inset of an edge.
    pub fn mesh_wall_is_redundant(&self, midpoint: Vec2, z: f64) -> bool {
        let Some(frame) = self.frame_at(midpoint, z) else {
            return false;
        };
        let inset = MESH_WALL_EDGE_INSET_M / self.units_per_sim_unit;
        let s = frame.lateral(midpoint);
        s >= frame.right_distance - inset || s <= -(frame.left_distance - inset)
    }
}

fn cell_of(x: f64, y: f64) -> (i32, i32) {
    (
        (x / GRID_CELL).floor().clamp(-1.0e9, 1.0e9) as i32,
        (y / GRID_CELL).floor().clamp(-1.0e9, 1.0e9) as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(left: f64, right: f64) -> TrackEdge {
        // Forward +X, right = -Y (sim heading is counter-clockwise from +X).
        let rows = (0..40)
            .map(|i| TrackEdgeRow {
                x: i as f64 * 10.0,
                y: 0.0,
                z: 0.0,
                forward: Vec2::new(1.0, 0.0),
                right: Vec2::new(0.0, -1.0),
                left_distance: left,
                right_distance: right,
                block: (i / 8) as u16,
            })
            .collect();
        TrackEdge::new(rows, 1.0).unwrap()
    }

    #[test]
    fn moving_away_or_resting_keeps_velocity() {
        let body = EdgeBody::observed(1.0, 1.0 / 32.0);
        let normal = Vec2::new(0.0, 1.0);
        let v = Vec2::new(20.0, -1.0);
        let r = contact_response(v, 0.1, normal, body);
        assert_eq!(r.velocity, v);
        assert_eq!(r.heading, 0.1);
    }

    #[test]
    fn invalid_rows_are_rejected() {
        let mut row = straight(2.0, 2.0).rows()[0];
        row.left_distance = 0.0;
        assert!(TrackEdge::new(vec![row], 1.0).is_err());
        let mut row = straight(2.0, 2.0).rows()[0];
        row.right = Vec2::new(0.0, 2.0);
        assert!(TrackEdge::new(vec![row], 1.0).is_err());
        let mut row = straight(2.0, 2.0).rows()[0];
        row.x = f64::NAN;
        assert!(TrackEdge::new(vec![row], 1.0).is_err());
    }
}
