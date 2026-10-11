//! Parameter sets of the four demo cars and the procedure that turns one into
//! faces. Every number here is invented for this project; names are made up.

use crate::geom::{Face, P, Role, boxp, cap, face, face_away, lerp, loft, q};

/// One lower-body cross-section.
#[derive(Clone, Copy, Debug)]
pub struct Station {
    /// Position along the car, forward positive.
    pub u: f64,
    /// Underside height.
    pub y_bottom: f64,
    /// Height of the hood / deck line.
    pub y_top: f64,
    /// Half width of the underside (sill).
    pub w_bottom: f64,
    /// Half width at the shoulder, the widest point.
    pub w_shoulder: f64,
    /// Half width of the hood / deck.
    pub w_top: f64,
}

const fn st(u: f64, yb: f64, yt: f64, wb: f64, ws: f64, wt: f64) -> Station {
    Station {
        u,
        y_bottom: yb,
        y_top: yt,
        w_bottom: wb,
        w_shoulder: ws,
        w_top: wt,
    }
}

/// One cabin cross-section; the first and last must sit on the body (roof at
/// the body line) so the cabin closes without caps.
#[derive(Clone, Copy, Debug)]
pub struct CabinStation {
    pub u: f64,
    /// Roof height; `None` means "on the body surface".
    pub roof: Option<f64>,
}

/// Materials of the faces between a cabin station and the next one.
#[derive(Clone, Copy, Debug)]
pub struct CabinSegment {
    pub top: Role,
    pub side: Role,
}

#[derive(Clone, Copy, Debug)]
pub struct Spoiler {
    pub u: f64,
    pub base_h: f64,
    pub wing_h: f64,
    pub half_span: f64,
    pub chord: f64,
    pub role: Role,
}

#[derive(Clone, Copy, Debug)]
pub struct WheelSpec {
    pub radius: f64,
    pub width: f64,
    pub track: f64,
    pub segments: usize,
    pub front_driven: bool,
    pub rear_driven: bool,
}

#[derive(Clone, Debug)]
pub struct Arcade {
    pub mass_kg: f32,
    pub peak_torque_nm: f32,
    pub idle_rpm: f32,
    pub redline_rpm: f32,
    pub gear_ratios: &'static [f32],
    pub reverse_ratio: f32,
    pub final_drive: f32,
    pub max_brake_force_n: f32,
    pub drag_area_m2: f32,
    pub downforce_area_m2: f32,
    pub rolling_resistance: f32,
    pub tyre_grip: f32,
    pub max_steer_angle_rad: f32,
    pub steer_falloff_speed_mps: f32,
    pub max_speed_mps: f32,
    pub front_drive_bias: f32,
    pub steer_rate_per_s: f32,
    pub steer_return_per_s: f32,
    pub lateral_grip_per_s: f32,
    pub sound: EngineSound,
}

/// Tuned classic-handling parameters of a design (see `ClassicVehicleParams`).
#[derive(Clone, Debug)]
pub struct Classic {
    pub upshift_mps: &'static [f32],
    pub downshift_mps: &'static [f32],
    pub top_speed_mps: f32,
    pub accel_scale: f32,
    pub coast_scale: f32,
    pub brake_scale: f32,
    pub shift_seconds: f32,
    pub reverse_speed_mps: f32,
    pub steer_gain: f32,
    pub grip: f32,
    pub low_speed_yaw_rate: f32,
    pub yaw_boost_speed_mps: f32,
}

/// Generic engine voice of a design (see `EngineSynthParams`).
#[derive(Clone, Debug)]
pub struct EngineSound {
    pub cylinders: u8,
    pub harmonics: &'static [f32],
    pub filter_hz_idle: f32,
    pub filter_hz_redline: f32,
    pub noise_gain: f32,
    pub gain_idle: f32,
    pub gain_redline: f32,
}

#[derive(Clone, Debug)]
pub struct Design {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub wheelbase: f64,
    pub stations: &'static [Station],
    pub cabin: &'static [CabinStation],
    pub cabin_segments: &'static [CabinSegment],
    /// Cabin top half width as a fraction of its base half width.
    pub cabin_taper: f64,
    pub spoiler: Option<Spoiler>,
    pub wheel: WheelSpec,
    /// Palette index of the default paint scheme (see `palette::PAINTS`).
    pub default_paint: usize,
    pub arcade: Arcade,
    pub classic: Classic,
}

