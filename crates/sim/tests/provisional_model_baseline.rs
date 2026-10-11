use racer_sim::{
    DriverInput, PreviewCollisionWorld, PreviewDrivePolicy, PreviewGripPolicy,
    PreviewGroundTriangle, PreviewWallSegment, RaceWorld, SimTiming, TrackRuntime, Vec2,
};
use serde_json::{json, Value};

fn snapshot(world: &RaceWorld) -> Value {
    let vehicle = world.vehicle;
    json!({
        "world_tick": world.tick,
        "position": [vehicle.position.x, vehicle.position.y, vehicle.position_z],
        "velocity": [vehicle.velocity.x, vehicle.velocity.y],
        "heading": vehicle.heading,
        "angular_velocity": vehicle.angular_velocity,
        "race_ticks": vehicle.race_ticks,
        "checkpoint": vehicle.checkpoint,
        "completed_laps": vehicle.completed_laps,
        "finished": vehicle.finished,
        "ground_normal": world.preview_ground_normal(),
    })
}

fn trace(mut world: RaceWorld, inputs: &[DriverInput]) -> Vec<Value> {
    inputs
        .iter()
        .map(|input| snapshot_after(&mut world, *input))
        .collect()
}

fn snapshot_after(world: &mut RaceWorld, input: DriverInput) -> Value {
    world.step(input);
    snapshot(world)
}

fn flat_collision(wall_x: f64) -> PreviewCollisionWorld {
    PreviewCollisionWorld {
        ground: vec![
            PreviewGroundTriangle {
                vertices: [[-10.0, -10.0, 0.0], [10.0, -10.0, 0.0], [10.0, 10.0, 0.0]],
            },
            PreviewGroundTriangle {
                vertices: [[-10.0, -10.0, 0.0], [10.0, 10.0, 0.0], [-10.0, 10.0, 0.0]],
            },
        ],
        walls: vec![PreviewWallSegment {
            start: Vec2::new(wall_x, -2.0),
            end: Vec2::new(wall_x, 2.0),
            min_z: -1.0,
            max_z: 2.0,
        }],
        vehicle_radius: 0.25,
        vehicle_min_z: 0.0,
        vehicle_max_z: 1.0,
        initial_ground_z: 0.0,
        units_per_sim_unit: 1.0,
        edge_rows: Vec::new(),
    }
}

fn inputs_to_json(inputs: &[DriverInput]) -> Value {
    Value::Array(
        inputs
            .iter()
            .map(|input| {
                json!([
                    input.throttle,
                    input.brake,
                    input.steer,
                    input.handbrake,
                    input.reset_request
                ])
            })
            .collect(),
    )
}

fn non_finite_input_descriptors(inputs: &[DriverInput]) -> Value {
    Value::Array(
        inputs
            .iter()
            .map(|input| {
                json!({
                    "throttle": if input.throttle.is_nan() { "NaN" } else if input.throttle.is_sign_positive() { "+Inf" } else { "-Inf" },
                    "brake": if input.brake.is_nan() { "NaN" } else if input.brake.is_sign_positive() { "+Inf" } else { "-Inf" },
                    "steer": if input.steer.is_nan() { "NaN" } else if input.steer.is_sign_positive() { "+Inf" } else { "-Inf" },
                    "handbrake": input.handbrake,
                })
            })
            .collect(),
    )
}

fn slope_collision() -> PreviewCollisionWorld {
    PreviewCollisionWorld {
        ground: vec![
            PreviewGroundTriangle {
                vertices: [[-10.0, -10.0, -1.0], [10.0, -10.0, 1.0], [10.0, 10.0, 1.0]],
            },
            PreviewGroundTriangle {
                vertices: [[-10.0, -10.0, -1.0], [10.0, 10.0, 1.0], [-10.0, 10.0, -1.0]],
            },
        ],
        walls: Vec::new(),
        vehicle_radius: 0.25,
        vehicle_min_z: 0.0,
        vehicle_max_z: 1.0,
        initial_ground_z: 0.0,
        units_per_sim_unit: 1.0,
        edge_rows: Vec::new(),
    }
}

