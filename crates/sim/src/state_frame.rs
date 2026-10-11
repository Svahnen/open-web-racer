//! Versioned, simulation-owned save/restore frames for fixed-step replay.
//!
//! This is an internal simulation contract. It is not a network message and
//! contains no render camera or renderer state.

use sha2::{Digest, Sha256};

use crate::{DriveState, PreviewCollisionWorld, RaceWorld, Vec2, Vehicle, VehicleModel};

/// Version 6 appended the optional provider-model state (tag, u32 length, bytes);
/// version 7 dropped the built-in model runtime state, which now travels
/// in the provider state; version 8 appended the manual-gearbox flag of the
/// drivetrain state; version 9 appended the auto-reverse flag.
pub const SIM_STATE_FRAME_VERSION: u16 = 9;
/// Bump whenever RaceWorld simulation behavior or hardcoded coefficients change.
pub const SIMULATION_MODEL_REVISION: &str = "provisional-raceworld-v1";
const MAGIC: &[u8; 8] = b"PRSIMF01";
const MAX_STATE_SCALAR: f64 = 1.0e12;

/// Complete dynamic simulation state at a fixed-step boundary. Static world
/// configuration is bound by `world_identity` and must already be installed.
#[derive(Clone, Debug, PartialEq)]
pub struct SimStateFrame {
    pub version: u16,
    pub world_identity: [u8; 32],
    pub tick: u64,
    pub vehicle: Vehicle,
    /// Continuity support state for the optional preview collision solver.
    pub preview_ground_z: Option<f64>,
    pub preview_ground_normal: Option<[f64; 3]>,
    /// Drivetrain/command state of the built-in gearbox model (idle default for
    /// other models).
    pub drive: DriveState,
    /// Opaque provider state; present exactly when the active model is
    /// `VehicleModel::Custom`.
    pub custom: Option<Vec<u8>>,
}

/// Upper bound on provider state carried in a frame.
pub const MAX_CUSTOM_STATE_BYTES: usize = 1 << 20;

