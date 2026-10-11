//! Vehicle dynamics parameters.

use crate::ai::VehicleAi;
use crate::validate::Issues;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A vehicle definition: generic arcade parameters, an external model, or both.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VehicleDef {
    pub id: String,
    pub name: String,
    /// Optional id of the car (appearance) document this vehicle uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub car: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arcade: Option<ArcadeVehicleParams>,
    /// Tuned classic-handling parameters. When present, project drives use the
    /// classic model with them; `arcade` stays the fallback model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classic: Option<ClassicVehicleParams>,
    /// Escape hatch for models supplied by an external extension crate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ExternalModel>,
    /// Limits for computer drivers using this vehicle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<VehicleAi>,
    /// Parameters for a generic synthesised engine voice, used by clients when
    /// no sampled audio is available for this vehicle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_sound: Option<EngineSynthParams>,
}

/// Parameters of a generic additive engine synthesiser. The firing frequency
/// is `rpm / 60 * cylinders / 2` hertz; `harmonics[k]` is the relative level of
/// harmonic `k + 1` of that frequency.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EngineSynthParams {
    pub idle_rpm: f32,
    pub redline_rpm: f32,
    /// Cylinder count (1..=16); sets the firing frequency.
    pub cylinders: u8,
    /// Relative level of harmonics 1.. (1 to 8 entries, each in 0..=1).
    pub harmonics: Vec<f32>,
    /// Low-pass cutoff at idle and at the redline, hertz.
    pub filter_hz_idle: f32,
    pub filter_hz_redline: f32,
    /// Broadband (intake/exhaust) noise level, 0..=1.
    pub noise_gain: f32,
    /// Overall level at idle and at the redline, 0..=1.
    pub gain_idle: f32,
    pub gain_redline: f32,
}

impl EngineSynthParams {
    pub fn validate(&self, path: &str, out: &mut Issues) {
        out.positive(&format!("{path}.idle_rpm"), self.idle_rpm);
        out.positive(&format!("{path}.redline_rpm"), self.redline_rpm);
        if self.redline_rpm <= self.idle_rpm {
            out.add(path, "redline_rpm must exceed idle_rpm");
        }
        if !(1..=16).contains(&self.cylinders) {
            out.add(&format!("{path}.cylinders"), "cylinders must be 1..=16");
        }
        if self.harmonics.is_empty() || self.harmonics.len() > 8 {
            out.add(
                &format!("{path}.harmonics"),
                "1 to 8 harmonics are required",
            );
        }
        for (i, h) in self.harmonics.iter().enumerate() {
            out.range(&format!("{path}.harmonics[{i}]"), *h, 0.0, 1.0);
        }
        out.range(
            &format!("{path}.filter_hz_idle"),
            self.filter_hz_idle,
            40.0,
            20000.0,
        );
        out.range(
            &format!("{path}.filter_hz_redline"),
            self.filter_hz_redline,
            40.0,
            20000.0,
        );
        out.range(&format!("{path}.noise_gain"), self.noise_gain, 0.0, 1.0);
        out.range(&format!("{path}.gain_idle"), self.gain_idle, 0.0, 1.0);
        out.range(&format!("{path}.gain_redline"), self.gain_redline, 0.0, 1.0);
    }
}

/// Parameters of the classic handling model: shift points, pace scales,
/// steering response. Speeds are metres per second.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassicVehicleParams {
    /// Speeds at which the automatic box shifts up from gear 1, 2, ...
    /// (ascending; the car has one more forward gear than entries, 2 to 8).
    pub upshift_mps: Vec<f32>,
    /// Speeds below which the automatic box shifts down from gear 2, 3, ...
    /// (ascending, each below the matching upshift speed).
    pub downshift_mps: Vec<f32>,
    /// Speed at which the top gear stops pulling.
    pub top_speed_mps: f32,
    /// Scale of the full-throttle acceleration (1.0 = reference car).
    pub accel_scale: f32,
    /// Scale of the coasting (engine drag) deceleration.
    pub coast_scale: f32,
    /// Scale of the brake deceleration.
    pub brake_scale: f32,
    /// Drive pause of an automatic shift, seconds.
    pub shift_seconds: f32,
    /// Reverse top speed.
    pub reverse_speed_mps: f32,
    /// Steering gain (about 18 to 20): sets the yaw level above the knee speed.
    pub steer_gain: f32,
    /// Grip (about 2.8 to 3.6): sets the yaw level above the knee speed.
    pub grip: f32,
    /// Full-lock yaw rate below the knee speed, rad/s.
    pub low_speed_yaw_rate: f32,
    /// Speed from which the full-lock yaw rate is boosted.
    pub yaw_boost_speed_mps: f32,
}

