# ADR 0001: Use direct wasm-bindgen output instead of Trunk

- Status: Accepted
- Date: 2026-10-06
- Deciders: project maintainers

## Context

The initial technical plan named Trunk. The client is a Rust `cdylib` built for `wasm32-unknown-unknown`; `wasm-bindgen` produces the JavaScript ES module imported by the existing plain JavaScript page. The page is served by the project’s Rust/Axum server or a static development host.

## Decision

Keep the small direct build pipeline: `cargo build --target wasm32-unknown-unknown`, run the pinned `wasm-bindgen-cli`, and serve the generated module from `web/`. Do not add Trunk to the toolchain.

## Rationale

The current source uses plain HTML, CSS, JavaScript, and one generated WASM module; it has no TypeScript compiler, frontend package graph, or asset-import pipeline for Trunk to manage. The direct commands already build the WASM artifact and generate the JavaScript loader consumed by the page. The project-owned Playwright server wrapper performs those same steps before browser tests. Adding Trunk would introduce a second build/orchestration layer without replacing a current need.

This decision is specific to the current static page and WASM module. Revisit it if the web client adds a multi-asset bundle, TypeScript compilation, or another feature Trunk demonstrably simplifies.

## Consequences

- `wasm-bindgen-cli` stays pinned to the workspace `wasm-bindgen` version.
- `scripts/start-dev-server.mjs` is the canonical local browser build/test entry point.
- Generated `web/pkg/` content remains ignored and is recreated from source.
- The repository documents direct-build commands; a future bundler change requires a new ADR and refreshed dependency notices.