impl SimStateFrame {
    /// Stable little-endian format usable by native and WebAssembly builds.
    /// Option tags are exactly 0 or 1; malformed/truncated/trailing bytes fail.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(224);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&self.version.to_le_bytes());
        bytes.extend_from_slice(&self.world_identity);
        bytes.extend_from_slice(&self.tick.to_le_bytes());
        for value in [
            self.vehicle.position.x,
            self.vehicle.position.y,
            self.vehicle.position_z,
            self.vehicle.heading,
            self.vehicle.velocity.x,
            self.vehicle.velocity.y,
            self.vehicle.angular_velocity,
        ] {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        bytes.extend_from_slice(&(self.vehicle.checkpoint as u64).to_le_bytes());
        bytes.extend_from_slice(&self.vehicle.completed_laps.to_le_bytes());
        bytes.extend_from_slice(&self.vehicle.race_ticks.to_le_bytes());
        bytes.push(u8::from(self.vehicle.finished));
        encode_option_f64(&mut bytes, self.preview_ground_z);
        match self.preview_ground_normal {
            None => bytes.push(0),
            Some(normal) => {
                bytes.push(1);
                for value in normal {
                    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
                }
            }
        }
        for value in [
            self.drive.throttle_command,
            self.drive.brake_command,
            self.drive.steer_command,
            self.drive.shift_timer,
            self.drive.slide_offset,
            self.drive.handbrake_seconds,
        ] {
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        bytes.push(self.drive.gear);
        bytes.push(self.drive.selector);
        bytes.push(u8::from(self.drive.launch_armed));
        bytes.push(u8::from(self.drive.shift_up_held));
        bytes.push(u8::from(self.drive.shift_down_held));
        bytes.push(u8::from(self.drive.manual));
        bytes.push(u8::from(self.drive.auto_reverse));
        match &self.custom {
            None => bytes.push(0),
            Some(state) => {
                bytes.push(1);
                bytes.extend_from_slice(&(state.len() as u32).to_le_bytes());
                bytes.extend_from_slice(state);
            }
        }
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        let mut input = ByteReader { bytes, offset: 0 };
        if input.take(8)? != MAGIC {
            return Err("simulation frame magic is invalid");
        }
        let version = input.u16()?;
        if version != SIM_STATE_FRAME_VERSION {
            return Err("simulation frame version is unsupported");
        }
        let mut world_identity = [0; 32];
        world_identity.copy_from_slice(input.take(32)?);
        let tick = input.u64()?;
        let position = Vec2::new(input.f64()?, input.f64()?);
        let position_z = input.f64()?;
        let heading = input.f64()?;
        let velocity = Vec2::new(input.f64()?, input.f64()?);
        let angular_velocity = input.f64()?;
        let checkpoint = usize::try_from(input.u64()?)
            .map_err(|_| "simulation frame checkpoint is out of range")?;
        let completed_laps = input.u32()?;
        let race_ticks = input.u64()?;
        let finished = input.boolean()?;
        let preview_ground_z = input.option_f64()?;
        let preview_ground_normal = match input.tag()? {
            0 => None,
            1 => Some([input.f64()?, input.f64()?, input.f64()?]),
            _ => return Err("simulation frame support tag is invalid"),
        };
        let drive = DriveState {
            throttle_command: input.f64()?,
            brake_command: input.f64()?,
            steer_command: input.f64()?,
            shift_timer: input.f64()?,
            slide_offset: input.f64()?,
            handbrake_seconds: input.f64()?,
            gear: input.take(1)?[0],
            selector: input.take(1)?[0],
            launch_armed: input.boolean()?,
            shift_up_held: input.boolean()?,
            shift_down_held: input.boolean()?,
            manual: input.boolean()?,
            auto_reverse: input.boolean()?,
        };
        let custom = match input.tag()? {
            0 => None,
            1 => {
                let len = input.u32()? as usize;
                if len > MAX_CUSTOM_STATE_BYTES {
                    return Err("simulation frame custom-model state is too large");
                }
                Some(input.take(len)?.to_vec())
            }
            _ => return Err("simulation frame custom-state tag is invalid"),
        };
        if input.offset != bytes.len() {
            return Err("simulation frame has trailing bytes");
        }
        let frame = Self {
            version,
            world_identity,
            tick,
            vehicle: Vehicle {
                position,
                position_z,
                heading,
                velocity,
                angular_velocity,
                checkpoint,
                completed_laps,
                race_ticks,
                finished,
            },
            preview_ground_z,
            preview_ground_normal,
            drive,
            custom,
        };
        validate_intrinsic(&frame)?;
        Ok(frame)
    }
}

impl RaceWorld {
    /// SHA-256 of a domain-separated, explicitly ordered encoding of all
    /// simulation-affecting static configuration in this world.
    pub fn simulation_identity(&self) -> Result<[u8; 32], &'static str> {
        validate_static_world(self)?;
        let mut hash = Sha256::new();
        hash.update(b"racer/simulation-world/v1\0");
        hash.update(SIMULATION_MODEL_REVISION.as_bytes());
        hash.update([0]);
        hash_u32(&mut hash, self.timing.physics_hz);
        hash_u32(&mut hash, self.timing.input_hz);
        hash_u32(&mut hash, self.timing.race_clock_hz);
        for value in [
            self.track.min.x,
            self.track.min.y,
            self.track.max.x,
            self.track.max.y,
        ] {
            hash_f64(&mut hash, value);
        }
        hash_u64(&mut hash, self.track.checkpoints.len() as u64);
        for checkpoint in &self.track.checkpoints {
            hash_f64(&mut hash, checkpoint.center.x);
            hash_f64(&mut hash, checkpoint.center.y);
            hash_f64(&mut hash, checkpoint.radius);
        }
        hash_u32(&mut hash, self.track.laps_to_finish);
        hash_f64(&mut hash, self.spawn.x);
        hash_f64(&mut hash, self.spawn.y);
        hash_u8(&mut hash, u8::from(self.shared_preview_handling));
        // The automatic box keeps the earlier identity bytes; a manual box
        // changes the dynamics and is domain-separated.
        if self.transmission().is_manual() {
            hash.update(b"manual-transmission");
        }
        if self.drive.auto_reverse {
            hash.update(b"auto-reverse");
        }
        // Provisional worlds keep their earlier identity bytes; other models
        // are domain-separated so mismatched peers fail the identity check.
        match self.vehicle_model {
            VehicleModel::Provisional => {}
            VehicleModel::Scripted => hash.update(crate::SCRIPTED_MODEL_REVISION.as_bytes()),
            VehicleModel::Arcade => {
                hash.update(crate::ARCADE_MODEL_REVISION.as_bytes());
                if let Some(arcade) = &self.arcade {
                    for word in arcade.identity_words() {
                        hash.update(word.to_le_bytes());
                    }
                }
            }
            VehicleModel::Classic => {
                hash.update(crate::CLASSIC_MODEL_REVISION.as_bytes());
                if let Some(classic) = &self.classic {
                    for word in classic.identity_words() {
                        hash.update(word.to_le_bytes());
                    }
                }
            }
            VehicleModel::Custom => {
                hash.update(b"custom-provider-model ");
                if let Some(custom) = &self.custom {
                    hash_u64(&mut hash, custom.id.len() as u64);
                    hash.update(custom.id.as_bytes());
                    hash_u64(&mut hash, custom.revision.len() as u64);
                    hash.update(custom.revision.as_bytes());
                    hash_u64(&mut hash, custom.params.len() as u64);
                    hash.update(&custom.params);
                }
            }
        }
        hash_f64(
            &mut hash,
            self.preview_grip_policy.lateral_recovery_per_second,
        );
        hash_f64(
            &mut hash,
            self.preview_grip_policy.handbrake_recovery_per_second,
        );
        hash_f64(&mut hash, self.preview_drive_policy.acceleration_per_second);
        hash_f64(&mut hash, self.preview_drive_policy.braking_per_second);
        hash_f64(&mut hash, self.preview_drive_policy.max_speed);
        hash_f64(&mut hash, self.preview_spawn_origin_z);
        match &self.preview_collision {
            None => hash_u8(&mut hash, 0),
            Some(collision) => {
                hash_u8(&mut hash, 1);
                hash_collision(&mut hash, collision);
            }
        }
        Ok(hash.finalize().into())
    }