const G: CabinSegment = CabinSegment {
    top: Role::Glass,
    side: Role::Glass,
};
const ROOF: CabinSegment = CabinSegment {
    top: Role::Paint,
    side: Role::Glass,
};
const ROOF_ACCENT: CabinSegment = CabinSegment {
    top: Role::Accent,
    side: Role::Glass,
};
const COCKPIT: CabinSegment = CabinSegment {
    top: Role::Trim,
    side: Role::Trim,
};
const HUMP: CabinSegment = CabinSegment {
    top: Role::Paint,
    side: Role::Paint,
};

const COUPE: &[Station] = &[
    st(2.15, 0.34, 0.52, 0.52, 0.60, 0.50),
    st(2.08, 0.22, 0.66, 0.66, 0.78, 0.66),
    st(1.70, 0.18, 0.80, 0.74, 0.82, 0.76),
    st(1.10, 0.18, 0.90, 0.76, 0.84, 0.80),
    st(0.30, 0.18, 0.94, 0.78, 0.86, 0.82),
    st(-0.60, 0.18, 0.96, 0.78, 0.86, 0.82),
    st(-1.30, 0.18, 0.98, 0.76, 0.85, 0.80),
    st(-1.95, 0.20, 0.94, 0.72, 0.80, 0.72),
    st(-2.15, 0.30, 0.82, 0.60, 0.68, 0.58),
];
const COUPE_CABIN: &[CabinStation] = &[
    CabinStation {
        u: 0.95,
        roof: None,
    },
    CabinStation {
        u: 0.35,
        roof: Some(1.34),
    },
    CabinStation {
        u: -0.95,
        roof: Some(1.32),
    },
    CabinStation {
        u: -1.55,
        roof: None,
    },
];

const ROADSTER: &[Station] = &[
    st(1.95, 0.32, 0.50, 0.50, 0.58, 0.48),
    st(1.88, 0.22, 0.62, 0.64, 0.74, 0.62),
    st(1.50, 0.17, 0.72, 0.70, 0.80, 0.72),
    st(0.90, 0.17, 0.78, 0.72, 0.82, 0.76),
    st(0.20, 0.17, 0.84, 0.74, 0.84, 0.80),
    st(-0.50, 0.17, 0.86, 0.74, 0.84, 0.80),
    st(-1.30, 0.17, 0.88, 0.72, 0.82, 0.76),
    st(-1.80, 0.20, 0.86, 0.66, 0.74, 0.66),
    st(-1.95, 0.30, 0.76, 0.54, 0.62, 0.50),
];
const ROADSTER_CABIN: &[CabinStation] = &[
    CabinStation {
        u: 0.60,
        roof: None,
    },
    CabinStation {
        u: 0.30,
        roof: Some(1.10),
    },
    CabinStation {
        u: -0.30,
        roof: Some(1.02),
    },
    CabinStation {
        u: -0.95,
        roof: None,
    },
];

const RALLY: &[Station] = &[
    st(1.98, 0.38, 0.62, 0.52, 0.62, 0.52),
    st(1.90, 0.26, 0.82, 0.68, 0.80, 0.70),
    st(1.50, 0.22, 0.90, 0.74, 0.84, 0.78),
    st(0.90, 0.22, 0.95, 0.76, 0.86, 0.80),
    st(0.10, 0.22, 0.98, 0.78, 0.86, 0.82),
    st(-0.90, 0.22, 1.00, 0.78, 0.86, 0.82),
    st(-1.60, 0.22, 1.02, 0.76, 0.85, 0.80),
    st(-1.92, 0.30, 0.98, 0.68, 0.80, 0.74),
    st(-1.98, 0.40, 0.90, 0.60, 0.70, 0.62),
];
const RALLY_CABIN: &[CabinStation] = &[
    CabinStation {
        u: 0.80,
        roof: None,
    },
    CabinStation {
        u: 0.20,
        roof: Some(1.50),
    },
    CabinStation {
        u: -1.55,
        roof: Some(1.48),
    },
    CabinStation {
        u: -1.82,
        roof: None,
    },
];

