# Testing

## Layers

- **Simulation** (`cargo test -p racer-sim`): deterministic scripted maneuvers, ordered checkpoint and lap progression, finite state over long runs, seeded bounded-input invariants, collision bounds, reset behavior, circuit parser rejection. Tests use an in-tree deterministic generator with fixed seeds; no host randomness or time enters the simulation inputs. Same-process runs of one build and input sequence are compared for exact state equality; cross-platform bit-identical floating point is not promised.
- **Protocol and server** (`cargo test -p racer-protocol -p racer-server`): malformed and oversized message handling, authority invariants, race lifecycle, disconnect cleanup, snapshots, computer opponents, generated-track rooms, plugin directory serving.
- **Computer drivers** (`cargo test -p racer-ai`): behaviour tests and a race simulation with seven opponents on the test circuit (movement, staying on the road, finishing, determinism, skill ordering, bounded reversing).
- **Generators** (`cargo test -p racer-trackgen -p racer-cargen`): determinism, validity and content-pack schema checks; see [TRACKGEN.md](TRACKGEN.md).
- **Web unit tests** (`npm test`, which runs `node --test tests/web/*.test.mjs`): HUD model, input handling, audio and engine synthesis, pack loader, plugin registry, multiplayer state and settings.
- **Browser end to end** (`npm run test:e2e`): Playwright on Chromium and Firefox against a local server.

## Browser tests

Build the WASM client and run the suite:

```sh
cargo build -p racer-client --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/racer_client.wasm --target web --out-dir web/pkg --out-name racer_client
npm run test:e2e
```

The Playwright configuration starts `scripts/start-dev-server.mjs` unless `RACER_BASE_URL` points at an already running server. The suite requires the WASM-backed `wgpu` renderer (WebGPU or WebGL2), checks framebuffer colors for the ground, track, gates and car, then exercises resize, keyboard input, reset, and mocked Gamepad throttle/steering, solo races against computer opponents, generated tracks, and multiplayer rooms. In headless browsers without a WebGPU adapter the WebGL2 path is selected; the Canvas 2D renderer is a final runtime fallback and does not satisfy the GPU assertions.

## Trace CSV format and comparison

A project-defined plain-text format for comparing vehicle-model output against recorded or expected traces. One row per simulation tick after the step:

```text
# tick,x,y,heading_radians,speed,lap,checkpoint
1,-19.99,0,0,0.1996,1,1
```

- The header is the first line beginning `tick,` (optionally prefixed `# `); other `#` lines are comments. The first column is the integer `tick`; all other columns are finite decimal numbers. Channels are matched by name, so extra or reordered columns are fine but unmatched channels are structural failures. Rows are joined on `tick`.
- Sim side: `racer_sim::trace_harness::run_script_trace_csv(&mut world, &[InputSegment::accelerate(n), ...])` produces this format from a `RaceWorld`.
- `cargo run -p racer-trace-compare -- expected.csv actual.csv --tol 1e-9 --channel-tol speed=1e-6` prints per-channel max error (and tick), RMS error, and the first tick exceeding tolerance. Exit code 0 pass, 1 tolerance or structure failure, 2 usage or parse error.
- `RaceWorld::set_vehicle_model` selects the vehicle model (provisional by default; arcade and custom models are available). `racer_sim::fabricated_car` holds an invented car used by model, state-frame, protocol, server and client tests.

## Rules

Run the narrowest relevant check after a change and broaden before committing. Report failures accurately and distinguish an unavailable tool from a failed check. Do not claim determinism, performance or browser support beyond observed evidence.
