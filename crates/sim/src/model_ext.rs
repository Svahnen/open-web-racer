//! Extension point for externally supplied vehicle models.
//!
//! The built-in [`crate::VehicleModel`] enum stays the fast, WASM-friendly path.
//! An external crate can instead implement [`VehicleModelProvider`] and register
//! it in a [`VehicleModelRegistry`]; a content pack selects it through the
//! opaque `model: { id, params }` field of a vehicle definition. The engine
//! never interprets `params`; it only passes the JSON text to the provider.
//!
//! `RaceWorld::install_custom_model` builds an instance from a registry and
//! `VehicleModel::Custom` steps it. The model id, the provider revision and the
//! opaque parameter bytes are part of the world identity hash; the provider's
//! own dynamic state travels in the rollback state frame as opaque bytes.

use crate::drivetrain::{DriveState, DriveTelemetry};
use crate::{DriverInput, Vec2, Vehicle};
use std::collections::BTreeMap;

/// Result of advancing one fixed physics step. Mirrors what the engine needs
/// back from any model: the new planar kinematic state and a proposed position
/// (the engine still owns collision, support, bounds and race progression).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VehicleStep {
    pub heading: f64,
    pub angular_velocity: f64,
    pub velocity: Vec2,
    pub proposed_position: Vec2,
}

/// A stateful, per-vehicle dynamics instance. Must be deterministic for a given
/// input sequence, and `save_state`/`load_state` must round-trip every piece of
/// internal state that influences later steps (rollback relies on it).
pub trait VehicleDynamics: Send {
    /// Clone this instance (worlds are cloneable for prediction).
    fn clone_box(&self) -> Box<dyn VehicleDynamics>;

    /// Serialise the dynamic state for rollback. Stateless models return empty.
    fn save_state(&self) -> Vec<u8> {
        Vec::new()
    }

    /// Restore state written by `save_state`; must reject malformed bytes
    /// without partially applying them.
    fn load_state(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() {
            Ok(())
        } else {
            Err("model keeps no state".into())
        }
    }

    /// Advance by `dt` seconds. `input` is already normalised to its documented ranges.
    fn step(&mut self, vehicle: Vehicle, input: DriverInput, dt: f64) -> VehicleStep;

    /// The side slope of the road under the vehicle for the coming `step`
    /// (sine of the roll angle, positive when the downhill side is on the
    /// vehicle's left; see [`crate::road_pull`]). Called before every step; it
    /// is an input, not model state, so it is not part of `save_state`.
    /// Defaults to ignoring it.
    fn set_road_slope(&mut self, _slope: f64) {}

    /// Reset internal state (called on respawn / race restart).
    fn reset(&mut self) {}

    /// Told the world's fixed step (seconds) when the model is installed, so
    /// it can validate restored state against the step rate. Defaults to no-op.
    fn set_step_seconds(&mut self, _dt: f64) {}

    /// The driver's gearbox mode (`true`: manual). Called when a model is
    /// installed into a world whose transmission is set, and whenever the
    /// world's transmission is set; a following `reset` restarts the
    /// drivetrain. Defaults to no change (models without a gearbox).
    fn set_transmission(&mut self, _manual: bool) {}

    /// The driver's auto-reverse option (see `drivetrain::apply_auto_reverse`).
    /// Called when a model is installed and whenever the world's option is
    /// set. Defaults to no change (models that do not support it).
    fn set_auto_reverse(&mut self, _enabled: bool) {}

    /// Called when the vehicle is placed on the grid (`RaceWorld::place_vehicle`).
    /// Defaults to no change.
    fn on_place(&mut self) {}

    /// Gear and engine-speed estimate for HUD/audio at the vehicle's current
    /// state; `None` when the model has no gearbox.
    fn telemetry(&self, _vehicle: &Vehicle) -> Option<DriveTelemetry> {
        None
    }

    /// Drivetrain state for diagnostics and traces, when the model keeps one.
    fn drive_state(&self) -> Option<DriveState> {
        None
    }
}

/// Factory for one kind of external model.
pub trait VehicleModelProvider: Send + Sync {
    /// Stable identifier matched against `model.id` in a vehicle definition.
    fn id(&self) -> &str;

    /// Revision string of the provider's equations; hashed into the world
    /// identity so peers with different implementations fail the check.
    fn revision(&self) -> &str {
        ""
    }

    /// Build an instance from the pack's opaque parameter block (bytes; JSON
    /// text in practice).
    fn create(&self, params: &[u8]) -> Result<Box<dyn VehicleDynamics>, String>;

    /// Diagnostic code a server reports for vehicles driven by this model
    /// (`VehicleSnapshot::handling_model`). Defaults to the generic external
    /// code (5); providers may use any other value not taken by the built-in
    /// models (0, 3, 4).
    fn wire_code(&self) -> u8 {
        5
    }

