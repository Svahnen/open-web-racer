//! Road model used by the drivers: rows with a frame, lane layout, preferred
//! lane and a target-speed profile. Rows can come from a closed loop of points
//! (derived lanes and a curvature-based speed profile), from a pack track, or
//! from imported road frames; explicit lane and speed data override the
//! derived values.
//!
//! Plane convention: positions are in the simulation plane (x, y), headings are
//! radians counter-clockwise from +x, and a lateral offset is positive to the
//! right of the driving direction. Lane slots are numbered 0..=13 from the far
//! left; slot 7 is the first lane right of the centre line.

use racer_pack_schema::ai::{FIRST_RIGHT_LANE, LANE_SLOTS, SegmentLanes, lane_mask_bit};
use racer_pack_schema::track::Track;

/// Slot index of the first lane left of the centre line.
pub const FIRST_LEFT_LANE: i32 = FIRST_RIGHT_LANE as i32 - 1;

/// One road segment.
#[derive(Clone, Debug, PartialEq)]
pub struct RoadRow {
    pub position: [f64; 2],
    pub forward: [f64; 2],
    /// Unit vector to the right of `forward`.
    pub right: [f64; 2],
    /// Distances from the centre line to the road edges (metres).
    pub left_dist: f64,
    pub right_dist: f64,
    pub left_count: u8,
    pub right_count: u8,
    /// Width of one lane per side (metres).
    pub left_lane_w: f64,
    pub right_lane_w: f64,
    /// Lane availability (bit `lane_mask_bit(slot)`).
    pub mask: u16,
    pub hint: Option<u8>,
}

impl RoadRow {
    /// Whether lane slot `lane` exists on this row.
    pub fn has_lane(&self, lane: i32) -> bool {
        (0..LANE_SLOTS as i32).contains(&lane) && self.mask & lane_mask_bit(lane) != 0
    }

    /// Whether `lane` is a shoulder slot (just outside the driving lanes).
    pub fn is_shoulder(&self, lane: i32) -> bool {
        lane == FIRST_LEFT_LANE - i32::from(self.left_count)
            || lane == FIRST_RIGHT_LANE as i32 + i32::from(self.right_count)
    }

    /// Lateral offset (metres, + right) of the centre of lane slot `lane`.
    pub fn lane_centre(&self, lane: i32) -> f64 {
        let lane = lane.clamp(0, LANE_SLOTS as i32 - 1);
        if lane <= FIRST_LEFT_LANE {
            -(f64::from(FIRST_LEFT_LANE - lane) + 0.5) * self.left_width()
        } else {
            (f64::from(lane - FIRST_RIGHT_LANE as i32) + 0.5) * self.right_width()
        }
    }

    /// Lane slot containing a lateral offset (clamped to the slot array).
    pub fn lane_of(&self, x: f64) -> i32 {
        let lane = if x < 0.0 {
            FIRST_LEFT_LANE + (x / self.left_width()).trunc() as i32
        } else {
            FIRST_RIGHT_LANE as i32 + (x / self.right_width()).trunc() as i32
        };
        lane.clamp(0, LANE_SLOTS as i32 - 1)
    }

    /// Left lane width, falling back to the right width on a side with no lanes.
    pub fn left_width(&self) -> f64 {
        if self.left_count > 0 || self.right_lane_w <= 0.0 {
            self.left_lane_w.max(0.5)
        } else {
            self.right_lane_w
        }
    }

    pub fn right_width(&self) -> f64 {
        if self.right_count > 0 || self.left_lane_w <= 0.0 {
            self.right_lane_w.max(0.5)
        } else {
            self.left_lane_w
        }
    }

    /// Heading of the road at this row.
    pub fn heading(&self) -> f64 {
        self.forward[1].atan2(self.forward[0])
    }

    /// Mask for `left`/`right` driving lanes with one shoulder slot each side.
    pub fn default_mask(left: u8, right: u8) -> u16 {
        let first = FIRST_LEFT_LANE - i32::from(left);
        let last = FIRST_RIGHT_LANE as i32 + i32::from(right);
        (first..=last)
            .filter(|s| (0..LANE_SLOTS as i32).contains(s))
            .fold(0u16, |m, s| m | lane_mask_bit(s))
    }
}