    /// Capture state only at a valid fixed-step boundary. Diagnostic trace
    /// recording/history are intentionally excluded because they do not alter
    /// simulation results.
    pub fn capture_state(&self) -> Result<SimStateFrame, &'static str> {
        let frame = SimStateFrame {
            version: SIM_STATE_FRAME_VERSION,
            world_identity: self.simulation_identity()?,
            tick: self.tick,
            vehicle: self.vehicle,
            preview_ground_z: self.preview_ground_z,
            preview_ground_normal: self.preview_ground_normal,
            drive: self.drive,
            custom: match (self.vehicle_model, &self.custom) {
                (VehicleModel::Custom, Some(model)) => Some(model.dynamics.save_state()),
                (VehicleModel::Custom, None) => {
                    return Err("custom vehicle model is not installed")
                }
                _ => None,
            },
        };
        validate_frame_for_world(self, &frame)?;
        Ok(frame)
    }

    /// Validate the entire frame before mutation. A rejected frame leaves every
    /// simulation field untouched. Static policy/geometry must match exactly.
    pub fn restore_state(&mut self, frame: &SimStateFrame) -> Result<(), &'static str> {
        validate_frame_for_world(self, frame)?;
        self.tick = frame.tick;
        self.vehicle = frame.vehicle;
        self.preview_ground_z = frame.preview_ground_z;
        self.preview_ground_normal = frame.preview_ground_normal;
        self.drive = frame.drive;
        if let (Some(state), Some(model)) = (&frame.custom, self.custom.as_mut()) {
            // Cannot fail: validate_frame_for_world already trial-loaded it.
            let _ = model.dynamics.load_state(state);
        }
        self.edge_unsupported_steps = 0;
        self.replay_trace.clear();
        Ok(())
    }
}

