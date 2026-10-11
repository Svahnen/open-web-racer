//! Generic drivetrain state shared by gearbox-style vehicle models (the
//! built-in arcade model and externally supplied models alike).

use crate::DriverInput;

/// Full-scale value of the smoothed throttle and brake commands.
pub const COMMAND_FULL: f64 = 248.0;
/// Full-lock value of the smoothed steering command (+-).
pub const STEER_FULL: f64 = 124.0;
/// Safety bound on the stored body-versus-travel heading offset (radians).
pub const SLIDE_OFFSET_MAX: f64 = 1.6;

/// Per-player gearbox mode. The automatic box chooses the forward gear by
/// itself (the shift keys still select reverse, neutral and drive); the
/// manual box moves one step per tap through R, N, 1, 2 ... n.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Transmission {
    #[default]
    Automatic,
    Manual,
}

impl Transmission {
    pub fn is_manual(self) -> bool {
        self == Self::Manual
    }

    pub fn from_manual(manual: bool) -> Self {
        if manual {
            Self::Manual
        } else {
            Self::Automatic
        }
    }
}

/// Dynamic drivetrain state carried by `RaceWorld`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriveState {
    /// Smoothed throttle command, 0..=248.
    pub throttle_command: f64,
    /// Smoothed brake command, 0..=248.
    pub brake_command: f64,
    /// Smoothed steering command, -124..=124. Positive turns counter-clockwise
    /// (the sim convention).
    pub steer_command: f64,
    /// Current gear, 1..=`ARCADE_MAX_GEARS`.
    pub gear: u8,
    /// Remaining shift time in seconds (drive and drag are mostly cut).
    pub shift_timer: f64,
    /// Body heading minus velocity direction, radians, sim sign (positive when
    /// the body points counter-clockwise of the travel direction).
    pub slide_offset: f64,
    /// Seconds the handbrake has been held (0 when released).
    pub handbrake_seconds: f64,
    /// Gear selector: `SELECTOR_REVERSE`, `SELECTOR_NEUTRAL` or
    /// `SELECTOR_DRIVE`. The automatic box starts in drive, the manual in neutral.
    pub selector: u8,
    /// Throttle in neutral selects drive until the first shift press (a rule of
    /// the earlier start-in-neutral reading; no start state sets it any more).
    /// Never applies to a manual box.
    pub launch_armed: bool,
    /// Shift buttons were down on the previous step (edge detection).
    pub shift_up_held: bool,
    pub shift_down_held: bool,
    /// Manual gearbox: the forward gear is chosen with the shift keys.
    pub manual: bool,
    /// Auto reverse: the driver never shifts. Brake at a standstill selects R
    /// and the brake pedal then drives backwards; throttle in R brakes, and
    /// once stopped selects first gear again. See `apply_auto_reverse`.
    pub auto_reverse: bool,
}

/// Speed (m/s) below which the car counts as stopped for auto reverse.
pub const AUTO_REVERSE_STOP_MPS: f64 = 0.5;
/// Pedal travel that counts as pressed for auto reverse.
pub const AUTO_REVERSE_PEDAL: f64 = 0.05;

/// Selector values of `DriveState::selector`.
pub const SELECTOR_REVERSE: u8 = 0;
pub const SELECTOR_NEUTRAL: u8 = 1;
pub const SELECTOR_DRIVE: u8 = 2;

/// `DriveTelemetry::gear` code for reverse ("R" on the HUD).
pub const GEAR_CODE_REVERSE: u8 = 250;
/// `DriveTelemetry::gear` code for neutral ("N" on the HUD).
pub const GEAR_CODE_NEUTRAL: u8 = 251;

impl Default for DriveState {
    fn default() -> Self {
        Self::fresh(Transmission::Automatic)
    }
}

impl DriveState {
    /// State at the start of a race for the given gearbox mode: the automatic
    /// box is in first gear (drive selected) from the start, the manual box in
    /// neutral.
    pub fn fresh(transmission: Transmission) -> Self {
        let manual = transmission.is_manual();
        Self {
            throttle_command: 0.0,
            brake_command: 0.0,
            steer_command: 0.0,
            gear: 1,
            shift_timer: 0.0,
            slide_offset: 0.0,
            handbrake_seconds: 0.0,
            selector: if manual {
                SELECTOR_NEUTRAL
            } else {
                SELECTOR_DRIVE
            },
            launch_armed: false,
            shift_up_held: false,
            shift_down_held: false,
            manual,
            auto_reverse: false,
        }
    }

