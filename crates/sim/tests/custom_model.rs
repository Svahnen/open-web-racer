//! Tests for provider-supplied models (`VehicleModel::Custom`) with a fake provider.

use racer_sim::model_ext::{
    VehicleDynamics, VehicleModelProvider, VehicleModelRegistry, VehicleStep,
};
use racer_sim::{
    DriverInput, RaceWorld, SimStateFrame, SimTiming, TrackRuntime, Vec2, Vehicle, VehicleModel,
};

/// Stateful fake: speed integrates `gain * throttle`; params = one gain byte.
#[derive(Clone)]
struct Accum {
    gain: f64,
    speed: f64,
}

impl VehicleDynamics for Accum {
    fn clone_box(&self) -> Box<dyn VehicleDynamics> {
        Box::new(self.clone())
    }
    fn save_state(&self) -> Vec<u8> {
        self.speed.to_bits().to_le_bytes().to_vec()
    }
    fn load_state(&mut self, bytes: &[u8]) -> Result<(), String> {
        let raw: [u8; 8] = bytes.try_into().map_err(|_| "bad length".to_string())?;
        self.speed = f64::from_bits(u64::from_le_bytes(raw));
        Ok(())
    }
    fn reset(&mut self) {
        self.speed = 0.0;
    }
    fn step(&mut self, vehicle: Vehicle, input: DriverInput, dt: f64) -> VehicleStep {
        self.speed += self.gain * input.throttle * dt;
        let heading = vehicle.heading + input.steer * dt;
        let velocity = Vec2::new(heading.cos() * self.speed, heading.sin() * self.speed);
        VehicleStep {
            heading,
            angular_velocity: input.steer,
            velocity,
            proposed_position: vehicle.position + velocity * dt,
        }
    }
}

struct Provider;
impl VehicleModelProvider for Provider {
    fn id(&self) -> &str {
        "fake.accum"
    }
    fn revision(&self) -> &str {
        "r1"
    }
    fn create(&self, params: &[u8]) -> Result<Box<dyn VehicleDynamics>, String> {
        match params {
            [gain] => Ok(Box::new(Accum {
                gain: f64::from(*gain),
                speed: 0.0,
            })),
            _ => Err("expected one gain byte".into()),
        }
    }
}

fn registry() -> VehicleModelRegistry {
    let mut r = VehicleModelRegistry::new();
    r.register(Box::new(Provider)).unwrap();
    r
}

fn timing() -> SimTiming {
    SimTiming {
        physics_hz: 60,
        input_hz: 60,
        race_clock_hz: 60,
    }
}

fn world(gain: u8) -> RaceWorld {
    let mut w = RaceWorld::new(timing(), TrackRuntime::open_preview());
    w.install_custom_model(&registry(), "fake.accum", &[gain])
        .unwrap();
    w
}

fn drive(throttle: f64, steer: f64) -> DriverInput {
    DriverInput {
        throttle,
        steer,
        ..DriverInput::default()
    }
}

#[test]
fn provider_model_is_stepped_and_deterministic() {
    let mut a = world(10);
    let mut b = world(10);
    assert_eq!(a.vehicle_model(), VehicleModel::Custom);
    assert_eq!(a.custom_model_id(), Some("fake.accum"));
    for i in 0..120 {
        let input = drive(1.0, if i % 30 < 15 { 0.2 } else { -0.2 });
        a.step(input);
        b.step(input);
    }
    assert_eq!(a.vehicle, b.vehicle);
    let v = a.vehicle.velocity;
    let speed = (v.x * v.x + v.y * v.y).sqrt();
    assert!((speed - 20.0).abs() < 1e-9, "speed {speed}");
}

#[test]
fn unknown_id_and_bad_params_are_rejected_without_side_effects() {
    let mut w = RaceWorld::new(timing(), TrackRuntime::open_preview());
    let before = w.simulation_identity().unwrap();
    assert!(w.install_custom_model(&registry(), "nope", &[1]).is_err());
    assert!(w
        .install_custom_model(&registry(), "fake.accum", &[])
        .is_err());
    assert_eq!(w.vehicle_model(), VehicleModel::Provisional);
    assert_eq!(w.simulation_identity().unwrap(), before);
    assert!(w.set_vehicle_model(VehicleModel::Custom).is_err());
}

#[test]
fn identity_changes_with_params_and_model() {
    let a = world(10).simulation_identity().unwrap();
    assert_eq!(a, world(10).simulation_identity().unwrap());
    assert_ne!(a, world(11).simulation_identity().unwrap());
    let mut w = world(10);
    w.set_vehicle_model(VehicleModel::Provisional).unwrap();
    assert_ne!(a, w.simulation_identity().unwrap());
}

#[test]
fn rollback_restores_provider_state() {
    let mut w = world(10);
    for _ in 0..30 {
        w.step(drive(1.0, 0.1));
    }
    let frame = w.capture_state().unwrap();
    let decoded = SimStateFrame::decode(&frame.encode()).unwrap();
    assert_eq!(decoded, frame);
    let mut reference = w.clone();
    for _ in 0..40 {
        w.step(drive(0.5, -0.1));
    }
    w.restore_state(&decoded).unwrap();
    assert_eq!(w.vehicle, reference.vehicle);
    for _ in 0..40 {
        w.step(drive(0.5, -0.1));
        reference.step(drive(0.5, -0.1));
    }
    assert_eq!(w.vehicle, reference.vehicle);
    assert_eq!(
        w.capture_state().unwrap(),
        reference.capture_state().unwrap()
    );
}

#[test]
fn malformed_or_mismatched_frames_are_rejected() {
    let mut w = world(10);
    w.step(drive(1.0, 0.0));
    let frame = w.capture_state().unwrap();
    let mut bad = frame.clone();
    bad.custom = Some(vec![1, 2, 3]);
    let before = w.vehicle;
    assert!(w.restore_state(&bad).is_err());
    assert_eq!(w.vehicle, before);
    let mut missing = frame.clone();
    missing.custom = None;
    assert!(w.restore_state(&missing).is_err());
    let bytes = frame.encode();
    assert!(SimStateFrame::decode(&bytes[..bytes.len() - 1]).is_err());
}

#[test]
fn respawn_resets_provider_state() {
    let mut w = world(10);
    for _ in 0..30 {
        w.step(drive(1.0, 0.0));
    }
    w.respawn();
    w.step(DriverInput::default());
    let v = w.vehicle.velocity;
    assert!(v.x.abs() + v.y.abs() < 1e-9);
}
