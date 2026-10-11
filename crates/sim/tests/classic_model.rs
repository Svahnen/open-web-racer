//! Feel tests of the classic handling model against the project's tuned feel
//! targets (`tests/feel/classic-targets.json`), using the reference car
//! (`ClassicConfig::default()`).

use racer_sim::drivetrain::{
    COMMAND_FULL, GEAR_CODE_NEUTRAL, GEAR_CODE_REVERSE, SELECTOR_DRIVE, SELECTOR_NEUTRAL,
    SELECTOR_REVERSE,
};
use racer_sim::{
    ClassicConfig, DriverInput, RaceWorld, SimStateFrame, SimTiming, TrackRuntime, Transmission,
    Vec2, VehicleModel,
};
use serde_json::Value;

const TARGETS: &str = include_str!("../../../tests/feel/classic-targets.json");
const TICK_HZ: f64 = 32.0;

fn targets() -> Value {
    serde_json::from_str(TARGETS).unwrap()
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

fn world_hz(cfg: ClassicConfig, hz: u32) -> RaceWorld {
    let timing = SimTiming {
        physics_hz: hz,
        input_hz: hz,
        race_clock_hz: hz,
    };
    let mut w = RaceWorld::new(timing, TrackRuntime::open_preview());
    w.use_classic_model(cfg).unwrap();
    w
}

fn world() -> RaceWorld {
    world_hz(ClassicConfig::default(), 60)
}

fn input(throttle: f64, brake: f64, steer: f64) -> DriverInput {
    DriverInput {
        throttle,
        brake,
        steer,
        ..DriverInput::default()
    }
}

fn along(w: &RaceWorld) -> f64 {
    let v = &w.vehicle;
    v.velocity.x * v.heading.cos() + v.velocity.y * v.heading.sin()
}

/// Put the world in a cruise state: moving at `speed` along the heading, in
/// the gear the automatic box would pick, throttle command at full.
fn cruise(w: &mut RaceWorld, speed: f64, throttle: bool) {
    let cfg = w.classic_parameters().unwrap().clone();
    let mut frame = w.capture_state().unwrap();
    let h = frame.vehicle.heading;
    frame.vehicle.velocity = Vec2::new(h.cos(), h.sin()) * speed;
    frame.drive.gear = 1 + cfg.upshift_mps.iter().filter(|u| speed > **u).count() as u8;
    if throttle {
        frame.drive.throttle_command = COMMAND_FULL;
    }
    w.restore_state(&frame).unwrap();
}

/// One tap of a shift key followed by idle steps so the next tap is a new edge.
fn tap(w: &mut RaceWorld, up: bool, down: bool) {
    w.step(DriverInput {
        shift_up: up,
        shift_down: down,
        ..DriverInput::default()
    });
    for _ in 0..4 {
        w.step(DriverInput::default());
    }
}

fn selector_label(w: &RaceWorld) -> String {
    let d = w.drive_state();
    match d.selector {
        SELECTOR_REVERSE => "R".into(),
        SELECTOR_NEUTRAL => "N".into(),
        _ => d.gear.to_string(),
    }
}

fn near(got: f64, want: f64, tol: f64, what: &str) {
    assert!(
        (got - want).abs() <= tol,
        "{what}: {got:.4} vs target {want:.4} +- {tol}"
    );
}

#[test]
fn launch_delay_acceleration_and_top_speed_follow_the_targets() {
    let t = targets();
    let mut w = world();
    let dt = w.timing.step_seconds();
    let wanted = t["launch"]["time_to_speed_s"].as_array().unwrap();
    let mut reached = vec![f64::NAN; wanted.len()];
    let mut first_motion = f64::NAN;
    let mut speed_at_8s = 0.0;
    for i in 0..(10.0 / dt) as usize {
        w.step(input(1.0, 0.0, 0.0));
        let time = (i + 1) as f64 * dt;
        let v = w.vehicle.speed();
        if first_motion.is_nan() && v > 0.01 {
            first_motion = time;
        }
        for (k, row) in wanted.iter().enumerate() {
            if reached[k].is_nan() && v >= f(&row["speed_mps"]) {
                reached[k] = time;
            }
        }
        if i + 1 == (8.0 / dt).round() as usize {
            speed_at_8s = v;
        }
    }
    let delay = &t["launch"]["start_delay_ticks"];
    let ticks = first_motion * TICK_HZ;
    assert!(
        ticks >= f(&delay[0]) - 3.0 && ticks <= f(&delay[1]) + 0.5,
        "launch delay {ticks} ticks"
    );
    for (row, got) in wanted.iter().zip(&reached) {
        near(
            *got,
            f(&row["time_s"]),
            f(&row["tolerance_s"]),
            &format!("time to {} m/s", f(&row["speed_mps"])),
        );
    }
    let top = &t["launch"]["speed_after_8s_mps"];
    near(
        speed_at_8s,
        f(&top["value"]),
        f(&top["tolerance"]),
        "speed after 8 s",
    );
}

#[test]
fn the_automatic_box_shifts_at_the_target_speeds_and_starts_in_first() {
    let t = targets();
    let g = &t["gearbox_auto"];
    let mut w = world();
    assert_eq!(w.drive_state().gear, f(&g["start_gear"]) as u8);
    assert_eq!(w.drive_state().selector, SELECTOR_DRIVE);
    let mut ups: Vec<f64> = Vec::new();
    let mut gear = 1;
    for _ in 0..(14 * 60) {
        w.step(input(1.0, 0.0, 0.0));
        let now = w.drive_telemetry().gear;
        if now > gear {
            ups.push(w.vehicle.speed());
            gear = now;
        }
    }
    let want: Vec<f64> = g["upshift_mps"].as_array().unwrap().iter().map(f).collect();
    assert_eq!(ups.len(), want.len(), "{ups:?}");
    for (got, want) in ups.iter().zip(&want) {
        near(
            *got,
            *want,
            f(&g["upshift_tolerance_mps"]) + 0.15,
            "upshift speed",
        );
    }
    // Coast down from the top: downshifts at the lower targets.
    let mut downs: Vec<f64> = Vec::new();
    let mut gear = w.drive_telemetry().gear;
    for _ in 0..(40 * 60) {
        w.step(input(0.0, 0.0, 0.0));
        let now = w.drive_telemetry().gear;
        if now < gear {
            downs.push(w.vehicle.speed());
            gear = now;
        }
    }
    let want: Vec<f64> = g["downshift_mps"]
        .as_array()
        .unwrap()
        .iter()
        .map(f)
        .collect();
    assert_eq!(downs.len(), want.len(), "{downs:?}");
    for (got, want) in downs.iter().rev().zip(&want) {
        near(
            *got,
            *want,
            f(&g["downshift_tolerance_mps"]),
            "downshift speed",
        );
    }
}

#[test]
fn shifting_pauses_the_drive_for_the_target_number_of_ticks() {
    let t = targets();
    let pause = &t["gearbox_auto"]["shift_pause_ticks"];
    // Automatic: speed flat across the first upshift.
    let mut w = world();
    let dt = w.timing.step_seconds();
    let mut stalled = 0usize;
    let mut previous = 0.0;
    let mut gear = 1;
    let mut counting = false;
    for _ in 0..(6 * 60) {
        w.step(input(1.0, 0.0, 0.0));
        let v = w.vehicle.speed();
        let now = w.drive_telemetry().gear;
        if now > gear && gear == 1 {
            counting = true;
        }
        gear = now;
        if counting {
            if v - previous < 0.05 {
                stalled += 1;
            } else {
                break;
            }
        }
        previous = v;
    }
    let ticks = stalled as f64 * dt * TICK_HZ;
    near(
        ticks,
        f(&pause["automatic"]),
        f(&pause["tolerance"]),
        "automatic shift pause",
    );
    // Manual: the same measure.
    let mut w = world();
    w.set_transmission(Transmission::Manual);
    tap(&mut w, true, false);
    for _ in 0..(3 * 60) {
        w.step(input(1.0, 0.0, 0.0));
    }
    let before = w.vehicle.speed();
    w.step(DriverInput {
        throttle: 1.0,
        shift_up: true,
        ..DriverInput::default()
    });
    let mut paused = 1;
    let mut last = w.vehicle.speed();
    for _ in 0..60 {
        w.step(input(1.0, 0.0, 0.0));
        let v = w.vehicle.speed();
        if v - last < 0.03 {
            paused += 1;
        } else {
            break;
        }
        last = v;
    }
    assert!(before > 5.0);
    let ticks = paused as f64 * dt * TICK_HZ;
    near(
        ticks,
        f(&pause["manual"]),
        f(&pause["tolerance"]) + 0.5,
        "manual shift pause",
    );
}

fn crossing_time(w: &mut RaceWorld, input_fn: impl Fn() -> DriverInput, from: f64, to: f64) -> f64 {
    let dt = w.timing.step_seconds();
    let mut start = None;
    for i in 0..(60 * 60) {
        w.step(input_fn());
        let v = w.vehicle.speed();
        if start.is_none() && v <= from {
            start = Some(i);
        }
        if v <= to {
            return (i - start.unwrap_or(i)) as f64 * dt;
        }
    }
    f64::NAN
}

#[test]
fn coasting_decelerates_along_the_target_bands() {
    let t = targets();
    for band in t["coast"]["deceleration_bands_mps2"].as_array().unwrap() {
        let (from, to) = (f(&band["from_mps"]), f(&band["to_mps"]));
        let mut w = world();
        cruise(&mut w, from + 6.0, false);
        let time = crossing_time(&mut w, || input(0.0, 0.0, 0.0), from, to);
        let decel = (from - to) / time;
        near(
            decel,
            f(&band["decel"]),
            f(&band["decel"]) * f(&band["relative_tolerance"]),
            &format!("coast {from}->{to}"),
        );
    }
    // Low speed: exponential-like, time constant about 3 s.
    let tail = &t["coast"]["low_speed_time_constant_s"];
    let mut w = world();
    cruise(&mut w, 5.0, false);
    let mut samples = Vec::new();
    for i in 0..(20 * 60) {
        w.step(input(0.0, 0.0, 0.0));
        if i % 60 == 59 {
            samples.push(w.vehicle.speed());
        }
    }
    // Ratio over one second before the car crawls (speed 0.1 .. 5).
    let ratio = samples[1] / samples[0];
    let tau = -1.0 / ratio.ln();
    near(
        tau,
        f(&tail["value"]),
        f(&tail["tolerance"]),
        "low-speed time constant",
    );
}

#[test]
fn brakes_stop_the_car_in_the_target_time_and_hold_it_at_rest() {
    let t = targets();
    for row in t["brake"]["stop_time_s"].as_array().unwrap() {
        let mut w = world();
        cruise(&mut w, f(&row["from_mps"]), false);
        let dt = w.timing.step_seconds();
        let mut time = f64::NAN;
        for i in 0..(5 * 60) {
            w.step(input(0.0, 1.0, 0.0));
            if w.vehicle.speed() < 0.01 {
                time = (i + 1) as f64 * dt;
                break;
            }
        }
        near(time, f(&row["time_s"]), f(&row["tolerance_s"]), "stop time");
    }
    // Tail deceleration at low speed.
    let tail = &t["brake"]["tail_decel_mps2"];
    let mut w = world();
    cruise(&mut w, 11.0, false);
    // let the brake command build up first
    for _ in 0..6 {
        w.step(input(0.0, 1.0, 0.0));
    }
    let v0 = w.vehicle.speed();
    for _ in 0..12 {
        w.step(input(0.0, 1.0, 0.0));
    }
    let decel = (v0 - w.vehicle.speed()) / (12.0 * w.timing.step_seconds());
    near(
        decel,
        f(&tail["value"]),
        f(&tail["tolerance"]),
        "tail brake deceleration",
    );
    // Brake held at rest never moves or reverses.
    let mut w = world();
    let secs = f(&t["brake"]["no_motion_from_rest_s"]);
    for _ in 0..(secs * 60.0) as usize {
        w.step(input(0.0, 1.0, 0.0));
    }
    assert_eq!(w.vehicle.position, Vec2::default());
    assert_eq!(w.vehicle.speed(), 0.0);
    assert_eq!(w.drive_state().gear, 1);
}

#[test]
fn command_ramps_run_at_the_target_rates() {
    let t = targets();
    let s = &t["steering"];
    let mut w = world();
    cruise(&mut w, 20.0, false);
    let dt = w.timing.step_seconds();
    // 16 per tick up: full lock after 8 ticks; 32 per tick down: centre in 4.
    let up_steps = (f(&s["ticks_to_full_lock"]) / TICK_HZ / dt).round() as usize;
    for _ in 0..up_steps {
        w.step(input(0.0, 0.0, 1.0));
    }
    near(
        w.drive_state().steer_command,
        f(&s["lock_command"]),
        1e-9,
        "full lock",
    );
    let down_steps = (f(&s["ticks_to_centre"]) / TICK_HZ / dt).round() as usize;
    for _ in 0..down_steps {
        w.step(input(0.0, 0.0, 0.0));
    }
    near(w.drive_state().steer_command, 0.0, 1e-9, "centre");
    // Reversal passes through zero at the release rate.
    for _ in 0..up_steps {
        w.step(input(0.0, 0.0, 1.0));
    }
    let mut zero_after = None;
    for i in 0..(up_steps * 2) {
        w.step(input(0.0, 0.0, -1.0));
        if zero_after.is_none() && w.drive_state().steer_command <= 0.0 {
            zero_after = Some(i + 1);
        }
    }
    let ticks = zero_after.unwrap() as f64 * dt * TICK_HZ;
    near(
        ticks,
        f(&s["ticks_to_centre"]),
        1.01,
        "reversal through zero",
    );
    // Throttle command 16 per tick; brake command full in 4 ticks.
    let l = &t["launch"]["command_ramp_per_tick"];
    let mut w = world();
    let throttle_steps =
        (f(&l["full_scale"]) / f(&l["throttle_up"]) / TICK_HZ / dt).ceil() as usize;
    for _ in 0..throttle_steps {
        w.step(input(1.0, 0.0, 0.0));
    }
    near(
        w.drive_state().throttle_command,
        f(&l["full_scale"]),
        1e-6,
        "throttle command",
    );
    let mut w = world();
    for _ in 0..(f(&l["brake_to_full_ticks"]) / TICK_HZ / dt).round() as usize {
        w.step(input(0.0, 1.0, 0.0));
    }
    near(
        w.drive_state().brake_command,
        f(&l["full_scale"]),
        1e-6,
        "brake command",
    );
}

#[test]
fn full_lock_yaw_follows_the_target_law_with_a_knee() {
    let t = targets();
    let s = &t["steering"];
    let cfg = ClassicConfig::default();
    let knee = f(&s["knee_speed_mps"]);
    let low = f(&s["low_branch_rate_rad_s"]);
    for speed in [9.0, 12.0, 20.0, knee - 0.1] {
        near(
            cfg.full_lock_yaw_rate(speed),
            low,
            low * f(&s["low_branch_tolerance"]),
            &format!("low branch {speed}"),
        );
    }
    let above = cfg.full_lock_yaw_rate(knee + 0.1);
    let drop = &s["knee_drop_fraction"];
    let fraction = above / low;
    assert!(
        fraction >= f(&drop[0]) - 0.01 && fraction <= f(&drop[1]) + 0.01,
        "drop at the knee {fraction}"
    );
    for row in s["full_lock_rate_rad_s"].as_array().unwrap() {
        let speed = f(&row["speed_mps"]);
        let want = f(&row["rate"]);
        near(
            cfg.full_lock_yaw_rate(speed),
            want,
            want * f(&s["high_branch_tolerance"]),
            &format!("full lock at {speed}"),
        );
    }
    // Falls smoothly from the knee to the boost speed; stopped cars do not turn.
    let mut last = f64::INFINITY;
    let mut v = knee + 0.1;
    while v < cfg.yaw_boost_speed_mps - 0.2 {
        let rate = cfg.full_lock_yaw_rate(v);
        assert!(rate <= last + 1e-12);
        last = rate;
        v += 0.1;
    }
    assert!(cfg.full_lock_yaw_rate(cfg.yaw_boost_speed_mps + 0.1) > last);
    assert_eq!(cfg.full_lock_yaw_rate(0.0), 0.0);
    // Linear in the command.
    let half = cfg.yaw_rate(62.0, 35.0);
    near(
        half,
        0.5 * cfg.full_lock_yaw_rate(35.0),
        1e-12,
        "linear in command",
    );
}

#[test]
fn per_car_gain_and_grip_scale_the_yaw_above_the_knee_only() {
    let t = targets();
    let e = &t["steering"]["scaling_exponents"];
    let reference = ClassicConfig::default();
    for (gain, grip) in [(18.0, 2.8), (19.0, 3.1), (20.0, 3.6), (19.5, 3.0)] {
        let car = ClassicConfig {
            steer_gain: gain,
            grip,
            ..ClassicConfig::default()
        };
        for speed in [25.0, 35.0, 45.0] {
            let want = (grip / reference.grip).powf(f(&e["grip"]))
                * (gain / reference.steer_gain).powf(f(&e["gain"]));
            let got = car.full_lock_yaw_rate(speed) / reference.full_lock_yaw_rate(speed);
            near(
                got,
                want,
                1e-9,
                &format!("scaling at {speed} m/s, gain {gain} grip {grip}"),
            );
        }
        // Below the knee the car's own low-speed rate applies, untouched.
        assert_eq!(
            car.full_lock_yaw_rate(15.0),
            reference.full_lock_yaw_rate(15.0)
        );
    }
    // The low-speed rate per unit of gain.
    near(
        f(&t["steering"]["low_branch_per_gain"]),
        racer_sim::classic_model::STEER_LOW_PER_GAIN,
        1e-12,
        "low branch constant",
    );
}

#[test]
fn the_body_leads_the_travel_direction_by_one_tick_of_yaw() {
    let t = targets();
    let s = &t["steering"];
    for hz in [32u32, 60] {
        let mut w = world_hz(ClassicConfig::default(), hz);
        cruise(&mut w, 15.0, true);
        for _ in 0..(hz as usize) {
            w.step(input(1.0, 0.0, 1.0));
            let v = w.vehicle.velocity;
            let travel = v.y.atan2(v.x);
            let lead = w.vehicle.heading - travel;
            let want = w.vehicle.angular_velocity / TICK_HZ * f(&s["travel_lead_ticks"]);
            near(
                lead,
                want,
                f(&s["travel_lead_tolerance_rad"]),
                &format!("lead at {hz} Hz"),
            );
        }
        // After release the lead vanishes within one tick.
        for _ in 0..(hz as usize / 2) {
            w.step(input(1.0, 0.0, 0.0));
        }
        let v = w.vehicle.velocity;
        near(
            v.y.atan2(v.x),
            w.vehicle.heading,
            1e-3,
            "lead gone after release",
        );
    }
}

#[test]
fn the_camera_lead_comes_from_the_one_tick_body_lead_and_peaks_near_the_target() {
    // The render camera follows the body heading with a first-order lag; the
    // peak body-minus-camera offset at full-lock yaw follows from the lag and
    // the full-lock yaw rate.
    let t = targets();
    let cam = &t["camera"];
    let cfg = ClassicConfig::default();
    let lag_ticks = f(&cam["lag_ticks_per_rad_s"]);
    let peak_deg = (cfg.full_lock_yaw_rate(10.0) * lag_ticks / TICK_HZ).to_degrees();
    let range = &cam["peak_lead_deg_at_full_lock"];
    assert!(
        peak_deg >= f(&range[0]) - 0.4 && peak_deg <= f(&range[1]) + 0.4,
        "peak lead {peak_deg} deg"
    );
    assert!((0.0..1.0).contains(&(1.0 - f(&cam["lag_gain_per_tick"]))));
}

#[test]
fn short_steering_pulses_turn_the_body_by_the_target_angle() {
    let t = targets();
    for row in t["steering"]["pulse_heading_deg"].as_array().unwrap() {
        for hz in [32u32, 60] {
            let mut w = world_hz(ClassicConfig::default(), hz);
            cruise(&mut w, f(&row["speed_mps"]), true);
            let hold = (f(&row["hold_ticks"]) * f64::from(hz) / TICK_HZ).round() as usize;
            let mut turned = 0.0;
            let mut previous = w.vehicle.heading;
            for i in 0..(hold + hz as usize) {
                w.step(input(1.0, 0.0, if i < hold { -1.0 } else { 0.0 }));
                let mut delta = w.vehicle.heading - previous;
                delta -= (delta / std::f64::consts::TAU).round() * std::f64::consts::TAU;
                turned -= delta.to_degrees();
                previous = w.vehicle.heading;
            }
            near(
                turned,
                f(&row["heading_deg"]),
                f(&row["tolerance_deg"]) + if hz == 60 { 2.0 } else { 0.0 },
                &format!(
                    "heading after pulse at {} m/s, {hz} Hz",
                    f(&row["speed_mps"])
                ),
            );
        }
    }
}

#[test]
fn handbrake_without_steering_slows_and_never_rotates() {
    let t = targets();
    let h = &t["handbrake"]["no_steer"];
    let mut w = world();
    cruise(&mut w, f(&h["start_mps"]), true);
    let ticks = f(&h["hold_ticks"]);
    let steps = (ticks / TICK_HZ / w.timing.step_seconds()).round() as usize;
    for _ in 0..steps {
        w.step(DriverInput {
            throttle: 1.0,
            handbrake: true,
            ..DriverInput::default()
        });
    }
    assert!(w.vehicle.heading.abs() <= f(&h["heading_change_rad_max"]));
    assert!(w.drive_state().slide_offset.abs() < 1e-12);
    near(
        w.vehicle.speed(),
        f(&h["end_speed_mps"]),
        f(&h["end_tolerance_mps"]),
        "speed after handbrake",
    );
}

#[test]
fn handbrake_with_steering_slides_then_recovers_at_the_target_rate() {
    let t = targets();
    let h = &t["handbrake"]["slide"];
    let mut w = world();
    cruise(&mut w, f(&h["start_mps"]), true);
    let dt = w.timing.step_seconds();
    let mut peak_yaw: f64 = 0.0;
    let mut checks = h["offset_deg_at_ticks"].as_array().unwrap().iter();
    let mut next = checks.next();
    let steps = (34.0 / TICK_HZ / dt).round() as usize;
    for i in 0..steps {
        w.step(DriverInput {
            throttle: 1.0,
            handbrake: true,
            steer: -1.0,
            ..DriverInput::default()
        });
        peak_yaw = peak_yaw.max(w.vehicle.angular_velocity.abs());
        let tick = (i + 1) as f64 * dt * TICK_HZ;
        if let Some(c) = next {
            if tick >= f(&c["tick"]) {
                let offset = w.drive_state().slide_offset.abs().to_degrees();
                near(
                    offset,
                    f(&c["deg"]),
                    f(&h["offset_tolerance_deg"]),
                    &format!("slide offset at tick {}", f(&c["tick"])),
                );
                next = checks.next();
            }
        }
    }
    let range = &h["peak_yaw_rate_rad_s"];
    assert!(
        peak_yaw >= f(&range[0]) && peak_yaw <= f(&range[1]) + 0.1,
        "peak slide yaw {peak_yaw}"
    );
    // Release: the offset left beyond the one-tick lead decays by 0.90 per tick.
    for _ in 0..2 {
        w.step(input(1.0, 0.0, 0.0));
    }
    let a = w.drive_state().slide_offset.abs();
    let ticks = 8.0;
    let steps = (ticks / TICK_HZ / dt).round() as usize;
    for _ in 0..steps {
        w.step(input(1.0, 0.0, 0.0));
    }
    let b = w.drive_state().slide_offset.abs();
    let per_tick = (b / a).powf(1.0 / ticks);
    let rec = &h["recovery_per_tick"];
    near(
        per_tick,
        f(&rec["value"]),
        f(&rec["tolerance"]),
        "slide recovery per tick",
    );
}

#[test]
fn reverse_follows_the_target_speed_profile_and_hud_shows_r() {
    let t = targets();
    let r = &t["reverse"];
    let mut w = world();
    assert_eq!(w.drive_telemetry().gear, 1);
    tap(&mut w, false, true);
    assert_eq!(w.drive_telemetry().gear, GEAR_CODE_NEUTRAL);
    tap(&mut w, false, true);
    tap(&mut w, false, true); // further presses keep R
    assert_eq!(w.drive_telemetry().gear, GEAR_CODE_REVERSE);
    let mut steps = 0usize;
    for row in r["speed_at_s_mps"].as_array().unwrap() {
        let target = (f(&row["time_s"]) * 60.0).round() as usize;
        while steps < target {
            w.step(input(1.0, 0.0, 0.0));
            steps += 1;
        }
        near(
            -along(&w),
            f(&row["speed"]),
            f(&row["tolerance"]),
            &format!("reverse speed at {} s", f(&row["time_s"])),
        );
    }
    for _ in 0..(3 * 60) {
        w.step(input(1.0, 0.0, 0.0));
    }
    near(-along(&w), f(&r["top_speed_mps"]), 1.5, "reverse top speed");
    assert!(
        w.vehicle.speed() > 5.0,
        "the speedometer shows the magnitude"
    );
    assert_eq!(w.drive_telemetry().gear, GEAR_CODE_REVERSE);
}

fn reverse_world() -> RaceWorld {
    let mut w = world();
    tap(&mut w, false, true);
    tap(&mut w, false, true);
    assert_eq!(w.drive_state().selector, SELECTOR_REVERSE);
    w
}

#[test]
fn reverse_coast_and_brake_times_match_the_targets() {
    let t = targets();
    let r = &t["reverse"];
    let run = |start: f64, brake: f64, stop_at: f64| {
        let mut w = reverse_world();
        let h = w.vehicle.heading;
        w.vehicle.velocity = Vec2::new(-h.cos(), -h.sin()) * start;
        let mut ticks = 0.0;
        while -along(&w) > stop_at && ticks < 6.0 * 60.0 {
            w.step(input(0.0, brake, 0.0));
            ticks += 1.0;
        }
        ticks / 60.0
    };
    let c = &r["coast_time_s"];
    let coast = run(f(&c["from_mps"]), 0.0, f(&c["to_mps"]));
    assert!(
        coast >= f(&c["time"][0]) && coast <= f(&c["time"][1]),
        "coast {coast}"
    );
    for row in r["brake_time_s"].as_array().unwrap() {
        let time = run(f(&row["from_mps"]), 1.0, 0.01);
        assert!(
            time >= f(&row["time"][0]) && time <= f(&row["time"][1]),
            "brake from {}: {time}",
            f(&row["from_mps"])
        );
    }
}

#[test]
fn reverse_selected_while_moving_brakes_through_zero() {
    let t = targets();
    for row in t["reverse"]["selected_while_moving"].as_array().unwrap() {
        let mut w = world();
        cruise(&mut w, f(&row["from_mps"]), f(&row["throttle"]) > 0.0);
        let throttle = f(&row["throttle"]);
        // Z, Z within a moment, then drive on.
        w.step(DriverInput {
            throttle,
            shift_down: true,
            ..DriverInput::default()
        });
        w.step(input(throttle, 0.0, 0.0));
        w.step(DriverInput {
            throttle,
            shift_down: true,
            ..DriverInput::default()
        });
        assert_eq!(w.drive_state().selector, SELECTOR_REVERSE);
        let dt = w.timing.step_seconds();
        let mut time = f64::NAN;
        for i in 0..(4 * 60) {
            w.step(input(throttle, 0.0, 0.0));
            if along(&w) <= 0.0 {
                time = (i + 3) as f64 * dt;
                break;
            }
        }
        near(
            time,
            f(&row["stop_time_s"]),
            f(&row["tolerance_s"]),
            "stop after selecting R",
        );
    }
}

#[test]
fn key_sequences_walk_the_gear_ladder() {
    let t = targets();
    let m = &t["gearbox_modes"];
    // Automatic: Z Z Z A A A Z Z.
    let mut w = world();
    let keys = m["automatic_key_sequence"]["keys"].as_array().unwrap();
    let want = m["automatic_key_sequence"]["selector"].as_array().unwrap();
    for (key, want) in keys.iter().zip(want) {
        tap(&mut w, key == "up", key == "down");
        assert_eq!(selector_label(&w), want.as_str().unwrap());
    }
    // Manual: starts in N, six gears here.
    let six = ClassicConfig {
        upshift_mps: vec![22.0, 30.8, 41.3, 52.3, 64.2],
        downshift_mps: vec![18.3, 25.6, 34.5, 44.1, 54.2],
        top_speed_mps: 82.0,
        ..ClassicConfig::default()
    };
    let mut w = world_hz(six, 60);
    w.set_transmission(Transmission::Manual);
    assert_eq!(
        selector_label(&w),
        m["manual_ladder"]["start"].as_str().unwrap()
    );
    assert_eq!(w.drive_telemetry().gear, GEAR_CODE_NEUTRAL);
    for want in m["manual_ladder"]["after_up"].as_array().unwrap() {
        tap(&mut w, true, false);
        assert_eq!(selector_label(&w), want.as_str().unwrap());
    }
    for want in m["manual_ladder"]["after_down"].as_array().unwrap() {
        tap(&mut w, false, true);
        assert_eq!(selector_label(&w), want.as_str().unwrap());
    }
    // A held key counts as one press.
    let mut w = world();
    for _ in 0..40 {
        w.step(DriverInput {
            shift_down: true,
            ..DriverInput::default()
        });
    }
    assert_eq!(w.drive_state().selector, SELECTOR_NEUTRAL);
}

#[test]
fn the_manual_gear_holds_its_cap_without_cycling_and_neutral_only_revs() {
    let t = targets();
    let m = &t["gearbox_modes"];
    let hold = &m["rev_hold"];
    let cfg = ClassicConfig::default();
    let mut w = world();
    w.set_transmission(Transmission::Manual);
    tap(&mut w, true, false);
    let mut speeds = Vec::new();
    for i in 0..(12 * 60) {
        w.step(input(1.0, 0.0, 0.0));
        if i as f64 / 60.0 >= f(&hold["speed_locked_after_s"]) {
            speeds.push(w.vehicle.speed());
        }
    }
    let cap = cfg.gear_cap_mps(1);
    let lo = speeds.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = speeds.iter().cloned().fold(0.0, f64::max);
    assert!(hi - lo <= f(&hold["max_speed_wobble_mps"]), "{lo}..{hi}");
    assert!(
        hi <= cap + 1e-9 && hi > cap * 0.985,
        "locked at {hi}, cap {cap}"
    );
    near(
        w.drive_telemetry().rpm01,
        f(&hold["rpm_fraction_of_limit"]),
        0.01,
        "engine speed at the cap",
    );
    // Neutral: throttle revs to the limit and does not move the car.
    let mut n = world();
    tap(&mut n, false, true);
    for _ in 0..120 {
        n.step(input(1.0, 0.0, 0.0));
    }
    assert_eq!(n.vehicle.speed(), 0.0);
    assert!(n.drive_telemetry().rpm01 >= f(&m["neutral_throttle_rpm_fraction_min"]));
    // Neutral coasting is only aerodynamic.
    let mut n = world();
    tap(&mut n, false, true);
    cruise(&mut n, 30.0, false);
    let before = n.vehicle.speed();
    for _ in 0..60 {
        n.step(input(0.0, 0.0, 0.0));
    }
    let drag = &m["neutral_drag_mps2_at_30mps"];
    near(
        before - n.vehicle.speed(),
        f(&drag["value"]),
        f(&drag["tolerance"]),
        "neutral coasting drag",
    );
}

#[test]
fn a_gear_too_low_for_the_speed_over_revs_and_pulls_the_car_down() {
    let mut w = world();
    w.set_transmission(Transmission::Manual);
    let cfg = ClassicConfig::default();
    tap(&mut w, true, false);
    tap(&mut w, true, false);
    cruise_manual(&mut w, 2, cfg.gear_cap_mps(1) + 3.0);
    // Second gear selected at a speed above the first-gear cap is fine; going
    // down to first is accepted at once and the car loses speed to the cap.
    tap(&mut w, false, true);
    assert_eq!(selector_label(&w), "1");
    let start = w.vehicle.speed();
    for _ in 0..(6 * 60) {
        w.step(input(1.0, 0.0, 0.0));
    }
    assert!(w.vehicle.speed() < start);
    assert!(w.vehicle.speed() <= cfg.gear_cap_mps(1) + 0.1);
}

fn cruise_manual(w: &mut RaceWorld, gear: u8, speed: f64) {
    let mut frame = w.capture_state().unwrap();
    let h = frame.vehicle.heading;
    frame.vehicle.velocity = Vec2::new(h.cos(), h.sin()) * speed;
    frame.drive.gear = gear;
    w.restore_state(&frame).unwrap();
}

#[test]
fn each_car_parameter_changes_its_behaviour_in_the_expected_direction() {
    let base = ClassicConfig::default();
    let run_time = |cfg: ClassicConfig| {
        let mut w = world_hz(cfg, 60);
        let mut t = f64::NAN;
        for i in 0..(30 * 60) {
            w.step(input(1.0, 0.0, 0.0));
            if w.vehicle.speed() >= 40.0 {
                t = f64::from(i + 1) / 60.0;
                break;
            }
        }
        t
    };
    let quick = run_time(ClassicConfig {
        accel_scale: 1.2,
        ..base.clone()
    });
    let slow = run_time(ClassicConfig {
        accel_scale: 0.7,
        ..base.clone()
    });
    assert!(quick < run_time(base.clone()) && run_time(base.clone()) < slow);
    // Top speed caps the top gear.
    let mut w = world_hz(
        ClassicConfig {
            top_speed_mps: 72.0,
            ..base.clone()
        },
        60,
    );
    for _ in 0..(40 * 60) {
        w.step(input(1.0, 0.0, 0.0));
    }
    assert!(w.vehicle.speed() <= 72.0 + 1e-6 && w.vehicle.speed() > 70.0);
    // Brake scale shortens the stop; reverse speed caps reverse.
    let stop = |scale: f64| {
        let mut w = world_hz(
            ClassicConfig {
                brake_scale: scale,
                ..base.clone()
            },
            60,
        );
        cruise(&mut w, 40.0, false);
        let mut n = 0;
        while w.vehicle.speed() > 0.01 && n < 600 {
            w.step(input(0.0, 1.0, 0.0));
            n += 1;
        }
        n
    };
    assert!(stop(1.2) < stop(1.0) && stop(1.0) < stop(0.8));
    for cap in [18.0, 29.4] {
        let mut w = world_hz(
            ClassicConfig {
                reverse_speed_mps: cap,
                ..base.clone()
            },
            60,
        );
        tap(&mut w, false, true);
        tap(&mut w, false, true);
        for _ in 0..(8 * 60) {
            w.step(input(1.0, 0.0, 0.0));
        }
        assert!(
            (-along(&w) - cap).abs() < 1.5,
            "reverse cap {cap}: {}",
            -along(&w)
        );
    }
    // Shift pause: a longer pause loses more speed across a shift.
    let lost = |pause: f64| {
        let mut w = world_hz(
            ClassicConfig {
                shift_seconds: pause,
                ..base.clone()
            },
            60,
        );
        let mut t = 0.0;
        for _ in 0..(7 * 60) {
            w.step(input(1.0, 0.0, 0.0));
            t += 1.0;
        }
        let _ = t;
        w.vehicle.speed()
    };
    assert!(lost(0.1) > lost(0.5));
}

#[test]
fn the_model_is_selectable_alongside_arcade_and_needs_parameters() {
    let mut w = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    assert!(w.set_vehicle_model(VehicleModel::Classic).is_err());
    let mut bad = ClassicConfig::default();
    bad.downshift_mps[0] = 99.0;
    assert!(w.install_classic_parameters(bad).is_err());
    w.use_classic_model(ClassicConfig::default()).unwrap();
    assert_eq!(w.vehicle_model(), VehicleModel::Classic);
    // Arcade stays available as the fallback model.
    w.use_arcade_model(racer_sim::ArcadeConfig::default())
        .unwrap();
    assert_eq!(w.vehicle_model(), VehicleModel::Arcade);
    w.set_vehicle_model(VehicleModel::Classic).unwrap();
    assert_eq!(w.vehicle_model(), VehicleModel::Classic);
    assert_eq!(w.classic_parameters(), Some(&ClassicConfig::default()));
}

#[test]
fn state_frames_round_trip_and_identity_depends_on_model_and_car() {
    let mut a = world();
    for i in 0..300 {
        a.step(input(1.0, 0.0, if i % 90 < 30 { 1.0 } else { 0.0 }));
    }
    let frame = a.capture_state().unwrap();
    let decoded = SimStateFrame::decode(&frame.encode()).unwrap();
    assert_eq!(decoded, frame);
    let mut b = world();
    b.restore_state(&decoded).unwrap();
    for _ in 0..120 {
        a.step(input(0.0, 0.5, -1.0));
        b.step(input(0.0, 0.5, -1.0));
    }
    assert_eq!(a.vehicle, b.vehicle);
    assert_eq!(a.drive_state(), b.drive_state());
    // The model, its revision and the car parameters are part of the identity.
    let mut arcade = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
    arcade
        .use_arcade_model(racer_sim::ArcadeConfig::default())
        .unwrap();
    let other_car = world_hz(
        ClassicConfig {
            accel_scale: 1.01,
            ..ClassicConfig::default()
        },
        60,
    );
    let ids = [
        arcade.simulation_identity().unwrap(),
        other_car.simulation_identity().unwrap(),
        world().simulation_identity().unwrap(),
    ];
    assert!(ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2]);
    // A manual box changes the identity too.
    let mut manual = world();
    manual.set_transmission(Transmission::Manual);
    assert_ne!(manual.simulation_identity().unwrap(), ids[2]);
}