const GT: &[Station] = &[
    st(2.25, 0.30, 0.42, 0.56, 0.64, 0.52),
    st(2.18, 0.16, 0.54, 0.76, 0.88, 0.74),
    st(1.70, 0.12, 0.66, 0.84, 0.92, 0.82),
    st(0.90, 0.12, 0.76, 0.86, 0.94, 0.86),
    st(0.00, 0.12, 0.86, 0.86, 0.94, 0.86),
    st(-1.00, 0.12, 0.96, 0.86, 0.94, 0.86),
    st(-1.80, 0.12, 1.02, 0.84, 0.92, 0.82),
    st(-2.15, 0.16, 1.00, 0.76, 0.84, 0.72),
    st(-2.25, 0.28, 0.92, 0.62, 0.70, 0.58),
];
const GT_CABIN: &[CabinStation] = &[
    CabinStation {
        u: 1.15,
        roof: None,
    },
    CabinStation {
        u: 0.55,
        roof: Some(1.22),
    },
    CabinStation {
        u: -0.60,
        roof: Some(1.19),
    },
    CabinStation {
        u: -1.90,
        roof: None,
    },
];

pub const DESIGNS: &[Design] = &[
    Design {
        id: "quill-coupe",
        name: "Quill Coupe",
        description: "Compact rear-drive coupe with a low roof and a lip spoiler.",
        wheelbase: 2.5,
        stations: COUPE,
        cabin: COUPE_CABIN,
        cabin_segments: &[G, ROOF, G],
        cabin_taper: 0.80,
        spoiler: Some(Spoiler {
            u: -2.0,
            base_h: 0.98,
            wing_h: 1.05,
            half_span: 0.62,
            chord: 0.16,
            role: Role::Accent,
        }),
        wheel: WheelSpec {
            radius: 0.31,
            width: 0.23,
            track: 1.56,
            segments: 12,
            front_driven: false,
            rear_driven: true,
        },
        default_paint: 0,
        arcade: Arcade {
            mass_kg: 1180.0,
            peak_torque_nm: 340.0,
            idle_rpm: 900.0,
            redline_rpm: 7200.0,
            gear_ratios: &[3.4, 2.2, 1.55, 1.15, 0.9],
            reverse_ratio: 3.2,
            final_drive: 3.7,
            max_brake_force_n: 11000.0,
            drag_area_m2: 0.62,
            downforce_area_m2: 0.1,
            rolling_resistance: 0.012,
            tyre_grip: 1.05,
            max_steer_angle_rad: 0.55,
            steer_falloff_speed_mps: 42.0,
            max_speed_mps: 62.0,
            front_drive_bias: 0.0,
            steer_rate_per_s: 5.0,
            steer_return_per_s: 7.0,
            lateral_grip_per_s: 7.0,
            sound: EngineSound {
                cylinders: 6,
                harmonics: &[1.0, 0.55, 0.35, 0.2],
                filter_hz_idle: 500.0,
                filter_hz_redline: 3400.0,
                noise_gain: 0.08,
                gain_idle: 0.30,
                gain_redline: 0.60,
            },
        },
        classic: Classic {
            upshift_mps: &[27.448_701, 39.921_32, 52.304_53, 62.318_387],
            downshift_mps: &[22.754_705, 33.126_2, 44.525_906, 53.198_623],
            top_speed_mps: 80.0,
            accel_scale: 1.0,
            coast_scale: 1.0,
            brake_scale: 1.0,
            shift_seconds: 0.2755,
            reverse_speed_mps: 29.4,
            steer_gain: 18.3,
            grip: 3.3,
            low_speed_yaw_rate: 1.7313,
            yaw_boost_speed_mps: 68.64,
        },
    },
    Design {
        id: "brisk-roadster",
        name: "Brisk Roadster",
        description: "Light open two-seater with a dark cockpit and a rear hump.",
        wheelbase: 2.3,
        stations: ROADSTER,
        cabin: ROADSTER_CABIN,
        cabin_segments: &[G, COCKPIT, HUMP],
        cabin_taper: 0.84,
        spoiler: None,
        wheel: WheelSpec {
            radius: 0.30,
            width: 0.22,
            track: 1.50,
            segments: 12,
            front_driven: false,
            rear_driven: true,
        },
        default_paint: 3,
        arcade: Arcade {
            mass_kg: 980.0,
            peak_torque_nm: 260.0,
            idle_rpm: 1000.0,
            redline_rpm: 7600.0,
            gear_ratios: &[3.5, 2.3, 1.65, 1.2, 0.95],
            reverse_ratio: 3.3,
            final_drive: 3.9,
            max_brake_force_n: 9000.0,
            drag_area_m2: 0.58,
            downforce_area_m2: 0.0,
            rolling_resistance: 0.012,
            tyre_grip: 1.0,
            max_steer_angle_rad: 0.58,
            steer_falloff_speed_mps: 40.0,
            max_speed_mps: 56.0,
            front_drive_bias: 0.0,
            steer_rate_per_s: 5.4,
            steer_return_per_s: 7.5,
            lateral_grip_per_s: 7.5,
            sound: EngineSound {
                cylinders: 4,
                harmonics: &[1.0, 0.6, 0.3, 0.25],
                filter_hz_idle: 650.0,
                filter_hz_redline: 3800.0,
                noise_gain: 0.07,
                gain_idle: 0.28,
                gain_redline: 0.55,
            },
        },
        classic: Classic {
            upshift_mps: &[25.8, 36.9, 48.4, 59.8],
            downshift_mps: &[21.4, 30.6, 41.1, 50.9],
            top_speed_mps: 84.0,
            accel_scale: 1.04,
            coast_scale: 0.95,
            brake_scale: 1.0,
            shift_seconds: 0.255,
            reverse_speed_mps: 27.0,
            steer_gain: 19.0,
            grip: 3.1,
            low_speed_yaw_rate: 1.807,
            yaw_boost_speed_mps: 66.0,
        },
    },
    Design {
        id: "tumble-rally",
        name: "Tumble Rally",
        description: "Tall short-tailed hatchback on big wheels with a roof wing, all-wheel drive.",
        wheelbase: 2.45,
        stations: RALLY,
        cabin: RALLY_CABIN,
        cabin_segments: &[G, ROOF_ACCENT, G],
        cabin_taper: 0.86,
        spoiler: Some(Spoiler {
            u: -1.86,
            base_h: 1.48,
            wing_h: 1.62,
            half_span: 0.70,
            chord: 0.22,
            role: Role::Accent,
        }),
        wheel: WheelSpec {
            radius: 0.34,
            width: 0.24,
            track: 1.52,
            segments: 12,
            front_driven: true,
            rear_driven: true,
        },
        default_paint: 1,
        arcade: Arcade {
            mass_kg: 1150.0,
            peak_torque_nm: 320.0,
            idle_rpm: 1000.0,
            redline_rpm: 7000.0,
            gear_ratios: &[3.6, 2.4, 1.7, 1.25, 0.95],
            reverse_ratio: 3.4,
            final_drive: 4.1,
            max_brake_force_n: 10500.0,
            drag_area_m2: 0.75,
            downforce_area_m2: 0.15,
            rolling_resistance: 0.02,
            tyre_grip: 1.15,
            max_steer_angle_rad: 0.62,
            steer_falloff_speed_mps: 36.0,
            max_speed_mps: 54.0,
            front_drive_bias: 0.5,
            steer_rate_per_s: 5.2,
            steer_return_per_s: 7.0,
            lateral_grip_per_s: 6.4,
            sound: EngineSound {
                cylinders: 4,
                harmonics: &[1.0, 0.7, 0.5, 0.3],
                filter_hz_idle: 550.0,
                filter_hz_redline: 3000.0,
                noise_gain: 0.14,
                gain_idle: 0.32,
                gain_redline: 0.58,
            },
        },
        classic: Classic {
            upshift_mps: &[24.0, 34.5, 45.5, 56.0],
            downshift_mps: &[19.9, 28.6, 38.7, 47.6],
            top_speed_mps: 79.2,
            accel_scale: 1.08,
            coast_scale: 1.05,
            brake_scale: 1.05,
            shift_seconds: 0.3,
            reverse_speed_mps: 24.0,
            steer_gain: 19.5,
            grip: 3.6,
            low_speed_yaw_rate: 1.854,
            yaw_boost_speed_mps: 58.0,
        },
    },
    Design {
        id: "shard-gt",
        name: "Shard GT",
        description: "Long low wedge-shaped grand tourer with a fastback and a tall rear wing.",
        wheelbase: 2.7,
        stations: GT,
        cabin: GT_CABIN,
        cabin_segments: &[G, ROOF, G],
        cabin_taper: 0.78,
        spoiler: Some(Spoiler {
            u: -2.05,
            base_h: 1.02,
            wing_h: 1.30,
            half_span: 0.78,
            chord: 0.26,
            role: Role::Accent,
        }),
        wheel: WheelSpec {
            radius: 0.32,
            width: 0.26,
            track: 1.72,
            segments: 12,
            front_driven: false,
            rear_driven: true,
        },
        default_paint: 6,
        arcade: Arcade {
            mass_kg: 1350.0,
            peak_torque_nm: 520.0,
            idle_rpm: 900.0,
            redline_rpm: 7800.0,
            gear_ratios: &[3.2, 2.1, 1.5, 1.12, 0.88, 0.72],
            reverse_ratio: 3.0,
            final_drive: 3.4,
            max_brake_force_n: 14000.0,
            drag_area_m2: 0.66,
            downforce_area_m2: 0.45,
            rolling_resistance: 0.011,
            tyre_grip: 1.2,
            max_steer_angle_rad: 0.5,
            steer_falloff_speed_mps: 45.0,
            max_speed_mps: 78.0,
            front_drive_bias: 0.0,
            steer_rate_per_s: 4.8,
            steer_return_per_s: 6.5,
            lateral_grip_per_s: 7.6,
            sound: EngineSound {
                cylinders: 8,
                harmonics: &[1.0, 0.5, 0.4, 0.3, 0.15],
                filter_hz_idle: 450.0,
                filter_hz_redline: 3000.0,
                noise_gain: 0.09,
                gain_idle: 0.34,
                gain_redline: 0.65,
            },
        },
        classic: Classic {
            upshift_mps: &[22.0, 30.8, 41.3, 52.3, 64.2],
            downshift_mps: &[18.3, 25.6, 34.5, 44.1, 54.2],
            top_speed_mps: 82.0,
            accel_scale: 0.52,
            coast_scale: 1.0,
            brake_scale: 1.0,
            shift_seconds: 0.2755,
            reverse_speed_mps: 24.8,
            steer_gain: 18.0,
            grip: 3.0,
            low_speed_yaw_rate: 1.7114,
            yaw_boost_speed_mps: 58.0,
        },
    },
];

