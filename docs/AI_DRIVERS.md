# Generic AI driver: content requirements

Status: requirements note for the public engine. It is game-neutral: it lists what a computer-driver module needs from a content pack, so the engine can implement the behaviour once and any content pack (project-owned, or a private pack that supplies richer data) can feed it. Numeric values below are placeholders for the shape of the data, not tuning advice.

## 1. Scope

A computer driver chooses, several times per second, a lane on the road, a target speed and a lateral target, and then steers and brakes toward them. It needs road geometry, a pre-authored speed profile, per-car limits and a small set of tuning parameters. The simulation must stay independent of render rate and be authoritative on the server; the driver therefore runs on fixed simulation ticks (decisions may run at a lower rate than motion) and uses only integer or fixed-point arithmetic where determinism across machines matters.

## 2. Per-segment road data

The road is a sequence of segments (rows) of roughly constant length, with a frame (position, forward, right) per segment. For each segment the driver needs:

| field | meaning |
|---|---|
| lane count left / right | number of driving lanes on each side of the centre line (0 to about 5) |
| lane width left / right | width of one lane on each side, in a fixed unit (for example half-metres) |
| lane availability mask | one bit per lane slot of a fixed lane array (for example 14 slots) telling which lanes exist on this segment, including shoulder slots just outside the driving lanes |
| edge distances | distance from the segment position to the road edge on each side (for containment) |
| lane hint (optional) | preferred lane, or "none", used as a small bias |

Lane model: a fixed array of lane slots indexed from the far left to the far right, with the first slot right of the centre line at a known index. A lane's centre offset follows from the lane width of its side. The driver needs both directions: lane to lateral offset and lateral offset to lane (with clamping to the array), and a way to compute which lanes a car body covers. Shoulder slots are present in the mask but discouraged.

A content pack that has no per-segment lane data may derive it from a single road width (lane count and widths constant along the road).

## 3. Per-segment target-speed profile

An authored profile gives a target speed in whole metres per second for each pair of consecutive segments (one value per two segments is enough; finer is allowed). Separate profiles per driving direction and per handling style are allowed. A missing profile falls back to a constant. The profile encodes corner speeds: the driver does not derive speed from road curvature.

## 4. Per-car data

| field | meaning |
|---|---|
| speed cap | maximum speed the driver will attempt, per car type |
| acceleration table | maximum forward acceleration as a function of speed (for example one entry per whole metre per second, up to about 110 m/s), per handling style |
| braking limit | maximum deceleration for the driver (scalar per class; may scale with a pace factor) |
| lateral and yaw acceleration caps | per handling style |
| misalignment limits | two heading-error thresholds and a maximum fractional slow-down; speed is reduced between the thresholds when the car points away from the road direction |
| shift pause (optional) | number of updates with zero acceleration after a speed threshold crossing |

## 5. Difficulty and personality parameters

Per skill level (for example beginner and advanced) and per driver personality tier:

- a speed factor (1.0 = unscaled) per tier and skill;
- a late-race factor per track and skill, applying after a progress threshold;
- an acceleration factor per tier and skill (optional, used in some race types);
- per-track pace trims applied after a progress threshold, per handling style;
- personality parameters: look-ahead distances for blockers, lane-change speed limits, a handling adjustment factor for the misalignment term, look-ahead rows for steering, a few lateral slack offsets with a re-draw probability, and optional aggression parameters (contact count that triggers it, its duration) that only the higher skill level may use;
- lane-choice weights: a set of weights for the penalty terms listed in section 7.

## 6. Catch-up curve parameters

The driver may adjust its pace and acceleration to stay near the leading human. The content pack supplies:

- a speed glue curve: a table of factors indexed by the driver's progress lead or deficit relative to the leading human, in steps of a fixed number of segments (for example 21 entries covering plus or minus ten steps), per personality;
- an acceleration glue curve of the same shape, used only when a catch-up option is on;
- a global scale for each curve (product of several selectable scales: race type, lap class, handling style, skill, traffic present, several opponents), applied when the gap is large (more than a few steps), as the scale when the factor is above 1.0 and as its reciprocal otherwise;
- fade-in thresholds: for each lap-count class, four progress thresholds; the factor's deviation from 1.0 is applied at one quarter, one half, three quarters and in full as progress passes them;
- clamps: a range applied to the combined pace factor while any human car is within a given number of segments, and neutralisation rules when the leading human is stopped or held.

