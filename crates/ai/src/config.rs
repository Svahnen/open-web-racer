//! Driver configuration: a pack `AiSet` plus per-car limits, with neutral
//! project-owned defaults so a pack without any AI data still races.

use racer_pack_schema::ai::{
    Aggression, AiSet, CatchUp, GLUE_CURVE_LEN, GainSet, Gains, LaneWeights, Misalignment,
    Personality, VehicleAi,
};

/// Skill level of the computer drivers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Skill {
    Beginner,
    #[default]
    Advanced,
}

impl Skill {
    pub fn index(self) -> usize {
        match self {
            Self::Beginner => 0,
            Self::Advanced => 1,
        }
    }

    /// 0 = beginner, anything else advanced.
    pub fn from_level(level: u8) -> Self {
        if level == 0 {
            Self::Beginner
        } else {
            Self::Advanced
        }
    }
}

/// Everything the drivers read besides the track.
#[derive(Clone, Debug, PartialEq)]
pub struct AiConfig {
    pub set: AiSet,
    pub vehicle: VehicleAi,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            set: default_ai_set(),
            vehicle: default_vehicle_ai(),
        }
    }
}

impl AiConfig {
    /// Acceleration table in 1/256 m/s^2 per whole m/s of speed (112 entries).
    pub fn accel_table(&self) -> Vec<i16> {
        accel_table_for(&self.vehicle)
    }
}

/// Speed (m/s) at which the generic acceleration curve reaches zero.
const GENERIC_ACCEL_RANGE_MPS: f64 = 39.6;

/// Acceleration table of one car's limits (see [`AiConfig::accel_table`]).
pub fn accel_table_for(vehicle: &VehicleAi) -> Vec<i16> {
    (0..112)
        .map(|i| {
            let a = match vehicle.acceleration_mps2.len() {
                // A generic road car without a measured table.
                0 => (9.0 * (1.0 - f64::from(i) / GENERIC_ACCEL_RANGE_MPS)).max(1.5),
                n => f64::from(vehicle.acceleration_mps2[(i as usize).min(n - 1)]),
            };
            (a * 256.0).round().clamp(-32768.0, 32767.0) as i16
        })
        .collect()
}

/// Project-owned limits for a generic road car.
pub fn default_vehicle_ai() -> VehicleAi {
    VehicleAi {
        speed_cap_mps: 60.0,
        acceleration_mps2: Vec::new(),
        brake_mps2: 15.0,
        lateral_accel_cap_mps2: 14.0,
        yaw_accel_cap_rad_s2: 14.0,
        misalignment: Some(Misalignment {
            start_deg: 8.0,
            full_deg: 40.0,
            max_slowdown: 0.3,
        }),
        shift_pause_updates: 0,
    }
}

fn glue(span: f32) -> Vec<f32> {
    // Index 10 is level; behind the leader (low index) speeds up, ahead slows.
    (0..GLUE_CURVE_LEN)
        .map(|i| (1.0 + span * (10.0 - i as f32) / 10.0).clamp(0.8, 1.25))
        .collect()
}

/// Project-owned personalities, catch-up curves, lane weights and gains.
pub fn default_ai_set() -> AiSet {
    let personality =
        |name: &str, beginner: f32, advanced: f32, look: f32, aggressive: bool| Personality {
            name: name.into(),
            speed_factor: [beginner, advanced],
            accel_factor: [0.9, 1.0],
            handling_adjust: 1.0,
            blocker_look_ahead_m: 30.0 + look,
            lane_change_speed_limit_mps: 70.0,
            steer_look_ahead_per_mps: 0.35 + look / 400.0,
            steer_look_ahead_min_rows: 2,
            steer_look_ahead_max_rows: 8,
            lane_slack_m: [-0.5, 0.0, 0.0, 0.5],
            slack_redraw_one_in: 30,
            speed_glue: glue(0.12),
            accel_glue: glue(0.1),
            aggression: aggressive.then_some(Aggression {
                contact_ticks: 6,
                duration_ticks: 40,
            }),
        };
    let normal = GainSet {
        lateral_pos_to_vel: 0.8,
        max_lateral_vel_mps: 3.0,
        lateral_vel_to_accel: 3.0,
        heading_to_yaw_rate: 2.2,
        max_yaw_rate_rad_s: 2.4,
        yaw_rate_to_accel: 10.0,
        low_speed_mps: 4.0,
        low_speed_lateral_factor: 0.4,
        low_speed_yaw_factor: 0.35,
    };
    let recovery = GainSet {
        lateral_pos_to_vel: 0.5,
        max_lateral_vel_mps: 2.0,
        lateral_vel_to_accel: 2.0,
        heading_to_yaw_rate: 1.6,
        max_yaw_rate_rad_s: 1.6,
        yaw_rate_to_accel: 8.0,
        low_speed_mps: 3.0,
        low_speed_lateral_factor: 0.5,
        low_speed_yaw_factor: 0.6,
    };
    AiSet {
        id: "default-drivers".into(),
        name: "Default drivers".into(),
        personalities: vec![
            personality("steady", 0.86, 0.97, 0.0, false),
            personality("quick", 0.88, 1.0, 10.0, false),
            personality("careful", 0.84, 0.94, -5.0, false),
            personality("bold", 0.87, 0.99, 15.0, true),
            personality("smooth", 0.85, 0.96, 5.0, false),
            personality("sharp", 0.88, 1.0, 20.0, true),
            personality("relaxed", 0.83, 0.93, 0.0, false),
        ],
        catch_up: CatchUp {
            speed_scale: 1.0,
            accel_scale: 1.0,
            step_segments: 8,
            fade_in_segments: [[10, 20, 30, 40], [20, 40, 60, 80], [30, 60, 90, 120]],
            clamp_range: [0.6, 1.3],
            clamp_within_segments: 6,
            start_boost_segments: 24,
            start_boost_max: 0.2,
        },
        lane_weights: LaneWeights {
            absent_now: 1000.0,
            absent_ahead: 400.0,
            absent_near: 600.0,
            shoulder: 150.0,
            edge: 300.0,
            adjacent_car: 500.0,
            blocker_speed: 25.0,
            obstacle: 30.0,
            hint: 40.0,
            block_human: 25.0,
        },
        gains: Gains { normal, recovery },
    }
}