/// How lanes and speeds are derived when a source gives none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeriveParams {
    /// Nominal lane width (metres); the lane count is `round(width / lane_width)`.
    pub lane_width: f64,
    /// Lateral acceleration the speed profile may demand in corners (m/s^2).
    pub corner_accel: f64,
    /// Braking the speed profile may assume when slowing for corners (m/s^2).
    pub brake: f64,
    pub min_speed: f64,
    pub max_speed: f64,
}

impl Default for DeriveParams {
    fn default() -> Self {
        Self {
            lane_width: 3.0,
            corner_accel: 9.0,
            brake: 9.0,
            min_speed: 4.0,
            max_speed: 40.0,
        }
    }
}

/// A road frame from an external source (imported rows, generated tracks).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadFrame {
    pub position: [f64; 2],
    pub forward: [f64; 2],
    pub left_dist: f64,
    pub right_dist: f64,
}

/// Why a track could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackError(pub String);

impl std::fmt::Display for TrackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TrackError {}

/// Rows plus profile and grid data.
#[derive(Clone, Debug, PartialEq)]
pub struct AiTrack {
    pub rows: Vec<RoadRow>,
    pub closed: bool,
    /// Mean row spacing (metres).
    pub row_length: f64,
    /// Segments per profile entry (2 in the usual profile).
    pub profile_entry_rows: usize,
    /// Target speed (whole m/s) per entry, in the forward direction.
    pub profile_forward: Vec<i32>,
    /// Profile for driving backwards (indexed by row from the end); empty
    /// means the forward profile is mirrored.
    pub profile_reverse: Vec<i32>,
    pub default_speed: i32,
    pub grid_start: usize,
    pub grid_spacing: usize,
    /// Late-race pace factor per skill level and the cumulative segments after
    /// which it applies.
    pub late_pace: [f64; 2],
    pub late_after: i32,
    /// `(after_segments, factor)`.
    pub pace_trim: Option<(i32, f64)>,
}

impl AiTrack {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Index after `row` in driving direction `dir` (+1 forward, -1 backward).
    pub fn step(&self, row: usize, dir: i32, by: usize) -> usize {
        let n = self.rows.len() as i64;
        let moved = row as i64 + i64::from(dir) * by as i64;
        if self.closed {
            moved.rem_euclid(n) as usize
        } else {
            moved.clamp(0, n - 1) as usize
        }
    }

    /// Target speed in whole m/s for a row and direction.
    pub fn section_speed(&self, row: usize, dir: i32) -> i32 {
        let per = self.profile_entry_rows.max(1);
        let (table, index) = if dir < 0 && !self.profile_reverse.is_empty() {
            (&self.profile_reverse, (self.rows.len() - 1 - row) / per)
        } else {
            (&self.profile_forward, row / per)
        };
        table.get(index).copied().unwrap_or(self.default_speed)
    }

    /// Row nearest to `position`, searching around `hint` first.
    pub fn locate(&self, position: [f64; 2], hint: Option<usize>) -> usize {
        let n = self.rows.len();
        let dist2 = |i: usize| {
            let p = self.rows[i].position;
            (p[0] - position[0]).powi(2) + (p[1] - position[1]).powi(2)
        };
        if let Some(h) = hint.filter(|h| *h < n) {
            let window = 12usize.min(n / 2);
            let mut best = h;
            let mut best_k = 0;
            let mut best_d = dist2(h);
            for k in 1..=window {
                for dir in [-1, 1] {
                    let i = self.step(h, dir, k);
                    let d = dist2(i);
                    if d < best_d {
                        best = i;
                        best_k = k;
                        best_d = d;
                    }
                }
            }
            // A minimum on the window edge may continue beyond it.
            if best_k < window {
                return best;
            }
        }
        (0..n)
            .min_by(|a, b| dist2(*a).total_cmp(&dist2(*b)))
            .unwrap_or(0)
    }

    /// Position on the road at `row` with lateral offset `x`.
    pub fn point(&self, row: usize, x: f64) -> [f64; 2] {
        let r = &self.rows[row];
        [
            r.position[0] + r.right[0] * x,
            r.position[1] + r.right[1] * x,
        ]
    }

