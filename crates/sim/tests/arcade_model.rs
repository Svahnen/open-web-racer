//! Invariant tests for the generic `VehicleModel::Arcade` model.

use racer_sim::drivetrain::{GEAR_CODE_NEUTRAL, GEAR_CODE_REVERSE};
use racer_sim::Transmission;
use racer_sim::{
    ArcadeConfig, DriverInput, RaceWorld, SimStateFrame, SimTiming, TrackRuntime, VehicleModel,
};

fn world_with(hz: u32, config: ArcadeConfig) -> RaceWorld {
    let timing = SimTiming {
        physics_hz: hz,
        input_hz: hz,
        race_clock_hz: hz,
    };
    let mut world = RaceWorld::new(timing, TrackRuntime::open_preview());
    world.use_arcade_model(config).unwrap();
    world
}

fn world(hz: u32) -> RaceWorld {
    world_with(hz, ArcadeConfig::default())
}

fn input(throttle: f64, brake: f64, steer: f64) -> DriverInput {
    DriverInput {
        throttle,
        brake,
        steer,
        ..DriverInput::default()
    }
}

fn forward_speed(w: &RaceWorld) -> f64 {
    let v = &w.vehicle;
    v.velocity.x * v.heading.cos() + v.velocity.y * v.heading.sin()
}

/// Body heading minus travel direction, radians (positive: body points
/// counter-clockwise of the travel direction).
fn lead(w: &RaceWorld) -> f64 {
    let v = &w.vehicle;
    let travel = v.velocity.y.atan2(v.velocity.x);
    let mut d = v.heading - travel;
    while d > std::f64::consts::PI {
        d -= std::f64::consts::TAU;
    }
    while d < -std::f64::consts::PI {
        d += std::f64::consts::TAU;
    }
    d
}

fn run(w: &mut RaceWorld, i: DriverInput, seconds: f64) {
    let steps = (seconds * f64::from(w.timing.physics_hz)).round() as u32;
    for _ in 0..steps {
        w.step(i);
    }
}

#[test]
fn requires_parameters_and_validates_them() {
    let mut w = RaceWorld::default();
    assert!(w.set_vehicle_model(VehicleModel::Arcade).is_err());
    let mut bad = ArcadeConfig::default();
    bad.redline_rpm = bad.idle_rpm;
    assert!(w.install_arcade_parameters(bad).is_err());
    let mut bad = ArcadeConfig::default();
    bad.gear_ratios = vec![1.0, 2.0];
    assert!(w.install_arcade_parameters(bad).is_err());
    w.use_arcade_model(ArcadeConfig::default()).unwrap();
    assert_eq!(w.vehicle_model(), VehicleModel::Arcade);
}

#[test]
fn identical_inputs_give_bit_identical_runs_and_rollback_resumes_exactly() {
    let script = |w: &mut RaceWorld, from: u32, to: u32| {
        for t in from..to {
            let steer = if (t / 40) % 2 == 0 { 0.8 } else { -0.6 };
            let brake = if t % 300 > 250 { 1.0 } else { 0.0 };
            w.step(DriverInput {
                throttle: 1.0,
                brake,
                steer,
                handbrake: t % 500 > 470,
                ..DriverInput::default()
            });
        }
    };
    let mut a = world(32);
    let mut b = world(32);
    script(&mut a, 0, 900);
    script(&mut b, 0, 900);
    assert_eq!(
        a.vehicle.position.x.to_bits(),
        b.vehicle.position.x.to_bits()
    );
    assert_eq!(
        a.vehicle.position.y.to_bits(),
        b.vehicle.position.y.to_bits()
    );
    assert_eq!(a.vehicle.heading.to_bits(), b.vehicle.heading.to_bits());
    assert_eq!(a.drive_state(), b.drive_state());

    // Capture mid-run, continue, restore, replay: the result is identical.
    let mut c = world(32);
    script(&mut c, 0, 400);
    let frame = c.capture_state().unwrap();
    let decoded = SimStateFrame::decode(&frame.encode()).unwrap();
    script(&mut c, 400, 900);
    let mut d = world(32);
    d.restore_state(&decoded).unwrap();
    script(&mut d, 400, 900);
    assert_eq!(
        c.vehicle.position.x.to_bits(),
        d.vehicle.position.x.to_bits()
    );
    assert_eq!(c.vehicle.heading.to_bits(), d.vehicle.heading.to_bits());
    assert_eq!(c.drive_state(), d.drive_state());
}

