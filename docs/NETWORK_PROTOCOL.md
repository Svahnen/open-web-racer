# Local multiplayer protocol

The first network milestone uses a same-origin WebSocket at `/ws`. The server serves the browser application and `/healthz` from the same Axum process. Protocol types live in `crates/protocol`; the WASM client exports the same Rust postcard codec used by the server.

## Connection and bounds

- The server assigns a player ID for each WebSocket connection. Client messages do not contain a player or room owner ID.
- The server sends `Welcome` and a lobby snapshot when the socket opens. The client must then send `Hello` with the current protocol major (`PROTOCOL_MAJOR` in `crates/protocol`, currently `14`) before any command. A different major receives `protocol_mismatch` and the connection closes.
- Browser connections must send an `Origin` whose authority matches the HTTP `Host`. Missing, malformed, or cross-origin headers are rejected. This initial same-origin policy does not provide account identity or Internet-facing authentication.
- Frames are binary postcard messages, at most `MAX_MESSAGE_BYTES` (4,096) bytes; the server also caps WebSocket message and frame size at that limit. Decoding rejects malformed messages and trailing bytes. JSON is used only between JavaScript and the WASM codec, never on the wire.
- At most 8 rooms and 8 players per room are accepted. Race names are at most 48 UTF-8 bytes; laps are 1 through 5; this milestone advertises `racer:test-circuit-v1` only.
- Each connection accepts at most 150 protocol messages per second, 90 driver inputs per second, and 12 room mutations per second. A per-connection input sequence drops duplicate and stale input. Inputs must be finite; throttle and brake clamp to `[0,1]`, steer to `[-1,1]`.
- Reliable/control output uses a bounded queue. If it fills, the server closes that slow connection and removes its player. State snapshots use a separate latest-value slot so old poses are coalesced rather than queued.

## Authority and room lifecycle

Supported client messages are `Hello`, `ListRaces`, `CreateRace`, `JoinRace`, `ChangeTrack`, `ChangeVehicle`, `ConfigureAi`, `LeaveRace`, `ReadyState`, `DriverInput`, `SetCarModel`, `SetTransmission`, and `Ping` (`CreateRace`, `JoinRace` and `ChangeVehicle` carry a car choice). Create/join explicitly require the project-owned content identity. The server rejects incompatible content and room operations with a typed response.

The server owns room membership, readiness, the 60 Hz clock, race countdown, each `RaceWorld`, transforms, checkpoints, laps, and finish state. `DriverInput` contains only sequence/timing metadata and controls; it has no position, velocity, lap, checkpoint, finish, player ID, or room ID fields. Every tick applies the latest accepted input to the connection's own vehicle. Reset is an edge action consumed by one server tick.

The server publishes `LobbySnapshot` and `StateSnapshot`, plus reliable countdown/checkpoint/lap/finish events. Joining is permitted only during the lobby state. A disconnect removes that socket-owned player; an empty room is deleted, while a remaining started room moves to room status `Finished` and remains visible in the lobby. This room status can mean the race was interrupted by a disconnect; it does not assert every remaining vehicle completed its laps. A participant can explicitly leave at any time.

## Deliberate limits

This is a LAN/development multiplayer slice. It has no account system, TLS termination, connection-count cap, public-host abuse controls, or prediction/reconciliation. Those are later deployment and networking milestones. Keep the listener local during development; an operator must choose a bind address explicitly with `RACER_BIND` before sharing it on a network.

## Protocol 4 and 5: race progress and gearbox display

Major 4 appends `race_position`, `current_lap_ticks`, `last_lap_ticks`, `best_lap_ticks`, and `finish_ticks` to `VehicleSnapshot` (postcard is positional, so this is a major bump). Ticks are 60 Hz race ticks. `race_position` is 1-based: finished players first by finish tick, then racing players by completed laps, gates passed this lap, and distance to the next gate; ties break by lower player ID. Open-drive rooms report 0 / no timings.

Major 5 appends `gear` (u8, 1..=5, 0 when the active vehicle model has no gearbox) and `rpm_q8` (u8, engine-speed estimate 0..=255 for HUD/audio) to `VehicleSnapshot`. Both come from the server's authoritative world and are display data only; they never feed back into motion.

## Protocol 9: computer opponents

Major 9 adds computer-driven cars to a room (Hello must send major 9; postcard is positional, so this is a major bump):

- `CreateRace` gains `ai_count` (0..=7), `ai_skill` (0 beginner, 1 advanced) and `ai_catch_up` (bool). Open-drive rooms ignore the count. Out-of-range values are rejected with `invalid_settings`.
- `ConfigureAi { race_id, selection_revision, ai_count, ai_skill, ai_catch_up }` is host-only and lobby-only. It bumps the selection revision and clears everybody's readiness, like a track or car change. Errors: `host_only`, `race_started`, `invalid_settings`. The `selection_revision` of `ConfigureAi`, `ChangeTrack` and `ChangeVehicle` is no longer compared (see "Concurrent lobby changes" below); `ReadyState` still is.
- `RaceSummary` reports `ai_count`, `ai_skill` and `ai_catch_up`.
- `VehicleSnapshot` gains `is_ai`; opponents report `handling_model` 3 (`HANDLING_SCRIPTED`), ids from 1,000,000,000 upward (connection ids stay far below), driver names `Computer N`, the host's car profile and a fixed colour per slot.

Server behaviour. Opponents are created when the countdown starts (every human ready; with opponents requested one human is enough, otherwise two are still required). At most 8 cars race, so opponents yield to humans (`min(ai_count, 8 - humans)`). Humans and opponents line up on the grid, humans on the rearmost slots in player-id order; rooms without opponents keep the classic spawn. Opponents run on the same 60 Hz tick (servo motion model, shared contact pass, ranking and snapshot) but are not players: no connection, input, readiness or rollback-frame entry (the internal Hub frame covers human players only; hub-step revision v4). The room finishes when every human has finished. `race_position` ranks opponents and humans together.

