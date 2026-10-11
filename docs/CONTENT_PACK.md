# Racer Content Pack (RCP) v1

A Racer Content Pack is a self-describing bundle of racing content: tracks,
cars, vehicle parameters, audio and HUD art. The engine loads packs through one
neutral schema, so content can come from hand-authored files, generators, or
third-party converters.

The reference implementation lives in `crates/pack-schema` (crate
`racer-pack-schema`). It provides serde types, JSON Schema export
(`json_schemas()`) and validation (`Pack::validate()`).

## 1. Layout

A pack is a directory (or an archive with the same layout):

```
pack.json                 manifest
tracks/<id>.json          one document per track
cars/<id>.json            one document per car
vehicles/<id>.json        one document per vehicle definition
audio/<n>.json            audio sets
hud/<n>.json              HUD sets
ai/<id>.json              computer-driver parameter sets
buffers/*.bin             little-endian binary buffers
```

All paths are relative, use forward slashes and must not contain `..`, a
leading `/`, a drive prefix or empty components. Document files are listed in
`pack.json`; every other file is a buffer file.

## 2. Conventions

- Units: metres, seconds, kilograms, newtons, radians (fields ending `_deg`
  are degrees). Rates end in `_hz`.
- Coordinates: right-handed, +Y up. A local "forward" is -Z. Track headings
  (`heading_rad`) are measured counter-clockwise seen from above.
- Colours: linear RGBA floats in [0, 1], or 8-bit RGBA where a field says so.
- All binary data is little-endian.
- All JSON objects reject unknown fields. Use `model.params` (vehicles) for
  extension data.

## 3. Manifest (`pack.json`)

| Field | Type | Notes |
| --- | --- | --- |
| `schema` | string | Must be `"rcp/1"`. |
| `id` | string | 1-64 chars of `a-z 0-9 - _ .` |
| `name` | string | Display name. |
| `version` | string? | Pack version. |
| `license` | string | SPDX expression covering the contents. |
| `author` | string | |
| `description` | string? | |
| `documents` | object | `tracks`, `cars`, `vehicles`, `audio`, `hud`, `ai`: arrays of `.json` paths. |

## 4. Binary buffers

Every document that carries binary data has a `data` table modelled on glTF:

- `buffers[]`: `{ uri, byte_length }`, a file in the pack.
- `views[]`: `{ buffer, byte_offset, byte_length, byte_stride? }`.
- `accessors[]`: `{ view, byte_offset, component, kind, count, normalized }`.

`component` is one of `u8`, `i16`, `u16`, `u32`, `f32`; `kind` is `scalar`,
`vec2`, `vec3` or `vec4`. `normalized` maps unsigned integers to [0, 1] and
signed to [-1, 1]. Accessors are referenced by index. Validation checks:
buffer file present with the declared length, view inside buffer, accessor
elements inside view (honouring stride), component alignment, expected
component/kind/count per use (for example UVs must be `f32 vec2` with one
element per vertex) and index values below the vertex count.

## 5. Track

Fields of `tracks/<id>.json`:

- `chunks[]`: `{ mesh, bounds? }`. World geometry split into chunks.
- `materials[]`, `textures[]`, `prop_meshes[]`.
- `props[]`: instances `{ mesh, transform, kind, animation_rate_hz?, trigger_radius_m? }`,
  `kind` is `static`, `animated` (needs a rate) or `triggered` (needs a radius).
- `sky?`: `backdrop` is `{type:"panorama", strips:[texture]}` or
  `{type:"gradient", zenith, horizon}`; plus `horizon` (`height_fraction`,
  `parallax`, optional haze) and `fill_above` / `fill_below` colours.
- `centerline[]`: segments `{ position, forward, up, length_m, left_width_m, right_width_m, surface }`.
  `forward` and `up` form the frame (banking and slope). `closed` joins the
  last segment to the first.
- `walls[]`: polylines `{ points, height_m, side, closed }`.
- `surfaces[]`: `{ name, friction_class, friction, rolling_resistance }` with
  `friction_class` one of `paved wet loose grass ice rough`. Triangles
  (`mesh.triangle_surface`, a `u8` per triangle) and centerline segments index
  this table.
- `spawn_slots[]`: `{ position, heading_rad }`, ordered by grid position.
- `checkpoints[]`: ordered gates `{ position, normal, width_m }`; index 0 is
  the start/finish line.
- `variants`: `{ allow_mirror, allow_reverse }` declares that the engine may
  mirror the track left/right or drive it backwards.
- `default_laps`.
- `ai?`: computer-driver data of the track (section 12): speed profile, grid,
  late-race pace and pace trim.