#[test]
fn world_identity_binds_the_parameters() {
    let a = world(32).simulation_identity().unwrap();
    let mut cfg = ArcadeConfig::default();
    cfg.mass_kg += 1.0;
    let b = world_with(32, cfg).simulation_identity().unwrap();
    assert_ne!(a, b);
    assert_eq!(a, world(32).simulation_identity().unwrap());
}

#[test]
fn reaches_the_declared_top_speed_and_never_exceeds_it() {
    for hz in [32, 60] {
        let cfg = ArcadeConfig::default();
        let top = cfg.max_speed_mps;
        let mut w = world_with(hz, cfg);
        let mut peak = 0.0f64;
        let mut reached_at = None;
        for tick in 0..(60 * hz) {
            w.step(input(1.0, 0.0, 0.0));
            let s = forward_speed(&w);
            peak = peak.max(s);
            if reached_at.is_none() && s >= 0.97 * top {
                reached_at = Some(f64::from(tick) / f64::from(hz));
            }
        }
        assert!(peak <= top + 1e-9, "hz {hz}: peak {peak} above {top}");
        let t = reached_at.unwrap_or_else(|| panic!("hz {hz}: never reached 97% of {top}"));
        assert!(t < 45.0, "hz {hz}: took {t} s");
        assert!(forward_speed(&w) > 0.97 * top);
    }
}

#[test]
fn a_lower_declared_top_speed_caps_the_car() {
    let mut cfg = ArcadeConfig::default();
    cfg.max_speed_mps = 30.0;
    let mut w = world_with(32, cfg);
    run(&mut w, input(1.0, 0.0, 0.0), 40.0);
    let s = forward_speed(&w);
    assert!((28.0..=30.0 + 1e-9).contains(&s), "{s}");
}

#[test]
fn accelerates_through_the_gears_with_sane_pace() {
    let mut w = world(32);
    let mut gears = std::collections::BTreeSet::new();
    let mut last = 0.0;
    let mut t_100kmh = None;
    for tick in 0..(20 * 32) {
        w.step(input(1.0, 0.0, 0.0));
        let s = forward_speed(&w);
        assert!(s >= last - 0.2, "tick {tick}: speed fell {last} -> {s}");
        last = s;
        gears.insert(w.drive_telemetry().gear);
        if t_100kmh.is_none() && s >= 27.8 {
            t_100kmh = Some(f64::from(tick) / 32.0);
        }
    }
    assert!(gears.len() >= 4, "{gears:?}");
    let t = t_100kmh.expect("reaches 100 km/h");
    assert!((3.0..12.0).contains(&t), "0-100 km/h in {t} s");
    let tel = w.drive_telemetry();
    assert!(tel.rpm01 > 0.0 && tel.rpm01 <= 1.0);
}

#[test]
fn braking_distance_is_sane_and_scales_with_speed() {
    let stop = |speed: f64| {
        let mut w = world(32);
        // Build the speed with the engine, hold it, then brake.
        while forward_speed(&w) < speed {
            w.step(input(1.0, 0.0, 0.0));
        }
        let start = w.vehicle.position;
        let mut steps = 0;
        while forward_speed(&w) > 0.05 && steps < 32 * 30 {
            w.step(input(0.0, 1.0, 0.0));
            steps += 1;
        }
        let d = ((w.vehicle.position.x - start.x).powi(2)
            + (w.vehicle.position.y - start.y).powi(2))
        .sqrt();
        (d, f64::from(steps) / 32.0)
    };
    let (d30, t30) = stop(30.0);
    // v^2 / 2a with a between 0.5 g and 1.2 g, plus a short pedal ramp.
    assert!((30.0..95.0).contains(&d30), "30 m/s stop took {d30} m");
    assert!(t30 < 8.0, "{t30}");
    let (d15, _) = stop(15.0);
    assert!(d15 < d30 * 0.45, "{d15} vs {d30}");
    // The car never rolls backwards under the brakes.
    let mut w = world(32);
    run(&mut w, input(1.0, 0.0, 0.0), 3.0);
    for _ in 0..(32 * 12) {
        w.step(input(0.0, 1.0, 0.0));
        assert!(forward_speed(&w) >= -1e-9);
    }
}

