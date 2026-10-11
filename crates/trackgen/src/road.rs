//! Road centreline: closed Catmull-Rom spline resampled at uniform spacing,
//! with slope, curvature and banking frames.

use crate::math::{V3, clamp, cos, sin, v3};
use crate::spec::TrackSpec;

/// Target spacing between road sections in metres.
pub const SECTION_SPACING_M: f64 = 4.0;
/// Kerb strip width on each side of the paved road.
pub const KERB_WIDTH_M: f64 = 1.2;

/// Road frame at one station.
#[derive(Clone, Copy, Debug)]
pub struct Section {
    /// Distance along the lap in metres.
    pub s: f64,
    /// Centre point on the road surface.
    pub pos: V3,
    pub fwd: V3,
    pub up: V3,
    pub right: V3,
    /// Half of the paved width.
    pub hw: f64,
    /// Smoothed signed horizontal curvature, 1/m (positive = right turn).
    pub kappa: f64,
    /// Bank angle in radians (positive = right side lower).
    pub bank: f64,
}

#[derive(Clone, Debug)]
pub struct Road {
    pub secs: Vec<Section>,
    pub length: f64,
    pub ds: f64,
}

/// Summary used by tests and the exporter.
#[derive(Clone, Copy, Debug)]
pub struct RoadStats {
    pub length_m: f64,
    pub min_radius_m: f64,
    pub max_grade: f64,
    pub max_bank_deg: f64,
    pub elevation_range_m: f64,
    /// Smallest ground distance between parts of the road that are at least
    /// 250 m apart along the lap.
    pub min_clearance_m: f64,
}

fn catmull(p0: V3, p1: V3, p2: V3, p3: V3, t: f64) -> V3 {
    let t2 = t * t;
    let t3 = t2 * t;
    (p1 * 2.0
        + (p2 - p0) * t
        + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2
        + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t3)
        * 0.5
}

impl Road {
    /// Build the road for `spec`. `kerb_plus_shoulder` is not needed here; the
    /// geometry builder adds those bands around `hw`.
    pub fn build(spec: &TrackSpec, bank_gain: f64, max_bank_deg: f64) -> Road {
        let n = spec.control.len();
        assert!(n >= 4, "a track needs at least 4 control points");
        // control points: x, elev, z and width in w
        let pts: Vec<V3> = spec.control.iter().map(|c| v3(c.x, c.elev, c.z)).collect();
        let widths: Vec<f64> = spec.control.iter().map(|c| c.width).collect();
        const K: usize = 48;
        let mut dense: Vec<(V3, f64)> = Vec::with_capacity(n * K + 1);
        for i in 0..n {
            let p0 = pts[(i + n - 1) % n];
            let p1 = pts[i];
            let p2 = pts[(i + 1) % n];
            let p3 = pts[(i + 2) % n];
            let w = |a: f64, b: f64, c: f64, d: f64, t: f64| {
                catmull(
                    v3(a, 0.0, 0.0),
                    v3(b, 0.0, 0.0),
                    v3(c, 0.0, 0.0),
                    v3(d, 0.0, 0.0),
                    t,
                )
                .x
            };
            let (w0, w1, w2, w3) = (
                widths[(i + n - 1) % n],
                widths[i],
                widths[(i + 1) % n],
                widths[(i + 2) % n],
            );
            for k in 0..K {
                let t = k as f64 / K as f64;
                dense.push((catmull(p0, p1, p2, p3, t), w(w0, w1, w2, w3, t)));
            }
        }
        dense.push(dense[0]);
        let mut cum = vec![0.0; dense.len()];
        for i in 1..dense.len() {
            cum[i] = cum[i - 1] + (dense[i].0 - dense[i - 1].0).len();
        }
        let length = cum[dense.len() - 1];
        let count = ((length / SECTION_SPACING_M).round() as usize).max(8);
        let ds = length / count as f64;
        let mut pos = Vec::with_capacity(count);
        let mut wid = Vec::with_capacity(count);
        let mut j = 0usize;
        for k in 0..count {
            let target = k as f64 * ds;
            while j + 2 < dense.len() && cum[j + 1] < target {
                j += 1;
            }
            let seg = (cum[j + 1] - cum[j]).max(1e-9);
            let t = clamp((target - cum[j]) / seg, 0.0, 1.0);
            pos.push(dense[j].0.lerp(dense[j + 1].0, t));
            wid.push(dense[j].1 + (dense[j + 1].1 - dense[j].1) * t);
        }
        // move the start line along the lap for spot variants
        let shift = ((spec.start_fraction.rem_euclid(1.0)) * count as f64) as usize % count;
        pos.rotate_left(shift);
        wid.rotate_left(shift);
        // smooth elevation so crests and dips are rounded
        for _ in 0..6 {
            let prev: Vec<f64> = pos.iter().map(|p| p.y).collect();
            for i in 0..count {
                pos[i].y = 0.25 * prev[(i + count - 1) % count]
                    + 0.5 * prev[i]
                    + 0.25 * prev[(i + 1) % count];
            }
        }
        // raw curvature
        let heading = |i: usize| {
            let a = pos[(i + 1) % count] - pos[(i + count - 1) % count];
            v3(a.x, 0.0, a.z).norm()
        };
        let mut raw = vec![0.0; count];
        for (i, r) in raw.iter_mut().enumerate() {
            let a = heading((i + count - 1) % count);
            let b = heading((i + 1) % count);
            *r = (a.x * b.z - a.z * b.x) / (2.0 * ds);
        }
        let win = 6isize;
        let mut kappa = vec![0.0; count];
        for (i, k) in kappa.iter_mut().enumerate() {
            let mut sum = 0.0;
            for d in -win..=win {
                sum += raw[(i as isize + d).rem_euclid(count as isize) as usize];
            }
            *k = sum / (2 * win + 1) as f64;
        }
        let max_bank = max_bank_deg.to_radians();
        let mut secs = Vec::with_capacity(count);
        for i in 0..count {
            let fwd = (pos[(i + 1) % count] - pos[(i + count - 1) % count]).norm();
            let u0 = (V3::UP - fwd * V3::UP.dot(fwd)).norm();
            let r0 = fwd.cross(u0).norm();
            let bank = clamp(kappa[i] * bank_gain, -max_bank, max_bank);
            let up = (u0 * cos(bank) + r0 * sin(bank)).norm();
            let right = fwd.cross(up).norm();
            secs.push(Section {
                s: i as f64 * ds,
                pos: pos[i],
                fwd,
                up,
                right,
                hw: wid[i] * 0.5,
                kappa: kappa[i],
                bank,
            });
        }
        Road { secs, length, ds }
    }

