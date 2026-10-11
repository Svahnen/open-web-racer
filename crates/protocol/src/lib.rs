//! Versioned, bounded wire types for the self-hosted race server.

use serde::{Deserialize, Serialize};

/// Major 15 adds `auto_reverse` to `ClientMessage::SetTransmission`: with it
/// the server-side drivetrain selects R when the brake is held at a standstill
/// (the brake then drives backwards) and first gear again when the throttle is
/// held in R, so the driver never shifts. Postcard is positional, so peers
/// must match exactly.
///
/// Major 14 removes the legacy `SetOriginalCar` / `OriginalCarAck` pair and the
/// two provider-specific handling codes: car parameters travel only
/// as the generic `SetCarModel` / `CarModelAck`, and `handling_model` of a
/// provider-driven vehicle is the provider's own code (`HANDLING_EXTERNAL` by
/// default). Postcard is positional, so peers must match exactly.
///
/// Major 13 adds the per-player gearbox mode: `ClientMessage::SetTransmission`
/// (lobby only) selects `Transmission::Automatic` or `Transmission::Manual` for
/// the sender's vehicle; the server applies it to that player's world and keeps
/// it across car and track changes. The default is automatic. Postcard is
/// positional, so peers must match exactly.
///
/// Major 10 adds a generic car choice to every vehicle: `CreateRace`,
/// `JoinRace` and `ChangeVehicle` carry `car_model` (a catalogue id, empty for
/// "no choice") and `car_variant` (index of the paint scheme of that model);
/// `VehicleSnapshot` reports both for humans and computer opponents, the
/// server assigning opponent cars deterministically so all clients agree.
/// Postcard is positional, so peers must match exactly.
///
/// Major 9 adds computer opponents: `CreateRace` and `ConfigureAi` carry the
/// opponent count, skill and catch-up option, `RaceSummary` reports them and
/// `VehicleSnapshot::is_ai` marks computer-driven cars (handling model
/// `HANDLING_SCRIPTED`). Postcard is positional, so peers must match exactly.
///
/// Major 8 changes the meaning of `paint_id`: it now carries a car colour
/// selection `paint-colour-N` (N = 0..=87, the colour-table index of
/// `CAR_PAINT_SELECTION_V0.md`) that peers render. The four legacy
/// `paint-project-*` ids remain valid aliases of fallback colours 2, 6, 4, 5.
/// The wire layout is unchanged from major 7, which added
/// `VehicleSnapshot::handling_model`.
pub const PROTOCOL_MAJOR: u16 = 15;

/// Gearbox mode of a player's vehicle (`SetTransmission`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transmission {
    /// The box picks the forward gear; the shift keys select R, N and drive.
    #[default]
    Automatic,
    /// The shift keys move one step through R, N, 1, 2 ... n.
    Manual,
}
/// Longest generic car-model identifier (`SetCarModel::model_id`) in bytes.
pub const MAX_MODEL_ID_BYTES: usize = 64;
/// Largest opaque parameter payload of `SetCarModel`. Kept below
/// `MAX_MESSAGE_BYTES` so the whole message always fits one frame.
pub const MAX_CAR_MODEL_PARAM_BYTES: usize = 3072;
/// Longest `CarModelAck::reason` in bytes.
pub const MAX_CAR_MODEL_REASON_BYTES: usize = 128;

/// A model id is 1..=`MAX_MODEL_ID_BYTES` bytes of `[a-z0-9_.:-]`.
pub fn is_valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_MODEL_ID_BYTES
        && id.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b':' | b'-')
        })
}

/// Why a `SetCarModel` is malformed, as a short stable reason string.
pub fn validate_car_model(model_id: &str, params: &[u8]) -> Result<(), &'static str> {
    if !is_valid_model_id(model_id) {
        return Err("invalid model id");
    }
    if params.len() > MAX_CAR_MODEL_PARAM_BYTES {
        return Err("params too large");
    }
    Ok(())
}
/// Longest `car_model` identifier in bytes.
pub const MAX_CAR_MODEL_BYTES: usize = 48;
/// Highest car colour index (the colour table has 88 rows).
pub const MAX_PAINT_COLOUR: u8 = 87;