#[test]
fn steering_leads_the_travel_direction_and_the_lead_decays_after_release() {
    let mut w = world(32);
    run(&mut w, input(1.0, 0.0, 0.0), 6.0);
    // Hold throttle at a steady speed with the key down.
    let speed = forward_speed(&w);
    assert!(speed > 15.0);
    let mut peak = 0.0f64;
    for _ in 0..(32 * 2) {
        w.step(input(0.4, 0.0, 1.0));
        peak = peak.max(lead(&w));
    }
    // Counter-clockwise steering: the body points counter-clockwise of travel
    // by a visible, bounded amount (about 1.5 to 20 degrees).
    assert!(peak > 0.03, "peak lead {peak}");
    assert!(peak < 0.35, "peak lead {peak}");
    let held = lead(&w);
    assert!(held > 0.02, "lead while held {held}");
    assert!(w.vehicle.angular_velocity > 0.1);
    // Release: the lead settles.
    let mut prev = held;
    for i in 0..(32 * 2) {
        w.step(input(0.4, 0.0, 0.0));
        if i == 32 / 2 {
            assert!(lead(&w) < held * 0.35, "lead after 0.5 s: {}", lead(&w));
        }
        prev = lead(&w);
    }
    assert!(prev.abs() < held * 0.05, "lead after 2 s: {prev}");
    assert!(w.vehicle.angular_velocity.abs() < 0.02);
    // Mirror image to the right.
    let mut r = world(32);
    run(&mut r, input(1.0, 0.0, 0.0), 6.0);
    run(&mut r, input(0.4, 0.0, -1.0), 2.0);
    assert!(lead(&r) < -0.02);
}

#[test]
fn steering_is_speed_sensitive_and_grip_limited() {
    let turn_rate = |speed: f64| {
        let mut w = world(32);
        while forward_speed(&w) < speed {
            w.step(input(1.0, 0.0, 0.0));
        }
        run(&mut w, input(0.0, 0.0, 1.0), 1.0);
        w.vehicle.angular_velocity
    };
    let slow = turn_rate(8.0);
    let mid = turn_rate(25.0);
    let fast = turn_rate(50.0);
    assert!(
        slow > mid && mid > fast && fast > 0.0,
        "{slow} {mid} {fast}"
    );
    // Lateral acceleration never beats the tyre limit (with downforce slack).
    for (speed, rate) in [(8.0, slow), (25.0, mid), (50.0, fast)] {
        assert!(speed * rate < 3.2 * 9.81, "{speed}: {}", speed * rate);
    }
    // No steering at a standstill.
    let mut w = world(32);
    run(&mut w, input(0.0, 0.0, 1.0), 1.0);
    assert!(w.vehicle.angular_velocity.abs() < 1e-9);
}

#[test]
fn handbrake_with_steering_slides_more_than_steering_alone() {
    let slide = |handbrake: bool| {
        let mut w = world(32);
        run(&mut w, input(1.0, 0.0, 0.0), 6.0);
        let mut peak = 0.0f64;
        for _ in 0..(32 * 2) {
            w.step(DriverInput {
                throttle: 0.5,
                steer: 1.0,
                handbrake,
                ..DriverInput::default()
            });
            peak = peak.max(lead(&w).abs());
        }
        peak
    };
    let normal = slide(false);
    let drift = slide(true);
    assert!(drift > normal * 1.5, "{drift} vs {normal}");
    assert!(drift < 1.4, "{drift}");
}

#[test]
fn reverse_works_through_the_gear_lanes() {
    let mut w = world(32);
    assert_eq!(w.drive_telemetry().gear, 1);
    // Two shift-downs at standstill select neutral, then reverse (edge triggered).
    for _ in 0..2 {
        w.step(DriverInput {
            shift_down: true,
            ..DriverInput::default()
        });
        w.step(DriverInput::default());
    }
    assert_eq!(w.drive_telemetry().gear, GEAR_CODE_REVERSE);
    run(&mut w, input(1.0, 0.0, 0.0), 3.0);
    let s = forward_speed(&w);
    assert!(s < -3.0, "reverse speed {s}");
    assert!(s > -20.0, "reverse speed {s}");
    // Reverse is slower than forward top speed and capped.
    run(&mut w, input(1.0, 0.0, 0.0), 20.0);
    let top = forward_speed(&w);
    assert!(top > -0.3 * ArcadeConfig::default().max_speed_mps, "{top}");
    // Reversing with the wheel turned counter-clockwise swings the nose clockwise.
    let heading_before = w.vehicle.heading;
    run(&mut w, input(0.3, 0.0, 1.0), 1.0);
    assert!(w.vehicle.angular_velocity < -0.05);
    let _ = heading_before;
    // Brake to a stop, then back to neutral and drive.
    run(&mut w, input(0.0, 1.0, 0.0), 8.0);
    assert!(forward_speed(&w).abs() < 0.05);
    w.step(DriverInput {
        shift_up: true,
        ..DriverInput::default()
    });
    w.step(DriverInput::default());
    assert_eq!(w.drive_telemetry().gear, GEAR_CODE_NEUTRAL);
    w.step(DriverInput {
        shift_up: true,
        ..DriverInput::default()
    });
    w.step(DriverInput::default());
    run(&mut w, input(1.0, 0.0, 0.0), 3.0);
    assert!(forward_speed(&w) > 5.0);
}