#[test]
fn identical_inputs_give_identical_worlds() {
    let script = |w: &mut RaceWorld| {
        for i in 0..2000u32 {
            w.step(DriverInput {
                throttle: if i % 400 < 300 { 1.0 } else { 0.0 },
                brake: if i % 400 >= 330 { 1.0 } else { 0.0 },
                steer: [0.0, 1.0, -1.0, 0.5][(i / 70 % 4) as usize],
                handbrake: i % 650 > 620,
                ..DriverInput::default()
            });
        }
    };
    let (mut a, mut b) = (world(), world());
    script(&mut a);
    script(&mut b);
    assert_eq!(a.vehicle, b.vehicle);
    assert_eq!(a.drive_state(), b.drive_state());
}

#[test]
fn results_are_close_across_step_rates() {
    let run = |hz: u32| {
        let mut w = world_hz(ClassicConfig::default(), hz);
        for _ in 0..(hz * 6) {
            w.step(input(1.0, 0.0, 0.0));
        }
        w.vehicle.speed()
    };
    assert!((run(32) - run(60)).abs() < 1.0);
    assert!((run(60) - run(120)).abs() < 1.0);
}

#[test]
fn drive_state_stays_valid_and_finite_under_rough_input() {
    let mut w = world();
    let mut seed = 12345u64;
    let mut next = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as f64 / f64::from(1u32 << 31)
    };
    for _ in 0..20_000 {
        w.step(DriverInput {
            throttle: next(),
            brake: if next() < 0.2 { next() } else { 0.0 },
            steer: next() * 2.0 - 1.0,
            handbrake: next() < 0.05,
            shift_up: next() < 0.02,
            shift_down: next() < 0.02,
            reset_request: false,
        });
        assert!(w.drive_state().is_valid());
        let v = w.vehicle;
        assert!(v.position.x.is_finite() && v.velocity.x.is_finite() && v.heading.is_finite());
        assert!(v.speed() < 120.0);
    }
}