fn create_vectors() -> Value {
    let generic_inputs = [
        DriverInput {
            throttle: 1.0,
            steer: 0.5,
            ..DriverInput::default()
        },
        DriverInput {
            throttle: 1.0,
            steer: 0.5,
            ..DriverInput::default()
        },
        DriverInput::default(),
        DriverInput {
            brake: 0.4,
            steer: -0.75,
            ..DriverInput::default()
        },
        DriverInput {
            throttle: 0.5,
            brake: 0.25,
            steer: 2.0,
            ..DriverInput::default()
        },
        DriverInput {
            throttle: -1.0,
            brake: 0.0,
            steer: -2.0,
            ..DriverInput::default()
        },
    ];
    let mut generic = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    generic.vehicle.heading = 0.3;
    generic.vehicle.velocity = Vec2::new(3.0, -1.0);
    let generic_initial = snapshot(&generic);
    let generic_trace = trace(generic, &generic_inputs);

    let reverse_shared_inputs = [DriverInput {
        steer: 0.5,
        ..DriverInput::default()
    }; 3];
    let mut reverse_shared = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    reverse_shared.enable_shared_preview_handling();
    reverse_shared.vehicle.heading = 0.4;
    reverse_shared.vehicle.velocity = Vec2::new(-8.0 * 0.4_f64.cos(), -8.0 * 0.4_f64.sin());
    let reverse_shared_initial = snapshot(&reverse_shared);
    let reverse_shared_trace = trace(reverse_shared, &reverse_shared_inputs);

    let shared_inputs = [
        DriverInput {
            throttle: 0.6,
            steer: 0.8,
            ..DriverInput::default()
        },
        DriverInput {
            throttle: 0.6,
            steer: 0.8,
            handbrake: true,
            ..DriverInput::default()
        },
        DriverInput {
            brake: 0.3,
            steer: -0.4,
            handbrake: true,
            ..DriverInput::default()
        },
        DriverInput::default(),
    ];
    let mut shared = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    shared.enable_shared_preview_handling();
    shared.vehicle.heading = 0.4;
    shared.vehicle.velocity = Vec2::new(8.0, 6.0);
    let shared_initial = snapshot(&shared);
    let shared_trace = trace(shared, &shared_inputs);

    let mut local = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    local
        .install_preview_collision(flat_collision(3.0))
        .unwrap();
    local.vehicle.velocity = Vec2::new(25.0, 2.0);
    local
        .set_preview_drive_policy(PreviewDrivePolicy {
            acceleration_per_second: 16.0,
            braking_per_second: 27.0,
            max_speed: 55.0,
        })
        .unwrap();
    local
        .set_preview_grip_policy(PreviewGripPolicy {
            lateral_recovery_per_second: 4.0,
            handbrake_recovery_per_second: 0.8,
        })
        .unwrap();
    let local_inputs = [DriverInput {
        throttle: 0.5,
        steer: 0.25,
        ..DriverInput::default()
    }; 10];
    let local_initial = snapshot(&local);
    let local_trace = trace(local, &local_inputs);

    let mut shared_collision = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    shared_collision.enable_shared_preview_handling();
    shared_collision
        .install_preview_collision(flat_collision(8.0))
        .unwrap();
    shared_collision.vehicle.heading = -0.2;
    shared_collision.vehicle.velocity = Vec2::new(10.0, -5.0);
    let shared_collision_inputs = [
        DriverInput {
            throttle: 0.4,
            steer: -0.6,
            handbrake: true,
            ..DriverInput::default()
        },
        DriverInput {
            throttle: 0.4,
            steer: -0.6,
            handbrake: true,
            ..DriverInput::default()
        },
        DriverInput {
            brake: 0.2,
            steer: 0.2,
            ..DriverInput::default()
        },
    ];
    let shared_collision_initial = snapshot(&shared_collision);
    let shared_collision_trace = trace(shared_collision, &shared_collision_inputs);

    let mut slope = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    slope.install_preview_collision(slope_collision()).unwrap();
    slope.vehicle.velocity = Vec2::new(15.0, 0.0);
    let slope_inputs = [DriverInput {
        throttle: 0.5,
        ..DriverInput::default()
    }; 8];
    let slope_initial = snapshot(&slope);
    let slope_trace = trace(slope, &slope_inputs);

    let mut speed_cap = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    speed_cap.vehicle.velocity = Vec2::new(44.9, 0.0);
    let speed_cap_input = DriverInput {
        throttle: 1.0,
        ..DriverInput::default()
    };
    let speed_cap_initial = snapshot(&speed_cap);
    let speed_cap_trace = trace(speed_cap, &[speed_cap_input]);

    let non_finite_inputs = [
        DriverInput {
            throttle: f64::NAN,
            brake: f64::INFINITY,
            steer: f64::NEG_INFINITY,
            ..DriverInput::default()
        },
        DriverInput {
            throttle: f64::INFINITY,
            brake: f64::NAN,
            steer: f64::INFINITY,
            ..DriverInput::default()
        },
    ];
    let mut non_finite = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    non_finite.vehicle.velocity = Vec2::new(2.0, -3.0);
    let non_finite_initial = snapshot(&non_finite);
    let non_finite_trace = trace(non_finite, &non_finite_inputs);

    let mut reset = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    reset.vehicle.position = Vec2::new(4.0, -2.0);
    reset.vehicle.position_z = 7.0;
    reset.vehicle.heading = 2.0;
    reset.vehicle.velocity = Vec2::new(5.0, 6.0);
    reset.vehicle.angular_velocity = 3.0;
    reset.vehicle.race_ticks = 17;
    let reset_input = DriverInput {
        throttle: 1.0,
        reset_request: true,
        ..DriverInput::default()
    };
    let reset_initial = snapshot(&reset);
    let reset_trace = trace(reset, &[reset_input]);

    let mut finished = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    finished.vehicle.position = Vec2::new(1.0, 2.0);
    finished.vehicle.velocity = Vec2::new(3.0, 4.0);
    finished.vehicle.heading = 0.75;
    finished.vehicle.angular_velocity = -1.5;
    finished.vehicle.race_ticks = 9;
    finished.vehicle.finished = true;
    let finished_input = DriverInput {
        throttle: 1.0,
        reset_request: true,
        ..DriverInput::default()
    };
    let finished_initial = snapshot(&finished);
    let finished_trace = trace(finished, &[finished_input]);

    json!({
        "metadata": {
            "schema": 1,
            "model": "current project provisional RaceWorld policy",
            "source": "project source revision 500ea0d before provisional model extraction (RaceWorld::step)",
            "timing": { "configuration": "SimTiming::default()", "configured_physics_hz": 60, "step_seconds": 0.016666666666666666, "scope": "project vector setup only; not claimed as a reference cadence" },
            "units": "project simulation units; physical units unknown",
            "confidence": "confirmed project behavior regression vector only; not reference behavior",
            "tolerance": 0.000000000001,
        },
        "cases": {
            "generic": { "policy": "generic drive: acceleration 12, braking 20, cap 45; no lateral recovery", "initial": generic_initial, "inputs": inputs_to_json(&generic_inputs), "outputs": generic_trace },
            "shared_reverse_body_forward_steering": { "policy": "shared preview default policy; negative body-forward speed", "initial": reverse_shared_initial, "inputs": inputs_to_json(&reverse_shared_inputs), "outputs": reverse_shared_trace },
            "shared_preview_without_collision": { "policy": "shared preview: default preview drive and grip policies", "initial": shared_initial, "inputs": inputs_to_json(&shared_inputs), "outputs": shared_trace },
            "collision_preview_custom_policy_and_wall": { "policy": "collision preview: custom drive 16/27/55 and grip 4/0.8 per second; flat synthetic ground and wall", "initial": local_initial, "inputs": inputs_to_json(&local_inputs), "wall_x": 3.0, "outputs": local_trace },
            "shared_with_collision_policy_precedence": { "policy": "shared flag plus collision: shared preview policy selected once; default preview drive and grip", "initial": shared_collision_initial, "inputs": inputs_to_json(&shared_collision_inputs), "wall_x": 8.0, "outputs": shared_collision_trace },
            "collision_preview_sloped_support": { "policy": "collision preview default policy; synthetic plane z=0.1*x", "initial": slope_initial, "inputs": inputs_to_json(&slope_inputs), "outputs": slope_trace },
            "generic_speed_cap_transition": { "policy": "generic drive: acceleration 12, braking 20, cap 45; initial forward speed 44.9", "initial": speed_cap_initial, "inputs": inputs_to_json(&[speed_cap_input]), "outputs": speed_cap_trace },
            "non_finite_input_normalization": { "policy": "generic drive; RaceWorld finite_clamp maps non-finite throttle/brake/steer to zero", "initial": non_finite_initial, "input_descriptors": non_finite_input_descriptors(&non_finite_inputs), "outputs": non_finite_trace },
            "reset_caller_invariant": { "policy": "reset branch; kinematics bypassed", "initial": reset_initial, "inputs": inputs_to_json(&[reset_input]), "outputs": reset_trace },
            "finished_caller_invariant": { "policy": "finished early-return; reset and kinematics bypassed", "initial": finished_initial, "inputs": inputs_to_json(&[finished_input]), "outputs": finished_trace },
        }
    })
}