#[test]
fn throttle_at_the_start_neutral_selects_drive_and_moves_forward() {
    let mut w = world(32);
    run(&mut w, input(1.0, 0.0, 0.0), 2.0);
    assert!(forward_speed(&w) > 5.0);
}

#[test]
fn two_cars_with_different_parameters_differ() {
    let mut light = ArcadeConfig::default();
    light.mass_kg = 800.0;
    light.max_speed_mps = 50.0;
    let mut a = world_with(32, light);
    let mut b = world(32);
    run(&mut a, input(1.0, 0.0, 0.0), 3.0);
    run(&mut b, input(1.0, 0.0, 0.0), 3.0);
    assert!(forward_speed(&a) > forward_speed(&b));
}

// ---- manual gearbox ----

fn manual_world(hz: u32) -> RaceWorld {
    let mut w = world(hz);
    w.set_transmission(Transmission::Manual);
    w
}

fn tap(w: &mut RaceWorld, up: bool, down: bool) {
    w.step(DriverInput {
        shift_up: up,
        shift_down: down,
        ..DriverInput::default()
    });
    w.step(DriverInput::default());
}

fn shown(w: &RaceWorld) -> String {
    match w.drive_telemetry().gear {
        GEAR_CODE_REVERSE => "R".into(),
        GEAR_CODE_NEUTRAL => "N".into(),
        g => g.to_string(),
    }
}

#[test]
fn manual_box_starts_in_neutral_and_throttle_alone_does_not_move_the_car() {
    let mut w = manual_world(32);
    assert_eq!(shown(&w), "N");
    run(&mut w, input(1.0, 0.0, 0.0), 3.0);
    assert!(forward_speed(&w).abs() < 1e-9);
    assert_eq!(shown(&w), "N");
}

#[test]
fn manual_sequence_walks_n_1_to_top_and_back_down_to_r_with_clamped_ends() {
    let count = ArcadeConfig::default().gear_count();
    let mut w = manual_world(32);
    let mut up = Vec::new();
    for _ in 0..count + 2 {
        tap(&mut w, true, false);
        up.push(shown(&w));
    }
    let mut expect: Vec<String> = (1..=count).map(|g| g.to_string()).collect();
    expect.extend([count.to_string(), count.to_string()]);
    assert_eq!(up, expect);
    let mut down = Vec::new();
    for _ in 0..count + 3 {
        tap(&mut w, false, true);
        down.push(shown(&w));
    }
    let mut expect: Vec<String> = (1..count).rev().map(|g| g.to_string()).collect();
    expect.extend(["N", "R", "R", "R"].map(String::from));
    assert_eq!(down, expect);
}

#[test]
fn manual_car_only_reaches_speed_by_shifting_up_and_holds_the_rev_limit() {
    let cfg = ArcadeConfig::default();
    let mut w = manual_world(32);
    tap(&mut w, true, false);
    assert_eq!(shown(&w), "1");
    run(&mut w, input(1.0, 0.0, 0.0), 12.0);
    let first_gear_cap = forward_speed(&w);
    // Held in first gear the speed locks just below the redline speed of the gear.
    let redline_speed = cfg.redline_rpm * cfg.wheel_radius_m * std::f64::consts::TAU
        / 60.0
        / (cfg.gear_ratios[0] * cfg.final_drive);
    assert!(
        first_gear_cap > 0.9 * redline_speed && first_gear_cap <= redline_speed,
        "{first_gear_cap} vs {redline_speed}"
    );
    // No limiter cycling: the speed is steady for seconds.
    let mut lo = f64::MAX;
    let mut hi = f64::MIN;
    for _ in 0..(5 * 32) {
        w.step(input(1.0, 0.0, 0.0));
        let s = forward_speed(&w);
        lo = lo.min(s);
        hi = hi.max(s);
    }
    assert!(hi - lo < 0.05, "limiter bounces: {lo}..{hi}");
    // Shifting through the gears reaches a much higher speed.
    for _ in 1..cfg.gear_count() {
        tap(&mut w, true, false);
        run(&mut w, input(1.0, 0.0, 0.0), 10.0);
    }
    assert!(forward_speed(&w) > 2.0 * first_gear_cap);
}

