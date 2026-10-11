//! Game-neutral computer drivers: a lane model with lane choice, target-speed
//! composition from a profile and difficulty/catch-up factors, a steering servo
//! that feeds the shared simulation integrator, and recovery.
//!
//! Nothing here refers to a particular game. Content (lane layouts, speed
//! profiles, personalities, limits) comes from a pack or from the neutral
//! defaults in [`config`].

pub mod config;
pub mod course;
pub mod field;
pub mod q16;
pub mod race;
pub mod rules;
pub mod track;
pub mod vehicle;

pub use course::{ImportedCourse, imported_course};
pub use config::{AiConfig, Skill, default_ai_set, default_vehicle_ai};
pub use field::{AiField, CarObs, Settings};
pub use race::{DEFAULT_OPPONENTS, MAX_OPPONENTS, Opponents, circuit_ai_track};
pub use racer_pack_schema::ai::VehicleAi;
pub use track::{AiTrack, DeriveParams, RoadFrame, RoadRow, TrackError};
pub use vehicle::{
    measured_acceleration, measured_acceleration_classic, vehicle_ai_from_arcade,
    vehicle_ai_from_classic,
};