- Each centerline segment may carry `lanes?` (section 12). Both are optional;
  without them the engine derives lanes and speeds from the centerline.

Generated tracks (`racer-trackgen`, `docs/TRACKGEN.md`) fill `ai` with a speed profile from curvature (racing line), a grid and a lane layout per segment.

### Mesh

`{ name, positions (f32 vec3), normals? (f32 vec3), uv? (f32 vec2), colors? (u8 or f32 vec4), indices (u16/u32), primitives[], triangle_surface? }`.
Triangle lists only. Each primitive `{ material, first_index, index_count }`
selects a range drawn with one material.

### Material and texture

- Texture: `{ name, width, height, pixels, wrap_u, wrap_v, smooth }`; `pixels`
  is a normalized `u8 vec4` accessor with `width*height` elements, row-major
  from the top-left. Maximum edge 8192.
- Material: `{ name, texture?, base_color, alpha_mode, alpha_cutoff, double_sided, unlit, animation? }`.
  `alpha_mode` is `opaque`, `cutout` (discard below `alpha_cutoff`) or `blend`.
  `animation` is `{ frames: [texture], rate_hz, looping }` for animated
  textures (at least two frames).

## 6. Car

`cars/<id>.json` describes appearance and anchors:

- `lods[]`: `{ mesh, max_distance_m? }`, most detailed first; distances
  increase and only the last may omit one.
- `material_slots[]`: `{ name, material, paintable }`. Primitive `material`
  indexes this list.
- `textures[]`.
- `paint?`: `{ texture, indexed_pixels, base_palette, options[] }`.
  `indexed_pixels` is a `u8` scalar accessor with one palette index per pixel
  of the target texture (255 = transparent). Each option has a `name` and
  `kind`: `recolour` (`entries: [{index, color}]` replaces selected palette
  entries) or `full_palette` (`colors`, same length as `base_palette`).
