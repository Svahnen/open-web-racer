//! A minimal external vehicle model: a point mass with a speed cap, a steering
//! rate and linear drag. It exists to prove that a separate crate can
//! implement [`VehicleModelProvider`] and be registered with the server and the
//! client without the engine knowing about it.
//!
//! Parameters are JSON text, for example
//! `{"max_speed": 40.0, "accel": 12.0, "steer_rate": 1.5, "drag": 0.2}`.
//! Every field is optional.

use racer_sim::model_ext::{
    VehicleDynamics, VehicleModelProvider, VehicleModelRegistry, VehicleStep,
};
use racer_sim::{DriverInput, Vec2, Vehicle};

/// Stable id clients use in `SetCarModel`.
pub const MODEL_ID: &str = "example.point-mass";

#[derive(Clone, Copy, Debug, PartialEq)]
struct Params {
    max_speed: f64,
    accel: f64,
    brake: f64,
    steer_rate: f64,
    drag: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            max_speed: 40.0,
            accel: 12.0,
            brake: 25.0,
            steer_rate: 1.5,
            drag: 0.2,
        }
    }
}

fn parse_params(bytes: &[u8]) -> Result<Params, String> {
    let mut p = Params::default();
    if bytes.is_empty() {
        return Ok(p);
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid params: {e}"))?;
    let object = value.as_object().ok_or("params must be a JSON object")?;
    for (key, field) in [
        ("max_speed", &mut p.max_speed),
        ("accel", &mut p.accel),
        ("brake", &mut p.brake),
        ("steer_rate", &mut p.steer_rate),
        ("drag", &mut p.drag),
    ] {
        if let Some(v) = object.get(key) {
            let n = v
                .as_f64()
                .ok_or_else(|| format!("{key} must be a number"))?;
            if !n.is_finite() || !(0.0..=1000.0).contains(&n) {
                return Err(format!("{key} out of range"));
            }
            *field = n;
        }
    }
    Ok(p)
}

struct PointMass {
    params: Params,
}

impl VehicleDynamics for PointMass {
    fn clone_box(&self) -> Box<dyn VehicleDynamics> {
        Box::new(PointMass {
            params: self.params,
        })
    }

    fn step(&mut self, vehicle: Vehicle, input: DriverInput, dt: f64) -> VehicleStep {
        let p = self.params;
        let forward = Vec2::new(vehicle.heading.cos(), vehicle.heading.sin());
        let speed = vehicle.velocity.x * forward.x + vehicle.velocity.y * forward.y;
        let throttle = input.throttle.clamp(0.0, 1.0);
        let brake = input.brake.clamp(0.0, 1.0);
        let speed = (speed + (throttle * p.accel - brake * p.brake - p.drag * speed) * dt)
            .clamp(0.0, p.max_speed);
        let angular_velocity =
            input.steer.clamp(-1.0, 1.0) * p.steer_rate * (speed / p.max_speed.max(1.0));
        let heading = vehicle.heading + angular_velocity * dt;
        let velocity = Vec2::new(heading.cos() * speed, heading.sin() * speed);
        VehicleStep {
            heading,
            angular_velocity,
            velocity,
            proposed_position: vehicle.position + velocity * dt,
        }
    }
}

pub struct ExampleProvider;

impl VehicleModelProvider for ExampleProvider {
    fn id(&self) -> &str {
        MODEL_ID
    }

    fn revision(&self) -> &str {
        "example-point-mass-v1"
    }

    fn create(&self, params: &[u8]) -> Result<Box<dyn VehicleDynamics>, String> {
        Ok(Box::new(PointMass {
            params: parse_params(params)?,
        }))
    }
}

/// Register this crate's providers; the same call works for the server and for
/// a client build.
pub fn register(registry: &mut VehicleModelRegistry) -> Result<(), String> {
    registry.register(Box::new(ExampleProvider))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_are_validated() {
        assert!(parse_params(b"").is_ok());
        assert!(parse_params(br#"{"accel": 5}"#).is_ok());
        assert!(parse_params(b"[]").is_err());
        assert!(parse_params(br#"{"accel": -1}"#).is_err());
        assert!(parse_params(br#"{"accel": "x"}"#).is_err());
    }

    #[test]
    fn full_throttle_accelerates_up_to_the_cap() {
        let mut registry = VehicleModelRegistry::new();
        register(&mut registry).unwrap();
        let mut m = registry.create(MODEL_ID, br#"{"max_speed": 10}"#).unwrap();
        let mut v = Vehicle {
            position: Vec2::new(0.0, 0.0),
            position_z: 0.0,
            heading: 0.0,
            velocity: Vec2::new(0.0, 0.0),
            angular_velocity: 0.0,
            checkpoint: 0,
            completed_laps: 0,
            race_ticks: 0,
            finished: false,
        };
        let input = DriverInput {
            throttle: 1.0,
            ..DriverInput::default()
        };
        for _ in 0..600 {
            let s = m.step(v, input, 1.0 / 60.0);
            v.heading = s.heading;
            v.velocity = s.velocity;
            v.position = s.proposed_position;
        }
        assert!((v.velocity.x - 10.0).abs() < 1e-3);
        assert!(v.position.x > 50.0);
    }
}
