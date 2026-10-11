# Classic handling model

`VehicleModel::Classic` (`crates/sim/src/classic_model.rs`) is the default handling of project cars in solo drives, rooms and for computer opponents' limits. It is a set of tuned response curves in the style of classic arcade racers, not a physical simulation. The generic `Arcade` model stays selectable as a fallback (`racer_content::classic_params::ProjectHandling`).

## What it does

- Launch with a short clutch delay, a five or six speed gearbox with a speed-driven automatic mode, and a short drive pause on each shift.
- A manual mode in which each gear holds its speed at the rev limit without cycling; a gear that is too low for the road speed pulls the car down to its own limit.
- Very strong brakes, strong engine drag when the throttle is lifted, aerodynamic-only coasting in neutral.
- Steering that is linear in the steering command (ramping +16 per tick up and -32 per tick down on a 124 scale at a 32 Hz tuning tick, with a one tick delay), a flat yaw rate at low speed, a sharp drop at a fixed knee speed and then a smooth fall. Per car the above-knee level scales with grip and steering gain.
- The body leads the direction of travel by exactly one tick of yaw; the chase camera in the renderer lags the body heading (first order, gain 0.365 per tick).
- Handbrake slides, reverse at any speed (a forward-rolling car is braked through zero), and an R / N / 1.. selector ladder.
- Road-edge walls are model independent (`crates/sim/src/track_edge.rs`): restitution 0, tangential friction 0.73 of the normal speed removed per tick, heading turning toward the wall by 0.19 of the angle per tick.

## Per-car parameters

`ClassicConfig` (RCP vehicle `classic` block, `ClassicVehicleParams`): shift points (`upshift_mps`, `downshift_mps`), `top_speed_mps`, `accel_scale`, `coast_scale`, `brake_scale`, `shift_seconds`, `reverse_speed_mps`, `steer_gain`, `grip`, `low_speed_yaw_rate`, `yaw_boost_speed_mps`. The four demo cars of `racer-cargen` carry values inside the tuned ranges listed under `car_ranges` in `tests/feel/classic-targets.json`; the first one is the reference car.

## Feel targets

`tests/feel/classic-targets.json` holds the numeric feel targets (times to speed, shift speeds, coast and brake rates, steering law, handbrake slide, reverse, gearbox modes, wall contact, parameter ranges). They are checked by `crates/sim/tests/classic_model.rs`, `crates/sim/tests/classic_wall_contact.rs` and `crates/content/tests/classic_project_tracks.rs`. Changing an equation or a table means bumping `CLASSIC_MODEL_REVISION`, which is part of the simulation identity.

## Determinism

The model uses only IEEE `+ - * /`, `sqrt` and the `sin`/`cos` of the heading; powers and exponentials are evaluated with small series, so a server and a browser client compute the same values.