    /// Grid slot `index` of `total` racers: world position and heading.
    /// Position 0 is the rearmost; even positions take the left column, odd the
    /// right; driving backwards flips the order and the parity.
    pub fn grid_slot(&self, index: usize, total: usize, dir: i32) -> ([f64; 2], f64) {
        let n = self.rows.len();
        let order = if dir >= 0 {
            index
        } else {
            total.saturating_sub(1).saturating_sub(index)
        };
        let row_index = self.step(self.grid_start, dir, self.grid_spacing * order + 1);
        let row = &self.rows[row_index.min(n - 1)];
        let odd = (index % 2 == 1) != (dir < 0);
        let x = if row.left_count > 0 {
            if odd {
                row.lane_centre(FIRST_RIGHT_LANE as i32)
            } else {
                row.lane_centre(FIRST_LEFT_LANE)
            }
        } else {
            let w = row.right_width();
            let h = f64::from(row.right_count / 2) * w;
            if odd { h + w * 0.5 } else { h - w * 0.5 }
        };
        let p = self.point(row_index.min(n - 1), x);
        let mut heading = row.heading();
        if dir < 0 {
            heading += std::f64::consts::PI;
        }
        (p, heading)
    }

    /// Replace the speed profile (whole m/s per `entry_rows` segments).
    pub fn set_speed_profile(&mut self, entry_rows: usize, forward: Vec<i32>, reverse: Vec<i32>) {
        self.profile_entry_rows = entry_rows.max(1);
        self.profile_forward = forward;
        self.profile_reverse = reverse;
    }

    /// Replace the lane layout of one row (hook for imported lane tables).
    pub fn set_row_lanes(&mut self, row: usize, lanes: &SegmentLanes) {
        if let Some(r) = self.rows.get_mut(row) {
            apply_lanes(r, lanes);
        }
    }

    /// Build from a centre line (one point per row, equal spacing preferred).
    /// `half_width` gives the distance to each road edge.
    pub fn from_centerline(
        points: &[[f64; 2]],
        closed: bool,
        left_dist: f64,
        right_dist: f64,
        params: &DeriveParams,
    ) -> Result<Self, TrackError> {
        let frames = frames_from_points(points, closed, left_dist, right_dist)?;
        Self::from_frames(&frames, closed, params)
    }

    /// Build from road frames (lanes from the widths, speeds from curvature).
    pub fn from_frames(
        frames: &[RoadFrame],
        closed: bool,
        params: &DeriveParams,
    ) -> Result<Self, TrackError> {
        if frames.len() < 3 {
            return Err(TrackError("a track needs at least 3 rows".into()));
        }
        let mut rows = Vec::with_capacity(frames.len());
        for f in frames {
            let len = f.forward[0].hypot(f.forward[1]);
            if !(len.is_finite() && len > 1e-9)
                || !f.position.iter().all(|v| v.is_finite())
                || !(f.left_dist.is_finite() && f.right_dist.is_finite())
                || f.left_dist < 0.0
                || f.right_dist < 0.0
                || f.left_dist + f.right_dist <= 0.0
            {
                return Err(TrackError("invalid road frame".into()));
            }
            let forward = [f.forward[0] / len, f.forward[1] / len];
            let lanes = |dist: f64| -> (u8, f64) {
                if dist < params.lane_width * 0.5 {
                    (0, params.lane_width)
                } else {
                    let n = (dist / params.lane_width).round().clamp(1.0, 7.0);
                    (n as u8, dist / n)
                }
            };
            let (lc, lw) = lanes(f.left_dist);
            let (rc, rw) = lanes(f.right_dist);
            let (lc, rc) = if lc == 0 && rc == 0 { (0, 1) } else { (lc, rc) };
            rows.push(RoadRow {
                position: f.position,
                forward,
                right: [forward[1], -forward[0]],
                left_dist: f.left_dist,
                right_dist: f.right_dist,
                left_count: lc,
                right_count: rc,
                left_lane_w: lw,
                right_lane_w: rw,
                mask: RoadRow::default_mask(lc, rc),
                hint: None,
            });
        }
        let total: f64 = (0..rows.len())
            .map(|i| {
                let j = if closed {
                    (i + 1) % rows.len()
                } else {
                    (i + 1).min(rows.len() - 1)
                };
                (rows[j].position[0] - rows[i].position[0])
                    .hypot(rows[j].position[1] - rows[i].position[1])
            })
            .sum();
        let row_length = (total / rows.len() as f64).max(0.1);
        let mut track = Self {
            rows,
            closed,
            row_length,
            profile_entry_rows: 2,
            profile_forward: Vec::new(),
            profile_reverse: Vec::new(),
            default_speed: params.max_speed.round().max(1.0) as i32,
            grid_start: 0,
            grid_spacing: 1,
            late_pace: [1.0, 1.0],
            late_after: 100,
            pace_trim: None,
        };
        track.derive_speed_profile(params);
        Ok(track)
    }

