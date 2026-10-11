# Deployment

Open Web Racer is one server binary plus a directory of static web files. There is no database and no external service.

## Shape

The `racer-server` process serves the browser application, `GET /healthz` (readiness) and the WebSocket endpoint `/ws` from one origin. Each instance is its own lobby universe. The browser must load the page and the WebSocket from the same origin; cross-origin WebSocket connections are rejected.

## Running

```sh
cargo build --release -p racer-server
# build the browser client as described in DEVELOPMENT.md so web/pkg exists
RACER_BIND=0.0.0.0:4173 ./target/release/racer-server
```

- `RACER_BIND` sets the listen address (default `127.0.0.1:4173`). The default keeps the server local; choose a wider address explicitly before sharing it on a network.
- `RACER_PLUGIN_DIR` optionally serves a web plugin directory, see [PLUGINS.md](PLUGINS.md).

The server has no accounts and no TLS. For use over the Internet, put it behind a reverse proxy that terminates TLS and forwards WebSocket upgrades on `/ws`, and add the abuse controls your environment needs.

## Firewall

When serving other machines, allow inbound TCP on the chosen port only for the networks you trust (for example a private LAN profile), and avoid exposing it on public networks.

## Hardening

Run as a non-root user, expose only the one port, keep request and message sizes bounded (the protocol already caps messages at 4 KiB), and watch `/healthz`.