#[test]
fn manual_shift_cuts_the_thrust_for_a_short_time() {
    let mut w = manual_world(32);
    tap(&mut w, true, false);
    run(&mut w, input(1.0, 0.0, 0.0), 2.0);
    let before = forward_speed(&w);
    w.step(DriverInput {
        shift_up: true,
        throttle: 1.0,
        ..DriverInput::default()
    });
    // During the pause the speed does not rise (drag only).
    let mut worst_gain = f64::MIN;
    let mut last = forward_speed(&w);
    for _ in 0..4 {
        w.step(input(1.0, 0.0, 0.0));
        let s = forward_speed(&w);
        worst_gain = worst_gain.max(s - last);
        last = s;
    }
    assert!(
        worst_gain <= 1e-9,
        "accelerated during the shift: {worst_gain}"
    );
    assert!(last <= before + 1e-9);
    run(&mut w, input(1.0, 0.0, 0.0), 1.5);
    assert!(forward_speed(&w) > before, "thrust returns after the pause");
}

#[test]
fn manual_downshift_at_speed_is_accepted_and_engine_braking_pulls_the_car_to_the_gear() {
    let mut w = manual_world(32);
    for _ in 0..3 {
        tap(&mut w, true, false);
        run(&mut w, input(1.0, 0.0, 0.0), 8.0);
    }
    assert_eq!(shown(&w), "3");
    let v0 = forward_speed(&w);
    for _ in 0..2 {
        tap(&mut w, false, true);
    }
    assert_eq!(shown(&w), "1");
    let rpm = w.drive_telemetry().rpm01;
    assert!(rpm >= 0.99, "over-rev shows the redline: {rpm}");
    run(&mut w, input(0.0, 0.0, 0.0), 1.0);
    assert!(forward_speed(&w) < v0 - 2.0);
}

#[test]
fn reverse_is_accepted_at_speed_in_both_modes_and_brakes_hard_through_zero() {
    for manual in [false, true] {
        let mut w = world(32);
        if manual {
            w.set_transmission(Transmission::Manual);
            tap(&mut w, true, false);
        }
        run(&mut w, input(1.0, 0.0, 0.0), 6.0);
        let v0 = forward_speed(&w);
        assert!(v0 > 15.0, "manual {manual}: {v0}");
        for _ in 0..10 {
            tap(&mut w, false, true);
        }
        assert_eq!(shown(&w), "R", "manual {manual}");
        // Nothing held: the car is braked by the reverse gear.
        let before = forward_speed(&w);
        run(&mut w, input(0.0, 0.0, 0.0), 1.0);
        let after = forward_speed(&w);
        assert!(
            before - after > 8.0,
            "manual {manual}: braked only {} m/s in 1 s",
            before - after
        );
        // With the throttle held it passes zero and reverses.
        run(&mut w, input(1.0, 0.0, 0.0), 8.0);
        assert!(forward_speed(&w) < -2.0, "manual {manual}");
    }
}

#[test]
fn transmission_is_bound_into_the_world_identity_and_the_state_frame() {
    let auto = world(32);
    let manual = manual_world(32);
    assert_ne!(
        auto.simulation_identity().unwrap(),
        manual.simulation_identity().unwrap()
    );
    let mut a = manual_world(32);
    tap(&mut a, true, false);
    run(&mut a, input(1.0, 0.0, 0.0), 1.0);
    let frame = a.capture_state().unwrap();
    assert!(frame.drive.manual);
    let decoded = SimStateFrame::decode(&frame.encode()).unwrap();
    assert_eq!(decoded, frame);
    let mut b = manual_world(32);
    b.restore_state(&decoded).unwrap();
    for _ in 0..64 {
        a.step(input(1.0, 0.0, 0.2));
        b.step(input(1.0, 0.0, 0.2));
    }
    assert_eq!(a.vehicle, b.vehicle);
    assert_eq!(a.drive_state(), b.drive_state());
}