impl Design {
    pub fn length(&self) -> f64 {
        self.stations[0].u - self.stations[self.stations.len() - 1].u
    }

    pub fn max_half_width(&self) -> f64 {
        let body = self
            .stations
            .iter()
            .map(|s| s.w_shoulder)
            .fold(0.0, f64::max);
        body.max(self.wheel.track * 0.5 + self.wheel.width * 0.5)
    }

    pub fn roof_height(&self) -> f64 {
        let cabin = self.cabin.iter().filter_map(|c| c.roof).fold(0.0, f64::max);
        let body = self.stations.iter().map(|s| s.y_top).fold(0.0, f64::max);
        let wing = self.spoiler.map_or(0.0, |s| s.wing_h + 0.03);
        cabin.max(body).max(wing)
    }

    /// Body station values interpolated at `u` (clamped to the ends).
    fn body_at(&self, u: f64) -> (f64, f64) {
        let s = self.stations;
        // Stations run from the nose (largest u) to the tail.
        if u >= s[0].u {
            return (s[0].y_top, s[0].w_top);
        }
        for pair in s.windows(2) {
            if u <= pair[0].u && u >= pair[1].u {
                let t = (pair[0].u - u) / (pair[0].u - pair[1].u);
                return (
                    lerp(pair[0].y_top, pair[1].y_top, t),
                    lerp(pair[0].w_top, pair[1].w_top, t),
                );
            }
        }
        let last = s[s.len() - 1];
        (last.y_top, last.w_top)
    }