## Protocol 10: car model and paint per vehicle

Major 10 lets every vehicle show a project-owned car model (Hello must send major 10; postcard is positional, so this is a major bump):

- `CreateRace`, `JoinRace` and `ChangeVehicle` gain `car_model` (catalogue id such as `quill-coupe`; empty means no choice) and `car_variant` (u8, index of the paint scheme of that model). The wire fields are generic: the server only knows the demo-car catalogue (`racer_content::project_cars`) and rejects any model it does not list, a variant past the model's paint count, a model over `MAX_CAR_MODEL_BYTES` (48), or a non-zero variant without a model, with `invalid_settings`. The choice is display data only and never changes handling.
- `VehicleSnapshot` gains `car_model` and `car_variant`. Humans report their own choice. Computer opponents report cars assigned by the server: the host's choice (or the first model) is the base of the field and opponent `k` (0-based, grid order) drives `ProjectCars::opponent_pick(base, k)`, the same rule the solo field uses, so every client draws the same car for every opponent and no two cars of a field repeat a model-and-paint pair.
- A `ChangeVehicle` in the lobby of a project room (the circuit or a generated track) updates the choice like any other vehicle change (bumps the selection revision, clears readiness). Clients that have no catalogue entry for a reported model draw their default car for that vehicle.

## Protocol 11: car-model parameters

Major 11 adds a model-agnostic way to hand handling parameters to the server (Hello must send major 11; postcard is positional, so this is a major bump).

- `ClientMessage::SetCarModel { model_id, params }`: `model_id` is 1 to `MAX_MODEL_ID_BYTES` (64) bytes of lowercase ASCII letters, digits and `_ . : -`; `params` is an opaque byte payload of at most `MAX_CAR_MODEL_PARAM_BYTES` (3,072), chosen so the whole message always fits `MAX_MESSAGE_BYTES`. The layout of `params` belongs to the named model, never to the transport. `validate_car_model` in `crates/protocol` checks both limits.
- `ServerMessage::CarModelAck { model_id, accepted, reason }` answers every `SetCarModel`. `reason` is empty when accepted. Malformed requests get `invalid model id` or `params too large`; a well-formed request for a model the server does not provide gets `unsupported model`. A server without a registered provider for the model answers `accepted = false`.
- `HANDLING_EXTERNAL` (5) is reserved for `VehicleSnapshot::handling_model` of vehicles driven by a model supplied this way. It is diagnostics and HUD text only.
- `ClientMessage::SetTransmission { transmission }` (protocol 13) chooses the sender's gearbox mode, `automatic` (default) or `manual`. It may be sent at any time after `Hello`; the server remembers it per connection and applies it to the player's world when the player creates or joins a room, and again after car or track changes. Inside a room it is accepted in the lobby only (`race_started` otherwise). It counts toward the room mutation limit and has no reply; the effect is visible in the gear reported by `VehicleSnapshot::gear` (manual starts in N, automatic in 1).
- `SetCarModel` counts toward the 12 room mutations per second limit and requires `Hello` first. Nothing it carries is stored, logged, or forwarded to other players.

## Protocol 14: generic car models only

Major 14 (Hello must send major 14) makes `SetCarModel` / `CarModelAck` the only way to hand car parameters to the server. `VehicleSnapshot::handling_model` of a provider-driven vehicle is the code the provider reports (`HANDLING_EXTERNAL` (5) unless the provider supplies its own). A provider can also decline a selection that does not describe the sender's selected vehicle profile; the server then keeps the selection but leaves the world on its default model. `PROJECT_CONTENT_ID` is `racer:test-circuit-v1`.

## Concurrent lobby changes and imported-track races

Concurrent lobby changes. Every selection belongs to one party: the track, laps and computer-car settings to the host (`ChangeTrack`, `ConfigureAi`), a car and paint to its player (`ChangeVehicle`). Changes by different parties cannot conflict, so the server applies each one on arrival, whatever `selection_revision` the sender saw (the field stays on the wire; hosts and players send what they last saw). Each applied change still bumps the room's revision and clears readiness. A car that does not exist on the track the host just chose is answered with `car_unavailable_on_track` / `profile_unavailable`; the rest of the room is unaffected. `ReadyState` keeps the revision check: a ready flag is only valid for the roster the player saw (`stale_revision`).

Imported-track rooms can race. A room on an imported track (content identity `owner-open-drive-v1`) takes `laps` 0..=5 in `CreateRace` and `ChangeTrack`: 0 is free driving (no gates, ranks or opponents, as before); 1..=5 race when the server found a course in the track's road rows, and are clamped to 0 when it did not (the summary reports the lap count in force). The course (`racer_ai::imported_course`) is derived from the road-edge rows alone: rows that return to their start make a circuit (laps), anything else is point to point and finishes at its last row (one run, whatever `laps` says); gates lie about 90 m apart along the road in the direction the start pose faces, with the finish line just ahead of the start grid. At the start everybody lines up on the course grid (humans on the rearmost slots in player-id order). `ai_count`, `ai_skill` and `ai_catch_up` apply to racing rooms exactly as on generated tracks: the opponents run on the room's tick over the track's ground and walls, are ranked with the humans and finish like them. Each opponent drives an imported car: the track's cars in profile-id order, starting with the one after the host's car and wrapping around, in the host's paint (`VehicleSnapshot.vehicle_profile_id`, `vehicle_fingerprint` and `paint_id` name it). A client can work this out in the lobby from `RaceSummary` and the advertised track cars, and `ReadyState.vehicle_templates` must list those cars as well as the players' own.