    /// Per-pair target speed from curvature: corner speed `sqrt(a / kappa)`,
    /// limited by braking distances so the car can slow in time.
    pub fn derive_speed_profile(&mut self, params: &DeriveParams) {
        let n = self.rows.len();
        let heading = |i: usize| self.rows[i].heading();
        let wrap = |a: f64| {
            let t = std::f64::consts::TAU;
            (a + std::f64::consts::PI).rem_euclid(t) - std::f64::consts::PI
        };
        let mut speed = vec![params.max_speed; n];
        for (i, s) in speed.iter_mut().enumerate() {
            let a = self.step(i, -1, 2);
            let b = self.step(i, 1, 2);
            let span = (self.row_length * 4.0).max(0.1);
            let kappa = wrap(heading(b) - heading(a)).abs() / span;
            if kappa > 1e-6 {
                *s = (params.corner_accel / kappa)
                    .sqrt()
                    .clamp(params.min_speed, params.max_speed);
            }
        }
        let passes = if self.closed { 2 * n } else { n };
        for k in 0..passes {
            let i = if self.closed {
                (2 * n - 1 - k) % n
            } else {
                n - 1 - k
            };
            let next = self.step(i, 1, 1);
            if next != i {
                let limit = (speed[next].powi(2) + 2.0 * params.brake * self.row_length).sqrt();
                if limit < speed[i] {
                    speed[i] = limit.max(params.min_speed);
                }
            }
        }
        let per = self.profile_entry_rows.max(1);
        self.profile_forward = speed
            .chunks(per)
            .map(|c| {
                c.iter()
                    .copied()
                    .fold(f64::INFINITY, f64::min)
                    .floor()
                    .max(1.0) as i32
            })
            .collect();
        self.profile_reverse = Vec::new();
    }

    /// Build from an RCP track (plane = `(x, -z)`). Uses the segment lane data,
    /// the pack's speed profile and grid when present, derives the rest.
    pub fn from_pack_track(track: &Track, params: &DeriveParams) -> Result<Self, TrackError> {
        let frames: Vec<RoadFrame> = track
            .centerline
            .iter()
            .map(|s| RoadFrame {
                position: [f64::from(s.position[0]), -f64::from(s.position[2])],
                forward: [f64::from(s.forward[0]), -f64::from(s.forward[2])],
                left_dist: f64::from(s.left_width_m),
                right_dist: f64::from(s.right_width_m),
            })
            .collect();
        Self::from_pack_track_frames(track, &frames, params)
    }