fn default_steer_rate() -> f32 {
    3.0
}
fn default_steer_return() -> f32 {
    5.0
}
fn default_lateral_grip() -> f32 {
    4.0
}

/// Opaque parameters for a model identified by `id`; the engine passes `params` through untouched.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExternalModel {
    pub id: String,
    pub params: serde_json::Value,
}

/// Generic arcade vehicle parameters. SI units throughout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArcadeVehicleParams {
    pub mass_kg: f32,
    pub wheelbase_m: f32,
    pub track_width_m: f32,
    pub wheel_radius_m: f32,
    /// Peak engine torque at the crank, newton metres.
    pub peak_torque_nm: f32,
    pub idle_rpm: f32,
    pub redline_rpm: f32,
    /// Overall ratio per forward gear (engine revolutions per driveshaft revolution), before final drive.
    pub gear_ratios: Vec<f32>,
    pub reverse_ratio: f32,
    pub final_drive: f32,
    /// Fraction of drive torque delivered to the wheels, 0..1.
    pub drivetrain_efficiency: f32,
    pub max_brake_force_n: f32,
    /// Aerodynamic drag area `Cd * A`, square metres.
    pub drag_area_m2: f32,
    /// Downforce coefficient area `Cl * A`, square metres.
    #[serde(default)]
    pub downforce_area_m2: f32,
    pub rolling_resistance: f32,
    /// Tyre friction multiplier applied on top of the surface friction.
    pub tyre_grip: f32,
    pub max_steer_angle_rad: f32,
    /// Speed at which steering lock is halved, metres per second.
    pub steer_falloff_speed_mps: f32,
    pub max_speed_mps: f32,
    /// Fraction of drive force on the front axle, 0..1.
    pub front_drive_bias: f32,
    /// Rate at which steering travel moves towards the input, full lock per second.
    #[serde(default = "default_steer_rate")]
    pub steer_rate_per_s: f32,
    /// Rate at which steering travel returns to centre after release, full lock per second.
    #[serde(default = "default_steer_return")]
    pub steer_return_per_s: f32,
    /// Rate at which the tyres remove sideways velocity, 1/s. Lower values let
    /// the body point further past the direction of travel while steering.
    #[serde(default = "default_lateral_grip")]
    pub lateral_grip_per_s: f32,
}

impl VehicleDef {
    pub fn validate(&self, path: &str, out: &mut Issues) {
        if self.id.is_empty() {
            out.add(&format!("{path}.id"), "id must not be empty");
        }
        if self.arcade.is_none() && self.classic.is_none() && self.model.is_none() {
            out.add(
                path,
                "vehicle needs `arcade` or `classic` parameters, an external `model`, or both",
            );
        }
        if let Some(m) = &self.model {
            if m.id.is_empty() {
                out.add(&format!("{path}.model.id"), "model id must not be empty");
            }
        }
        if let Some(a) = &self.arcade {
            a.validate(&format!("{path}.arcade"), out);
        }
        if let Some(c) = &self.classic {
            c.validate(&format!("{path}.classic"), out);
        }
        if let Some(a) = &self.ai {
            a.validate(&format!("{path}.ai"), out);
        }
        if let Some(e) = &self.engine_sound {
            e.validate(&format!("{path}.engine_sound"), out);
        }
    }
}

impl ClassicVehicleParams {
    pub fn validate(&self, path: &str, out: &mut Issues) {
        let n = self.upshift_mps.len();
        if !(1..=7).contains(&n) || self.downshift_mps.len() != n {
            out.add(
                path,
                "upshift_mps needs 1 to 7 entries and downshift_mps the same number",
            );
            return;
        }
        for (i, v) in self.upshift_mps.iter().enumerate() {
            out.positive(&format!("{path}.upshift_mps[{i}]"), *v);
        }
        for (i, v) in self.downshift_mps.iter().enumerate() {
            out.positive(&format!("{path}.downshift_mps[{i}]"), *v);
        }
        if self.upshift_mps.windows(2).any(|w| w[1] <= w[0])
            || self.downshift_mps.windows(2).any(|w| w[1] <= w[0])
        {
            out.add(path, "shift speeds must ascend");
        }
        if self
            .upshift_mps
            .iter()
            .zip(&self.downshift_mps)
            .any(|(up, down)| down >= up)
        {
            out.add(path, "each downshift speed must be below its upshift speed");
        }
        let last = self.upshift_mps[n - 1];
        out.range(
            &format!("{path}.top_speed_mps"),
            self.top_speed_mps,
            last * 1.05,
            200.0,
        );
        out.range(&format!("{path}.accel_scale"), self.accel_scale, 0.3, 3.0);
        out.range(&format!("{path}.coast_scale"), self.coast_scale, 0.3, 3.0);
        out.range(&format!("{path}.brake_scale"), self.brake_scale, 0.3, 3.0);
        out.range(
            &format!("{path}.shift_seconds"),
            self.shift_seconds,
            0.02,
            0.6,
        );
        out.range(
            &format!("{path}.reverse_speed_mps"),
            self.reverse_speed_mps,
            5.0,
            60.0,
        );
        out.range(&format!("{path}.steer_gain"), self.steer_gain, 2.0, 60.0);
        out.range(&format!("{path}.grip"), self.grip, 0.5, 10.0);
        out.range(
            &format!("{path}.low_speed_yaw_rate"),
            self.low_speed_yaw_rate,
            0.2,
            4.0,
        );
        out.range(
            &format!("{path}.yaw_boost_speed_mps"),
            self.yaw_boost_speed_mps,
            20.5,
            self.top_speed_mps,
        );
    }
}

