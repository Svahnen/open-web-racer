# Procedural tracks (`racer-trackgen`)

`crates/trackgen` (crate `racer-trackgen`) generates complete, project-owned
[RCP v1](CONTENT_PACK.md) tracks from a seed, spline control points and a theme.
Everything is computed (noise, shapes, simple geometry); no external images,
models or data are involved. Licence of the generated content: CC0-1.0.

## Determinism

The same `TrackSpec` always yields the same bytes. All floating point work is
IEEE basic operations; sine, cosine and arctangent are implemented in the crate
(`math.rs`) instead of calling the platform libm, noise and the random stream
are integer based. `TrackSpec::fingerprint()` (hex SHA-256 over the spec and
`GENERATOR_VERSION`) identifies the content for room identity checks; bump
`GENERATOR_VERSION` whenever output changes for an unchanged spec.

## What a generated track contains

| Feature | Notes |
| --- | --- |
| Road | closed Catmull-Rom loop, 4 m sections, smoothed elevation, curvature-driven banking, per-point width |
| Markings | road texture with edge lines, dashed centre line and tyre wear; start/finish chequer strip |
| Kerbs, shoulders | striped kerb (1.2 m), sloping shoulder (sand, gravel, concrete or red dirt by theme) |
| Barriers | continuous box barrier on both sides (concrete, rail, wood or stone texture) with footing; matching `walls[]` polylines |
| Terrain skirt | height field blended from road level to a theme field (hills, mesas, sea) plus noise; two tile resolutions with stitched seams; per-cell grass, verge, sand or rock |
| Surfaces | `surfaces[]` (asphalt, kerb, shoulder, grass, sand, rock, concrete, water) and one `triangle_surface` id per triangle |
| Props | instanced low-poly trees (full pines, cheap far pines for the distant forest), palms, bushes, boulders, mesas, cacti, chevron and warning signs, rock-cutting faces, log cabins, lamps, grandstands, houses, lighthouse, buildings, warehouses, containers, cranes, tanks and the start gantry; all meshes are generated |
| Structures | `structures.rs` builds a tunnel through a rocky mound (Pine Ridge, Red Mesa) and a stone overpass (Pine Ridge, Coastline) from the road frames as world-space prop meshes; positions are measured from the designed start so `-sNN` variants show them in the same places |
| Guard rails | Pine Ridge swaps its wooden fence for a steel guard rail where the ground falls away or on the outside of tight turns |
| Textures | 27 RGBA8 textures (road, kerb, shoulder, grass, verge, sand, rock, water, barrier, corrugated, bark, foliage, plaster, windows, crowd, chevron, checker, snow, dirt, log, signs, armco, stone, 4 sky strips) |
| Sky | 4-strip procedural panorama (gradient, sun or moon, stars, clouds, hill silhouettes, a snow-capped far range on Pine Ridge) and zenith/ground fill colours |
| Fog and grading | the horizon haze colour and distance become `RcpTrackScene::fog`; the renderer fades geometry into the haze colour and applies warm-sun / cool-shade lighting to project scenes only (`SceneFog`, no effect on imported scenes) |
| Start grid | 8 slots in two staggered columns behind the line; slot 0 is the pole |
| Checkpoints | ordered gates about every 380 m, index 0 is the start/finish line |
| AI | per-segment `lanes` (two driving lanes per side), `track.ai` with a target-speed profile derived from curvature, banking and acceleration/braking limits (one entry per 2 segments), and the grid; `racer-ai` drives it |
| Variants | `allow_mirror` / `allow_reverse` are set; `TrackSpec::mirrored()` and `reversed()` generate those layouts (`<id>-mirror`, `<id>-reverse`), `starting_at(n)` moves the start to n% of the lap (`<id>-sNN`, used for scenic screenshots) |

## Demo tracks

| Id | Name | Theme | Length |
| --- | --- | --- | --- |
| `coastline-run` | Coastline Run | seaside: sand, palms, villas, sea, lighthouse | 3.3 km |
| `pine-ridge` | Pine Ridge | alpine forest: climbs 49 m, banked hairpins, dense pines, cabins, rock cuttings, guard rails, a tunnel, a stone overpass, snow-capped peaks | 4.8 km |
| `red-mesa` | Red Mesa | desert plateau: red rock, mesas, cacti, wide road | 3.5 km |
| `night-harbour` | Night Harbour | floodlit port after dark: containers, cranes, lamps, water | 2.3 km |

## Packs are generated, not committed