    /// Whether every field is finite and within its documented range.
    pub fn is_valid(&self) -> bool {
        (0.0..=COMMAND_FULL).contains(&self.throttle_command)
            && (0.0..=COMMAND_FULL).contains(&self.brake_command)
            && (-STEER_FULL..=STEER_FULL).contains(&self.steer_command)
            && (1..=crate::arcade_model::ARCADE_MAX_GEARS as u8).contains(&self.gear)
            && (0.0..=1.0).contains(&self.shift_timer)
            && (-SLIDE_OFFSET_MAX..=SLIDE_OFFSET_MAX).contains(&self.slide_offset)
            && self.handbrake_seconds.is_finite()
            && self.handbrake_seconds >= 0.0
            && self.selector <= SELECTOR_DRIVE
    }
}

/// Auto reverse. Returns the input the model should see.
///
/// * Stopped (|forward speed| below `AUTO_REVERSE_STOP_MPS`) with the brake
///   held and no throttle: select R.
/// * In R the pedals swap roles: brake drives backwards, throttle brakes.
///   Throttle alone, once the car is stopped or rolling forward, selects first
///   gear and the pedals act normally again.
/// * Throttle in neutral selects first gear.
/// * Braking while rolling forward above the threshold never selects R.
///
/// `forward_mps` is the speed along the body axis, positive forwards.
pub fn apply_auto_reverse(
    drive: &mut DriveState,
    mut input: DriverInput,
    forward_mps: f64,
) -> DriverInput {
    if !drive.auto_reverse {
        return input;
    }
    let stopped = forward_mps.abs() < AUTO_REVERSE_STOP_MPS;
    let throttle = input.throttle > AUTO_REVERSE_PEDAL;
    let brake = input.brake > AUTO_REVERSE_PEDAL;
    if drive.selector == SELECTOR_REVERSE {
        if throttle && !brake && forward_mps > -AUTO_REVERSE_STOP_MPS {
            drive.selector = SELECTOR_DRIVE;
            drive.gear = 1;
            return input;
        }
    } else if stopped && brake && !throttle {
        drive.selector = SELECTOR_REVERSE;
    } else if drive.selector == SELECTOR_NEUTRAL && throttle && !brake {
        // Neutral (the manual box's start): throttle engages first gear.
        drive.selector = SELECTOR_DRIVE;
        drive.gear = 1;
    }
    if drive.selector == SELECTOR_REVERSE {
        std::mem::swap(&mut input.throttle, &mut input.brake);
    }
    input
}

/// What a selector update changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectorChange {
    /// The manual box moved between two forward gears (a shift pause follows).
    pub forward_shift: bool,
}

/// Apply the shift keys (rising edges) and the start-neutral launch rule.
///
/// Both modes walk the same ladder R, N, then the forward gears; every step
/// is accepted at any speed. Shift-down goes down the ladder, shift-up goes up
/// it (up has priority when both arrive), the ends are clamped.
///
/// * Automatic: a single forward slot, drive. The box chooses the gear.
/// * Manual: drive holds the gear in `drive.gear`, 1..=`gear_count`; up in
///   drive moves to the next gear, down moves to the previous one and from
///   first gear to neutral.
///
/// The automatic box with `launch_armed` also selects drive when the throttle
/// is pressed in the start neutral.
pub fn update_selector(
    drive: &mut DriveState,
    input: &DriverInput,
    gear_count: u8,
) -> SelectorChange {
    let up = input.shift_up && !drive.shift_up_held;
    let down = input.shift_down && !drive.shift_down_held;
    drive.shift_up_held = input.shift_up;
    drive.shift_down_held = input.shift_down;
    let mut change = SelectorChange::default();
    if up || down {
        drive.launch_armed = false;
    }
    let gear_count = gear_count.max(1);
    drive.gear = drive.gear.clamp(1, gear_count);
    if up {
        match drive.selector {
            SELECTOR_REVERSE => drive.selector = SELECTOR_NEUTRAL,
            SELECTOR_NEUTRAL => {
                drive.selector = SELECTOR_DRIVE;
                if drive.manual {
                    drive.gear = 1;
                }
            }
            _ if drive.manual && drive.gear < gear_count => {
                drive.gear += 1;
                change.forward_shift = true;
            }
            _ => {}
        }
    } else if down {
        match drive.selector {
            SELECTOR_DRIVE if drive.manual && drive.gear > 1 => {
                drive.gear -= 1;
                change.forward_shift = true;
            }
            SELECTOR_DRIVE => drive.selector = SELECTOR_NEUTRAL,
            SELECTOR_NEUTRAL => drive.selector = SELECTOR_REVERSE,
            _ => {}
        }
    }
    if drive.launch_armed
        && !drive.manual
        && drive.selector == SELECTOR_NEUTRAL
        && input.throttle > 0.0
    {
        drive.selector = SELECTOR_DRIVE;
        drive.launch_armed = false;
    }
    change
}