fn assert_json_close(expected: &Value, actual: &Value, tolerance: f64, path: &str) {
    match (expected, actual) {
        (Value::Number(expected), Value::Number(actual)) => {
            let expected = expected.as_f64().unwrap();
            let actual = actual.as_f64().unwrap();
            assert!(
                (expected - actual).abs() <= tolerance,
                "{path}: expected {expected:?}, got {actual:?} (tolerance {tolerance})"
            );
        }
        (Value::Array(expected), Value::Array(actual)) => {
            assert_eq!(expected.len(), actual.len(), "{path}: array length differs");
            for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
                assert_json_close(expected, actual, tolerance, &format!("{path}[{index}]"));
            }
        }
        (Value::Object(expected), Value::Object(actual)) => {
            assert_eq!(
                expected.keys().collect::<Vec<_>>(),
                actual.keys().collect::<Vec<_>>(),
                "{path}: object keys differ"
            );
            for (key, expected) in expected {
                assert_json_close(
                    expected,
                    actual.get(key).unwrap(),
                    tolerance,
                    &format!("{path}.{key}"),
                );
            }
        }
        _ => assert_eq!(expected, actual, "{path}"),
    }
}

#[test]
fn current_provisional_model_matches_pre_extraction_vectors() {
    let expected: Value = serde_json::from_str(include_str!(
        "../../../tests/reference/project-provisional-model-baseline-v0.json"
    ))
    .unwrap();
    let actual = create_vectors();
    let tolerance = expected["metadata"]["tolerance"].as_f64().unwrap();
    assert_json_close(&expected, &actual, tolerance, "root");
}