    /// Wheel hub centres in the car frame, in the order fl, fr, rl, rr.
    pub fn wheel_centres(&self) -> [(&'static str, P); 4] {
        let f = self.wheelbase * 0.5;
        let w = self.wheel.track * 0.5;
        let r = self.wheel.radius;
        [
            ("wheel_fl", [f, -w, r]),
            ("wheel_fr", [f, w, r]),
            ("wheel_rl", [-f, -w, r]),
            ("wheel_rr", [-f, w, r]),
        ]
    }

    /// Body faces without wheels.
    pub fn body_faces(&self) -> Vec<Face> {
        let mut out = Vec::new();
        let ring = |s: &Station| -> Vec<P> {
            let ym = lerp(s.y_bottom, s.y_top, 0.42);
            vec![
                [s.u, -s.w_bottom, s.y_bottom],
                [s.u, -s.w_shoulder, ym],
                [s.u, -s.w_top, s.y_top],
                [s.u, s.w_top, s.y_top],
                [s.u, s.w_shoulder, ym],
                [s.u, s.w_bottom, s.y_bottom],
            ]
        };
        let rings: Vec<Vec<P>> = self.stations.iter().map(ring).collect();
        // Edges: sill, lower side, hood, lower side, sill, underside.
        let body_role = |_s: usize, e: usize| match e {
            0 | 4 | 5 => Role::Trim,
            _ => Role::Paint,
        };
        loft(&rings, true, &body_role, &mut out);
        cap(&rings[0], Role::Paint, [1.0, 0.0, 0.0], &mut out);
        cap(
            &rings[rings.len() - 1],
            Role::Paint,
            [-1.0, 0.0, 0.0],
            &mut out,
        );

        // Cabin.
        let cabin_rings: Vec<Vec<P>> = self
            .cabin
            .iter()
            .map(|c| {
                let (y_base, w_base) = self.body_at(c.u);
                let wc = w_base * 0.96;
                let y0 = y_base - 0.02;
                let (yr, wr) = match c.roof {
                    Some(h) => (h, wc * self.cabin_taper),
                    None => (y0, wc),
                };
                vec![[c.u, -wc, y0], [c.u, -wr, yr], [c.u, wr, yr], [c.u, wc, y0]]
            })
            .collect();
        let segs = self.cabin_segments;
        let cabin_role = |s: usize, e: usize| match e {
            1 => segs[s].top,
            _ => segs[s].side,
        };
        loft(&cabin_rings, false, &cabin_role, &mut out);

        // Bumpers, lamps, grille and tail panel.
        let nose = self.stations[1];
        let tail = self.stations[self.stations.len() - 2];
        boxp(
            [nose.u + 0.01, 0.0, nose.y_bottom + 0.10],
            [0.045, nose.w_shoulder * 0.8, 0.07],
            Role::Trim,
            &mut out,
        );
        boxp(
            [tail.u - 0.01, 0.0, tail.y_bottom + 0.10],
            [0.045, tail.w_shoulder * 0.8, 0.07],
            Role::Trim,
            &mut out,
        );
        for side in [-1.0, 1.0] {
            boxp(
                [
                    nose.u + 0.01,
                    side * nose.w_shoulder * 0.62,
                    lerp(nose.y_bottom, nose.y_top, 0.68),
                ],
                [0.035, 0.15, 0.055],
                Role::LampFront,
                &mut out,
            );
            boxp(
                [
                    tail.u - 0.01,
                    side * tail.w_shoulder * 0.66,
                    lerp(tail.y_bottom, tail.y_top, 0.68),
                ],
                [0.035, 0.17, 0.05],
                Role::LampRear,
                &mut out,
            );
        }

        if let Some(sp) = self.spoiler {
            for side in [-1.0, 1.0] {
                boxp(
                    [
                        sp.u,
                        side * sp.half_span * 0.7,
                        0.5 * (sp.base_h + sp.wing_h),
                    ],
                    [0.02, 0.025, 0.5 * (sp.wing_h - sp.base_h)],
                    Role::Trim,
                    &mut out,
                );
            }
            boxp(
                [sp.u, 0.0, sp.wing_h + 0.01],
                [sp.chord * 0.5, sp.half_span, 0.02],
                sp.role,
                &mut out,
            );
        }
        for f in &mut out {
            for p in &mut f.pts {
                *p = [q(p[0]), q(p[1]), q(p[2])];
            }
        }
        out
    }

    /// Wheel faces centred on the origin, rotation axis along `w`, outer face
    /// towards `+w`.
    pub fn wheel_faces(&self) -> Vec<Face> {
        let r = self.wheel.radius;
        let hw = self.wheel.width * 0.5;
        let n = self.wheel.segments;
        let rim_r = r * 0.62;
        let hub_r = r * 0.2;
        let at = |radius: f64, i: usize, w: f64| -> P {
            let a = std::f64::consts::TAU * (i % n) as f64 / n as f64;
            [radius * a.cos(), w, radius * a.sin()]
        };
        let mut out = Vec::new();
        for i in 0..n {
            // Tread.
            if let Some(f) = face_away(
                vec![
                    at(r, i, -hw),
                    at(r, i, hw),
                    at(r, i + 1, hw),
                    at(r, i + 1, -hw),
                ],
                Role::Tyre,
                [0.0; 3],
            ) {
                out.push(f);
            }
            // Outer sidewall ring.
            if let Some(f) = face(
                vec![
                    at(r, i, hw),
                    at(r, i + 1, hw),
                    at(rim_r, i + 1, hw),
                    at(rim_r, i, hw),
                ],
                Role::Tyre,
                [0.0, 1.0, 0.0],
            ) {
                out.push(f);
            }
            // Rim disc: alternating wedges show rotation.
            let role = if i % 2 == 0 { Role::Rim } else { Role::Trim };
            if let Some(f) = face(
                vec![
                    [0.0, hw - 0.012, 0.0],
                    at(rim_r, i, hw - 0.012),
                    at(rim_r, i + 1, hw - 0.012),
                ],
                role,
                [0.0, 1.0, 0.0],
            ) {
                out.push(f);
            }
            // Hub cap.
            if let Some(f) = face(
                vec![
                    [0.0, hw + 0.01, 0.0],
                    at(hub_r, i, hw + 0.01),
                    at(hub_r, i + 1, hw + 0.01),
                ],
                Role::Rim,
                [0.0, 1.0, 0.0],
            ) {
                out.push(f);
            }
            // Inner face.
            if let Some(f) = face(
                vec![[0.0, -hw, 0.0], at(r, i, -hw), at(r, i + 1, -hw)],
                Role::Tyre,
                [0.0, -1.0, 0.0],
            ) {
                out.push(f);
            }
        }
        for f in &mut out {
            for p in &mut f.pts {
                *p = [q(p[0]), q(p[1]), q(p[2])];
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn designs_are_distinct_and_sane() {
        let mut ids = std::collections::BTreeSet::new();
        for d in DESIGNS {
            assert!(ids.insert(d.id));
            assert!(d.length() > 3.5 && d.length() < 5.0, "{}", d.id);
            assert!(d.roof_height() > 1.0 && d.roof_height() < 1.8, "{}", d.id);
            assert!(d.cabin_segments.len() + 1 == d.cabin.len());
            assert!(d.stations.windows(2).all(|p| p[0].u > p[1].u));
            assert!(d.cabin.windows(2).all(|p| p[0].u > p[1].u));
            assert!(d.cabin.first().unwrap().roof.is_none());
            assert!(d.cabin.last().unwrap().roof.is_none());
        }
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn body_has_faces_in_every_role_used() {
        for d in DESIGNS {
            let faces = d.body_faces();
            assert!(faces.len() > 80, "{}", d.id);
            for role in [
                Role::Paint,
                Role::Glass,
                Role::Trim,
                Role::LampFront,
                Role::LampRear,
            ] {
                assert!(faces.iter().any(|f| f.role == role), "{} {:?}", d.id, role);
            }
        }
    }
}