/// HUD/audio view of the drivetrain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriveTelemetry {
    /// 1..=`ARCADE_MAX_GEARS`; `GEAR_CODE_REVERSE` / `GEAR_CODE_NEUTRAL` for
    /// R / N; 0 means the active vehicle model has no gearbox.
    pub gear: u8,
    /// Engine speed in 0..=1 (project estimate).
    pub rpm01: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tap(drive: &mut DriveState, up: bool, down: bool, gears: u8) -> SelectorChange {
        let change = update_selector(
            drive,
            &DriverInput {
                shift_up: up,
                shift_down: down,
                ..DriverInput::default()
            },
            gears,
        );
        // Release between taps.
        update_selector(drive, &DriverInput::default(), gears);
        change
    }

    /// Selector position as the HUD shows it: R, N or the gear digit.
    fn label(drive: &DriveState) -> String {
        match drive.selector {
            SELECTOR_REVERSE => "R".into(),
            SELECTOR_NEUTRAL => "N".into(),
            _ => drive.gear.to_string(),
        }
    }

    #[test]
    fn manual_ladder_moves_one_step_per_tap_and_clamps_at_both_ends() {
        let mut d = DriveState::fresh(Transmission::Manual);
        assert_eq!(label(&d), "N");
        assert!(!d.launch_armed);
        let mut seen = Vec::new();
        for _ in 0..9 {
            tap(&mut d, true, false, 6);
            seen.push(label(&d));
        }
        assert_eq!(seen, ["1", "2", "3", "4", "5", "6", "6", "6", "6"]);
        seen.clear();
        for _ in 0..9 {
            tap(&mut d, false, true, 6);
            seen.push(label(&d));
        }
        assert_eq!(seen, ["5", "4", "3", "2", "1", "N", "R", "R", "R"]);
    }

    #[test]
    fn manual_up_from_neutral_engages_first_even_after_reverse() {
        let mut d = DriveState::fresh(Transmission::Manual);
        tap(&mut d, false, true, 5);
        tap(&mut d, false, true, 5);
        assert_eq!(label(&d), "R");
        let mut seen = Vec::new();
        for _ in 0..5 {
            tap(&mut d, true, false, 5);
            seen.push(label(&d));
        }
        assert_eq!(seen, ["N", "1", "2", "3", "4"]);
    }

    #[test]
    fn automatic_ladder_is_r_n_drive_and_a_in_drive_does_nothing() {
        let mut d = DriveState::fresh(Transmission::Automatic);
        assert_eq!(d.selector, SELECTOR_DRIVE);
        let mut seen = Vec::new();
        for (up, down) in [
            (false, true),
            (false, true),
            (false, true),
            (true, false),
            (true, false),
            (true, false),
            (false, true),
            (false, true),
        ] {
            tap(&mut d, up, down, 5);
            seen.push(label(&d));
        }
        // Z, Z, Z, A, A, A, Z, Z walk the ladder: N, R, R, N, 1, 1, N, R.
        assert_eq!(seen, ["N", "R", "R", "N", "1", "1", "N", "R"]);
    }

    #[test]
    fn shift_requests_are_accepted_at_any_time_and_held_keys_count_once() {
        let mut d = DriveState::fresh(Transmission::Manual);
        let held = DriverInput {
            shift_up: true,
            ..DriverInput::default()
        };
        for _ in 0..40 {
            update_selector(&mut d, &held, 6);
        }
        assert_eq!(label(&d), "1");
    }

    #[test]
    fn forward_shifts_are_reported_for_the_shift_pause() {
        let mut d = DriveState::fresh(Transmission::Manual);
        assert!(!tap(&mut d, true, false, 6).forward_shift); // N -> 1 (pulling away)
        assert!(tap(&mut d, true, false, 6).forward_shift); // 1 -> 2
        assert!(tap(&mut d, false, true, 6).forward_shift); // 2 -> 1
        assert!(!tap(&mut d, false, true, 6).forward_shift); // 1 -> N
        assert!(!tap(&mut d, false, true, 6).forward_shift); // N -> R
    }

    #[test]
    fn throttle_in_the_start_neutral_selects_drive_only_for_the_automatic_box() {
        let throttle = DriverInput {
            throttle: 1.0,
            ..DriverInput::default()
        };
        let mut auto = DriveState::fresh(Transmission::Automatic);
        auto.selector = SELECTOR_NEUTRAL;
        auto.launch_armed = true;
        update_selector(&mut auto, &throttle, 5);
        assert_eq!(auto.selector, SELECTOR_DRIVE);
        let mut manual = DriveState::fresh(Transmission::Manual);
        manual.launch_armed = true;
        update_selector(&mut manual, &throttle, 5);
        assert_eq!(manual.selector, SELECTOR_NEUTRAL);
    }
}