fn validate_frame_for_world(world: &RaceWorld, frame: &SimStateFrame) -> Result<(), &'static str> {
    validate_intrinsic(frame)?;
    if frame.version != SIM_STATE_FRAME_VERSION {
        return Err("simulation frame version is unsupported");
    }
    if frame.world_identity != world.simulation_identity()? {
        return Err("simulation frame world identity does not match");
    }
    validate_vehicle(world, frame.tick, frame.vehicle)?;
    match (world.vehicle_model, &world.custom, &frame.custom) {
        (VehicleModel::Custom, Some(model), Some(state)) => {
            let mut trial = model.dynamics.clone_box();
            trial
                .load_state(state)
                .map_err(|_| "simulation frame custom-model state is rejected by the model")?;
        }
        (VehicleModel::Custom, _, _) | (_, _, Some(_)) => {
            return Err("simulation frame custom-model state does not match the world");
        }
        _ => {}
    }
    if frame.preview_ground_z.is_some() != world.preview_collision.is_some()
        || frame.preview_ground_normal.is_some() != world.preview_collision.is_some()
    {
        return Err("simulation frame preview support state is incomplete");
    }
    if let Some(height) = frame.preview_ground_z {
        if !valid_scalar(height) {
            return Err("simulation frame preview support height is invalid");
        }
    }
    if let Some(normal) = frame.preview_ground_normal {
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if normal.iter().any(|value| !valid_scalar(*value))
            || normal[2] <= 0.0
            || !length.is_finite()
            || (length - 1.0).abs() > 1.0e-6
        {
            return Err("simulation frame preview support normal is invalid");
        }
    }
    Ok(())
}

fn validate_intrinsic(frame: &SimStateFrame) -> Result<(), &'static str> {
    if frame.version != SIM_STATE_FRAME_VERSION {
        return Err("simulation frame version is unsupported");
    }
    let vehicle = frame.vehicle;
    if [
        vehicle.position.x,
        vehicle.position.y,
        vehicle.position_z,
        vehicle.heading,
        vehicle.velocity.x,
        vehicle.velocity.y,
        vehicle.angular_velocity,
    ]
    .into_iter()
    .any(|value| !valid_scalar(value))
        || frame
            .preview_ground_z
            .is_some_and(|value| !valid_scalar(value))
    {
        return Err("simulation frame contains a non-finite or out-of-range scalar");
    }
    if !frame.drive.is_valid() {
        return Err("simulation frame drivetrain state is invalid");
    }
    if let Some(normal) = frame.preview_ground_normal {
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if normal.into_iter().any(|value| !valid_scalar(value))
            || normal[2] <= 0.0
            || !length.is_finite()
            || (length - 1.0).abs() > 1.0e-6
        {
            return Err("simulation frame support normal is invalid");
        }
    }
    Ok(())
}

fn validate_vehicle(
    world: &RaceWorld,
    frame_tick: u64,
    vehicle: Vehicle,
) -> Result<(), &'static str> {
    let scalars = [
        vehicle.position.x,
        vehicle.position.y,
        vehicle.position_z,
        vehicle.heading,
        vehicle.velocity.x,
        vehicle.velocity.y,
        vehicle.angular_velocity,
    ];
    if scalars.iter().any(|value| !valid_scalar(*value)) {
        return Err("simulation frame vehicle contains an invalid scalar");
    }
    let lap_state_is_valid = if world.track.laps_to_finish == 0 {
        vehicle.completed_laps == 0 && !vehicle.finished
    } else if vehicle.finished {
        vehicle.completed_laps == world.track.laps_to_finish
    } else {
        vehicle.completed_laps < world.track.laps_to_finish
    };
    if !world.track.contains(vehicle.position)
        || vehicle.checkpoint > world.track.checkpoints.len().saturating_sub(1)
        || vehicle.race_ticks > frame_tick
        || !lap_state_is_valid
    {
        return Err("simulation frame vehicle state is inconsistent with its world");
    }
    Ok(())
}