    /// Like [`Self::from_pack_track`] but with road frames the caller computed
    /// (one per centerline segment), for engines that run the track in a
    /// simulation frame other than the pack's plane.
    pub fn from_pack_track_frames(
        track: &Track,
        frames: &[RoadFrame],
        params: &DeriveParams,
    ) -> Result<Self, TrackError> {
        if frames.len() != track.centerline.len() {
            return Err(TrackError("one road frame per centerline segment".into()));
        }
        let mut out = Self::from_frames(frames, track.closed, params)?;
        for (i, seg) in track.centerline.iter().enumerate() {
            if let Some(lanes) = &seg.lanes {
                apply_lanes(&mut out.rows[i], lanes);
            }
        }
        if let Some(ai) = &track.ai {
            out.default_speed = ai.default_speed_mps.round().max(1.0) as i32;
            if let Some(p) = &ai.speed_profile {
                out.set_speed_profile(
                    p.segments_per_entry as usize,
                    p.forward_mps.iter().map(|v| i32::from(*v)).collect(),
                    p.reverse_mps
                        .as_ref()
                        .map(|r| r.iter().map(|v| i32::from(*v)).collect())
                        .unwrap_or_default(),
                );
            }
            if let Some(g) = &ai.grid {
                out.grid_start = g.start_segment as usize;
                out.grid_spacing = g.spacing_segments.max(1) as usize;
            }
            for (k, f) in ai.late_pace.iter().take(2).enumerate() {
                out.late_pace[k] = f64::from(*f);
            }
            out.late_after = ai.late_pace_after_segments as i32;
            out.pace_trim = ai
                .pace_trim
                .as_ref()
                .map(|t| (t.after_segments as i32, f64::from(t.factor)));
        }
        Ok(out)
    }
}

fn apply_lanes(row: &mut RoadRow, lanes: &SegmentLanes) {
    row.left_count = lanes.left_count;
    row.right_count = lanes.right_count;
    row.left_lane_w = f64::from(lanes.left_width_m);
    row.right_lane_w = f64::from(lanes.right_width_m);
    row.left_dist = f64::from(lanes.left_count) * row.left_lane_w;
    row.right_dist = f64::from(lanes.right_count) * row.right_lane_w;
    row.mask = lanes
        .availability_mask
        .unwrap_or_else(|| RoadRow::default_mask(lanes.left_count, lanes.right_count));
    row.hint = lanes.preferred_lane;
}

/// Frames along a point list (central-difference tangents).
pub fn frames_from_points(
    points: &[[f64; 2]],
    closed: bool,
    left_dist: f64,
    right_dist: f64,
) -> Result<Vec<RoadFrame>, TrackError> {
    let n = points.len();
    if n < 3 {
        return Err(TrackError("a track needs at least 3 points".into()));
    }
    let at = |i: i64| -> [f64; 2] {
        if closed {
            points[i.rem_euclid(n as i64) as usize]
        } else {
            points[i.clamp(0, n as i64 - 1) as usize]
        }
    };
    Ok((0..n)
        .map(|i| {
            let a = at(i as i64 - 1);
            let b = at(i as i64 + 1);
            RoadFrame {
                position: points[i],
                forward: [b[0] - a[0], b[1] - a[1]],
                left_dist,
                right_dist,
            }
        })
        .collect())
}