A project that wants no rubber-banding supplies curves of 1.0 and thresholds of zero.

## 7. Lane-choice scoring

The driver scores the lane to the left, the current lane and the lane to the right, and moves at most one lane per decision toward the highest score (ties keep the current lane). Score terms, each with a weight from the content pack:

- lane missing on this segment, or missing within a speed-dependent look-ahead, or missing at a nearer look-ahead point;
- shoulder lane, and road edge nearer than the car's half-width;
- another car close beside or immediately ahead in the neighbouring lane;
- slower car ahead in the lane within the look-ahead: penalty proportional to the speed excess over a cap derived from the other car's speed and the gap;
- obstacles ahead in the lane, proportional to closing speed;
- agreement with the lane hint (bonus);
- for skilled drivers, a bonus for blocking a human that is close behind.

Engines without a content pack can use a reduced set (road edges, lane existence, blockers by closing speed).

## 8. Grid slots

Race set-up supplies the lineup: for each car slot a grid position. The engine places grid positions at a fixed spacing of whole segments in two columns, one per side of the centre line (left column for even positions, right for odd positions; a road with no left lanes uses columns centred on half of the right lanes), with the highest position at the front, and flips the lateral parity when racing in reverse. The content pack supplies the start segment, the segment spacing between positions and the driving direction.

## 9. Control structure the engine implements itself

The pack does not need to provide code. The engine implements: progress and gap bookkeeping (cumulative segment count, signed wrap-around difference); target speed composition (profile, cap, misalignment, catch-up, difficulty, trim, proximity clamp, start boost); the finished-race roll-out rule (slow target while inside the road, near-stop otherwise); bang-bang longitudinal control with the tables above; proportional steering cascades toward a look-ahead point using gain sets for normal and recovery regimes (gains from the pack); recovery by reversing after wall contact or leaving the road; and a servo-style motion model that feeds the shared integrator and collision pipeline instead of the player's detailed vehicle model.

## 10. Gain sets

Two gain sets (normal and recovery), each with: lateral-position-to-lateral-velocity gain and cap, lateral-velocity-to-acceleration gain, heading-error-to-yaw-rate gain and cap, yaw-rate-to-yaw-acceleration gain, speed range and factors that limit lateral and yaw rates at low speed. Values come from the content pack.

## 11. What a pack may omit

Everything above has a neutral default so that project-owned content can start with constant lanes, a constant speed profile, factors of 1.0 and a small set of weights and gains, and add data incrementally.

## 12. Where each requirement lives in the engine

| section | schema (`docs/CONTENT_PACK.md`, section 12) | engine (`crates/ai`) |
|---|---|---|
| 2 road data, lane model | `centerline[].lanes` | `track.rs` (`RoadRow`, lane centre and lane-of-offset), `rules::lane_of_offset`, `rules::lane_bit` |
| 3 speed profile | `track.ai.speed_profile` | `AiTrack::section_speed`, derived from curvature when absent |
| 4 per-car data | `vehicle.ai` | `AiConfig`, `rules::accel_limit`, `rules::brake_limit`, `rules::misalignment` |
| 5 difficulty and personality | `ai/<id>.json` personalities, `track.ai.late_pace` | `rules::skill_factor`, `field.rs` |
| 6 catch-up | `catch_up`, personality curves | `rules::speed_catchup`, `rules::fade_in`, `rules::proximity_clamp` |
| 7 lane choice | `lane_weights`, `preferred_lane` | `field.rs` lane scores, `rules::pick_lane` |
| 8 grid | `track.ai.grid` | `AiTrack::grid_slot` |
| 9 control structure | (engine) | `rules::target_speed`, `field.rs` servo and recovery, `sim::scripted_model` |
| 10 gain sets | `gains` | `field.rs` servo |

The integer rules (sections 5 to 7 arithmetic) are Q16 fixed point and exact;
the lane scoring, servo and recovery use floats and are not bit-reproducible
across platforms. The server owns the drivers; clients only render and rank.
