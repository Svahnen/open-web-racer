# Development

## Prerequisites

- Stable Rust (see `rust-toolchain.toml`) with the `wasm32-unknown-unknown` target.
- `wasm-bindgen-cli`, pinned to the `wasm-bindgen` version in `Cargo.lock`.
- Node.js 20 or newer. Playwright browsers for the end-to-end tests: `npm install && npm run install:browsers`.

## Build and run

```sh
cargo build -p racer-client --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/racer_client.wasm \
  --target web --out-dir web/pkg --out-name racer_client
cargo run -p racer-server
```

Then open <http://127.0.0.1:4173/>. `node scripts/start-dev-server.mjs` performs the same three steps and runs the server; it is also what the Playwright configuration starts. Generated `web/pkg/` output is git-ignored.

## Everyday workflow

1. Make a focused change.
2. Run the narrowest relevant check (`cargo test -p racer-sim`, `node --test tests/web/<file>`), then broaden before committing.
3. Keep commits small and descriptive.

## Code conventions

Stable Rust, `rustfmt` clean, `clippy -D warnings` for project code, structured errors for malformed input, no `unsafe` (the workspace forbids it), and no panics on ordinary malformed network or content input. Comments explain invariants and rationale.

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm test
npm run test:e2e
```

See [TESTING.md](TESTING.md) for the test layers. Report failures accurately, and do not claim a check passed without its output.

## Content rules

Only add code and content you wrote or have the right to use. See [LEGAL.md](LEGAL.md).
