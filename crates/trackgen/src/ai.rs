//! Computer-driver data: per-segment lane layout, a curvature-derived target
//! speed profile and the start grid, in the schema of `racer_pack_schema::ai`.

use crate::math::{clamp, sin};
use crate::road::Road;
use racer_pack_schema::ai::{GridLayout, SegmentLanes, SpeedProfile, TrackAi};

/// Lateral grip budget used for corner speeds (m/s^2).
pub const LATERAL_ACCEL: f64 = 10.0;
/// Braking and acceleration limits used to shape the profile (m/s^2). The
/// braking limit stays below what every demo car can brake with, so a driver
/// that follows the profile can always slow down in time.
pub const BRAKE_DECEL: f64 = 7.0;
pub const ACCEL: f64 = 7.5;
pub const MAX_SPEED: f64 = 64.0;
pub const MIN_SPEED: f64 = 14.0;
/// Segments covered by one speed-profile entry.
pub const SEGMENTS_PER_ENTRY: usize = 2;
/// Grid positions the AI field lines up (two columns, spacing in segments).
pub const GRID_SPACING_SEGMENTS: usize = 2;
const GRID_POSITIONS: usize = 8;

fn wrap(i: isize, n: usize) -> usize {
    i.rem_euclid(n as isize) as usize
}

/// Target speed per section (m/s): corner speed from curvature and banking,
/// shaped by braking and acceleration limits around the closed lap.
pub fn section_speeds(road: &Road) -> Vec<f64> {
    let n = road.len();
    let limit: Vec<f64> = (0..n)
        .map(|i| {
            let s = &road.secs[i];
            // the racing line takes corners about 20% wider than the centreline
            let k_eff = (s.kappa.abs() * 0.8).max(1e-5);
            let bank_help = 9.81 * sin(s.bank.abs());
            clamp(
                ((LATERAL_ACCEL + bank_help) / k_eff).sqrt(),
                MIN_SPEED,
                MAX_SPEED,
            )
        })
        .collect();
    shape_profile(limit, road.ds)
}

/// Apply braking and acceleration limits around the closed lap.
fn shape_profile(mut v: Vec<f64>, ds: f64) -> Vec<f64> {
    let n = v.len();
    for _ in 0..3 {
        for step in (0..2 * n).rev() {
            let i = step % n;
            let next = v[(i + 1) % n];
            let allowed = (next * next + 2.0 * BRAKE_DECEL * ds).sqrt();
            if v[i] > allowed {
                v[i] = allowed;
            }
        }
        for step in 0..2 * n {
            let i = step % n;
            let j = (i + 1) % n;
            let allowed = (v[i] * v[i] + 2.0 * ACCEL * ds).sqrt();
            if v[j] > allowed {
                v[j] = allowed;
            }
        }
    }
    v
}

/// Two driving lanes per side across the paved width.
pub fn segment_lanes(road: &Road) -> Vec<SegmentLanes> {
    road.secs
        .iter()
        .map(|s| SegmentLanes {
            left_count: 2,
            right_count: 2,
            left_width_m: (s.hw / 2.0) as f32,
            right_width_m: (s.hw / 2.0) as f32,
            availability_mask: None,
            preferred_lane: None,
        })
        .collect()
}

/// Speed profile, grid and neutral pace settings of the track.
pub fn track_ai(road: &Road) -> TrackAi {
    let speeds = section_speeds(road);
    let forward_mps: Vec<u16> = speeds
        .chunks(SEGMENTS_PER_ENTRY)
        .map(|c| {
            c.iter()
                .copied()
                .fold(f64::INFINITY, f64::min)
                .floor()
                .max(1.0) as u16
        })
        .collect();
    let n = road.len();
    let back = (GRID_POSITIONS - 1) * GRID_SPACING_SEGMENTS;
    TrackAi {
        default_speed_mps: 40.0,
        speed_profile: Some(SpeedProfile {
            segments_per_entry: SEGMENTS_PER_ENTRY as u32,
            forward_mps,
            reverse_mps: None,
        }),
        grid: Some(GridLayout {
            start_segment: wrap(-(back as isize), n) as u32,
            spacing_segments: GRID_SPACING_SEGMENTS as u32,
        }),
        late_pace: Vec::new(),
        late_pace_after_segments: 100,
        pace_trim: None,
    }
}