/// Smooth closed loop through `gates` (centripetal-free Catmull-Rom), resampled
/// at about `row_length` metres. Row 0 is `gates[0]`.
pub fn smooth_loop(gates: &[[f64; 2]], row_length: f64) -> Vec<[f64; 2]> {
    let n = gates.len();
    if n < 3 || row_length <= 0.0 {
        return gates.to_vec();
    }
    let sample = |seg: usize, t: f64| -> [f64; 2] {
        let p0 = gates[(seg + n - 1) % n];
        let p1 = gates[seg % n];
        let p2 = gates[(seg + 1) % n];
        let p3 = gates[(seg + 2) % n];
        let t2 = t * t;
        let t3 = t2 * t;
        let f = |a: f64, b: f64, c: f64, d: f64| {
            0.5 * ((2.0 * b)
                + (-a + c) * t
                + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                + (-a + 3.0 * b - 3.0 * c + d) * t3)
        };
        [f(p0[0], p1[0], p2[0], p3[0]), f(p0[1], p1[1], p2[1], p3[1])]
    };
    const SUB: usize = 64;
    let mut dense = Vec::with_capacity(n * SUB + 1);
    for seg in 0..n {
        for k in 0..SUB {
            dense.push(sample(seg, k as f64 / SUB as f64));
        }
    }
    dense.push(dense[0]);
    let mut cumulative = vec![0.0];
    for w in dense.windows(2) {
        let last = *cumulative.last().expect("non-empty");
        cumulative.push(last + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
    }
    let length = *cumulative.last().expect("non-empty");
    let count = ((length / row_length).round() as usize).max(8);
    let step = length / count as f64;
    let mut out = Vec::with_capacity(count);
    let mut j = 0;
    for r in 0..count {
        let target = r as f64 * step;
        while j + 1 < cumulative.len() - 1 && cumulative[j + 1] < target {
            j += 1;
        }
        let span = (cumulative[j + 1] - cumulative[j]).max(1e-12);
        let u = ((target - cumulative[j]) / span).clamp(0.0, 1.0);
        out.push([
            dense[j][0] + (dense[j + 1][0] - dense[j][0]) * u,
            dense[j][1] + (dense[j + 1][1] - dense[j][1]) * u,
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ellipse() -> AiTrack {
        let gates = [[-20.0, 0.0], [0.0, 12.0], [20.0, 0.0], [0.0, -12.0]];
        let pts = smooth_loop(&gates, 2.0);
        AiTrack::from_centerline(&pts, true, 6.0, 6.0, &DeriveParams::default()).unwrap()
    }

    #[test]
    fn derived_loop_has_lanes_masks_and_curvature_limited_speeds() {
        let t = ellipse();
        assert!(t.len() > 40);
        let r = &t.rows[0];
        assert_eq!((r.left_count, r.right_count), (2, 2));
        assert!(r.has_lane(5) && r.has_lane(8));
        assert!(r.is_shoulder(4) && r.is_shoulder(9));
        assert!(!r.has_lane(3));
        // Row 0 sits on the first gate.
        assert!((r.position[0] + 20.0).abs() < 1e-6 && r.position[1].abs() < 1e-6);
        let min = t.profile_forward.iter().min().copied().unwrap();
        let max = t.profile_forward.iter().max().copied().unwrap();
        assert!(min >= 4 && max > min, "{min} {max}");
    }

    #[test]
    fn lane_centres_and_lane_of_are_inverse() {
        let t = ellipse();
        let r = &t.rows[3];
        for lane in 5..=8 {
            assert_eq!(r.lane_of(r.lane_centre(lane)), lane);
        }
        assert!(r.lane_centre(6) < 0.0 && r.lane_centre(7) > 0.0);
        assert_eq!(r.lane_of(-1000.0), 0);
        assert_eq!(r.lane_of(1000.0), 13);
    }

    #[test]
    fn grid_alternates_columns_and_pole_is_ahead() {
        let t = ellipse();
        let (a, _) = t.grid_slot(0, 4, 1);
        let (b, _) = t.grid_slot(1, 4, 1);
        let (c, _) = t.grid_slot(3, 4, 1);
        // Positions are distinct and the highest index is furthest along.
        assert!(a != b && b != c);
        let ra = t.locate(a, None);
        let rc = t.locate(c, None);
        assert!(rc > ra);
    }

    #[test]
    fn locate_follows_a_hint_and_falls_back_to_a_global_search() {
        let t = ellipse();
        let p = t.rows[20].position;
        assert_eq!(t.locate(p, Some(18)), 20);
        assert_eq!(t.locate(p, Some(t.len() - 1)), 20);
    }

    #[test]
    fn rejects_degenerate_input() {
        assert!(
            AiTrack::from_centerline(&[[0.0, 0.0]; 2], true, 3.0, 3.0, &DeriveParams::default())
                .is_err()
        );
        let bad = [RoadFrame {
            position: [0.0, 0.0],
            forward: [0.0, 0.0],
            left_dist: 3.0,
            right_dist: 3.0,
        }; 4];
        assert!(AiTrack::from_frames(&bad, false, &DeriveParams::default()).is_err());
    }

    #[test]
    fn explicit_lane_and_speed_data_override_derived_values() {
        let mut t = ellipse();
        t.set_row_lanes(
            5,
            &SegmentLanes {
                left_count: 1,
                right_count: 3,
                left_width_m: 3.5,
                right_width_m: 3.0,
                availability_mask: None,
                preferred_lane: Some(8),
            },
        );
        assert_eq!(t.rows[5].hint, Some(8));
        assert!(t.rows[5].has_lane(9) && !t.rows[5].has_lane(4));
        t.set_speed_profile(2, vec![33; 40], Vec::new());
        assert_eq!(t.section_speed(7, 1), 33);
    }
}
