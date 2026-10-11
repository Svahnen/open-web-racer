# Contributing to Open Web Racer

Thanks for your interest. This is a hobby project; small, focused changes are the easiest to review.

## Ground rules

- Contribute only work that is your own, or that you have the right to submit under the Apache License 2.0. By opening a pull request you agree that your contribution is licensed under that license.
- Do not submit code, models, textures, audio or data taken from proprietary software or media, and do not submit anything derived from such material.
- Keep new content your own: generated or hand-made, with invented names.
- Keep the simulation independent of the rendering frame rate. The server is authoritative; the client predicts and interpolates.

## Development setup

See the quick start in the [README](README.md) and [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

Before opening a pull request, run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace
npm test
```

Browser tests (`npm run test:e2e`) are appreciated for UI changes.

## Pull requests

- One topic per pull request, with a short description of what and why.
- Add or update tests for behavior changes.
- Update the documentation and the [CHANGELOG](CHANGELOG.md) when user-visible behavior changes.

## Conduct

Be kind and constructive. Harassment or personal attacks are not tolerated.
