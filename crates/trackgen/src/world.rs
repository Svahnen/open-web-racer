//! The ground around the road: road cross-section profile, nearest-road
//! queries and the terrain height field.

use crate::math::{V3, clamp, fbm2, lerp, smoothstep, v3};
use crate::road::{KERB_WIDTH_M, Road, Section};
use crate::style::{Style, bump_amplitude, far_height};

/// Downward slope of the shoulder away from the road (m per m).
pub const SHOULDER_SLOPE: f64 = 0.03;
/// Height of the kerb above the road plane.
pub const KERB_RAISE: f64 = 0.04;
/// Terrain sits this far below the barrier base next to the road.
pub const TERRAIN_DROP: f64 = 0.35;
/// Terrain under the road surface is pushed down this far (hidden).
pub const UNDER_ROAD_DROP: f64 = 0.9;
const GRID_CELL: f64 = 48.0;
/// Terrain is generated out to this distance from the road edge.
pub const TERRAIN_REACH_M: f64 = 460.0;

pub struct Near {
    pub idx: usize,
    /// Horizontal distance from the query point to the section centre.
    pub dist: f64,
    /// Signed lateral offset along the section's horizontal right axis.
    pub lat: f64,
}

pub struct World<'a> {
    pub road: &'a Road,
    pub style: &'a Style,
    pub seed: u64,
    x0: f64,
    z0: f64,
    nx: i64,
    nz: i64,
    cells: Vec<Vec<u32>>,
}

/// Lateral distance (from the centreline) of the barrier face.
pub fn edge_lat(style: &Style, sec: &Section) -> f64 {
    sec.hw + KERB_WIDTH_M + style.shoulder_width
}

/// Point on the road cross-section at lateral offset `lat` (+ right): banked
/// plane, raised kerb and sloping shoulder.
pub fn surface_point(sec: &Section, lat: f64) -> V3 {
    let a = lat.abs();
    let mut p = sec.pos + sec.right * lat;
    if a > sec.hw {
        p.y += KERB_RAISE * ((a - sec.hw) / KERB_WIDTH_M).min(1.0);
        if a > sec.hw + KERB_WIDTH_M {
            p.y -= SHOULDER_SLOPE * (a - sec.hw - KERB_WIDTH_M);
        }
    }
    p
}

impl<'a> World<'a> {
    pub fn new(road: &'a Road, style: &'a Style, seed: u64) -> Self {
        let (mut x0, mut x1, mut z0, mut z1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for s in &road.secs {
            x0 = x0.min(s.pos.x);
            x1 = x1.max(s.pos.x);
            z0 = z0.min(s.pos.z);
            z1 = z1.max(s.pos.z);
        }
        let pad = TERRAIN_REACH_M + 60.0;
        let (x0, z0) = (x0 - pad, z0 - pad);
        let nx = ((x1 + pad - x0) / GRID_CELL).ceil() as i64 + 1;
        let nz = ((z1 + pad - z0) / GRID_CELL).ceil() as i64 + 1;
        let mut cells = vec![Vec::new(); (nx * nz) as usize];
        for (i, s) in road.secs.iter().enumerate() {
            let cx = ((s.pos.x - x0) / GRID_CELL) as i64;
            let cz = ((s.pos.z - z0) / GRID_CELL) as i64;
            cells[(cz * nx + cx) as usize].push(i as u32);
        }
        World {
            road,
            style,
            seed,
            x0,
            z0,
            nx,
            nz,
            cells,
        }
    }

    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        (
            self.x0,
            self.z0,
            self.x0 + self.nx as f64 * GRID_CELL,
            self.z0 + self.nz as f64 * GRID_CELL,
        )
    }

    /// Nearest road section within `max_radius` (horizontal).
    pub fn nearest(&self, x: f64, z: f64, max_radius: f64) -> Option<Near> {
        let cx = ((x - self.x0) / GRID_CELL).floor() as i64;
        let cz = ((z - self.z0) / GRID_CELL).floor() as i64;
        let max_ring = (max_radius / GRID_CELL).ceil() as i64 + 1;
        let mut best: Option<(usize, f64)> = None;
        for ring in 0..=max_ring {
            for dz in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs().max(dz.abs()) != ring {
                        continue;
                    }
                    let (ix, iz) = (cx + dx, cz + dz);
                    if ix < 0 || iz < 0 || ix >= self.nx || iz >= self.nz {
                        continue;
                    }
                    for &si in &self.cells[(iz * self.nx + ix) as usize] {
                        let s = &self.road.secs[si as usize];
                        let d = ((s.pos.x - x).powi(2) + (s.pos.z - z).powi(2)).sqrt();
                        if best.is_none_or(|(_, bd)| d < bd) {
                            best = Some((si as usize, d));
                        }
                    }
                }
            }
            if let Some((_, bd)) = best {
                if bd <= ring as f64 * GRID_CELL {
                    break;
                }
            }
        }
        let (idx, dist) = best?;
        if dist > max_radius {
            return None;
        }
        let s = &self.road.secs[idx];
        let rh = v3(s.right.x, 0.0, s.right.z).norm();
        let lat = (x - s.pos.x) * rh.x + (z - s.pos.z) * rh.z;
        Some(Near { idx, dist, lat })
    }

    /// Distance beyond the barrier line (0 inside the road corridor).
    pub fn outside_distance(&self, x: f64, z: f64) -> Option<f64> {
        let n = self.nearest(x, z, TERRAIN_REACH_M + 120.0)?;
        let edge = edge_lat(self.style, &self.road.secs[n.idx]);
        Some((n.dist - edge).max(0.0))
    }

    /// Terrain surface height at ground position (x, z).
    pub fn height(&self, x: f64, z: f64) -> f64 {
        let theme = self.style.theme;
        let far = far_height(theme, self.seed, x, z);
        let Some(n) = self.nearest(x, z, TERRAIN_REACH_M + 120.0) else {
            return far;
        };
        let sec = &self.road.secs[n.idx];
        let edge = edge_lat(self.style, sec);
        let o = (n.dist - edge).max(0.0);
        if n.dist <= edge && o == 0.0 {
            // Under the road corridor: hide the ground below the surface.
            let lat = clamp(n.lat, -edge, edge);
            return surface_point(sec, lat).y - UNDER_ROAD_DROP;
        }
        let side = if n.lat >= 0.0 { 1.0 } else { -1.0 };
        let base = surface_point(sec, side * edge).y - TERRAIN_DROP;
        let t_far = smoothstep(4.0, 170.0, o);
        let bump = (fbm2(self.seed ^ 0xB0, x / 38.0, z / 38.0, 3, 0, 0) - 0.5)
            * 2.0
            * bump_amplitude(theme)
            * smoothstep(3.0, 45.0, o);
        let y = lerp(base, far, t_far) + bump;
        // do not let ground tower over the barrier right next to the road
        y.min(base + 0.5 + 0.5 * o)
    }

    /// Terrain normal from central differences.
    pub fn normal(&self, x: f64, z: f64, step: f64) -> V3 {
        let hx = self.height(x + step, z) - self.height(x - step, z);
        let hz = self.height(x, z + step) - self.height(x, z - step);
        v3(-hx / (2.0 * step), 1.0, -hz / (2.0 * step)).norm()
    }
}