fn validate_static_world(world: &RaceWorld) -> Result<(), &'static str> {
    let track = &world.track;
    if ![track.min.x, track.min.y, track.max.x, track.max.y]
        .into_iter()
        .all(f64::is_finite)
        || ![world.spawn.x, world.spawn.y].into_iter().all(valid_scalar)
        || track.min.x >= track.max.x
        || track.min.y >= track.max.y
        || !track.contains(world.spawn)
        || (track.checkpoints.is_empty() != (track.laps_to_finish == 0))
        || track.checkpoints.iter().any(|checkpoint| {
            !valid_scalar(checkpoint.center.x)
                || !valid_scalar(checkpoint.center.y)
                || !valid_scalar(checkpoint.radius)
                || checkpoint.radius <= 0.0
                || !track.contains(checkpoint.center)
        })
    {
        return Err("simulation world track configuration is invalid");
    }
    if !world.preview_grip_policy.is_valid() || !world.preview_drive_policy.is_valid() {
        return Err("simulation world handling policy is invalid");
    }
    if !valid_scalar(world.preview_spawn_origin_z) {
        return Err("simulation world preview spawn context is invalid");
    }
    if let Some(collision) = &world.preview_collision {
        collision.validate()?;
    }
    Ok(())
}

fn hash_collision(hash: &mut Sha256, collision: &PreviewCollisionWorld) {
    hash_u64(hash, collision.ground.len() as u64);
    for triangle in &collision.ground {
        for vertex in triangle.vertices {
            for value in vertex {
                hash_f64(hash, value);
            }
        }
    }
    hash_u64(hash, collision.walls.len() as u64);
    for wall in &collision.walls {
        for value in [
            wall.start.x,
            wall.start.y,
            wall.end.x,
            wall.end.y,
            wall.min_z,
            wall.max_z,
        ] {
            hash_f64(hash, value);
        }
    }
    for value in [
        collision.vehicle_radius,
        collision.vehicle_min_z,
        collision.vehicle_max_z,
        collision.initial_ground_z,
        collision.units_per_sim_unit,
    ] {
        hash_f64(hash, value);
    }
    // Worlds without edge rows keep their previous identity bytes.
    if !collision.edge_rows.is_empty() {
        hash.update(crate::TRACK_EDGE_MODEL_REVISION.as_bytes());
        hash_u64(hash, collision.edge_rows.len() as u64);
        for row in &collision.edge_rows {
            for value in [
                row.x,
                row.y,
                row.z,
                row.forward.x,
                row.forward.y,
                row.right.x,
                row.right.y,
                row.left_distance,
                row.right_distance,
            ] {
                hash_f64(hash, value);
            }
            hash.update(row.block.to_le_bytes());
        }
    }
}

fn valid_scalar(value: f64) -> bool {
    value.is_finite() && value.abs() <= MAX_STATE_SCALAR
}

fn hash_f64(hash: &mut Sha256, value: f64) {
    let canonical = if value == 0.0 { 0.0 } else { value };
    hash.update(canonical.to_bits().to_le_bytes());
}

fn hash_u8(hash: &mut Sha256, value: u8) {
    hash.update([value]);
}
fn hash_u32(hash: &mut Sha256, value: u32) {
    hash.update(value.to_le_bytes());
}
fn hash_u64(hash: &mut Sha256, value: u64) {
    hash.update(value.to_le_bytes());
}

fn encode_option_f64(bytes: &mut Vec<u8>, value: Option<f64>) {
    match value {
        None => bytes.push(0),
        Some(value) => {
            bytes.push(1);
            bytes.extend_from_slice(&value.to_bits().to_le_bytes());
        }
    }
}