- `wheels[]`: `{ name, position, radius_m, width_m, steered, driven }`.
- `wheel_mesh?`: one shared wheel mesh, authored at the origin with its axis
  along +X and its outer face towards +X. Engines instance it at every wheel
  anchor (rotated 180 degrees about +Y when the anchor's X is negative), so a
  renderer can spin and steer wheels apart from the body. Primitives index
  `material_slots` like the body mesh.
- `lights[]`: `{ kind (head brake reverse indicator), position, direction, color }`.
- `cameras[]`: `{ name, position, forward, fov_deg }`.
- `mirrors[]`: `{ name, position, forward, width_m, height_m }`.
- `collision`: `{ half_extents_m, center }`.

## 7. Vehicle definition

`vehicles/<id>.json`: `{ id, name, car?, arcade?, model? }`; at least one of
`arcade` or `model` is required.

`arcade` is `ArcadeVehicleParams` (SI): `mass_kg`, `wheelbase_m`,
`track_width_m`, `wheel_radius_m`, `peak_torque_nm`, `idle_rpm`,
`redline_rpm`, `gear_ratios[]` (non-increasing), `reverse_ratio`,
`final_drive`, `drivetrain_efficiency`, `max_brake_force_n`, `drag_area_m2`,
`downforce_area_m2`, `rolling_resistance`, `tyre_grip`,
`max_steer_angle_rad`, `steer_falloff_speed_mps`, `max_speed_mps`,
`front_drive_bias`, and three optional handling fields with defaults:
`steer_rate_per_s` (steering travel towards the input, full lock per second,
default 3), `steer_return_per_s` (return to centre after release, default 5)
and `lateral_grip_per_s` (rate at which the tyres remove sideways velocity,
default 4; lower values let the body point further past the direction of
travel while steering). Validation also checks that the top gear reaches
`max_speed_mps` before the redline.

The engine's default `arcade` vehicle model (`racer_sim::arcade_model`)
is driven by exactly these fields: a torque-curve engine behind an automatic
gearbox with hysteresis, grip-limited drive and braking, aerodynamic and
rolling drag, a steering lock that falls with speed (halved at
`steer_falloff_speed_mps`) and is limited to what the tyres can hold, a
first-order lateral tyre response (the body leads the travel direction while
a turn is held and settles after release), a modest handbrake slide, and a
governor that holds `max_speed_mps` (reverse is capped at a quarter of it).
Reverse uses the same gear lanes as every model: shift down from neutral at
standstill.

`engine_sound?` is `EngineSynthParams`, a generic additive engine voice used
when no sampled audio is loaded: `idle_rpm`, `redline_rpm`, `cylinders` (the
firing frequency is `rpm / 60 * cylinders / 2` hertz), `harmonics[]` (1 to 8
relative levels of harmonics 1..), `filter_hz_idle` / `filter_hz_redline`
(low-pass cutoff), `noise_gain`, `gain_idle` / `gain_redline`. The browser
synthesiser is `web/engine-synth.js`.

`ai?` gives computer drivers the per-car limits (section 12).

`model` is an escape hatch, `{ id, params }`. The engine does not interpret
`params`; it hands the JSON text to the registered vehicle-model provider with
that id (section 10).

## 8. Audio

`audio/<n>.json`: `{ clips[], engine?, cues[] }`.

- Clip: `{ id, sample_rate, channels (1|2), format, samples, loop_points? }`.
  `format` is `pcm16` (`i16` accessor) or `float32` (`f32` accessor). Samples
  are interleaved. `loop_points` is `{ start_frame, end_frame }` with
  `start < end <= frames`.
- `EngineSoundParams`: `{ idle_rpm, max_rpm, layers[] }`. A layer is
  `{ clip, throttle (on|off), reference_rpm, gain_curve, pitch_curve }`:
  layers marked `on` fade in under throttle, `off` layers on lift-off.
  Curves are piecewise-linear `{ points: [[rpm, value], ...] }` with strictly
  increasing x; gain in [0, 1], pitch is a playback-rate multiplier above 0.
- Event cues: `{ kind (skid impact horn), clip, gain, pitch, cooldown_s }`.

## 9. HUD

`hud/<n>.json`:

- `atlas`: a texture; `rects`: named pixel rectangles inside it.
- `reference_resolution`: `[w, h]` the layout is authored for.
- `layout[]`: `{ id, anchor, offset, scale, content }`. `anchor` is one of
  nine screen anchors (`top_left` ... `bottom_right`); `offset` is in
  reference pixels (+x right, +y down). `content` is `sprite {rect}`,
  `gauge {gauge}` or `number {strip, source, digits}`.
- `gauges[]`: `{ id, style (needle|bar), source, min, max, rect, angle_min_deg, angle_max_deg, pivot }`.
- `digit_strips[]`: `{ id, glyphs, rects[], spacing_px }`, one rect per glyph.

Value `source` names (for example `speed`, `engine_rpm`, `lap`) are supplied
by the engine at runtime.

## 10. Plugin API

### Browser: `window.RacerPackLoader`

```js
window.RacerPackLoader.register({
  id: "my-loader",                       // a-z 0-9 . _ -
  detect(files) { /* sync, cheap */ },   // files: [{ path, size, read(): Promise<ArrayBuffer> }]
  async load(ctx) { /* ... */ },         // ctx: { files, signal, progress(fraction, label) }
});
```

`register` returns an unregister function and throws on a malformed or
duplicate loader. `load` must resolve to a pack object whose
`manifest.schema` is `"rcp/1"`; loaders must not upload or persist the input
files. The registry also exposes `list()`, `detect(files)` and
`load(files, { loader?, signal?, progress? })`. The module is
`web/pack-loader.js`; it is not wired into the import UI yet.

### Rust: content providers

`racer_content::provider::ContentProvider` (`id`, `detect(&dyn FileSource)`,
`ingest(&dyn FileSource) -> Result<Pack, IngestError>`) and
`ProviderRegistry`. The built-in `RcpProvider` loads native packs.

### Rust: vehicle models

`racer_sim::model_ext::{VehicleModelProvider, VehicleDynamics, VehicleModelRegistry}`.
A provider has a stable `id()` matching `model.id` and builds a
`VehicleDynamics` from the opaque `params` JSON text; the instance advances
one fixed step at a time and must be deterministic. The engine retains
ownership of collision, support, bounds and race progression.

## 11. Validation

`Pack::validate()` reports every problem at once with a path such as
`tracks[0].checkpoints` and a message. It checks manifest fields and paths,
buffer presence and ranges, accessor types and counts, cross references
(materials, textures, surfaces, props, clips, rects, cars), finite numbers,
unit-length vectors, positive dimensions and counts, and unique ids.

## 12. Computer drivers

Computer-controlled cars (generic crate `racer-ai`) read the following data.
Everything is optional: a pack without it still races, with lanes derived from
the centerline widths, corner speeds derived from centerline curvature, a
two-by-two lane road and the engine's neutral defaults. Requirements and
rationale: `docs/AI_DRIVERS.md`.

### Per-segment lanes (`centerline[].lanes`)

`{ left_count, right_count, left_width_m, right_width_m, availability_mask?, preferred_lane? }`

- Counts are driving lanes left and right of the centre line (0..=7, at least
  one in total); widths are metres per lane and are ignored on a side with no
  lanes.
- Lane **slots** are numbered 0..=13 from the far left; slot 7 is the first lane
  right of the centre line, slot 6 the first on the left. The lane centre offset
  is `-((6 - s) + 0.5) * left_width` for `s <= 6` and
  `((s - 7) + 0.5) * right_width` otherwise (negative = left).
- `availability_mask`: bit `1 << (14 - s)` is set when slot `s` exists on the
  segment, shoulder slots (one just outside each side's driving lanes) included.
  Only bits 1..=14 may be set. When absent the driving lanes plus one shoulder
  slot per side exist. Shoulders are available but discouraged.
- `preferred_lane`: slot 0..=13 used as a small bias, or none.

### Track data (`track.ai`)

- `default_speed_mps`: target speed where no profile entry exists (default 50).
- `speed_profile`: `{ segments_per_entry, forward_mps[], reverse_mps? }`. One
  whole-metre-per-second target per `segments_per_entry` segments, at least
  enough entries to cover every segment. `reverse_mps` (indexed from the end of
  the track) is used when racing backwards; without it the forward profile is
  mirrored. The profile encodes corner speeds; the driver does not read
  curvature when a profile is present.
- `grid`: `{ start_segment, spacing_segments }`. Grid position 0 is the rearmost
  slot; positions are `spacing_segments` apart, alternating between the lane
  left of and the lane right of the centre line (a road with no left lanes uses
  two columns centred on half of the right lanes); the highest position is on
  pole; racing backwards flips order and parity.
- `late_pace` (one factor per skill level) and `late_pace_after_segments`
  (default 100): pace factor applied after that many cumulative segments.
- `pace_trim`: `{ after_segments, factor }`.

### Per-car limits (`vehicle.ai`)

`speed_cap_mps` (the driver never aims above it and stops accelerating beyond
it), `acceleration_mps2[]` (by whole m/s of speed; absent = derived curve),
`brake_mps2`, `lateral_accel_cap_mps2`, `yaw_accel_cap_rad_s2`,
`misalignment { start_deg, full_deg, max_slowdown }` (speed is reduced between
the two heading-error thresholds when the car points away from the road
direction) and `shift_pause_updates`.

### AI parameter set (`ai/<id>.json`)

`{ id, name, personalities[], catch_up, lane_weights, gains }`:

- `personalities[]`: `speed_factor[beginner, advanced]`, `accel_factor[2]`,
  `handling_adjust`, `blocker_look_ahead_m`, `lane_change_speed_limit_mps`,
  steering look-ahead (`steer_look_ahead_per_mps`, `_min_rows`, `_max_rows`),
  `lane_slack_m[4]` with `slack_redraw_one_in`, `speed_glue[21]` and
  `accel_glue[21]` catch-up curves (index 10 = level, one step per
  `catch_up.step_segments`; lower indices are behind the leading human) and
  optional `aggression { contact_ticks, duration_ticks }` (advanced skill only).
  Opponent `k` (1-based) uses personality `(k - 1) % len`.
- `catch_up`: `speed_scale`, `accel_scale` (applied beyond two steps),
  `step_segments`, `fade_in_segments[3][4]` per lap class (<=2, <=4, more laps),
  `clamp_range` and `clamp_within_segments` (pace clamp while a human is near),
  and a launch assist (`start_boost_segments`, `start_boost_max`). A project
  that wants no rubber-banding uses curves of 1.0, fade thresholds of 0 and no
  boost.
- `lane_weights`: penalty weights of the lane choice (absent now / ahead / near,
  shoulder, edge, adjacent car, blocker speed, obstacle, hint, block human).
- `gains`: `normal` and `recovery` steering gain sets (lateral position to
  velocity with cap, lateral velocity to acceleration, heading error to yaw rate
  with cap, yaw rate to yaw acceleration, low-speed limits).

### Engine behaviour (summary)

Decisions run at 7.5 Hz per car (staggered), motion every 60 Hz tick. A decision
picks one lane at most (left, current, right; ties keep the lane), composes the
target speed (profile x cap x difficulty x personality x catch-up x misalignment
x trim, clamped near humans, with the launch assist) and limits it by blockers;
a recovery state reverses for a bounded time when the car is stuck or off the
road. Opponents are servo-driven (`VehicleModel::Scripted`): the driver supplies
forward, lateral and yaw accelerations and the shared integrator, walls,
checkpoints and contact pass do the rest. Skill is 0 (beginner) or 1 (advanced);
the catch-up option turns the curves on or off.

Validation (`Pack::validate`) checks lane counts, widths, mask bits, preferred
lanes, profile coverage and positivity, grid indices, late-pace and trim ranges,
vehicle limits, personalities (curve lengths, ranges, look-ahead order), catch-up
thresholds and gain ranges, with paths such as
`tracks[0].centerline[3].lanes.availability_mask`.