Generation takes well under a second per track, so the packs are produced at
run time instead of being committed: the buffers would be 5-8 MiB of `.bin`
each (which `scripts/provenance_check.py` and `.gitignore` reject on purpose).
The engine calls `racer_content::project_tracks::load_project_track(id)`,
which generates the pack files, passes them through the generic
`ProviderRegistry` / `RcpProvider` ingest (parse, validate) and converts the
result with `racer_content::rcp_track::build_track_scene`. To inspect a
pack on disk:

```text
cargo run -p racer-trackgen -- all out/ --pack --map
```

writes `out/<id>/pack.json`, `tracks/<id>.json`, `buffers/<id>.bin` and
`<id>-map.png` (top-down layout).

## Engine ingest (subset)

`rcp_track.rs` implements the part of RCP v1 these tracks need:

* chunks and props become scene meshes/instances; yaw-only, uniformly scaled
  props are instanced, other transforms are baked; axes map RCP (x, y up, z)
  to the renderer's (x, -z, up);
* textures are resampled to one layer size (bilinear); untextured materials use
  a white layer; `cutout` uses the renderer's alpha discard, `blend` straight
  alpha; `unlit` is approximated by pointing normals at the light;
* the panorama sky becomes a camera-centred dome of background meshes;
* drivable ground is every triangle near the centerline surface (inside the
  corridor of `left_width_m` / `right_width_m`, within 0.6 m of the road plane);
  centerline widths become the road-edge wall rows, `walls` become wall
  segments, gates become circular checkpoints, the simulation frame is anchored
  at spawn slot 0;
* not interpreted: friction classes of surfaces (the simulation has one grip
  model), texture animation, animated and triggered props, gradient skies,
  car/vehicle/audio/HUD documents.

Computer drivers (`racer-ai`) use the track document kept in
`RcpTrackScene::pack_track` (lanes, speed profile, grid) with road frames from
the simulation-frame edge rows (`AiTrack::from_pack_track_frames`); opponents get
the track's ground collision (`Opponents::install_collision`) so they follow
relief and respect the road-edge walls. Solo (`&ai=N`) and server rooms
(`ai_count`) both race opponents on generated tracks. The content test drives a
full lap on every track with a pure-pursuit controller on the centerline.

## Selecting tracks

* Solo: `?practice=local&track=<id>` or the track menu next to "Drive solo". The menu page previews Pine Ridge in the lobby view and preselects it for "Drive solo" (`MENU_PREVIEW_TRACK` in `web/bootstrap.js`); joining a room on the test circuit swaps the preview back to the circuit, and `?practice=local` without `track=` still opens the test circuit. Automated browsers (`navigator.webdriver`, software GL) keep the light circuit unless `?menupreview=1` is given; `?menupreview=0` switches the preview off.
* Cars: the demo-car selector works on generated tracks too. The player drives the chosen car and paint and computer opponents drive other models in other paints (`install_project_track(id, car, paint, opponents)` registers the field as render profiles on the track's texture page; changing the selection reinstalls the scene with `reinstall_project_track_cars`). In rooms every vehicle is drawn with the car and paint its `VehicleSnapshot` names (protocol 10); the page registers missing profiles on demand and rebuilds the scene when more than eight distinct cars have been seen. Wheels are still baked into the body mesh (no spin or steering).
* Rooms: the lobby's track menu lists the generated tracks. The server accepts
  `track-profile-rcp-<id>` with content identity `project-generated-tracks-v1`
  and the track fingerprint from `project_track_catalog()`, uses the project
  default car, gives each player a grid slot and runs gates, laps, walls and
  ground collision authoritatively.
* `racer-trackgen` ids with `-mirror`, `-reverse` or `-sNN` suffixes work for
  solo (`track=` parameter) but are not offered in the lobby.

## Testing

```text
cargo test -p racer-trackgen                  # determinism, validation, geometry bounds
cargo test -p racer-content --test project_tracks   # ingest, lap driven on every track
cargo test -p racer-server --test project_tracks_ws # rooms over WebSocket
node --test tests/web/project-tracks.test.mjs
pwsh -File scripts/run-e2e-ephemeral.ps1 tests/e2e/project-tracks.spec.js --project=chromium
```

`scripts/run-e2e-ephemeral.ps1` starts the built server on a loopback ephemeral
port and points Playwright at it, so a long-running server on 4173 is never
used. The Chromium run of `project-tracks.spec.js` also writes 640 px chase-camera
screenshots and per-track frame rates to `test-results/track-screenshots`
(headless software GL, so absolute fps is low).