/// Canonical `paint_id` for a colour index (0..=87).
pub fn paint_id_for_colour(colour: u8) -> Option<String> {
    (colour <= MAX_PAINT_COLOUR).then(|| format!("paint-colour-{colour}"))
}

/// Colour index selected by a `paint_id`, or `None` when it is not valid.
/// Canonical decimal only: no sign, no leading zeros, at most 87.
pub fn paint_colour_index(paint_id: &str) -> Option<u8> {
    match paint_id {
        "paint-project-red" => return Some(2),
        "paint-project-blue" => return Some(6),
        "paint-project-yellow" => return Some(4),
        "paint-project-green" => return Some(5),
        _ => {}
    }
    let digits = paint_id.strip_prefix("paint-colour-")?;
    if digits.is_empty()
        || digits.len() > 2
        || (digits.len() > 1 && digits.starts_with('0'))
        || !digits.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    digits.parse::<u8>().ok().filter(|c| *c <= MAX_PAINT_COLOUR)
}
/// Major 4 appended race-progress fields (position and lap timing in 60 Hz
/// race ticks) to `VehicleSnapshot`. Postcard is positional, so any field
/// change to a wire struct is a major bump; peers must match exactly.
pub const BUILD_ID: &str = env!("CARGO_PKG_VERSION");
pub const PROJECT_CONTENT_ID: &str = "racer:test-circuit-v1";
pub const MAX_MESSAGE_BYTES: usize = 4096;
pub const MAX_ROOMS: usize = 8;
pub const MAX_PLAYERS_PER_ROOM: usize = 8;
/// Most computer opponents in a room (the room holds eight cars in all).
pub const MAX_AI_OPPONENTS: u8 = 7;
/// Opponent count a client offers by default.
pub const DEFAULT_AI_OPPONENTS: u8 = 7;
/// Skill levels: 0 = beginner, 1 = advanced.
pub const MAX_AI_SKILL: u8 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientMessage {
    Hello {
        protocol_major: u16,
        build_id: String,
        content_capabilities: Vec<String>,
    },
    ListRaces,
    CreateRace {
        name: String,
        laps: u8,
        content_identity: String,
        track_profile_id: String,
        track_fingerprint: String,
        vehicle_profile_id: String,
        vehicle_fingerprint: String,
        driver_name: String,
        paint_id: String,
        /// Catalogue id of the chosen car model (empty: no choice).
        #[serde(default)]
        car_model: String,
        /// Paint scheme index within `car_model`.
        #[serde(default)]
        car_variant: u8,
        /// Computer opponents requested (0..=`MAX_AI_OPPONENTS`); ignored for open drive.
        ai_count: u8,
        /// 0 = beginner, 1 = advanced.
        ai_skill: u8,
        /// Rubber-banding catch-up curves on or off.
        ai_catch_up: bool,
    },
    JoinRace {
        race_id: u32,
        content_identity: String,
        track_profile_id: String,
        track_fingerprint: String,
        vehicle_profile_id: String,
        vehicle_fingerprint: String,
        driver_name: String,
        paint_id: String,
        #[serde(default)]
        car_model: String,
        #[serde(default)]
        car_variant: u8,
    },
    ChangeTrack {
        race_id: u32,
        selection_revision: u32,
        content_identity: String,
        track_profile_id: String,
        track_fingerprint: String,
        laps: u8,
    },
    ChangeVehicle {
        race_id: u32,
        selection_revision: u32,
        vehicle_profile_id: String,
        vehicle_fingerprint: String,
        paint_id: String,
        #[serde(default)]
        car_model: String,
        #[serde(default)]
        car_variant: u8,
    },
    /// Host only, lobby only: change the computer opponents.
    ConfigureAi {
        race_id: u32,
        selection_revision: u32,
        ai_count: u8,
        ai_skill: u8,
        ai_catch_up: bool,
    },
    LeaveRace,
    ReadyState {
        ready: bool,
        selection_revision: u32,
        track_fingerprint: String,
        vehicle_templates: Vec<AvailableCarProfile>,
    },
    DriverInput {
        sequence: u32,
        client_tick: u64,
        throttle: f32,
        brake: f32,
        steer: f32,
        handbrake: bool,
        shift_up: bool,
        shift_down: bool,
        reset_request: bool,
    },
    Ping {
        nonce: u32,
    },
    /// Generic car-model parameters: an id naming a handling model and an
    /// opaque, size-limited parameter payload whose layout the model owns.
    /// Validate with `validate_car_model`. Protocol 11.
    SetCarModel {
        model_id: String,
        params: Vec<u8>,
    },
    /// Choose the sender's gearbox mode. Lobby only (`race_started` after).
    /// Protocol 13; `auto_reverse` since protocol 15.
    SetTransmission {
        transmission: Transmission,
        auto_reverse: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RaceStatus {
    Lobby,
    Countdown { ticks_left: u16 },
    Racing,
    Finished,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VehicleSnapshot {
    pub player_id: u32,
    pub driver_name: String,
    pub vehicle_profile_id: String,
    pub vehicle_fingerprint: String,
    pub paint_id: String,
    /// Chosen car model catalogue id (empty: none) and its paint scheme index.
    pub car_model: String,
    pub car_variant: u8,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub velocity_x: f64,
    pub velocity_y: f64,
    pub heading: f64,
    pub speed: f64,
    pub ground_normal: [f64; 3],
    pub checkpoint: u16,
    pub completed_laps: u32,
    pub finished: bool,
    /// 1-based race rank (finished by finish tick, then laps/gates/distance).
    /// 0 means unranked, e.g. open drive.
    pub race_position: u8,
    /// Ticks elapsed in the lap in progress (0 when finished or unranked).
    pub current_lap_ticks: u32,
    pub last_lap_ticks: Option<u32>,
    pub best_lap_ticks: Option<u32>,
    pub finish_ticks: Option<u32>,
    /// Gearbox gear 1..=5 for models with a gearbox; 0 when the active vehicle
    /// model has none (HUD shows no gear).
    pub gear: u8,
    /// Engine speed estimate quantized to 0..=255 for HUD/audio (255 = 1.0).
    pub rpm_q8: u8,
    /// Vehicle model driving this vehicle on the server (`HANDLING_*`), for
    /// diagnostics and HUD only.
    pub handling_model: u8,
    /// True for a computer-driven opponent.
    pub is_ai: bool,
}

pub const HANDLING_PROVISIONAL: u8 = 0;
/// Servo-driven motion of a computer opponent.
pub const HANDLING_SCRIPTED: u8 = 3;
/// Generic project-owned arcade model (default of the public engine).
pub const HANDLING_ARCADE: u8 = 4;
/// Handling supplied through the generic `SetCarModel` message. Providers may
/// report other codes (1, 2 and 7 and above are free for them).
pub const HANDLING_EXTERNAL: u8 = 5;
/// Tuned classic arcade-racer handling (default of project cars).
pub const HANDLING_CLASSIC: u8 = 6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RaceSummary {
    pub race_id: u32,
    pub host_player_id: u32,
    pub name: String,
    pub player_count: u8,
    pub max_players: u8,
    pub laps: u8,
    pub status: RaceStatus,
    pub content_identity: String,
    pub track_profile_id: String,
    pub track_fingerprint: String,
    pub open_drive: bool,
    pub selection_revision: u32,
    /// Computer opponents that will join when the race starts.
    pub ai_count: u8,
    pub ai_skill: u8,
    pub ai_catch_up: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AvailableCarProfile {
    pub profile_id: String,
    pub source_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AvailableTrackProfile {
    pub profile_id: String,
    pub source_fingerprint: String,
    pub cars: Vec<AvailableCarProfile>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome {
        player_id: u32,
        protocol_major: u16,
        build_id: String,
    },
    LobbySnapshot {
        races: Vec<RaceSummary>,
        available_tracks: Vec<AvailableTrackProfile>,
    },
    RaceCreated {
        race_id: u32,
    },
    JoinAccepted {
        race_id: u32,
        player_id: u32,
        content_requirement: String,
        track_profile_id: String,
        track_fingerprint: String,
        selection_revision: u32,
    },
    JoinRejected {
        reason: String,
    },
    LeftRace,
    ReadyAccepted {
        race_id: u32,
        ready: bool,
        selection_revision: u32,
    },
    Countdown {
        race_id: u32,
        ticks_left: u16,
    },
    StateSnapshot {
        race_id: u32,
        server_tick: u64,
        /// Highest sequence accepted while the player was racing. This is a
        /// receipt marker only; it does not imply that a world step used it.
        ack_input_sequence: Option<u32>,
        /// Latest normalized intent sequence incorporated by the player's
        /// completed world step associated with `ack_applied_server_tick`.
        ack_applied_input_sequence: Option<u32>,
        /// Hub tick for the post-step, post-contact state in this snapshot.
        ack_applied_server_tick: Option<u64>,
        status: RaceStatus,
        selection_revision: u32,
        vehicles: Vec<VehicleSnapshot>,
    },
    ReliableEvent {
        race_id: u32,
        event: String,
        player_id: Option<u32>,
        value: u32,
    },
    Pong {
        nonce: u32,
        server_tick: u64,
    },
    /// Result of `SetCarModel`; `reason` is empty when accepted.
    CarModelAck {
        model_id: String,
        accepted: bool,
        reason: String,
    },
    Error {
        code: String,
        detail: String,
    },
}

pub fn encode_client(message: &ClientMessage) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(message)
}

pub fn decode_client(bytes: &[u8]) -> Result<ClientMessage, postcard::Error> {
    let (message, remainder) = postcard::take_from_bytes(bytes)?;
    if !remainder.is_empty() {
        return Err(postcard::Error::DeserializeBadEncoding);
    }
    Ok(message)
}

pub fn encode_server(message: &ServerMessage) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(message)
}

pub fn decode_server(bytes: &[u8]) -> Result<ServerMessage, postcard::Error> {
    let (message, remainder) = postcard::take_from_bytes(bytes)?;
    if !remainder.is_empty() {
        return Err(postcard::Error::DeserializeBadEncoding);
    }
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_round_trip_and_no_client_authority_fields() {
        assert_eq!(PROTOCOL_MAJOR, 15);
        let input = ClientMessage::DriverInput {
            sequence: 7,
            client_tick: 12,
            throttle: 0.75,
            brake: 0.0,
            steer: -0.25,
            handbrake: false,
            shift_up: true,
            shift_down: false,
            reset_request: false,
        };
        let bytes = encode_client(&input).unwrap();
        assert_eq!(decode_client(&bytes).unwrap(), input);
        assert!(bytes.len() <= MAX_MESSAGE_BYTES);

        let snapshot = ServerMessage::StateSnapshot {
            race_id: 4,
            server_tick: 90,
            ack_input_sequence: Some(8),
            ack_applied_input_sequence: Some(7),
            ack_applied_server_tick: Some(89),
            status: RaceStatus::Racing,
            selection_revision: 2,
            vehicles: Vec::new(),
        };
        let bytes = encode_server(&snapshot).unwrap();
        assert_eq!(decode_server(&bytes).unwrap(), snapshot);
    }

    #[test]
    fn paint_ids_map_to_colour_indices_canonically() {
        for colour in 0..=MAX_PAINT_COLOUR {
            let id = paint_id_for_colour(colour).unwrap();
            assert_eq!(paint_colour_index(&id), Some(colour));
        }
        assert_eq!(paint_id_for_colour(88), None);
        for bad in [
            "paint-colour-88",
            "paint-colour-088",
            "paint-colour-00",
            "paint-colour-",
            "paint-colour--1",
            "paint-colour-+1",
            "paint-colour-1x",
            "paint-colour-100",
            "paint-project-purple",
            "",
        ] {
            assert_eq!(paint_colour_index(bad), None, "{bad}");
        }
        assert_eq!(paint_colour_index("paint-project-red"), Some(2));
        assert_eq!(paint_colour_index("paint-project-blue"), Some(6));
        assert_eq!(paint_colour_index("paint-project-yellow"), Some(4));
        assert_eq!(paint_colour_index("paint-project-green"), Some(5));
    }

    #[test]
    fn colour_selection_messages_stay_within_the_wire_limit_and_round_trip() {
        let id = "p".repeat(64);
        let hash = "a".repeat(64);
        let change = ClientMessage::ChangeVehicle {
            race_id: u32::MAX,
            selection_revision: u32::MAX,
            vehicle_profile_id: id.clone(),
            vehicle_fingerprint: hash.clone(),
            paint_id: paint_id_for_colour(MAX_PAINT_COLOUR).unwrap(),
            car_model: "quill-coupe".into(),
            car_variant: 2,
        };
        let bytes = encode_client(&change).unwrap();
        assert!(bytes.len() <= MAX_MESSAGE_BYTES);
        assert_eq!(decode_client(&bytes).unwrap(), change);
        let join = ClientMessage::JoinRace {
            race_id: u32::MAX,
            content_identity: "owner-open-drive-v1".into(),
            track_profile_id: id.clone(),
            track_fingerprint: hash.clone(),
            vehicle_profile_id: id,
            vehicle_fingerprint: hash,
            driver_name: "d".repeat(24),
            paint_id: paint_id_for_colour(MAX_PAINT_COLOUR).unwrap(),
            car_model: "quill-coupe".into(),
            car_variant: 2,
        };
        assert!(encode_client(&join).unwrap().len() <= MAX_MESSAGE_BYTES);
        let snapshot = ServerMessage::StateSnapshot {
            race_id: 1,
            server_tick: 1,
            ack_input_sequence: None,
            ack_applied_input_sequence: None,
            ack_applied_server_tick: None,
            status: RaceStatus::Racing,
            selection_revision: 1,
            vehicles: (0..MAX_PLAYERS_PER_ROOM)
                .map(|player_id| VehicleSnapshot {
                    player_id: player_id as u32,
                    driver_name: "d".repeat(24),
                    vehicle_profile_id: "p".repeat(64),
                    vehicle_fingerprint: "a".repeat(64),
                    paint_id: paint_id_for_colour(MAX_PAINT_COLOUR).unwrap(),
                    car_model: "quill-coupe".into(),
                    car_variant: 2,
                    x: f64::MAX,
                    y: f64::MAX,
                    z: f64::MAX,
                    velocity_x: f64::MAX,
                    velocity_y: f64::MAX,
                    heading: f64::MAX,
                    speed: f64::MAX,
                    ground_normal: [f64::MAX; 3],
                    checkpoint: u16::MAX,
                    completed_laps: u32::MAX,
                    finished: true,
                    race_position: u8::MAX,
                    current_lap_ticks: u32::MAX,
                    last_lap_ticks: Some(u32::MAX),
                    best_lap_ticks: Some(u32::MAX),
                    finish_ticks: Some(u32::MAX),
                    gear: u8::MAX,
                    rpm_q8: u8::MAX,
                    handling_model: u8::MAX,
                    is_ai: true,
                })
                .collect(),
        };
        let bytes = encode_server(&snapshot).unwrap();
        assert!(
            bytes.len() <= MAX_MESSAGE_BYTES,
            "snapshot with colour ids is {} bytes",
            bytes.len()
        );
        assert_eq!(decode_server(&bytes).unwrap(), snapshot);
    }

    #[test]
    fn malformed_bounded_messages_are_rejected() {
        assert!(decode_client(&[0xff; MAX_MESSAGE_BYTES]).is_err());
        assert!(decode_client(&[0]).is_err());
        for len in 0..=MAX_MESSAGE_BYTES {
            let malformed = vec![0xa5; len];
            let _ = decode_client(&malformed);
        }
        let encoded = encode_client(&ClientMessage::ListRaces).unwrap();
        let mut suffixed = encoded;
        suffixed.push(0);
        assert!(decode_client(&suffixed).is_err());
        let summary = RaceSummary {
            race_id: u32::MAX,
            host_player_id: u32::MAX,
            name: "x".repeat(48),
            player_count: MAX_PLAYERS_PER_ROOM as u8,
            max_players: MAX_PLAYERS_PER_ROOM as u8,
            laps: 5,
            status: RaceStatus::Countdown {
                ticks_left: u16::MAX,
            },
            content_identity: PROJECT_CONTENT_ID.into(),
            track_profile_id: "track-profile-project-circuit".into(),
            track_fingerprint: "0".repeat(64),
            open_drive: false,
            selection_revision: 1,
            ai_count: MAX_AI_OPPONENTS,
            ai_skill: MAX_AI_SKILL,
            ai_catch_up: true,
        };
        let lobby = encode_server(&ServerMessage::LobbySnapshot {
            races: vec![summary; MAX_ROOMS],
            available_tracks: vec![],
        })
        .unwrap();
        assert!(
            lobby.len() <= MAX_MESSAGE_BYTES,
            "bounded lobby snapshot is {} bytes",
            lobby.len()
        );
        let vehicles = (0..MAX_PLAYERS_PER_ROOM)
            .map(|player_id| VehicleSnapshot {
                player_id: player_id as u32,
                driver_name: "Driver".into(),
                vehicle_profile_id: "vehicle-profile-project-default".into(),
                vehicle_fingerprint: "0".repeat(64),
                paint_id: "paint-project-red".into(),
                car_model: "quill-coupe".into(),
                car_variant: 2,
                x: f64::MAX,
                y: f64::MAX,
                z: f64::MAX,
                velocity_x: f64::MAX,
                velocity_y: f64::MAX,
                heading: f64::MAX,
                speed: f64::MAX,
                ground_normal: [f64::MAX; 3],
                checkpoint: u16::MAX,
                completed_laps: u32::MAX,
                finished: true,
                race_position: u8::MAX,
                current_lap_ticks: u32::MAX,
                last_lap_ticks: Some(u32::MAX),
                best_lap_ticks: Some(u32::MAX),
                finish_ticks: Some(u32::MAX),
                gear: u8::MAX,
                rpm_q8: u8::MAX,
                handling_model: u8::MAX,
                is_ai: true,
            })
            .collect();
        let snapshot = encode_server(&ServerMessage::StateSnapshot {
            race_id: u32::MAX,
            server_tick: u64::MAX,
            ack_input_sequence: Some(u32::MAX),
            ack_applied_input_sequence: Some(u32::MAX),
            ack_applied_server_tick: Some(u64::MAX),
            status: RaceStatus::Finished,
            selection_revision: u32::MAX,
            vehicles,
        })
        .unwrap();
        assert!(
            snapshot.len() <= MAX_MESSAGE_BYTES,
            "bounded state snapshot is {} bytes",
            snapshot.len()
        );
    }

    #[test]
    fn maximum_supported_multiplayer_messages_fit_wire_limit() {
        let id = "p".repeat(64);
        let hash = "a".repeat(64);
        let templates = (0..MAX_PLAYERS_PER_ROOM)
            .map(|_| AvailableCarProfile {
                profile_id: id.clone(),
                source_fingerprint: hash.clone(),
            })
            .collect::<Vec<_>>();
        let client_messages = [
            ClientMessage::CreateRace {
                name: "r".repeat(48),
                laps: 0,
                content_identity: "owner-open-drive-v1".into(),
                track_profile_id: id.clone(),
                track_fingerprint: hash.clone(),
                vehicle_profile_id: id.clone(),
                vehicle_fingerprint: hash.clone(),
                driver_name: "d".repeat(24),
                paint_id: "paint-project-yellow".into(),
                car_model: "quill-coupe".into(),
                car_variant: 2,
                ai_count: u8::MAX,
                ai_skill: u8::MAX,
                ai_catch_up: true,
            },
            ClientMessage::ConfigureAi {
                race_id: u32::MAX,
                selection_revision: u32::MAX,
                ai_count: u8::MAX,
                ai_skill: u8::MAX,
                ai_catch_up: true,
            },
            ClientMessage::JoinRace {
                race_id: u32::MAX,
                content_identity: "owner-open-drive-v1".into(),
                track_profile_id: id.clone(),
                track_fingerprint: hash.clone(),
                vehicle_profile_id: id.clone(),
                vehicle_fingerprint: hash.clone(),
                driver_name: "d".repeat(24),
                paint_id: "paint-project-yellow".into(),
                car_model: "quill-coupe".into(),
                car_variant: 2,
            },
            ClientMessage::ChangeTrack {
                race_id: u32::MAX,
                selection_revision: u32::MAX,
                content_identity: "owner-open-drive-v1".into(),
                track_profile_id: id.clone(),
                track_fingerprint: hash.clone(),
                laps: 0,
            },
            ClientMessage::ChangeVehicle {
                race_id: u32::MAX,
                selection_revision: u32::MAX,
                vehicle_profile_id: id.clone(),
                vehicle_fingerprint: hash.clone(),
                paint_id: "paint-project-yellow".into(),
                car_model: "quill-coupe".into(),
                car_variant: 2,
            },
            ClientMessage::ReadyState {
                ready: true,
                selection_revision: u32::MAX,
                track_fingerprint: hash.clone(),
                vehicle_templates: templates,
            },
        ];
        for message in client_messages {
            let bytes = encode_client(&message).unwrap();
            assert!(
                bytes.len() <= MAX_MESSAGE_BYTES,
                "client message: {}",
                bytes.len()
            );
        }

        let summary = RaceSummary {
            race_id: u32::MAX,
            host_player_id: u32::MAX,
            name: "r".repeat(48),
            player_count: MAX_PLAYERS_PER_ROOM as u8,
            max_players: MAX_PLAYERS_PER_ROOM as u8,
            laps: 0,
            status: RaceStatus::Lobby,
            content_identity: "owner-open-drive-v1".into(),
            track_profile_id: id.clone(),
            track_fingerprint: hash.clone(),
            open_drive: true,
            selection_revision: u32::MAX,
            ai_count: u8::MAX,
            ai_skill: u8::MAX,
            ai_catch_up: true,
        };
        let cars: Vec<AvailableCarProfile> = (0..2)
            .map(|_| AvailableCarProfile {
                profile_id: id.clone(),
                source_fingerprint: hash.clone(),
            })
            .collect();
        let catalog = (0..2)
            .map(|_| AvailableTrackProfile {
                profile_id: id.clone(),
                source_fingerprint: hash.clone(),
                cars: cars.clone(),
            })
            .collect();
        let lobby = encode_server(&ServerMessage::LobbySnapshot {
            races: vec![summary.clone(); MAX_ROOMS],
            available_tracks: catalog,
        })
        .unwrap();
        assert!(
            lobby.len() <= MAX_MESSAGE_BYTES,
            "lobby snapshot: {}",
            lobby.len()
        );

        let vehicles = (0..MAX_PLAYERS_PER_ROOM)
            .map(|player_id| VehicleSnapshot {
                player_id: player_id as u32,
                driver_name: "d".repeat(24),
                vehicle_profile_id: id.clone(),
                vehicle_fingerprint: hash.clone(),
                paint_id: "paint-project-yellow".into(),
                car_model: "quill-coupe".into(),
                car_variant: 2,
                x: f64::MAX,
                y: f64::MAX,
                z: f64::MAX,
                velocity_x: f64::MAX,
                velocity_y: f64::MAX,
                heading: f64::MAX,
                speed: f64::MAX,
                ground_normal: [f64::MAX; 3],
                checkpoint: u16::MAX,
                completed_laps: u32::MAX,
                finished: true,
                race_position: u8::MAX,
                current_lap_ticks: u32::MAX,
                last_lap_ticks: Some(u32::MAX),
                best_lap_ticks: Some(u32::MAX),
                finish_ticks: Some(u32::MAX),
                gear: u8::MAX,
                rpm_q8: u8::MAX,
                handling_model: u8::MAX,
                is_ai: true,
            })
            .collect();
        let snapshot = encode_server(&ServerMessage::StateSnapshot {
            race_id: u32::MAX,
            server_tick: u64::MAX,
            ack_input_sequence: Some(u32::MAX),
            ack_applied_input_sequence: Some(u32::MAX),
            ack_applied_server_tick: Some(u64::MAX),
            status: RaceStatus::Finished,
            selection_revision: u32::MAX,
            vehicles,
        })
        .unwrap();
        assert!(
            snapshot.len() <= MAX_MESSAGE_BYTES,
            "snapshot: {}",
            snapshot.len()
        );
    }

    #[test]
    fn transmission_message_round_trips_and_defaults_to_automatic() {
        for transmission in [Transmission::Automatic, Transmission::Manual] {
            let msg = ClientMessage::SetTransmission {
                transmission,
                auto_reverse: true,
            };
            let bytes = encode_client(&msg).unwrap();
            assert!(bytes.len() <= MAX_MESSAGE_BYTES);
            assert_eq!(decode_client(&bytes).unwrap(), msg);
        }
        assert_eq!(Transmission::default(), Transmission::Automatic);
    }

    #[test]
    fn car_model_messages_round_trip_and_respect_limits() {
        let msg = ClientMessage::SetCarModel {
            model_id: "example:model-1".into(),
            params: vec![7; MAX_CAR_MODEL_PARAM_BYTES],
        };
        let bytes = encode_client(&msg).unwrap();
        assert!(bytes.len() <= MAX_MESSAGE_BYTES);
        assert_eq!(decode_client(&bytes).unwrap(), msg);
        let ack = ServerMessage::CarModelAck {
            model_id: "example:model-1".into(),
            accepted: false,
            reason: "unsupported model".into(),
        };
        assert_eq!(decode_server(&encode_server(&ack).unwrap()).unwrap(), ack);
        assert_eq!(validate_car_model("a", &[]), Ok(()));
        assert_eq!(validate_car_model("", &[]), Err("invalid model id"));
        assert_eq!(validate_car_model("Bad Id", &[]), Err("invalid model id"));
        assert_eq!(
            validate_car_model(&"a".repeat(MAX_MODEL_ID_BYTES + 1), &[]),
            Err("invalid model id")
        );
        assert_eq!(
            validate_car_model("a", &vec![0; MAX_CAR_MODEL_PARAM_BYTES + 1]),
            Err("params too large")
        );
        assert!(
            validate_car_model(
                &"a".repeat(MAX_MODEL_ID_BYTES),
                &[0; MAX_CAR_MODEL_PARAM_BYTES]
            )
            .is_ok()
        );
        let worst = ClientMessage::SetCarModel {
            model_id: "a".repeat(MAX_MODEL_ID_BYTES),
            params: vec![255; MAX_CAR_MODEL_PARAM_BYTES],
        };
        assert!(encode_client(&worst).unwrap().len() <= MAX_MESSAGE_BYTES);
    }
}