    pub fn len(&self) -> usize {
        self.secs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.secs.is_empty()
    }

    pub fn at(&self, i: isize) -> &Section {
        &self.secs[i.rem_euclid(self.secs.len() as isize) as usize]
    }

    /// Interpolated frame at distance `s` along the lap (wraps).
    pub fn frame_at(&self, s: f64) -> Section {
        let l = self.length;
        let s = s - (s / l).floor() * l;
        let f = s / self.ds;
        let i = f.floor() as isize;
        let t = f - f.floor();
        let (a, b) = (self.at(i), self.at(i + 1));
        let lerp = |x: f64, y: f64| x + (y - x) * t;
        Section {
            s,
            pos: a.pos.lerp(b.pos, t),
            fwd: a.fwd.lerp(b.fwd, t).norm(),
            up: a.up.lerp(b.up, t).norm(),
            right: a.right.lerp(b.right, t).norm(),
            hw: lerp(a.hw, b.hw),
            kappa: lerp(a.kappa, b.kappa),
            bank: lerp(a.bank, b.bank),
        }
    }

    pub fn stats(&self) -> RoadStats {
        let n = self.len();
        let max_k = self
            .secs
            .iter()
            .map(|s| s.kappa.abs())
            .fold(0.0_f64, f64::max);
        let max_grade = self
            .secs
            .iter()
            .map(|s| s.fwd.y.abs() / s.fwd.hlen().max(1e-9))
            .fold(0.0_f64, f64::max);
        let max_bank = self
            .secs
            .iter()
            .map(|s| s.bank.abs())
            .fold(0.0_f64, f64::max);
        let (lo, hi) = self.secs.iter().fold((f64::MAX, f64::MIN), |(lo, hi), s| {
            (lo.min(s.pos.y), hi.max(s.pos.y))
        });
        let mut clearance = f64::MAX;
        let min_sep = (250.0 / self.ds) as usize;
        for i in (0..n).step_by(2) {
            for j in ((i + 1)..n).step_by(2) {
                let sep = (j - i).min(n - (j - i));
                if sep < min_sep {
                    continue;
                }
                let d = self.secs[i].pos - self.secs[j].pos;
                clearance = clearance.min(d.hlen());
            }
        }
        RoadStats {
            length_m: self.length,
            min_radius_m: if max_k > 1e-9 { 1.0 / max_k } else { f64::MAX },
            max_grade,
            max_bank_deg: max_bank.to_degrees(),
            elevation_range_m: hi - lo,
            min_clearance_m: clearance,
        }
    }
}