    /// Whether a selection made with `params` applies to the vehicle profile
    /// `vehicle_profile_id` (providers whose parameters describe one specific
    /// car can decline others). Defaults to every vehicle.
    fn applies_to(&self, _params: &[u8], _vehicle_profile_id: &str) -> bool {
        true
    }
}

/// Registry of providers keyed by id.
#[derive(Default)]
pub struct VehicleModelRegistry {
    providers: BTreeMap<String, Box<dyn VehicleModelProvider>>,
}

impl VehicleModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a provider; returns an error if the id is already taken.
    pub fn register(&mut self, provider: Box<dyn VehicleModelProvider>) -> Result<(), String> {
        let id = provider.id().to_string();
        if id.is_empty() {
            return Err("provider id must not be empty".into());
        }
        if self.providers.contains_key(&id) {
            return Err(format!("vehicle model provider {id:?} already registered"));
        }
        self.providers.insert(id, provider);
        Ok(())
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.providers.keys().map(String::as_str)
    }

    /// Diagnostic wire code of the provider `id` (5 when unknown).
    pub fn wire_code(&self, id: &str) -> u8 {
        self.providers.get(id).map_or(5, |p| p.wire_code())
    }

    /// Whether the selection `params` of provider `id` applies to a vehicle
    /// profile (true for unknown ids).
    pub fn applies_to(&self, id: &str, params: &[u8], vehicle_profile_id: &str) -> bool {
        self.providers
            .get(id)
            .is_none_or(|p| p.applies_to(params, vehicle_profile_id))
    }

    pub fn create(&self, id: &str, params: &[u8]) -> Result<Box<dyn VehicleDynamics>, String> {
        self.providers
            .get(id)
            .ok_or_else(|| format!("no vehicle model provider registered for {id:?}"))?
            .create(params)
    }

    /// Build a [`CustomModel`] (instance plus the data bound into identity).
    pub fn instantiate(&self, id: &str, params: &[u8]) -> Result<CustomModel, String> {
        let provider = self
            .providers
            .get(id)
            .ok_or_else(|| format!("no vehicle model provider registered for {id:?}"))?;
        let dynamics = provider.create(params)?;
        Ok(CustomModel {
            id: id.to_string(),
            revision: provider.revision().to_string(),
            params: params.to_vec(),
            dynamics,
        })
    }
}

/// A provider-supplied model installed in a `RaceWorld`.
pub struct CustomModel {
    pub(crate) id: String,
    pub(crate) revision: String,
    pub(crate) params: Vec<u8>,
    pub(crate) dynamics: Box<dyn VehicleDynamics>,
}

impl CustomModel {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn params(&self) -> &[u8] {
        &self.params
    }
}

impl Clone for CustomModel {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            revision: self.revision.clone(),
            params: self.params.clone(),
            dynamics: self.dynamics.clone_box(),
        }
    }
}

impl std::fmt::Debug for CustomModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomModel")
            .field("id", &self.id)
            .field("revision", &self.revision)
            .field("params_len", &self.params.len())
            .finish()
    }
}

impl PartialEq for CustomModel {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.revision == other.revision
            && self.params == other.params
            && self.dynamics.save_state() == other.dynamics.save_state()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Constant;
    impl VehicleDynamics for Constant {
        fn clone_box(&self) -> Box<dyn VehicleDynamics> {
            Box::new(Constant)
        }
        fn step(&mut self, vehicle: Vehicle, _input: DriverInput, dt: f64) -> VehicleStep {
            VehicleStep {
                heading: vehicle.heading,
                angular_velocity: 0.0,
                velocity: vehicle.velocity,
                proposed_position: vehicle.position + vehicle.velocity * dt,
            }
        }
    }
    struct Provider;
    impl VehicleModelProvider for Provider {
        fn id(&self) -> &str {
            "test.constant"
        }
        fn create(&self, params: &[u8]) -> Result<Box<dyn VehicleDynamics>, String> {
            if params.is_empty() {
                Err("params required".into())
            } else {
                Ok(Box::new(Constant))
            }
        }
    }

    #[test]
    fn registry_registers_creates_and_rejects_duplicates() {
        let mut r = VehicleModelRegistry::new();
        r.register(Box::new(Provider)).unwrap();
        assert!(r.register(Box::new(Provider)).is_err());
        assert_eq!(r.ids().collect::<Vec<_>>(), vec!["test.constant"]);
        assert!(r.create("missing", b"{}").is_err());
        assert!(r.create("test.constant", b"").is_err());
        let mut m = r.create("test.constant", b"{}").unwrap();
        let v = Vehicle {
            position: Vec2::new(0.0, 0.0),
            position_z: 0.0,
            heading: 0.0,
            velocity: Vec2::new(2.0, 0.0),
            angular_velocity: 0.0,
            checkpoint: 0,
            completed_laps: 0,
            race_ticks: 0,
            finished: false,
        };
        let s = m.step(v, DriverInput::default(), 0.5);
        assert_eq!(s.proposed_position, Vec2::new(1.0, 0.0));
    }
}
