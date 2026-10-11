//! Auto reverse: no shifting. Brake at a standstill selects R and drives
//! backwards; throttle in R brakes, then selects first gear and drives on.

use racer_sim::drivetrain::GEAR_CODE_REVERSE;
use racer_sim::{
    ArcadeConfig, ClassicConfig, DriverInput, RaceWorld, SimTiming, TrackRuntime, Transmission,
};

fn world(classic: bool, transmission: Transmission, auto: bool) -> RaceWorld {
    let timing = SimTiming {
        physics_hz: 60,
        input_hz: 60,
        race_clock_hz: 60,
    };
    let mut w = RaceWorld::new(timing, TrackRuntime::open_preview());
    if classic {
        w.use_classic_model(ClassicConfig::default()).unwrap();
    } else {
        w.use_arcade_model(ArcadeConfig::default()).unwrap();
    }
    w.set_transmission(transmission);
    w.set_auto_reverse(auto);
    w
}

fn pedals(throttle: f64, brake: f64) -> DriverInput {
    DriverInput {
        throttle,
        brake,
        ..DriverInput::default()
    }
}

fn forward(w: &RaceWorld) -> f64 {
    let v = &w.vehicle;
    v.velocity.x * v.heading.cos() + v.velocity.y * v.heading.sin()
}

fn run(w: &mut RaceWorld, i: DriverInput, seconds: f64) {
    for _ in 0..(seconds * 60.0) as u32 {
        w.step(i);
    }
}

fn gear(w: &RaceWorld) -> u8 {
    w.drive_telemetry().gear
}

fn for_every_setup(check: impl Fn(bool, Transmission)) {
    for classic in [false, true] {
        for t in [Transmission::Automatic, Transmission::Manual] {
            check(classic, t);
        }
    }
}

#[test]
fn stopped_then_hold_brake_selects_r_and_reverses() {
    for_every_setup(|classic, t| {
        let mut w = world(classic, t, true);
        run(&mut w, pedals(0.0, 1.0), 4.0);
        assert_eq!(gear(&w), GEAR_CODE_REVERSE, "classic={classic} {t:?}");
        assert!(forward(&w) < -2.0, "reversing: {}", forward(&w));
    });
}

#[test]
fn throttle_in_r_brakes_then_selects_first_and_drives_forward() {
    for_every_setup(|classic, t| {
        let mut w = world(classic, t, true);
        run(&mut w, pedals(0.0, 1.0), 4.0);
        assert!(forward(&w) < -2.0);
        // Throttle held: the car brakes first, still in R while moving back.
        let mut saw_r_while_braking = false;
        let mut last = forward(&w);
        let mut first_forward_gear = None;
        for _ in 0..(8 * 60) {
            w.step(pedals(1.0, 0.0));
            if forward(&w) < -0.6 {
                assert_eq!(gear(&w), GEAR_CODE_REVERSE, "still R while rolling back");
                assert!(forward(&w) >= last - 1e-9, "throttle brakes, not accelerates");
                saw_r_while_braking = true;
            }
            last = forward(&w);
            if first_forward_gear.is_none() && gear(&w) != GEAR_CODE_REVERSE {
                first_forward_gear = Some(gear(&w));
            }
        }
        assert!(saw_r_while_braking, "classic={classic} {t:?}");
        assert_eq!(first_forward_gear, Some(1), "back in first gear");
        assert!(forward(&w) > 3.0, "driving forward: {}", forward(&w));
    });
}

#[test]
fn forward_braking_above_the_threshold_never_selects_r() {
    for_every_setup(|classic, t| {
        let mut w = world(classic, t, true);
        // Get rolling forward (manual box needs a shift to first once).
        if t == Transmission::Manual {
            w.step(DriverInput {
                shift_up: true,
                ..DriverInput::default()
            });
            w.step(DriverInput::default());
        }
        run(&mut w, pedals(1.0, 0.0), 4.0);
        assert!(forward(&w) > 8.0);
        for _ in 0..(10 * 60) {
            w.step(pedals(0.0, 1.0));
            if forward(&w) >= 0.5 {
                assert_ne!(gear(&w), GEAR_CODE_REVERSE, "classic={classic} {t:?}");
            }
        }
    });
}

#[test]
fn without_the_option_brake_never_reverses() {
    for_every_setup(|classic, t| {
        let mut w = world(classic, t, false);
        run(&mut w, pedals(0.0, 1.0), 4.0);
        assert_ne!(gear(&w), GEAR_CODE_REVERSE);
        assert!(forward(&w).abs() < 0.1);
    });
}

#[test]
fn manual_box_starts_in_neutral_and_throttle_engages_first() {
    let mut w = world(false, Transmission::Manual, true);
    assert_eq!(gear(&w), 251);
    run(&mut w, pedals(1.0, 0.0), 3.0);
    assert!(forward(&w) > 3.0);
}