impl ArcadeVehicleParams {
    pub fn validate(&self, path: &str, out: &mut Issues) {
        let pos = [
            ("mass_kg", self.mass_kg),
            ("wheelbase_m", self.wheelbase_m),
            ("track_width_m", self.track_width_m),
            ("wheel_radius_m", self.wheel_radius_m),
            ("peak_torque_nm", self.peak_torque_nm),
            ("idle_rpm", self.idle_rpm),
            ("redline_rpm", self.redline_rpm),
            ("final_drive", self.final_drive),
            ("max_brake_force_n", self.max_brake_force_n),
            ("tyre_grip", self.tyre_grip),
            ("max_steer_angle_rad", self.max_steer_angle_rad),
            ("steer_falloff_speed_mps", self.steer_falloff_speed_mps),
            ("max_speed_mps", self.max_speed_mps),
            ("steer_rate_per_s", self.steer_rate_per_s),
            ("steer_return_per_s", self.steer_return_per_s),
            ("lateral_grip_per_s", self.lateral_grip_per_s),
        ];
        for (n, v) in pos {
            out.positive(&format!("{path}.{n}"), v);
        }
        if self.redline_rpm <= self.idle_rpm {
            out.add(path, "redline_rpm must exceed idle_rpm");
        }
        if self.gear_ratios.is_empty() {
            out.add(
                &format!("{path}.gear_ratios"),
                "at least one forward gear is required",
            );
        }
        for (i, g) in self.gear_ratios.iter().enumerate() {
            out.positive(&format!("{path}.gear_ratios[{i}]"), *g);
        }
        if self.gear_ratios.windows(2).any(|w| w[1] > w[0]) {
            out.add(
                &format!("{path}.gear_ratios"),
                "ratios must be non-increasing",
            );
        }
        out.positive(&format!("{path}.reverse_ratio"), self.reverse_ratio.abs());
        out.range(
            &format!("{path}.drivetrain_efficiency"),
            self.drivetrain_efficiency,
            0.01,
            1.0,
        );
        out.range(
            &format!("{path}.front_drive_bias"),
            self.front_drive_bias,
            0.0,
            1.0,
        );
        out.range(
            &format!("{path}.drag_area_m2"),
            self.drag_area_m2,
            0.0,
            20.0,
        );
        out.range(
            &format!("{path}.downforce_area_m2"),
            self.downforce_area_m2,
            0.0,
            20.0,
        );
        out.range(
            &format!("{path}.rolling_resistance"),
            self.rolling_resistance,
            0.0,
            1.0,
        );
        out.range(
            &format!("{path}.steer_rate_per_s"),
            self.steer_rate_per_s,
            0.1,
            50.0,
        );
        out.range(
            &format!("{path}.steer_return_per_s"),
            self.steer_return_per_s,
            0.1,
            50.0,
        );
        out.range(
            &format!("{path}.lateral_grip_per_s"),
            self.lateral_grip_per_s,
            1.0,
            40.0,
        );
        out.range(
            &format!("{path}.max_steer_angle_rad"),
            self.max_steer_angle_rad,
            0.01,
            1.2,
        );
        // The top gear must be able to reach the declared top speed before the
        // rev limiter, otherwise the declared top speed is unreachable.
        if let Some(top) = self.gear_ratios.last() {
            let wheel_rpm_at_limit =
                self.redline_rpm / (top * self.final_drive).max(f32::MIN_POSITIVE);
            let limit_speed =
                wheel_rpm_at_limit * std::f32::consts::TAU / 60.0 * self.wheel_radius_m;
            if limit_speed < self.max_speed_mps {
                out.add(
                    &format!("{path}.max_speed_mps"),
                    format!("top gear reaches only {limit_speed:.1} m/s at the redline"),
                );
            }
        }
    }
}