struct ByteReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl ByteReader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], &'static str> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or("simulation frame length overflow")?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or("simulation frame is truncated")?;
        self.offset = end;
        Ok(value)
    }
    fn tag(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }
    fn boolean(&mut self) -> Result<bool, &'static str> {
        match self.tag()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("simulation frame boolean is invalid"),
        }
    }
    fn u16(&mut self) -> Result<u16, &'static str> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, &'static str> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, &'static str> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, &'static str> {
        Ok(f64::from_bits(self.u64()?))
    }
    fn option_f64(&mut self) -> Result<Option<f64>, &'static str> {
        match self.tag()? {
            0 => Ok(None),
            1 => Ok(Some(self.f64()?)),
            _ => Err("simulation frame option tag is invalid"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DriverInput, PreviewGroundTriangle, PreviewWallSegment, SimTiming, TrackRuntime};

    fn preview_collision() -> PreviewCollisionWorld {
        PreviewCollisionWorld {
            ground: vec![PreviewGroundTriangle {
                vertices: [[-20.0, -20.0, 0.0], [20.0, -20.0, 0.0], [-20.0, 20.0, 0.0]],
            }],
            walls: vec![PreviewWallSegment {
                start: Vec2::new(10.0, -20.0),
                end: Vec2::new(10.0, 20.0),
                min_z: -1.0,
                max_z: 2.0,
            }],
            vehicle_radius: 0.8,
            vehicle_min_z: -0.5,
            vehicle_max_z: 0.5,
            initial_ground_z: 0.0,
            units_per_sim_unit: 1.0,
            edge_rows: Vec::new(),
        }
    }

    fn preview_world() -> RaceWorld {
        let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        world
            .install_preview_collision(preview_collision())
            .unwrap();
        world.enable_shared_preview_handling();
        world
    }

    #[test]
    fn frame_binary_roundtrip_and_restore_replay_match_on_the_same_world() {
        let mut source = preview_world();
        let inputs = [
            DriverInput {
                throttle: 1.0,
                steer: 0.2,
                ..DriverInput::default()
            },
            DriverInput {
                throttle: 0.8,
                steer: -0.1,
                ..DriverInput::default()
            },
            DriverInput {
                brake: 0.1,
                steer: -0.4,
                handbrake: true,
                ..DriverInput::default()
            },
            DriverInput::default(),
        ];
        for input in inputs[..2].iter().copied() {
            source.step(input);
        }
        let frame = source.capture_state().unwrap();
        let encoded = frame.encode();
        assert_eq!(SimStateFrame::decode(&encoded).unwrap(), frame);

        let mut restored = preview_world();
        restored
            .restore_state(&SimStateFrame::decode(&encoded).unwrap())
            .unwrap();
        assert_eq!(restored.capture_state().unwrap(), frame);
        for input in inputs[2..].iter().copied() {
            source.step(input);
            restored.step(input);
            assert_eq!(
                restored.capture_state().unwrap(),
                source.capture_state().unwrap()
            );
        }
    }

    #[test]
    fn world_identity_binds_timing_track_collision_and_handling_configuration() {
        let baseline = preview_world().simulation_identity().unwrap();
        assert_eq!(SIMULATION_MODEL_REVISION, "provisional-raceworld-v1");
        assert_eq!(
            baseline
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            "1263b3f20782ae19787c7528d9a2c3a0cb34ea08340b0c269c196ef76770887f"
        );
        assert_eq!(baseline, preview_world().simulation_identity().unwrap());
        let mut changed_timing = preview_world();
        changed_timing.timing.physics_hz += 1;
        assert_ne!(baseline, changed_timing.simulation_identity().unwrap());

        let mut changed_collision = preview_world();
        changed_collision.preview_collision.as_mut().unwrap().walls[0].max_z += 1.0;
        assert_ne!(baseline, changed_collision.simulation_identity().unwrap());

        let mut changed_policy = preview_world();
        changed_policy
            .set_preview_grip_policy(crate::PreviewGripPolicy {
                lateral_recovery_per_second: 6.0,
                handbrake_recovery_per_second: 1.0,
            })
            .unwrap();
        assert_ne!(baseline, changed_policy.simulation_identity().unwrap());

        let mut changed_drive = preview_world();
        changed_drive
            .set_preview_drive_policy(crate::PreviewDrivePolicy {
                acceleration_per_second: 19.0,
                ..crate::PreviewDrivePolicy::default()
            })
            .unwrap();
        assert_ne!(baseline, changed_drive.simulation_identity().unwrap());

        let mut changed_cached_spawn = preview_world();
        changed_cached_spawn.spawn.x = 0.5;
        assert_ne!(
            baseline,
            changed_cached_spawn.simulation_identity().unwrap()
        );

        let mut circuit_a = RaceWorld::default();
        let identity_a = circuit_a.simulation_identity().unwrap();
        circuit_a.track.checkpoints[0].radius += 0.5;
        assert_ne!(identity_a, circuit_a.simulation_identity().unwrap());
        circuit_a.track.checkpoints[0].radius -= 0.5;
        circuit_a.track.laps_to_finish += 1;
        assert_ne!(identity_a, circuit_a.simulation_identity().unwrap());
    }

    #[test]
    fn invalid_frames_are_rejected_transactionally() {
        let mut world = preview_world();
        world.set_trace_recording(true);
        world.step(DriverInput {
            throttle: 1.0,
            ..DriverInput::default()
        });
        let baseline = world.clone();
        let valid = world.capture_state().unwrap();

        let mut cases = Vec::new();
        let mut unsupported = valid.clone();
        unsupported.version = SIM_STATE_FRAME_VERSION + 1;
        cases.push(unsupported);
        let mut wrong_world = valid.clone();
        wrong_world.world_identity[0] ^= 1;
        cases.push(wrong_world);
        let mut non_finite = valid.clone();
        non_finite.vehicle.velocity.x = f64::NAN;
        cases.push(non_finite);
        let mut partial_support = valid.clone();
        partial_support.preview_ground_normal = None;
        cases.push(partial_support);
        let mut invalid_checkpoint = valid.clone();
        invalid_checkpoint.vehicle.checkpoint = usize::MAX;
        cases.push(invalid_checkpoint);

        for frame in cases {
            assert!(world.restore_state(&frame).is_err());
            assert_eq!(world, baseline);
        }
    }

    #[test]
    fn frame_preserves_checkpoint_lap_and_finished_race_state() {
        let mut source = RaceWorld::default();
        source.tick = 21;
        source.vehicle.race_ticks = 18;
        source.vehicle.checkpoint = 2;
        source.vehicle.completed_laps = 1;
        let frame = source.capture_state().unwrap();
        let mut restored = RaceWorld::default();
        restored
            .restore_state(&SimStateFrame::decode(&frame.encode()).unwrap())
            .unwrap();
        assert_eq!(restored.capture_state().unwrap(), frame);

        source.vehicle.completed_laps = source.track.laps_to_finish;
        source.vehicle.checkpoint = 0;
        source.vehicle.finished = true;
        source.tick += 1;
        source.vehicle.race_ticks += 1;
        let finished = source.capture_state().unwrap();
        restored.restore_state(&finished).unwrap();
        restored.step(DriverInput {
            throttle: 1.0,
            ..DriverInput::default()
        });
        source.step(DriverInput {
            throttle: 1.0,
            ..DriverInput::default()
        });
        assert_eq!(
            restored.capture_state().unwrap(),
            source.capture_state().unwrap()
        );
    }

    #[test]
    fn decoded_lap_boundary_inconsistencies_are_rejected_without_mutation() {
        let mut world = RaceWorld::default();
        world.tick = 12;
        world.vehicle.race_ticks = 12;
        let baseline = world.clone();
        let target = world.track.laps_to_finish;

        let mut over_limit_finished = world.capture_state().unwrap();
        over_limit_finished.vehicle.completed_laps = target + 1;
        over_limit_finished.vehicle.finished = true;
        let mut target_but_unfinished = world.capture_state().unwrap();
        target_but_unfinished.vehicle.completed_laps = target;
        target_but_unfinished.vehicle.finished = false;
        let mut below_target_but_finished = world.capture_state().unwrap();
        below_target_but_finished.vehicle.completed_laps = target - 1;
        below_target_but_finished.vehicle.finished = true;

        for malformed in [
            over_limit_finished,
            target_but_unfinished,
            below_target_but_finished,
        ] {
            let decoded = SimStateFrame::decode(&malformed.encode()).unwrap();
            assert!(world.restore_state(&decoded).is_err());
            assert_eq!(world, baseline);
        }
    }

    #[test]
    fn binary_decoder_rejects_truncation_trailing_data_bad_tags_and_non_finite_values() {
        let frame = preview_world().capture_state().unwrap();
        let bytes = frame.encode();
        assert!(SimStateFrame::decode(&bytes[..bytes.len() - 1]).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(SimStateFrame::decode(&trailing).is_err());
        let mut bad_bool = bytes.clone();
        bad_bool[126] = 2;
        assert!(SimStateFrame::decode(&bad_bool).is_err());
        let mut non_finite = bytes;
        non_finite[50..58].copy_from_slice(&f64::NAN.to_bits().to_le_bytes());
        assert!(SimStateFrame::decode(&non_finite).is_err());
    }
}
