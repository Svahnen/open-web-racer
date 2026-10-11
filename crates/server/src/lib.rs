//! Small same-origin authoritative lobby and race service.

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header::ORIGIN},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use racer_protocol::{
    self as protocol, BUILD_ID, ClientMessage, HANDLING_ARCADE, HANDLING_CLASSIC,
    HANDLING_PROVISIONAL, HANDLING_SCRIPTED, MAX_AI_OPPONENTS, MAX_AI_SKILL, MAX_MESSAGE_BYTES,
    MAX_PLAYERS_PER_ROOM, MAX_ROOMS, PROJECT_CONTENT_ID, PROTOCOL_MAJOR, RaceStatus, RaceSummary,
    ServerMessage, VehicleSnapshot,
};
use racer_sim::{
    ClassicConfig, DriverInput, RaceWorld, SIMULATION_MODEL_REVISION, SimStateFrame, SimTiming,
    TrackRuntime, VEHICLE_CONTACT_MODEL_REVISION, Vec2, VehicleContactBody, VehicleModel,
    rank_race, resolve_pairwise_vehicle_contacts,
};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

mod gameplay_data;
mod opponents;
mod private_assets;
mod project_tracks;
pub use gameplay_data::{GameplayCar, GameplayDataStore, GameplayTrack, ProjectRuntime};
pub use private_assets::PrivateAssetConfig;
use project_tracks::{PROJECT_VEHICLE_PROFILE_ID, is_project_profile, resolve_project_selection};
use racer_content::classic_params::{default_project_classic_config, project_classic_config};
use racer_content::project_tracks::PROJECT_TRACK_CONTENT_ID;
pub use racer_content::provider::{ContentProvider, ProviderRegistry};
pub use racer_sim::model_ext::{VehicleDynamics, VehicleModelProvider, VehicleModelRegistry};
use tokio::sync::{Mutex, mpsc, watch};

const COUNTDOWN_TICKS: u16 = 180;
const MAX_NAME_BYTES: usize = 48;
const INPUTS_PER_SECOND: u32 = 90;
const MUTATIONS_PER_SECOND: u32 = 12;
const HUB_REPLAY_HISTORY_TICKS: usize = 256;
#[allow(dead_code)] // Internal capture/restore is not wired to the network path.
const HUB_SIMULATION_FRAME_VERSION: u16 = 1;
/// Bump for changes to server tick ordering, intent scheduling, contact
/// orchestration, post-contact constraints, finish evaluation, or snapshot boundary.
#[allow(dead_code)] // Bound into the internal frame identity below.
const HUB_SIMULATION_MODEL_REVISION: &str = "provisional-hub-step-v4";

#[derive(Clone)]
pub struct AppState(
    Arc<Mutex<Hub>>,
    Option<Arc<private_assets::PrivateAssetStore>>,
    Option<Arc<GameplayDataStore>>,
);

#[derive(Clone)]
struct Hub {
    next_player: u32,
    next_room: u32,
    server_tick: u64,
    clients: HashMap<u32, ClientChannel>,
    rooms: HashMap<u32, Room>,
    available_tracks: Vec<protocol::AvailableTrackProfile>,
    tick_schedules: VecDeque<HubTickSchedule>,
    tick_schedule_recording_enabled: bool,
    /// Provider models registered by the embedding binary.
    vehicle_models: Arc<VehicleModelRegistry>,
    /// Registry id of the model new worlds of imported (gameplay-catalogue)
    /// cars start with, when the embedding binary supplies one.
    imported_default_model: Option<String>,
    content_providers: Arc<ProviderRegistry>,
    /// Per connection: the provider model (id, opaque params) the player
    /// selected with `SetCarModel`. Removed on disconnect.
    external_cars: HashMap<u32, (String, Vec<u8>)>,
    /// Per connection: the gearbox mode chosen with `SetTransmission`
    /// (automatic when absent). Applied to the player's world when it is
    /// created and kept across car and track changes. Removed on disconnect.
    transmissions: HashMap<u32, (protocol::Transmission, bool)>,
    #[cfg(test)]
    identity_build_count: Arc<AtomicUsize>,
}

/// Models for building a player's world: the provider registry plus the id of
/// the model imported cars start with.
#[derive(Clone)]
struct ModelSet {
    registry: Arc<VehicleModelRegistry>,
    imported_default: Option<String>,
}

impl ModelSet {
    fn of(hub: &Hub) -> Self {
        Self {
            registry: Arc::clone(&hub.vehicle_models),
            imported_default: hub.imported_default_model.clone(),
        }
    }
}

fn handling_model_id(world: &RaceWorld, registry: &VehicleModelRegistry) -> u8 {
    match world.vehicle_model() {
        VehicleModel::Provisional => HANDLING_PROVISIONAL,
        VehicleModel::Scripted => HANDLING_SCRIPTED,
        VehicleModel::Arcade => HANDLING_ARCADE,
        VehicleModel::Classic => HANDLING_CLASSIC,
        VehicleModel::Custom => world
            .custom_model_id()
            .map_or(protocol::HANDLING_EXTERNAL, |id| registry.wire_code(id)),
    }
}

#[derive(Clone, Debug, PartialEq)]
struct HubTickSchedule {
    tick: u64,
    identity: [u8; 32],
    before_boundary: [u8; 32],
    after_boundary: [u8; 32],
    racing_rooms: Vec<RacingRoomTickSchedule>,
}

#[derive(Clone, Debug, PartialEq)]
struct RacingRoomTickSchedule {
    room_id: u32,
    selection_revision: u32,
    participants: Vec<AppliedIntentSample>,
}

#[derive(Clone, Debug, PartialEq)]
struct AppliedIntentSample {
    player_id: u32,
    world_identity: [u8; 32],
    input: DriverInput,
    received_sequence: Option<u32>,
    applied_sequence: Option<u32>,
}

#[derive(Clone)]
struct ClientChannel {
    reliable: mpsc::Sender<ServerMessage>,
    snapshots: watch::Sender<Option<ServerMessage>>,
    close: watch::Sender<bool>,
}

#[derive(Clone)]
struct Room {
    summary: RaceSummary,
    owner_player_id: u32,
    gameplay_track: Option<Arc<GameplayTrack>>,
    players: HashMap<u32, Player>,
    countdown: u16,
    /// Computer opponents, created when the countdown starts.
    ai: Option<opponents::RoomAi>,
}

#[derive(Clone)]
struct Player {
    world: RaceWorld,
    driver_name: String,
    vehicle_profile_id: String,
    vehicle_fingerprint: String,
    paint_id: String,
    /// Chosen generic car model (catalogue id, empty for none) and paint scheme.
    car_model: String,
    car_variant: u8,
    /// Gearbox mode of this player's world (see `apply_transmission`).
    transmission: protocol::Transmission,
    auto_reverse: bool,
    contact_radius: f64,
    contact_min_z: f64,
    contact_max_z: f64,
    ready: bool,
    input: DriverInput,
    /// Shift requests (up, down) received since the last step. The client
    /// sends the key level at most every few ticks; a press released before the
    /// next 32 Hz step must still reach the gearbox as one rising edge.
    shift_latch: (bool, bool),
    ack_sequence: Option<u32>,
    applied_sequence: Option<u32>,
    applied_server_tick: Option<u64>,
}

#[allow(dead_code)] // Internal frame remains disconnected from wire/client state.
#[derive(Clone, Debug, PartialEq)]
struct HubSimulationFrame {
    version: u16,
    identity: [u8; 32],
    authoritative_tick: u64,
    rooms: Vec<RoomSimulationFrame>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
struct RoomSimulationFrame {
    room_id: u32,
    owner_player_id: u32,
    host_player_id: u32,
    selection_revision: u32,
    content_identity: String,
    track_profile_id: String,
    track_fingerprint: String,
    laps: u8,
    open_drive: bool,
    status: RaceStatus,
    countdown: u16,
    participants: Vec<ParticipantSimulationFrame>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
struct ParticipantSimulationFrame {
    player_id: u32,
    vehicle_profile_id: String,
    vehicle_fingerprint: String,
    world: SimStateFrame,
    contact_radius: f64,
    contact_min_z: f64,
    contact_max_z: f64,
    ready: bool,
    latest_input: DriverInput,
    received_sequence: Option<u32>,
    applied_sequence: Option<u32>,
    applied_server_tick: Option<u64>,
}

/// Most laps a room on an imported track can race.
const MAX_OWNER_LAPS: u8 = 5;

/// Laps of a room on an imported track: the requested count when the track
/// has a race course, else 0 (free driving).
fn owner_room_laps(track: Option<&Arc<GameplayTrack>>, laps: u8) -> u8 {
    if track.is_some_and(|t| t.course.is_some()) {
        laps.min(MAX_OWNER_LAPS)
    } else {
        0
    }
}

impl Room {
    /// True when the room scores a race (checkpoints, laps, positions,
    /// finish): every circuit room, and imported-track rooms with laps.
    /// Imported-track rooms without laps are free driving.
    fn races(&self) -> bool {
        !self.summary.open_drive || self.summary.laps > 0
    }

    /// The race course of an imported-track room that races.
    fn owner_course(&self) -> Option<Arc<racer_ai::ImportedCourse>> {
        if !self.summary.open_drive || self.summary.laps == 0 {
            return None;
        }
        self.gameplay_track.as_ref()?.course.clone()
    }

    fn status(&self) -> RaceStatus {
        match self.summary.status {
            RaceStatus::Countdown { .. } => RaceStatus::Countdown {
                ticks_left: self.countdown,
            },
            ref status => status.clone(),
        }
    }
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            next_player: 1,
            next_room: 1,
            server_tick: 0,
            clients: HashMap::new(),
            rooms: HashMap::new(),
            available_tracks: Vec::new(),
            tick_schedules: VecDeque::new(),
            tick_schedule_recording_enabled: false,
            vehicle_models: Arc::new(VehicleModelRegistry::new()),
            imported_default_model: None,
            content_providers: Arc::new(ProviderRegistry::with_native()),
            external_cars: HashMap::new(),
            transmissions: HashMap::new(),
            #[cfg(test)]
            identity_build_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[allow(dead_code)] // Reviewed follow-up phase may consume this internal API.
impl Hub {
    /// Replay recording is an opt-in offline diagnostic; normal authoritative
    /// server ticks leave it disabled to avoid hashing static world data.
    #[allow(dead_code)]
    fn set_tick_schedule_recording(&mut self, enabled: bool) {
        self.tick_schedule_recording_enabled = enabled;
        self.tick_schedules.clear();
    }

    /// Capture all server-owned simulation state at one authoritative tick.
    /// Transport channels and client-owned display state are not simulation.
    fn capture_simulation_frame(&self) -> Result<HubSimulationFrame, &'static str> {
        let mut rooms = self.rooms.iter().collect::<Vec<_>>();
        rooms.sort_by_key(|(room_id, _)| **room_id);
        let mut room_frames = Vec::with_capacity(rooms.len());
        for (room_id, room) in rooms {
            let mut players = room.players.iter().collect::<Vec<_>>();
            players.sort_by_key(|(player_id, _)| **player_id);
            let mut participants = Vec::with_capacity(players.len());
            for (player_id, player) in players {
                participants.push(ParticipantSimulationFrame {
                    player_id: *player_id,
                    vehicle_profile_id: player.vehicle_profile_id.clone(),
                    vehicle_fingerprint: player.vehicle_fingerprint.clone(),
                    world: player.world.capture_state()?,
                    contact_radius: player.contact_radius,
                    contact_min_z: player.contact_min_z,
                    contact_max_z: player.contact_max_z,
                    ready: player.ready,
                    latest_input: player.input,
                    received_sequence: player.ack_sequence,
                    applied_sequence: player.applied_sequence,
                    applied_server_tick: player.applied_server_tick,
                });
            }
            room_frames.push(RoomSimulationFrame {
                room_id: *room_id,
                owner_player_id: room.owner_player_id,
                host_player_id: room.summary.host_player_id,
                selection_revision: room.summary.selection_revision,
                content_identity: room.summary.content_identity.clone(),
                track_profile_id: room.summary.track_profile_id.clone(),
                track_fingerprint: room.summary.track_fingerprint.clone(),
                laps: room.summary.laps,
                open_drive: room.summary.open_drive,
                status: room.status(),
                countdown: room.countdown,
                participants,
            });
        }
        let frame = HubSimulationFrame {
            version: HUB_SIMULATION_FRAME_VERSION,
            identity: self.simulation_identity()?,
            authoritative_tick: self.server_tick,
            rooms: room_frames,
        };
        self.validate_simulation_frame(&frame)?;
        Ok(frame)
    }

    /// Restore every room transactionally. Each nested RaceWorld is restored
    /// on a clone first, so a late participant failure cannot partially mutate
    /// the hub.
    fn restore_simulation_frame(&mut self, frame: &HubSimulationFrame) -> Result<(), &'static str> {
        self.validate_simulation_frame(frame)?;
        let mut worlds = Vec::new();
        for room_frame in &frame.rooms {
            let room = self
                .rooms
                .get(&room_frame.room_id)
                .ok_or("hub frame room is missing")?;
            for participant in &room_frame.participants {
                let player = room
                    .players
                    .get(&participant.player_id)
                    .ok_or("hub frame participant is missing")?;
                let mut world = player.world.clone();
                world.restore_state(&participant.world)?;
                worlds.push((room_frame.room_id, participant.player_id, world));
            }
        }

        self.server_tick = frame.authoritative_tick;
        for room_frame in &frame.rooms {
            let room = self
                .rooms
                .get_mut(&room_frame.room_id)
                .expect("validated hub frame room exists");
            room.summary.status = room_frame.status.clone();
            room.countdown = room_frame.countdown;
            room.owner_player_id = room_frame.owner_player_id;
            room.summary.host_player_id = room_frame.host_player_id;
            for participant in &room_frame.participants {
                let player = room
                    .players
                    .get_mut(&participant.player_id)
                    .expect("validated hub frame participant exists");
                player.input = participant.latest_input;
                player.ready = participant.ready;
                player.ack_sequence = participant.received_sequence;
                player.applied_sequence = participant.applied_sequence;
                player.applied_server_tick = participant.applied_server_tick;
            }
        }
        for (room_id, player_id, world) in worlds {
            self.rooms
                .get_mut(&room_id)
                .expect("validated hub frame room exists")
                .players
                .get_mut(&player_id)
                .expect("validated hub frame participant exists")
                .world = world;
        }
        self.tick_schedules
            .retain(|schedule| tick_is_not_after(schedule.tick, frame.authoritative_tick));
        Ok(())
    }

    fn validate_simulation_frame(&self, frame: &HubSimulationFrame) -> Result<(), &'static str> {
        if frame.version != HUB_SIMULATION_FRAME_VERSION {
            return Err("hub simulation frame version is unsupported");
        }
        if frame.rooms.len() > MAX_ROOMS || frame.identity != self.simulation_identity()? {
            return Err("hub simulation frame identity does not match");
        }
        let mut live_room_ids = self.rooms.keys().copied().collect::<Vec<_>>();
        live_room_ids.sort_unstable();
        if frame
            .rooms
            .iter()
            .map(|room| room.room_id)
            .collect::<Vec<_>>()
            != live_room_ids
        {
            return Err("hub simulation frame room set is incomplete or unordered");
        }
        for room_frame in &frame.rooms {
            let room = self
                .rooms
                .get(&room_frame.room_id)
                .ok_or("hub frame room is missing")?;
            if matches!(room_frame.status, RaceStatus::Countdown { ticks_left } if ticks_left != room_frame.countdown || !(1..=COUNTDOWN_TICKS).contains(&ticks_left))
            {
                return Err("hub simulation frame countdown is outside its reachable range");
            }
            let mut live_player_ids = room.players.keys().copied().collect::<Vec<_>>();
            live_player_ids.sort_unstable();
            if room_frame.selection_revision != room.summary.selection_revision
                || room_frame.owner_player_id != room.owner_player_id
                || room_frame.host_player_id != room.summary.host_player_id
                || room_frame.content_identity != room.summary.content_identity
                || room_frame.track_profile_id != room.summary.track_profile_id
                || room_frame.track_fingerprint != room.summary.track_fingerprint
                || room_frame.laps != room.summary.laps
                || room_frame.open_drive != room.summary.open_drive
                || !live_player_ids.contains(&room_frame.owner_player_id)
                || room_frame.host_player_id != room_frame.owner_player_id
            {
                return Err("hub simulation frame room configuration does not match");
            }
            if room_frame.participants.is_empty()
                || room_frame.participants.len() > MAX_PLAYERS_PER_ROOM
                || room_frame
                    .participants
                    .iter()
                    .map(|participant| participant.player_id)
                    .collect::<Vec<_>>()
                    != live_player_ids
            {
                return Err("hub simulation frame participant set is incomplete or unordered");
            }
            let all_ready = room_frame
                .participants
                .iter()
                .all(|participant| participant.ready);
            match room_frame.status {
                RaceStatus::Lobby => {
                    if room_frame.countdown != 0
                        || (room_frame.participants.len() >= 2 && all_ready)
                    {
                        return Err("hub lobby state is not reachable from ready handling");
                    }
                }
                RaceStatus::Countdown { .. } => {
                    if room_frame.participants.len() < 2 || !all_ready {
                        return Err("hub countdown state requires a ready multiplayer roster");
                    }
                }
                RaceStatus::Racing => {
                    if room_frame.countdown != 0 || room_frame.participants.len() < 2 || !all_ready
                    {
                        return Err("hub racing state requires a ready multiplayer roster");
                    }
                }
                // A participant can leave during countdown/racing, which marks
                // the remaining room Finished and clears readiness. A racing
                // leave can also leave one participant and prior input markers.
                RaceStatus::Finished => {}
            }
            for participant in &room_frame.participants {
                let player = room
                    .players
                    .get(&participant.player_id)
                    .ok_or("hub frame participant is missing")?;
                if participant.vehicle_profile_id != player.vehicle_profile_id
                    || participant.vehicle_fingerprint != player.vehicle_fingerprint
                    || participant.contact_radius.to_bits() != player.contact_radius.to_bits()
                    || participant.contact_min_z.to_bits() != player.contact_min_z.to_bits()
                    || participant.contact_max_z.to_bits() != player.contact_max_z.to_bits()
                {
                    return Err("hub simulation frame vehicle/contact profile does not match");
                }
                validate_contact_profile(
                    participant.contact_radius,
                    participant.contact_min_z,
                    participant.contact_max_z,
                )?;
                if !tick_is_not_after(participant.world.tick, frame.authoritative_tick) {
                    return Err("hub participant world tick is ahead of the authoritative tick");
                }
                validate_latest_input(participant.latest_input)?;
                if matches!(
                    room_frame.status,
                    RaceStatus::Lobby | RaceStatus::Countdown { .. }
                ) && (participant.latest_input != DriverInput::default()
                    || participant.received_sequence.is_some()
                    || participant.applied_sequence.is_some()
                    || participant.applied_server_tick.is_some())
                {
                    return Err("hub pre-race state contains an input schedule");
                }
                match (
                    participant.received_sequence,
                    participant.applied_sequence,
                    participant.applied_server_tick,
                ) {
                    (None, None, None) => {
                        if participant.latest_input != DriverInput::default() {
                            return Err("hub frame has input without a received sequence");
                        }
                    }
                    (Some(received), Some(applied), Some(applied_tick)) => {
                        if (applied != received && !sequence_is_newer(received, applied))
                            || !tick_is_not_after(applied_tick, frame.authoritative_tick)
                        {
                            return Err("hub frame applied input marker is inconsistent");
                        }
                    }
                    (Some(_), None, None) => {}
                    _ => return Err("hub frame input acknowledgement fields are incomplete"),
                }
                let mut world = player.world.clone();
                world.restore_state(&participant.world)?;
            }
        }
        Ok(())
    }

    fn simulation_identity(&self) -> Result<[u8; 32], &'static str> {
        #[cfg(test)]
        self.identity_build_count.fetch_add(1, Ordering::Relaxed);
        if self.rooms.len() > MAX_ROOMS {
            return Err("hub room count exceeds its frame bound");
        }
        let mut hash = Sha256::new();
        hash.update(b"racer/authoritative-hub-frame/v1\0");
        hash_string(&mut hash, HUB_SIMULATION_MODEL_REVISION);
        hash_string(&mut hash, SIMULATION_MODEL_REVISION);
        hash_string(&mut hash, VEHICLE_CONTACT_MODEL_REVISION);
        let mut rooms = self.rooms.iter().collect::<Vec<_>>();
        rooms.sort_by_key(|(room_id, _)| **room_id);
        hash_u64(&mut hash, rooms.len() as u64);
        for (room_id, room) in rooms {
            hash_u32(&mut hash, *room_id);
            hash_u32(&mut hash, room.summary.selection_revision);
            hash_string(&mut hash, &room.summary.content_identity);
            hash_string(&mut hash, &room.summary.track_profile_id);
            hash_string(&mut hash, &room.summary.track_fingerprint);
            hash.update([room.summary.laps, u8::from(room.summary.open_drive)]);
            let mut players = room.players.iter().collect::<Vec<_>>();
            if players.is_empty() || players.len() > MAX_PLAYERS_PER_ROOM {
                return Err("hub participant count is outside its frame bound");
            }
            players.sort_by_key(|(player_id, _)| **player_id);
            hash_u64(&mut hash, players.len() as u64);
            for (player_id, player) in players {
                validate_contact_profile(
                    player.contact_radius,
                    player.contact_min_z,
                    player.contact_max_z,
                )?;
                hash_u32(&mut hash, *player_id);
                hash_string(&mut hash, &player.vehicle_profile_id);
                hash_string(&mut hash, &player.vehicle_fingerprint);
                hash.update(player.world.simulation_identity()?);
                for value in [
                    player.contact_radius,
                    player.contact_min_z,
                    player.contact_max_z,
                ] {
                    hash_f64(&mut hash, value);
                }
            }
        }
        Ok(hash.finalize().into())
    }

    /// Return a complete contiguous suffix of retained authoritative tick
    /// schedules. The exclusive start is the checkpoint tick.
    fn capture_tick_schedule(
        &self,
        start_exclusive: u64,
        end_inclusive: u64,
    ) -> Result<Vec<HubTickSchedule>, &'static str> {
        if !self.tick_schedule_recording_enabled {
            return Err("tick schedule recording is disabled");
        }
        let count = end_inclusive.wrapping_sub(start_exclusive);
        if count == 0 || count > HUB_REPLAY_HISTORY_TICKS as u64 {
            return Err("requested replay suffix is outside the retained history bound");
        }
        let mut schedules = Vec::with_capacity(count as usize);
        let mut expected = start_exclusive.wrapping_add(1);
        for _ in 0..count {
            let schedule = self
                .tick_schedules
                .iter()
                .find(|schedule| schedule.tick == expected)
                .ok_or("requested replay suffix contains an evicted or missing tick")?;
            schedules.push(schedule.clone());
            expected = expected.wrapping_add(1);
        }
        Ok(schedules)
    }

    /// Replay a recorded suffix on a private Hub clone. Commit only if every
    /// tick has a matching precondition and reproduces its recorded boundary.
    fn replay_tick_schedule(&mut self, schedules: &[HubTickSchedule]) -> Result<(), &'static str> {
        if !self.tick_schedule_recording_enabled {
            return Err("tick schedule replay is disabled");
        }
        if schedules.is_empty() || schedules.len() > HUB_REPLAY_HISTORY_TICKS {
            return Err("replay suffix is empty or exceeds its history bound");
        }
        let mut staged = self.clone();
        for schedule in schedules {
            staged.apply_scheduled_tick(schedule)?;
        }
        self.server_tick = staged.server_tick;
        self.rooms = staged.rooms;
        self.tick_schedules = staged.tick_schedules;
        Ok(())
    }

    fn apply_scheduled_tick(&mut self, schedule: &HubTickSchedule) -> Result<(), &'static str> {
        if schedule.tick != self.server_tick.wrapping_add(1)
            || schedule.identity != self.simulation_identity()?
            || schedule.before_boundary != self.replay_boundary_identity()?
        {
            return Err("replay tick does not continue this authoritative boundary");
        }

        let mut updates = Vec::new();
        {
            let mut live_racing_rooms = self
                .rooms
                .iter()
                .filter(|(_, room)| matches!(room.summary.status, RaceStatus::Racing))
                .collect::<Vec<_>>();
            live_racing_rooms.sort_by_key(|(room_id, _)| **room_id);
            if schedule.racing_rooms.len() != live_racing_rooms.len() {
                return Err("replay tick racing-room set does not match the checkpoint");
            }
            for ((room_id, room), recorded_room) in
                live_racing_rooms.iter().zip(&schedule.racing_rooms)
            {
                if recorded_room.room_id != **room_id
                    || recorded_room.selection_revision != room.summary.selection_revision
                    || recorded_room.participants.len() != room.players.len()
                {
                    return Err("replay tick room roster or selection revision changed");
                }
                let mut players = room.players.iter().collect::<Vec<_>>();
                players.sort_by_key(|(player_id, _)| **player_id);
                if recorded_room
                    .participants
                    .iter()
                    .map(|sample| sample.player_id)
                    .collect::<Vec<_>>()
                    != players
                        .iter()
                        .map(|(player_id, _)| **player_id)
                        .collect::<Vec<_>>()
                {
                    return Err("replay tick participant set is incomplete or unordered");
                }
                for ((player_id, player), sample) in players.iter().zip(&recorded_room.participants)
                {
                    if sample.world_identity != player.world.simulation_identity()?
                        || sample.applied_sequence
                            != (!player.world.vehicle.finished)
                                .then_some(sample.received_sequence)
                                .flatten()
                    {
                        return Err("replay tick sample does not match the live vehicle state");
                    }
                    validate_latest_input(sample.input)?;
                    match (player.ack_sequence, sample.received_sequence) {
                        (None, None) if sample.input == DriverInput::default() => {}
                        (None, Some(_)) => {}
                        (Some(previous), Some(next)) if previous == next => {
                            if sample.input != player.input {
                                return Err("replay tick changed intent without a newer sequence");
                            }
                        }
                        (Some(previous), Some(next)) if sequence_is_newer(next, previous) => {}
                        _ => return Err("replay tick input sequence is stale or incomplete"),
                    }
                    updates.push((recorded_room.room_id, **player_id, (*sample).clone()));
                }
            }
        }
        for (room_id, player_id, sample) in updates {
            let player = self
                .rooms
                .get_mut(&room_id)
                .and_then(|room| room.players.get_mut(&player_id))
                .ok_or("replay tick participant disappeared")?;
            player.input = sample.input;
            player.ack_sequence = sample.received_sequence;
        }

        advance_authoritative_tick_internal(self, false);
        if self.tick_schedules.back() != Some(schedule) {
            return Err("replayed tick did not reproduce its recorded boundary");
        }
        Ok(())
    }

    /// Hash simulation state and lifecycle preconditions, excluding the latest
    /// input/received sequence because those are supplied by each tick record.
    fn replay_boundary_identity(&self) -> Result<[u8; 32], &'static str> {
        let mut hash = Sha256::new();
        hash.update(b"racer/hub-replay-boundary/v1\0");
        hash.update(self.simulation_identity()?);
        hash_u64(&mut hash, self.server_tick);
        let mut rooms = self.rooms.iter().collect::<Vec<_>>();
        rooms.sort_by_key(|(room_id, _)| **room_id);
        hash_u64(&mut hash, rooms.len() as u64);
        for (room_id, room) in rooms {
            hash_u32(&mut hash, *room_id);
            hash_u32(&mut hash, room.summary.selection_revision);
            hash_u32(&mut hash, room.owner_player_id);
            hash_u32(&mut hash, room.summary.host_player_id);
            hash_u64(&mut hash, room.players.len() as u64);
            hash.update(room.summary.player_count.to_le_bytes());
            hash_race_status(&mut hash, &room.status());
            hash.update(room.countdown.to_le_bytes());
            let mut players = room.players.iter().collect::<Vec<_>>();
            players.sort_by_key(|(player_id, _)| **player_id);
            for (player_id, player) in players {
                hash_u32(&mut hash, *player_id);
                hash.update([u8::from(player.ready)]);
                hash.update(player.world.capture_state()?.encode());
                hash_option_u32(&mut hash, player.applied_sequence);
                hash_option_u64(&mut hash, player.applied_server_tick);
            }
        }
        Ok(hash.finalize().into())
    }
}

impl AppState {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Hub::default())), None, None)
    }
}
impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate the project track catalog now (blocking). The server binary calls
/// this on a background thread at startup.
pub fn warm_project_tracks() {
    project_tracks::warm();
}

pub fn router(web_root: impl Into<std::path::PathBuf>) -> Router {
    router_with_private_assets(web_root, None).expect("disabled private asset provider is valid")
}

pub fn router_with_private_assets(
    web_root: impl Into<std::path::PathBuf>,
    config: Option<PrivateAssetConfig>,
) -> Result<Router, String> {
    router_with_private_assets_and_gameplay(web_root, config, None)
}

pub fn router_with_private_assets_and_gameplay(
    web_root: impl Into<std::path::PathBuf>,
    config: Option<PrivateAssetConfig>,
    gameplay: Option<GameplayDataStore>,
) -> Result<Router, String> {
    ServerConfig::new()
        .private_assets(config)
        .gameplay(gameplay)
        .router(web_root)
}

/// Builder for a server that embeds external providers. A separate binary
/// crate registers its vehicle models and content providers here and then runs
/// the stock server via [`ServerConfig::router`].
pub struct ServerConfig {
    private_assets: Option<PrivateAssetConfig>,
    gameplay: Option<GameplayDataStore>,
    vehicle_models: VehicleModelRegistry,
    content_providers: ProviderRegistry,
    plugin_dir: Option<std::path::PathBuf>,
    imported_default_model: Option<String>,
}

/// Environment variable naming a directory of browser plugins to serve.
pub const PLUGIN_DIR_ENV: &str = "RACER_PLUGIN_DIR";

impl Default for ServerConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerConfig {
    pub fn new() -> Self {
        Self {
            private_assets: None,
            gameplay: None,
            vehicle_models: VehicleModelRegistry::new(),
            content_providers: ProviderRegistry::with_native(),
            imported_default_model: None,
            plugin_dir: std::env::var_os(PLUGIN_DIR_ENV)
                .filter(|value| !value.is_empty())
                .map(std::path::PathBuf::from),
        }
    }

    /// Directory of browser plugins to serve at `/plugins/ext/` and advertise
    /// through `/plugins.json` (default: the `RACER_PLUGIN_DIR` environment
    /// variable). `None` serves nothing.
    pub fn plugin_dir(mut self, dir: Option<std::path::PathBuf>) -> Self {
        self.plugin_dir = dir;
        self
    }

    pub fn private_assets(mut self, config: Option<PrivateAssetConfig>) -> Self {
        self.private_assets = config;
        self
    }

    pub fn gameplay(mut self, gameplay: Option<GameplayDataStore>) -> Self {
        self.gameplay = gameplay;
        self
    }

    /// Providers a client may select with `SetCarModel`.
    pub fn with_vehicle_models(mut self, registry: VehicleModelRegistry) -> Self {
        self.vehicle_models = registry;
        self
    }

    /// Registry id of the model that worlds of imported (gameplay-catalogue)
    /// cars start with. Without one they keep the engine's default model.
    pub fn with_imported_default_model(mut self, id: impl Into<String>) -> Self {
        self.imported_default_model = Some(id.into());
        self
    }

    /// Content providers available to the server (use
    /// `ProviderRegistry::with_native` to keep the native pack provider).
    pub fn with_content_providers(mut self, registry: ProviderRegistry) -> Self {
        self.content_providers = registry;
        self
    }

    pub fn router(self, web_root: impl Into<std::path::PathBuf>) -> Result<Router, String> {
        build_router(web_root.into(), self)
    }
}

fn build_router(web_root: std::path::PathBuf, cfg: ServerConfig) -> Result<Router, String> {
    let ServerConfig {
        private_assets: config,
        gameplay,
        vehicle_models,
        content_providers,
        plugin_dir,
        imported_default_model,
    } = cfg;
    let private_assets = config
        .map(private_assets::PrivateAssetStore::open)
        .transpose()?
        .map(Arc::new);
    let mut hub = Hub::default();
    hub.vehicle_models = Arc::new(vehicle_models);
    hub.content_providers = Arc::new(content_providers);
    hub.imported_default_model = imported_default_model;
    if let Some(catalog) = &gameplay {
        hub.available_tracks = catalog
            .tracks()
            .iter()
            .map(|track| {
                let mut cars = track
                    .cars
                    .values()
                    .map(|car| protocol::AvailableCarProfile {
                        profile_id: car.profile_id.clone(),
                        source_fingerprint: car.source_fingerprint.clone(),
                    })
                    .collect::<Vec<_>>();
                cars.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
                protocol::AvailableTrackProfile {
                    profile_id: track.profile_id.clone(),
                    source_fingerprint: track.source_fingerprint.clone(),
                    cars,
                }
            })
            .collect();
        hub.available_tracks
            .sort_by(|left, right| left.profile_id.cmp(&right.profile_id));
    }
    let state = AppState(
        Arc::new(Mutex::new(hub)),
        private_assets,
        gameplay.map(Arc::new),
    );
    let mut routes = Router::new()
        .route("/healthz", get(health))
        .route("/ws", get(websocket));
    if state.1.is_some() {
        routes = routes
            .route(
                "/private-assets/manifest.json",
                get(private_assets::manifest),
            )
            .route("/private-assets/{asset_id}", get(private_assets::asset));
    }
    if let Some(dir) = plugin_dir {
        match plugin_manifest_json(&dir) {
            Ok(manifest) => {
                let manifest = Arc::new(manifest);
                routes = routes
                    .route(
                        "/plugins.json",
                        get(move || {
                            let manifest = Arc::clone(&manifest);
                            async move {
                                (
                                    [
                                        ("content-type", "application/json"),
                                        ("cache-control", "no-store"),
                                    ],
                                    manifest.as_str().to_owned(),
                                )
                            }
                        }),
                    )
                    .nest_service("/plugins/ext", tower_http::services::ServeDir::new(dir));
            }
            Err(error) => eprintln!("browser plugins disabled: {error}"),
        }
    }
    let app = routes
        .fallback_service(tower_http::services::ServeDir::new(web_root))
        .layer(axum::middleware::map_response(revalidate_by_default))
        .with_state(state.clone());
    tokio::spawn(tick_loop(state));
    Ok(app)
}

/// Browsers must revalidate static files on every load (ETag/Last-Modified still give 304s), so a
/// redeploy never leaves a page running a stale mix of scripts and wasm. Routes that set their own
/// `cache-control` keep it.
async fn revalidate_by_default(mut response: axum::response::Response) -> axum::response::Response {
    response
        .headers_mut()
        .entry(axum::http::header::CACHE_CONTROL)
        .or_insert(axum::http::HeaderValue::from_static("no-cache"));
    response
}

/// Body of `/plugins.json` for a plugin directory: its `plugin.json`
/// (`{ "plugins": ["main.js"] }`, file names relative to the directory) or
/// `index.js` when that file exists, mapped to `/plugins/ext/` URLs.
pub fn plugin_manifest_json(dir: &std::path::Path) -> Result<String, String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    let names: Vec<String> = match std::fs::read_to_string(dir.join("plugin.json")) {
        Ok(text) => {
            let value: serde_json::Value =
                serde_json::from_str(&text).map_err(|error| format!("plugin.json: {error}"))?;
            value
                .get("plugins")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| "plugin.json needs a \"plugins\" array".to_owned())?
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        }
        Err(_) if dir.join("index.js").is_file() => vec!["index.js".to_owned()],
        Err(_) => Vec::new(),
    };
    let mut urls = Vec::new();
    for name in names {
        if !valid_plugin_file_name(&name) {
            return Err(format!(
                "plugin.json lists an unsupported file name {name:?}"
            ));
        }
        urls.push(format!("/plugins/ext/{name}"));
    }
    serde_json::to_string(&serde_json::json!({ "plugins": urls }))
        .map_err(|error| error.to_string())
}

fn valid_plugin_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && (name.ends_with(".js") || name.ends_with(".mjs"))
        && name.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
}

async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        format!(r#"{{"status":"ok","protocol_major":{PROTOCOL_MAJOR}}}"#),
    )
}

async fn websocket(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| connection(socket, state))
}

fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Some(host) = headers.get("host").and_then(|v| v.to_str().ok()) else {
        return false;
    };
    origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .is_some_and(|authority| authority.eq_ignore_ascii_case(host))
}

async fn connection(socket: WebSocket, state: AppState) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (reliable_tx, mut reliable_rx) = mpsc::channel::<ServerMessage>(32);
    let (snapshots_tx, mut snapshots_rx) = watch::channel::<Option<ServerMessage>>(None);
    let (close_tx, mut close_rx) = watch::channel(false);
    let player_id = {
        let mut hub = state.0.lock().await;
        let id = hub.next_player;
        hub.next_player = hub.next_player.wrapping_add(1).max(1);
        hub.clients.insert(
            id,
            ClientChannel {
                reliable: reliable_tx.clone(),
                snapshots: snapshots_tx,
                close: close_tx,
            },
        );
        let _ = reliable_tx.try_send(ServerMessage::Welcome {
            player_id: id,
            protocol_major: PROTOCOL_MAJOR,
            build_id: BUILD_ID.into(),
        });
        send_lobby(&hub, Some(id));
        id
    };
    let mut limiter = RateLimiter::default();
    let mut handshaken = false;
    loop {
        tokio::select! {
            _ = close_rx.changed() => break,
            outbound = reliable_rx.recv() => match outbound {
                Some(message) => match protocol::encode_server(&message) {
                    Ok(bytes) if bytes.len() <= MAX_MESSAGE_BYTES => if !send_ws(&mut ws_tx, Message::Binary(bytes.into())).await { break },
                    _ => break,
                },
                None => break,
            },
            changed = snapshots_rx.changed() => {
                if changed.is_err() { break; }
                let latest_snapshot = { snapshots_rx.borrow_and_update().clone() };
                if let Some(message) = latest_snapshot {
                    match protocol::encode_server(&message) {
                        Ok(bytes) if bytes.len() <= MAX_MESSAGE_BYTES => if !send_ws(&mut ws_tx, Message::Binary(bytes.into())).await { break; },
                        _ => break,
                    }
                }
            }
            incoming = ws_rx.next() => match incoming {
                Some(Ok(Message::Binary(bytes))) => {
                    if bytes.len() > MAX_MESSAGE_BYTES { break; }
                    let Ok(message) = protocol::decode_client(&bytes) else {
                        let _ = reliable_tx.try_send(ServerMessage::Error { code: "malformed".into(), detail: "invalid protocol message".into() });
                        continue;
                    };
                    if !handshaken {
                        match &message {
                            ClientMessage::Hello { protocol_major, build_id, content_capabilities } if *protocol_major == PROTOCOL_MAJOR => {
                                if build_id.len() > 32 || content_capabilities.len() > 16 || content_capabilities.iter().any(|s| s.len() > 64) {
                                    let _ = reliable_tx.try_send(ServerMessage::Error { code: "hello_too_large".into(), detail: "hello capabilities exceed limits".into() });
                                    continue;
                                }
                                handshaken = true;
                            }
                            ClientMessage::Hello { .. } => {
                                let error = ServerMessage::Error { code: "protocol_mismatch".into(), detail: format!("server protocol {PROTOCOL_MAJOR}") };
                                if let Ok(bytes) = protocol::encode_server(&error) { let _ = send_ws(&mut ws_tx, Message::Binary(bytes.into())).await; }
                                break;
                            }
                            _ => {
                                let _ = reliable_tx.try_send(ServerMessage::Error { code: "hello_required".into(), detail: "send protocol Hello first".into() });
                                continue;
                            }
                        }
                    }
                    if !limiter.allow(&message) {
                        let _ = reliable_tx.try_send(ServerMessage::Error { code: "rate_limited".into(), detail: "message rate exceeded".into() });
                        continue;
                    }
                    process_message(&state, player_id, message).await;
                }
                Some(Ok(Message::Ping(bytes))) => { if !send_ws(&mut ws_tx, Message::Pong(bytes)).await { break; } }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => { let _ = reliable_tx.try_send(ServerMessage::Error { code: "binary_required".into(), detail: "send postcard binary messages".into() }); }
            }
        }
    }
    disconnect(&state, player_id).await;
}

async fn send_ws(
    tx: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: Message,
) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(2), tx.send(message)).await,
        Ok(Ok(()))
    )
}

#[derive(Default)]
struct RateLimiter {
    epoch: Option<Instant>,
    total: u32,
    inputs: u32,
    mutations: u32,
}
impl RateLimiter {
    fn allow(&mut self, message: &ClientMessage) -> bool {
        let now = Instant::now();
        if self
            .epoch
            .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(1))
        {
            self.epoch = Some(now);
            self.total = 0;
            self.inputs = 0;
            self.mutations = 0;
        }
        self.total += 1;
        if self.total > 150 {
            return false;
        }
        match message {
            ClientMessage::DriverInput { .. } => {
                self.inputs += 1;
                self.inputs <= INPUTS_PER_SECOND
            }
            ClientMessage::CreateRace { .. }
            | ClientMessage::JoinRace { .. }
            | ClientMessage::ChangeTrack { .. }
            | ClientMessage::ChangeVehicle { .. }
            | ClientMessage::ConfigureAi { .. }
            | ClientMessage::LeaveRace
            | ClientMessage::SetCarModel { .. }
            | ClientMessage::SetTransmission { .. }
            | ClientMessage::ReadyState { .. } => {
                self.mutations += 1;
                self.mutations <= MUTATIONS_PER_SECOND
            }
            _ => true,
        }
    }
}

async fn process_message(state: &AppState, player_id: u32, message: ClientMessage) {
    let mut hub = state.0.lock().await;
    match message {
        ClientMessage::Hello {
            protocol_major,
            build_id,
            content_capabilities,
        } => {
            if protocol_major != PROTOCOL_MAJOR {
                send(
                    &hub,
                    player_id,
                    ServerMessage::Error {
                        code: "protocol_mismatch".into(),
                        detail: format!(
                            "server protocol {PROTOCOL_MAJOR}; client {protocol_major}"
                        ),
                    },
                );
            } else if build_id.len() > 32
                || content_capabilities.len() > 16
                || content_capabilities.iter().any(|s| s.len() > 64)
            {
                send(
                    &hub,
                    player_id,
                    ServerMessage::Error {
                        code: "hello_too_large".into(),
                        detail: "hello capabilities exceed limits".into(),
                    },
                );
            } else {
                send_lobby(&hub, Some(player_id));
            }
        }
        ClientMessage::ListRaces => send_lobby(&hub, Some(player_id)),
        ClientMessage::CreateRace {
            name,
            laps,
            content_identity,
            track_profile_id,
            track_fingerprint,
            vehicle_profile_id,
            vehicle_fingerprint,
            driver_name,
            paint_id,
            car_model,
            car_variant,
            ai_count,
            ai_skill,
            ai_catch_up,
        } => {
            if room_for_player(&hub, player_id).is_some() {
                send_error(&hub, player_id, "already_joined");
            } else if hub.rooms.len() >= MAX_ROOMS {
                send_error(&hub, player_id, "room_limit");
            } else if !valid_name(&name)
                || !valid_driver_name(&driver_name)
                || !valid_paint(&paint_id)
                || !valid_car_choice(&car_model, car_variant)
                || ai_count > MAX_AI_OPPONENTS
                || ai_skill > MAX_AI_SKILL
            {
                send_error(&hub, player_id, "invalid_settings");
            } else {
                let selection = resolve_selection(
                    &state,
                    &track_profile_id,
                    &track_fingerprint,
                    &vehicle_profile_id,
                    &vehicle_fingerprint,
                    &content_identity,
                    laps,
                );
                match selection {
                    Err(code) => send_error(&hub, player_id, code),
                    Ok((gameplay_track, gameplay_car, open_drive)) => {
                        let laps = if open_drive {
                            owner_room_laps(gameplay_track.as_ref(), laps)
                        } else {
                            laps
                        };
                        let project_room = is_project_room_track(gameplay_track.as_ref());
                        let room_content_identity = if open_drive {
                            "owner-open-drive-v1"
                        } else if project_room {
                            PROJECT_TRACK_CONTENT_ID
                        } else {
                            PROJECT_CONTENT_ID
                        };
                        let world = match new_room_world(
                            gameplay_track.as_ref(),
                            laps,
                            open_drive,
                            gameplay_car.as_ref(),
                            &ModelSet::of(&hub),
                            0,
                            &car_model,
                        ) {
                            Ok(world) => world,
                            Err(code) => {
                                send_error(&hub, player_id, code);
                                return;
                            }
                        };
                        let id = hub.next_room;
                        hub.next_room = hub.next_room.wrapping_add(1).max(1);
                        let mut player = new_player(
                            world,
                            driver_name,
                            vehicle_profile_id,
                            vehicle_fingerprint,
                            paint_id,
                            (car_model, car_variant),
                            gameplay_car.as_ref(),
                        );
                        let (transmission, auto_reverse) = hub
                            .transmissions
                            .get(&player_id)
                            .copied()
                            .unwrap_or_default();
                        apply_transmission(&mut player, transmission, auto_reverse);
                        let mut players = HashMap::new();
                        players.insert(player_id, player);
                        hub.rooms.insert(
                            id,
                            Room {
                                summary: RaceSummary {
                                    race_id: id,
                                    host_player_id: player_id,
                                    name,
                                    player_count: 1,
                                    max_players: MAX_PLAYERS_PER_ROOM as u8,
                                    laps,
                                    status: RaceStatus::Lobby,
                                    content_identity: room_content_identity.into(),
                                    track_profile_id: track_profile_id.clone(),
                                    track_fingerprint: track_fingerprint.clone(),
                                    open_drive,
                                    selection_revision: 1,
                                    ai_count: if open_drive && laps == 0 { 0 } else { ai_count },
                                    ai_skill,
                                    ai_catch_up,
                                },
                                owner_player_id: player_id,
                                gameplay_track,
                                players,
                                countdown: 0,
                                ai: None,
                            },
                        );
                        reapply_externals(&mut hub, id);
                        send(&hub, player_id, ServerMessage::RaceCreated { race_id: id });
                        send(
                            &hub,
                            player_id,
                            ServerMessage::JoinAccepted {
                                race_id: id,
                                player_id,
                                content_requirement: room_content_identity.into(),
                                track_profile_id,
                                track_fingerprint,
                                selection_revision: 1,
                            },
                        );
                        publish(&hub, Some(id));
                    }
                }
            }
        }
        ClientMessage::JoinRace {
            race_id,
            content_identity,
            track_profile_id,
            track_fingerprint,
            vehicle_profile_id,
            vehicle_fingerprint,
            driver_name,
            paint_id,
            car_model,
            car_variant,
        } => {
            let models = ModelSet::of(&hub);
            let (transmission, auto_reverse) = hub
                .transmissions
                .get(&player_id)
                .copied()
                .unwrap_or_default();
            if room_for_player(&hub, player_id).is_some() {
                reject_join(&hub, player_id, "already_joined");
            } else if !valid_driver_name(&driver_name)
                || !valid_paint(&paint_id)
                || !valid_car_choice(&car_model, car_variant)
            {
                reject_join(&hub, player_id, "invalid_settings");
            } else if let Some(room) = hub.rooms.get_mut(&race_id) {
                if room.players.len() >= MAX_PLAYERS_PER_ROOM {
                    reject_join(&hub, player_id, "room_full");
                } else if !matches!(room.summary.status, RaceStatus::Lobby) {
                    reject_join(&hub, player_id, "race_started");
                } else if !selection_matches_room(
                    room,
                    &content_identity,
                    &track_profile_id,
                    &track_fingerprint,
                    &vehicle_profile_id,
                    &vehicle_fingerprint,
                ) {
                    reject_join(&hub, player_id, "content_mismatch");
                } else {
                    let gameplay_car = room
                        .gameplay_track
                        .as_ref()
                        .and_then(|track| track.cars.get(&vehicle_profile_id));
                    let world = match new_room_world(
                        room.gameplay_track.as_ref(),
                        room.summary.laps,
                        room.summary.open_drive,
                        gameplay_car,
                        &models,
                        room.players.len(),
                        &car_model,
                    ) {
                        Ok(world) => world,
                        Err(code) => {
                            reject_join(&hub, player_id, code);
                            return;
                        }
                    };
                    let mut joined = new_player(
                        world,
                        driver_name,
                        vehicle_profile_id,
                        vehicle_fingerprint,
                        paint_id,
                        (car_model, car_variant),
                        gameplay_car,
                    );
                    apply_transmission(&mut joined, transmission, auto_reverse);
                    room.players.insert(player_id, joined);
                    room.summary.selection_revision =
                        room.summary.selection_revision.wrapping_add(1).max(1);
                    for player in room.players.values_mut() {
                        player.ready = false;
                    }
                    room.summary.player_count = room.players.len() as u8;
                    let join_accepted = ServerMessage::JoinAccepted {
                        race_id,
                        player_id,
                        content_requirement: room.summary.content_identity.clone(),
                        track_profile_id: room.summary.track_profile_id.clone(),
                        track_fingerprint: room.summary.track_fingerprint.clone(),
                        selection_revision: room.summary.selection_revision,
                    };
                    send(&hub, player_id, join_accepted);
                    reapply_externals(&mut hub, race_id);
                    publish(&hub, Some(race_id));
                }
            } else {
                reject_join(&hub, player_id, "race_not_found");
            }
        }
        ClientMessage::ChangeTrack {
            race_id,
            // The track belongs to the host alone, so another player's
            // concurrent change cannot conflict with it: the revision the host
            // saw is not compared.
            selection_revision: _,
            content_identity,
            track_profile_id,
            track_fingerprint,
            laps,
        } => {
            let models = ModelSet::of(&hub);
            if let Some(room) = hub.rooms.get_mut(&race_id) {
                if room.owner_player_id != player_id {
                    send_error(&hub, player_id, "host_only");
                } else if !matches!(room.summary.status, RaceStatus::Lobby) {
                    send_error(&hub, player_id, "race_started");
                } else {
                    match resolve_track_selection(
                        &state,
                        &track_profile_id,
                        &track_fingerprint,
                        &content_identity,
                        laps,
                    ) {
                        Ok((gameplay_track, open_drive)) => {
                            let laps = if open_drive {
                                owner_room_laps(gameplay_track.as_ref(), laps)
                            } else {
                                laps
                            };
                            match change_room_track(
                                room,
                                track_profile_id,
                                track_fingerprint,
                                content_identity,
                                laps,
                                gameplay_track,
                                open_drive,
                                &models,
                            ) {
                                Ok(()) => {
                                    reapply_externals(&mut hub, race_id);
                                    publish(&hub, Some(race_id))
                                }
                                Err(code) => send_error(&hub, player_id, code),
                            }
                        }
                        Err(code) => send_error(&hub, player_id, code),
                    }
                }
            } else {
                send_error(&hub, player_id, "race_not_found");
            }
        }
        ClientMessage::ChangeVehicle {
            race_id,
            // A player's car is theirs alone, so concurrent changes by other
            // players (or the host's track change) cannot conflict with it:
            // the revision the player saw is not compared. The server checks
            // the car against the room's current track instead.
            selection_revision: _,
            vehicle_profile_id,
            vehicle_fingerprint,
            paint_id,
            car_model,
            car_variant,
        } => {
            let models = ModelSet::of(&hub);
            if !valid_paint(&paint_id)
                || !valid_car_choice(&car_model, car_variant)
                || !valid_fingerprint_or_project(&vehicle_fingerprint)
            {
                send_error(&hub, player_id, "invalid_settings");
            } else if let Some(room) = hub.rooms.get_mut(&race_id) {
                if !matches!(room.summary.status, RaceStatus::Lobby) {
                    send_error(&hub, player_id, "race_started");
                } else {
                    match change_player_vehicle(
                        room,
                        player_id,
                        vehicle_profile_id,
                        vehicle_fingerprint,
                        paint_id,
                        (car_model, car_variant),
                        &models,
                    ) {
                        Ok(()) => {
                            reapply_externals(&mut hub, race_id);
                            publish(&hub, Some(race_id))
                        }
                        Err(code) => send_error(&hub, player_id, code),
                    }
                }
            } else {
                send_error(&hub, player_id, "not_in_race");
            }
        }
        ClientMessage::ConfigureAi {
            race_id,
            // Host-only settings; see ChangeTrack.
            selection_revision: _,
            ai_count,
            ai_skill,
            ai_catch_up,
        } => {
            if let Some(room) = hub.rooms.get_mut(&race_id) {
                if room.owner_player_id != player_id {
                    send_error(&hub, player_id, "host_only");
                } else if !matches!(room.summary.status, RaceStatus::Lobby) {
                    send_error(&hub, player_id, "race_started");
                } else if ai_count > MAX_AI_OPPONENTS
                    || ai_skill > MAX_AI_SKILL
                    || (!room.races() && ai_count > 0)
                {
                    send_error(&hub, player_id, "invalid_settings");
                } else {
                    room.summary.ai_count = ai_count;
                    room.summary.ai_skill = ai_skill;
                    room.summary.ai_catch_up = ai_catch_up;
                    room.summary.selection_revision =
                        room.summary.selection_revision.wrapping_add(1).max(1);
                    for player in room.players.values_mut() {
                        player.ready = false;
                    }
                    publish(&hub, Some(race_id));
                }
            } else {
                send_error(&hub, player_id, "race_not_found");
            }
        }
        ClientMessage::LeaveRace => {
            leave_room(&mut hub, player_id);
            publish(&hub, None);
        }
        ClientMessage::ReadyState {
            ready,
            selection_revision,
            track_fingerprint,
            vehicle_templates,
        } => {
            if let Some(id) = room_for_player(&hub, player_id) {
                let mut error_code = None;
                let mut ready_revision = None;
                let mut countdown_recipients = Vec::new();
                if let Some(room) = hub.rooms.get_mut(&id) {
                    if !matches!(room.summary.status, RaceStatus::Lobby) {
                        error_code = Some("race_started");
                    } else if room.summary.selection_revision != selection_revision {
                        error_code = Some("stale_revision");
                    } else if ready
                        && !client_has_current_roster_templates(
                            room,
                            &track_fingerprint,
                            &vehicle_templates,
                        )
                    {
                        error_code = Some("render_profile_missing");
                    } else if let Some(p) = room.players.get_mut(&player_id) {
                        p.ready = ready;
                        let has_opponents = room.races() && room.summary.ai_count > 0;
                        if (room.players.len() >= 2 || has_opponents)
                            && room.players.values().all(|p| p.ready)
                        {
                            opponents::prepare_start(room);
                            room.countdown = COUNTDOWN_TICKS;
                            room.summary.status = RaceStatus::Countdown {
                                ticks_left: room.countdown,
                            };
                            countdown_recipients = room.players.keys().copied().collect();
                        }
                        ready_revision = Some(room.summary.selection_revision);
                    } else {
                        error_code = Some("not_in_race");
                    }
                } else {
                    error_code = Some("not_in_race");
                }
                if let Some(code) = error_code {
                    send_error(&hub, player_id, code);
                } else if let Some(revision) = ready_revision {
                    for recipient in countdown_recipients {
                        send(
                            &hub,
                            recipient,
                            ServerMessage::Countdown {
                                race_id: id,
                                ticks_left: COUNTDOWN_TICKS,
                            },
                        );
                    }
                    send(
                        &hub,
                        player_id,
                        ServerMessage::ReadyAccepted {
                            race_id: id,
                            ready,
                            selection_revision: revision,
                        },
                    );
                    publish(&hub, Some(id));
                }
            } else {
                send_error(&hub, player_id, "not_in_race");
            }
        }
        ClientMessage::DriverInput {
            sequence,
            client_tick: _,
            throttle,
            brake,
            steer,
            handbrake,
            shift_up,
            shift_down,
            reset_request,
        } => {
            if let Some(input) = normalize_input(
                throttle,
                brake,
                steer,
                handbrake,
                (shift_up, shift_down),
                reset_request,
            ) {
                if let Some(id) = room_for_player(&hub, player_id) {
                    let room = hub.rooms.get_mut(&id).expect("room index points to room");
                    if matches!(room.summary.status, RaceStatus::Racing) {
                        if let Some(p) = room.players.get_mut(&player_id) {
                            if p.ack_sequence
                                .is_none_or(|old| sequence_is_newer(sequence, old))
                            {
                                p.shift_latch.0 |= input.shift_up;
                                p.shift_latch.1 |= input.shift_down;
                                p.input = input;
                                p.ack_sequence = Some(sequence);
                            }
                        }
                    }
                } else {
                    send_error(&hub, player_id, "not_in_race");
                }
            } else {
                send_error(&hub, player_id, "non_finite_input");
            }
        }
        ClientMessage::SetCarModel { model_id, params } => {
            let ack_id: String = model_id
                .chars()
                .take(protocol::MAX_MODEL_ID_BYTES)
                .collect();
            let result = protocol::validate_car_model(&model_id, &params)
                .map_err(str::to_string)
                .and_then(|()| {
                    hub.vehicle_models
                        .instantiate(&model_id, &params)
                        .map(|_| ())
                });
            let (accepted, reason) = match result {
                Ok(()) => {
                    let entry = (model_id, params);
                    hub.external_cars.insert(player_id, entry.clone());
                    if let Some(id) = room_for_player(&hub, player_id) {
                        let registry = Arc::clone(&hub.vehicle_models);
                        if let Some(room) = hub.rooms.get_mut(&id) {
                            apply_external_to_player(room, player_id, &registry, &entry);
                        }
                        publish(&hub, Some(id));
                    }
                    (true, String::new())
                }
                Err(reason) => (false, reason),
            };
            send(
                &hub,
                player_id,
                ServerMessage::CarModelAck {
                    model_id: ack_id,
                    accepted,
                    reason,
                },
            );
        }
        ClientMessage::SetTransmission {
            transmission,
            auto_reverse,
        } => {
            hub.transmissions
                .insert(player_id, (transmission, auto_reverse));
            match room_for_player(&hub, player_id) {
                None => {} // remembered for the room this player creates or joins
                Some(id) => {
                    let in_lobby = hub
                        .rooms
                        .get(&id)
                        .is_some_and(|room| matches!(room.summary.status, RaceStatus::Lobby));
                    if !in_lobby {
                        hub.transmissions.remove(&player_id);
                        send_error(&hub, player_id, "race_started");
                    } else {
                        if let Some(player) = hub
                            .rooms
                            .get_mut(&id)
                            .and_then(|room| room.players.get_mut(&player_id))
                        {
                            apply_transmission(player, transmission, auto_reverse);
                        }
                        publish(&hub, Some(id));
                    }
                }
            }
        }
        ClientMessage::Ping { nonce } => send(
            &hub,
            player_id,
            ServerMessage::Pong {
                nonce,
                server_tick: hub.server_tick,
            },
        ),
    }
}

fn new_track_runtime(laps: u8, open_drive: bool) -> TrackRuntime {
    if open_drive {
        let mut runtime = TrackRuntime::open_preview();
        runtime.laps_to_finish = 0;
        runtime
    } else {
        let mut runtime = TrackRuntime::synthetic_circuit();
        runtime.laps_to_finish = u32::from(laps);
        runtime
    }
}

fn is_project_room_track(track: Option<&Arc<GameplayTrack>>) -> bool {
    track.is_some_and(|t| t.project.is_some())
}

fn default_project_vehicle(profile_id: &str, fingerprint: &str) -> bool {
    profile_id == PROJECT_VEHICLE_PROFILE_ID && fingerprint == "0".repeat(64)
}

/// World for one player of a room. Generated project tracks use their own
/// gates, laps and start grid (`slot` is the player's grid position) with the
/// project default car's collision; other rooms use the circuit or the owner's
/// open-drive geometry.
fn new_room_world(
    gameplay_track: Option<&Arc<GameplayTrack>>,
    laps: u8,
    open_drive: bool,
    car: Option<&GameplayCar>,
    models: &ModelSet,
    slot: usize,
    car_model: &str,
) -> Result<RaceWorld, &'static str> {
    if let (Some(project), Some(car)) = (gameplay_track.and_then(|t| t.project.as_ref()), car) {
        let mut runtime = project.runtime.clone();
        runtime.laps_to_finish = u32::from(laps);
        let mut world = RaceWorld::new(SimTiming::default(), runtime);
        world.enable_shared_preview_handling();
        let spawn = project
            .spawn_slots
            .get(slot)
            .or(project.spawn_slots.last())
            .copied()
            .unwrap_or(Vec2::new(0.0, 0.0));
        world.set_spawn(spawn);
        world.install_preview_collision(car.collision.clone())?;
        // Project cars drive with the classic model, with the parameters
        // of the car the player chose.
        world.use_classic_model(car_classic_config(car_model))?;
        return Ok(world);
    }
    let runtime = match gameplay_track.and_then(|t| t.course.as_ref()) {
        Some(course) if open_drive && laps > 0 => course.runtime_for_laps(u32::from(laps)),
        _ => new_track_runtime(laps, open_drive),
    };
    new_world_with(runtime, car, models, car_model)
}

/// Classic handling parameters of a chosen catalogue car; no choice (or an
/// unknown id) drives the default demo car.
fn car_classic_config(car_model: &str) -> ClassicConfig {
    project_classic_config(car_model).unwrap_or_else(default_project_classic_config)
}

fn resolve_track_selection(
    state: &AppState,
    track_profile_id: &str,
    track_fingerprint: &str,
    content_identity: &str,
    laps: u8,
) -> Result<(Option<Arc<GameplayTrack>>, bool), &'static str> {
    if track_profile_id == "track-profile-project-circuit" {
        if content_identity != PROJECT_CONTENT_ID
            || track_fingerprint != "0".repeat(64)
            || !(1..=5).contains(&laps)
        {
            return Err("content_mismatch");
        }
        return Ok((None, false));
    }
    if is_project_profile(track_profile_id) {
        let track =
            resolve_project_selection(track_profile_id, track_fingerprint, content_identity, laps)?;
        return Ok((Some(track), false));
    }
    if content_identity != "owner-open-drive-v1"
        || laps > MAX_OWNER_LAPS
        || !valid_fingerprint(track_fingerprint)
    {
        return Err("invalid_settings");
    }
    let catalog = state.2.as_ref().ok_or("profile_unavailable")?;
    let track = catalog
        .track(track_profile_id)
        .ok_or("profile_unavailable")?;
    if track.source_fingerprint != track_fingerprint {
        return Err("content_mismatch");
    }
    Ok((Some(Arc::clone(track)), true))
}

fn change_room_track(
    room: &mut Room,
    track_profile_id: String,
    track_fingerprint: String,
    content_identity: String,
    laps: u8,
    gameplay_track: Option<Arc<GameplayTrack>>,
    open_drive: bool,
    models: &ModelSet,
) -> Result<(), &'static str> {
    let mut replacements = Vec::with_capacity(room.players.len());
    let mut ordered_ids: Vec<u32> = room.players.keys().copied().collect();
    ordered_ids.sort_unstable();
    for (player_id, player) in &room.players {
        let slot = ordered_ids.binary_search(player_id).unwrap_or(0);
        let project_room = is_project_room_track(gameplay_track.as_ref());
        let car = if project_room {
            if !default_project_vehicle(&player.vehicle_profile_id, &player.vehicle_fingerprint) {
                return Err("car_unavailable_on_track");
            }
            gameplay_track
                .as_ref()
                .and_then(|t| t.cars.get(PROJECT_VEHICLE_PROFILE_ID))
                .cloned()
        } else if open_drive {
            let track = gameplay_track.as_ref().ok_or("profile_unavailable")?;
            let car = track
                .cars
                .get(&player.vehicle_profile_id)
                .ok_or("car_unavailable_on_track")?;
            if car.source_fingerprint != player.vehicle_fingerprint {
                return Err("car_unavailable_on_track");
            }
            Some(car.clone())
        } else {
            if player.vehicle_profile_id != "vehicle-profile-project-default"
                || player.vehicle_fingerprint != "0".repeat(64)
            {
                return Err("car_unavailable_on_track");
            }
            None
        };
        let world = new_room_world(
            gameplay_track.as_ref(),
            laps,
            open_drive,
            car.as_ref(),
            models,
            slot,
            &player.car_model,
        )?;
        let mut replacement = new_player(
            world,
            player.driver_name.clone(),
            player.vehicle_profile_id.clone(),
            player.vehicle_fingerprint.clone(),
            player.paint_id.clone(),
            (player.car_model.clone(), player.car_variant),
            car.as_ref(),
        );
        apply_transmission(&mut replacement, player.transmission, player.auto_reverse);
        replacement.ready = false;
        replacements.push((*player_id, replacement));
    }

    for (player_id, replacement) in replacements {
        room.players.insert(player_id, replacement);
    }
    room.gameplay_track = gameplay_track;
    room.summary.track_profile_id = track_profile_id;
    room.summary.track_fingerprint = track_fingerprint;
    room.summary.content_identity = content_identity;
    room.summary.open_drive = open_drive;
    room.summary.laps = laps;
    if open_drive && laps == 0 {
        room.summary.ai_count = 0;
    }
    room.summary.status = RaceStatus::Lobby;
    room.summary.selection_revision = room.summary.selection_revision.wrapping_add(1).max(1);
    room.countdown = 0;
    for player in room.players.values_mut() {
        player.ready = false;
    }
    Ok(())
}

fn change_player_vehicle(
    room: &mut Room,
    player_id: u32,
    vehicle_profile_id: String,
    vehicle_fingerprint: String,
    paint_id: String,
    car_choice: (String, u8),
    models: &ModelSet,
) -> Result<(), &'static str> {
    let player = room.players.get(&player_id).ok_or("not_in_race")?;
    let project_room = is_project_room_track(room.gameplay_track.as_ref());
    let mut ordered_ids: Vec<u32> = room.players.keys().copied().collect();
    ordered_ids.sort_unstable();
    let slot = ordered_ids.binary_search(&player_id).unwrap_or(0);
    let car = if project_room {
        if !default_project_vehicle(&vehicle_profile_id, &vehicle_fingerprint) {
            return Err("profile_unavailable");
        }
        room.gameplay_track
            .as_ref()
            .and_then(|t| t.cars.get(PROJECT_VEHICLE_PROFILE_ID))
            .cloned()
    } else if room.summary.open_drive {
        let track = room.gameplay_track.as_ref().ok_or("profile_unavailable")?;
        let car = track
            .cars
            .get(&vehicle_profile_id)
            .ok_or("profile_unavailable")?;
        if car.source_fingerprint != vehicle_fingerprint {
            return Err("content_mismatch");
        }
        Some(car.clone())
    } else {
        if vehicle_profile_id != "vehicle-profile-project-default"
            || vehicle_fingerprint != "0".repeat(64)
        {
            return Err("profile_unavailable");
        }
        None
    };
    let world = new_room_world(
        room.gameplay_track.as_ref(),
        room.summary.laps,
        room.summary.open_drive,
        car.as_ref(),
        models,
        slot,
        &car_choice.0,
    )?;
    let mut replacement = new_player(
        world,
        player.driver_name.clone(),
        vehicle_profile_id,
        vehicle_fingerprint,
        paint_id,
        car_choice,
        car.as_ref(),
    );
    apply_transmission(&mut replacement, player.transmission, player.auto_reverse);
    replacement.ready = false;
    room.players.insert(player_id, replacement);
    room.summary.selection_revision = room.summary.selection_revision.wrapping_add(1).max(1);
    for player in room.players.values_mut() {
        player.ready = false;
    }
    Ok(())
}

fn client_has_current_roster_templates(
    room: &Room,
    track_fingerprint: &str,
    vehicle_templates: &[protocol::AvailableCarProfile],
) -> bool {
    if track_fingerprint != room.summary.track_fingerprint
        || vehicle_templates.is_empty()
        || vehicle_templates.len() > MAX_PLAYERS_PER_ROOM
    {
        return false;
    }
    let mut expected = HashMap::new();
    if room.summary.open_drive {
        let Some(track) = room.gameplay_track.as_ref() else {
            return false;
        };
        for player in room.players.values() {
            let Some(car) = track.cars.get(&player.vehicle_profile_id) else {
                return false;
            };
            if car.source_fingerprint != player.vehicle_fingerprint {
                return false;
            }
            expected.insert(car.profile_id.clone(), car.source_fingerprint.clone());
        }
        // Computer opponents drive other cars of the track; the client
        // prepares those models too, so it can draw them when the race starts.
        if room.races() {
            let (host, paint) = room
                .players
                .get(&room.owner_player_id)
                .map_or(("", ""), |p| (p.vehicle_profile_id.as_str(), p.paint_id.as_str()));
            for (profile_id, fingerprint, _) in opponents::owner_ai_roster(
                track,
                host,
                paint,
                opponents::owner_ai_count(room),
            ) {
                expected.insert(profile_id, fingerprint);
            }
        }
    } else {
        expected.insert("vehicle-profile-project-default".to_owned(), "0".repeat(64));
    }
    let mut actual = HashMap::new();
    for template in vehicle_templates {
        if template.profile_id.is_empty()
            || template.profile_id.len() > 64
            || !valid_fingerprint_or_project(&template.source_fingerprint)
            || actual
                .insert(
                    template.profile_id.as_str(),
                    template.source_fingerprint.as_str(),
                )
                .is_some()
        {
            return false;
        }
    }
    actual.len() == expected.len()
        && expected.iter().all(|(profile_id, fingerprint)| {
            actual.get(profile_id.as_str()) == Some(&fingerprint.as_str())
        })
}

#[cfg(test)]
fn new_world(
    track: TrackRuntime,
    car: Option<&GameplayCar>,
    models: &ModelSet,
) -> Result<RaceWorld, &'static str> {
    new_world_with(track, car, models, "")
}

/// Imported cars start with the embedding build's default model (when it
/// supplied one); player-selected provider models are applied afterwards by
/// `reapply_externals`. Without a registered default the world keeps the
/// engine's default model.
fn new_world_with(
    track: TrackRuntime,
    car: Option<&GameplayCar>,
    models: &ModelSet,
    car_model: &str,
) -> Result<RaceWorld, &'static str> {
    let mut world = RaceWorld::new(SimTiming::default(), track);
    world.enable_shared_preview_handling();
    if car.is_none() {
        // The project circuit (no imported data) uses the classic model.
        world.use_classic_model(car_classic_config(car_model))?;
    }
    if let Some(car) = car {
        if let Some(id) = &models.imported_default {
            let _ = world.install_custom_model(&models.registry, id, b"");
        }
        world.install_preview_collision(car.collision.clone())?;
    }
    Ok(world)
}

/// Installs the player's selected provider model into their lobby world.
/// Only lobby worlds change; a running race keeps its model.
fn apply_external_to_player(
    room: &mut Room,
    player_id: u32,
    registry: &VehicleModelRegistry,
    (model_id, params): &(String, Vec<u8>),
) {
    if !matches!(room.summary.status, RaceStatus::Lobby) {
        return;
    }
    if let Some(player) = room.players.get_mut(&player_id) {
        if !registry.applies_to(model_id, params, &player.vehicle_profile_id) {
            return;
        }
        let _ = player
            .world
            .install_custom_model(registry, model_id, params);
    }
}

/// Re-applies provider selections after worlds were rebuilt (join, track or
/// vehicle change).
fn reapply_externals(hub: &mut Hub, race_id: u32) {
    let registry = Arc::clone(&hub.vehicle_models);
    let selections = hub.external_cars.clone();
    if let Some(room) = hub.rooms.get_mut(&race_id) {
        for (player_id, entry) in &selections {
            apply_external_to_player(room, *player_id, &registry, entry);
        }
    }
}

/// Set a player's gearbox mode in the record and in their world.
fn apply_transmission(
    player: &mut Player,
    transmission: protocol::Transmission,
    auto_reverse: bool,
) {
    player.transmission = transmission;
    player.auto_reverse = auto_reverse;
    player.world.set_auto_reverse(auto_reverse);
    player.world.set_transmission(match transmission {
        protocol::Transmission::Automatic => racer_sim::Transmission::Automatic,
        protocol::Transmission::Manual => racer_sim::Transmission::Manual,
    });
}

fn new_player(
    world: RaceWorld,
    driver_name: String,
    vehicle_profile_id: String,
    vehicle_fingerprint: String,
    paint_id: String,
    (car_model, car_variant): (String, u8),
    car: Option<&GameplayCar>,
) -> Player {
    Player {
        world,
        driver_name,
        vehicle_profile_id,
        vehicle_fingerprint,
        paint_id,
        car_model,
        car_variant,
        transmission: protocol::Transmission::Automatic,
        auto_reverse: false,
        contact_radius: car.map_or(1.0, |car| car.collision.vehicle_radius),
        contact_min_z: car.map_or(-0.5, |car| car.collision.vehicle_min_z),
        contact_max_z: car.map_or(0.5, |car| car.collision.vehicle_max_z),
        ready: false,
        input: DriverInput::default(),
        shift_latch: (false, false),
        ack_sequence: None,
        applied_sequence: None,
        applied_server_tick: None,
    }
}

fn resolve_selection(
    state: &AppState,
    track_profile_id: &str,
    track_fingerprint: &str,
    vehicle_profile_id: &str,
    vehicle_fingerprint: &str,
    content_identity: &str,
    laps: u8,
) -> Result<(Option<Arc<GameplayTrack>>, Option<GameplayCar>, bool), &'static str> {
    if track_profile_id == "track-profile-project-circuit" {
        if content_identity != PROJECT_CONTENT_ID
            || track_fingerprint != &"0".repeat(64)
            || vehicle_profile_id != "vehicle-profile-project-default"
            || vehicle_fingerprint != &"0".repeat(64)
            || !(1..=5).contains(&laps)
        {
            return Err("content_mismatch");
        }
        return Ok((None, None, false));
    }
    if is_project_profile(track_profile_id) {
        let track =
            resolve_project_selection(track_profile_id, track_fingerprint, content_identity, laps)?;
        if !default_project_vehicle(vehicle_profile_id, vehicle_fingerprint) {
            return Err("content_mismatch");
        }
        let car = track.cars.get(PROJECT_VEHICLE_PROFILE_ID).cloned();
        return Ok((Some(track), car, false));
    }
    if content_identity != "owner-open-drive-v1"
        || laps > MAX_OWNER_LAPS
        || !valid_fingerprint(track_fingerprint)
        || !valid_fingerprint(vehicle_fingerprint)
    {
        return Err("invalid_settings");
    }
    let catalog = state.2.as_ref().ok_or("profile_unavailable")?;
    let track = catalog
        .track(track_profile_id)
        .ok_or("profile_unavailable")?;
    if track.source_fingerprint != track_fingerprint {
        return Err("content_mismatch");
    }
    let car = track
        .cars
        .get(vehicle_profile_id)
        .ok_or("profile_unavailable")?;
    if car.source_fingerprint != vehicle_fingerprint {
        return Err("content_mismatch");
    }
    Ok((Some(Arc::clone(track)), Some(car.clone()), true))
}

fn selection_matches_room(
    room: &Room,
    content_identity: &str,
    track_profile_id: &str,
    track_fingerprint: &str,
    vehicle_profile_id: &str,
    vehicle_fingerprint: &str,
) -> bool {
    if room.summary.open_drive {
        if content_identity != "owner-open-drive-v1"
            || track_profile_id != room.summary.track_profile_id
            || track_fingerprint != room.summary.track_fingerprint
            || !valid_fingerprint(vehicle_fingerprint)
        {
            return false;
        }
        return room.gameplay_track.as_ref().is_some_and(|track| {
            track
                .cars
                .get(vehicle_profile_id)
                .is_some_and(|car| car.source_fingerprint == vehicle_fingerprint)
        });
    }
    if is_project_room_track(room.gameplay_track.as_ref()) {
        return content_identity == PROJECT_TRACK_CONTENT_ID
            && track_profile_id == room.summary.track_profile_id
            && track_fingerprint == room.summary.track_fingerprint
            && default_project_vehicle(vehicle_profile_id, vehicle_fingerprint);
    }
    content_identity == PROJECT_CONTENT_ID
        && track_profile_id == "track-profile-project-circuit"
        && track_fingerprint == "0".repeat(64)
        && vehicle_profile_id == "vehicle-profile-project-default"
        && vehicle_fingerprint == "0".repeat(64)
}
fn normalize_input(
    throttle: f32,
    brake: f32,
    steer: f32,
    handbrake: bool,
    (shift_up, shift_down): (bool, bool),
    reset_request: bool,
) -> Option<DriverInput> {
    if !throttle.is_finite() || !brake.is_finite() || !steer.is_finite() {
        return None;
    }
    Some(DriverInput {
        throttle: f64::from(throttle.clamp(0.0, 1.0)),
        brake: f64::from(brake.clamp(0.0, 1.0)),
        steer: f64::from(steer.clamp(-1.0, 1.0)),
        handbrake,
        shift_up,
        shift_down,
        reset_request,
    })
}
fn sequence_is_newer(candidate: u32, previous: u32) -> bool {
    candidate != previous && candidate.wrapping_sub(previous) < (1 << 31)
}
#[allow(dead_code)]
fn tick_is_not_after(candidate: u64, reference: u64) -> bool {
    reference.wrapping_sub(candidate) < (1 << 63)
}
#[allow(dead_code)]
fn validate_latest_input(input: DriverInput) -> Result<(), &'static str> {
    if !input.throttle.is_finite()
        || !(0.0..=1.0).contains(&input.throttle)
        || !input.brake.is_finite()
        || !(0.0..=1.0).contains(&input.brake)
        || !input.steer.is_finite()
        || !(-1.0..=1.0).contains(&input.steer)
    {
        return Err("hub frame latest input is not normalized");
    }
    if f64::from(input.throttle as f32) != input.throttle
        || f64::from(input.brake as f32) != input.brake
        || f64::from(input.steer as f32) != input.steer
    {
        return Err("hub frame latest input is not wire-representable");
    }
    Ok(())
}
#[allow(dead_code)]
fn validate_contact_profile(radius: f64, min_z: f64, max_z: f64) -> Result<(), &'static str> {
    if !radius.is_finite()
        || !(0.01..=100.0).contains(&radius)
        || !min_z.is_finite()
        || !max_z.is_finite()
        || min_z >= max_z
        || min_z.abs() > 1.0e6
        || max_z.abs() > 1.0e6
    {
        return Err("hub vehicle contact profile is invalid");
    }
    Ok(())
}
#[allow(dead_code)]
fn hash_string(hash: &mut Sha256, value: &str) {
    hash_u64(hash, value.len() as u64);
    hash.update(value.as_bytes());
}
#[allow(dead_code)]
fn hash_u32(hash: &mut Sha256, value: u32) {
    hash.update(value.to_le_bytes());
}
#[allow(dead_code)]
fn hash_u64(hash: &mut Sha256, value: u64) {
    hash.update(value.to_le_bytes());
}
#[allow(dead_code)]
fn hash_f64(hash: &mut Sha256, value: f64) {
    hash.update(
        if value == 0.0 { 0.0f64 } else { value }
            .to_bits()
            .to_le_bytes(),
    );
}
#[allow(dead_code)]
fn hash_option_u32(hash: &mut Sha256, value: Option<u32>) {
    match value {
        None => hash.update([0]),
        Some(value) => {
            hash.update([1]);
            hash_u32(hash, value);
        }
    }
}
#[allow(dead_code)]
fn hash_option_u64(hash: &mut Sha256, value: Option<u64>) {
    match value {
        None => hash.update([0]),
        Some(value) => {
            hash.update([1]);
            hash_u64(hash, value);
        }
    }
}
#[allow(dead_code)]
fn hash_race_status(hash: &mut Sha256, status: &RaceStatus) {
    match status {
        RaceStatus::Lobby => hash.update([0]),
        RaceStatus::Countdown { ticks_left } => {
            hash.update([1]);
            hash.update(ticks_left.to_le_bytes());
        }
        RaceStatus::Racing => hash.update([2]),
        RaceStatus::Finished => hash.update([3]),
    }
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= MAX_NAME_BYTES && !name.chars().any(char::is_control)
}
fn valid_driver_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 24 && !name.chars().any(char::is_control)
}
/// A car choice is either none (empty model, variant 0) or a model of the
/// project catalogue with a paint scheme that exists for it.
fn valid_car_choice(model: &str, variant: u8) -> bool {
    if model.is_empty() {
        return variant == 0;
    }
    if model.len() > racer_protocol::MAX_CAR_MODEL_BYTES {
        return false;
    }
    let cars = racer_content::project_cars::project_cars();
    cars.index_of(model)
        .is_some_and(|car| usize::from(variant) < cars.cars()[car].paints.len())
}

fn valid_paint(paint_id: &str) -> bool {
    racer_protocol::paint_colour_index(paint_id).is_some()
}
fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn valid_fingerprint_or_project(value: &str) -> bool {
    valid_fingerprint(value) || value == "0".repeat(64)
}
fn room_for_player(hub: &Hub, player: u32) -> Option<u32> {
    hub.rooms
        .iter()
        .find_map(|(id, room)| room.players.contains_key(&player).then_some(*id))
}
fn reject_join(hub: &Hub, player: u32, reason: &str) {
    send(
        hub,
        player,
        ServerMessage::JoinRejected {
            reason: reason.into(),
        },
    );
}
fn send_error(hub: &Hub, player: u32, code: &str) {
    send(
        hub,
        player,
        ServerMessage::Error {
            code: code.into(),
            detail: code.replace('_', " "),
        },
    );
}
fn send(hub: &Hub, player: u32, message: ServerMessage) {
    if let Some(client) = hub.clients.get(&player) {
        if matches!(&message, ServerMessage::StateSnapshot { .. }) {
            client.snapshots.send_replace(Some(message));
        } else if let Err(mpsc::error::TrySendError::Full(_)) = client.reliable.try_send(message) {
            // Control and lobby events are reliable; terminate slow clients on overflow.
            let _ = client.close.send(true);
        }
    }
}
fn send_lobby(hub: &Hub, player: Option<u32>) {
    let races = hub
        .rooms
        .values()
        .map(|room| {
            let mut summary = room.summary.clone();
            summary.status = room.status();
            summary
        })
        .collect();
    let msg = ServerMessage::LobbySnapshot {
        races,
        available_tracks: hub.available_tracks.clone(),
    };
    if let Some(player) = player {
        send(hub, player, msg);
    } else {
        for id in hub.clients.keys().copied() {
            send(hub, id, msg.clone());
        }
    }
}
fn publish(hub: &Hub, room_id: Option<u32>) {
    send_lobby(hub, None);
    if let Some(id) = room_id {
        send_room_snapshot(hub, id);
    }
}
fn send_room_snapshot(hub: &Hub, id: u32) {
    let Some(room) = hub.rooms.get(&id) else {
        return;
    };
    // Free driving on an imported track scores nothing.
    let open_drive = !room.races();
    let ai_worlds = opponents::worlds(room);
    let mut ranked_ids: Vec<u32> = room
        .players
        .keys()
        .copied()
        .chain(ai_worlds.iter().map(|(id, _)| *id))
        .collect();
    ranked_ids.sort_unstable();
    let entries: Vec<_> = ranked_ids
        .iter()
        .map(|id| match room.players.get(id) {
            Some(p) => p.world.rank_entry(*id),
            None => ai_worlds.iter().find(|(ai_id, _)| ai_id == id).map_or_else(
                || RaceWorld::default().rank_entry(*id),
                |(_, w)| w.rank_entry(*id),
            ),
        })
        .collect();
    let gate_count = room
        .players
        .values()
        .next()
        .map_or(0, |p| p.world.track.checkpoints.len());
    let positions = rank_race(gate_count, &entries);
    let ticks32 = |ticks: u64| u32::try_from(ticks).unwrap_or(u32::MAX);
    let vehicles: Vec<_> = room
        .players
        .iter()
        .map(|(player_id, p)| VehicleSnapshot {
            player_id: *player_id,
            driver_name: p.driver_name.clone(),
            vehicle_profile_id: p.vehicle_profile_id.clone(),
            vehicle_fingerprint: p.vehicle_fingerprint.clone(),
            paint_id: p.paint_id.clone(),
            car_model: p.car_model.clone(),
            car_variant: p.car_variant,
            x: p.world.vehicle.position.x,
            y: p.world.vehicle.position.y,
            z: p.world.vehicle.position_z,
            velocity_x: p.world.vehicle.velocity.x,
            velocity_y: p.world.vehicle.velocity.y,
            heading: p.world.vehicle.heading,
            speed: p.world.vehicle.speed(),
            ground_normal: p.world.preview_ground_normal().unwrap_or([0.0, 0.0, 1.0]),
            checkpoint: if open_drive {
                0
            } else {
                p.world.vehicle.checkpoint as u16
            },
            completed_laps: if open_drive {
                0
            } else {
                p.world.vehicle.completed_laps
            },
            finished: !open_drive && p.world.vehicle.finished,
            race_position: if open_drive {
                0
            } else {
                ranked_ids
                    .binary_search(player_id)
                    .ok()
                    .map_or(0, |i| positions[i].min(u16::from(u8::MAX)) as u8)
            },
            current_lap_ticks: if open_drive {
                0
            } else {
                ticks32(p.world.current_lap_ticks())
            },
            last_lap_ticks: p
                .world
                .lap_times()
                .last_lap_ticks
                .filter(|_| !open_drive)
                .map(ticks32),
            best_lap_ticks: p
                .world
                .lap_times()
                .best_lap_ticks
                .filter(|_| !open_drive)
                .map(ticks32),
            finish_ticks: p
                .world
                .lap_times()
                .finish_ticks
                .filter(|_| !open_drive)
                .map(ticks32),
            gear: p.world.drive_telemetry().gear,
            rpm_q8: (p.world.drive_telemetry().rpm01.clamp(0.0, 1.0) * 255.0).round() as u8,
            handling_model: handling_model_id(&p.world, &hub.vehicle_models),
            is_ai: false,
        })
        .collect();
    let mut vehicles = vehicles;
    let position_of = |id: u32| -> u8 {
        if open_drive {
            0
        } else {
            ranked_ids
                .binary_search(&id)
                .ok()
                .map_or(0, |i| positions[i].min(u16::from(u8::MAX)) as u8)
        }
    };
    vehicles.extend(opponents::snapshots(room, &position_of, &ticks32));
    vehicles.sort_by_key(|vehicle| vehicle.player_id);
    for (id, p) in &room.players {
        send(
            hub,
            *id,
            ServerMessage::StateSnapshot {
                race_id: room.summary.race_id,
                server_tick: hub.server_tick,
                ack_input_sequence: p.ack_sequence,
                ack_applied_input_sequence: p.applied_sequence,
                ack_applied_server_tick: p.applied_server_tick,
                status: room.status(),
                selection_revision: room.summary.selection_revision,
                vehicles: vehicles.clone(),
            },
        );
    }
}
fn leave_room(hub: &mut Hub, player: u32) {
    let Some(id) = room_for_player(hub, player) else {
        return;
    };
    if let Some(room) = hub.rooms.get_mut(&id) {
        room.players.remove(&player);
        if room.players.is_empty() {
            hub.rooms.remove(&id);
        } else {
            room.summary.player_count = room.players.len() as u8;
            room.summary.selection_revision =
                room.summary.selection_revision.wrapping_add(1).max(1);
            for player in room.players.values_mut() {
                player.ready = false;
            }
            if room.owner_player_id == player {
                if let Some(next_host) = room.players.keys().copied().min() {
                    room.owner_player_id = next_host;
                    room.summary.host_player_id = next_host;
                }
            }
            if !matches!(room.summary.status, RaceStatus::Lobby) {
                room.summary.status = RaceStatus::Finished;
            }
        }
    }
    send(hub, player, ServerMessage::LeftRace);
}
async fn disconnect(state: &AppState, player: u32) {
    let mut hub = state.0.lock().await;
    hub.clients.remove(&player);
    hub.external_cars.remove(&player);
    hub.transmissions.remove(&player);
    leave_room(&mut hub, player);
    publish(&hub, None);
}

fn advance_authoritative_tick(hub: &mut Hub) {
    advance_authoritative_tick_internal(hub, true);
}

fn advance_authoritative_tick_internal(hub: &mut Hub, publish_updates: bool) {
    let (identity, before_boundary, racing_rooms) = if hub.tick_schedule_recording_enabled {
        let before_boundary = hub.replay_boundary_identity().ok();
        let identity = hub.simulation_identity().ok();
        let mut racing_rooms = Vec::new();
        let mut rooms = hub.rooms.iter().collect::<Vec<_>>();
        rooms.sort_by_key(|(room_id, _)| **room_id);
        let mut valid = true;
        for (room_id, room) in rooms {
            if !matches!(room.summary.status, RaceStatus::Racing) {
                continue;
            }
            let mut players = room.players.iter().collect::<Vec<_>>();
            players.sort_by_key(|(player_id, _)| **player_id);
            let mut participants = Vec::with_capacity(players.len());
            for (player_id, player) in players {
                let Ok(world_identity) = player.world.simulation_identity() else {
                    valid = false;
                    break;
                };
                participants.push(AppliedIntentSample {
                    player_id: *player_id,
                    world_identity,
                    input: player.input,
                    received_sequence: player.ack_sequence,
                    applied_sequence: (!player.world.vehicle.finished)
                        .then_some(player.ack_sequence)
                        .flatten(),
                });
            }
            if !valid {
                break;
            }
            racing_rooms.push(RacingRoomTickSchedule {
                room_id: *room_id,
                selection_revision: room.summary.selection_revision,
                participants,
            });
        }
        (identity, before_boundary, valid.then_some(racing_rooms))
    } else {
        (None, None, None)
    };
    hub.server_tick = hub.server_tick.wrapping_add(1);
    let server_tick = hub.server_tick;
    let mut changed = Vec::new();
    let mut events = Vec::new();
    for (room_id, room) in &mut hub.rooms {
        match room.summary.status {
            RaceStatus::Countdown { .. } => {
                room.countdown = room.countdown.saturating_sub(1);
                if room.countdown == 0 {
                    room.summary.status = RaceStatus::Racing;
                    changed.push(*room_id);
                } else {
                    room.summary.status = RaceStatus::Countdown {
                        ticks_left: room.countdown,
                    };
                }
            }
            RaceStatus::Racing => {
                let mut player_ids = room.players.keys().copied().collect::<Vec<_>>();
                player_ids.sort_unstable();
                let races = room.races();
                let ai_before = opponents::step(room);
                let mut previous_positions = HashMap::with_capacity(player_ids.len());
                let mut post_step_positions = HashMap::with_capacity(player_ids.len());
                let mut step_sequences = HashMap::with_capacity(player_ids.len());
                for player_id in &player_ids {
                    let p = room
                        .players
                        .get_mut(player_id)
                        .expect("sorted player ID exists");
                    let old_checkpoint = p.world.vehicle.checkpoint;
                    let old_laps = p.world.vehicle.completed_laps;
                    let was_finished = p.world.vehicle.finished;
                    let before_step_position = p.world.vehicle.position;
                    let mut input = p.input;
                    input.shift_up |= p.shift_latch.0;
                    input.shift_down |= p.shift_latch.1;
                    p.shift_latch = (false, false);
                    if !was_finished {
                        step_sequences.insert(*player_id, p.ack_sequence);
                    }
                    p.input.reset_request = false;
                    p.world.step(input);
                    // Reset is a discontinuous teleport. Do not sweep its old
                    // location through intervening cars; still resolve overlap
                    // at the reset destination on this tick.
                    previous_positions.insert(
                        *player_id,
                        contact_sweep_origin(
                            before_step_position,
                            p.world.vehicle.position,
                            input.reset_request,
                        ),
                    );
                    post_step_positions.insert(*player_id, p.world.vehicle.position);
                    if races && p.world.vehicle.checkpoint != old_checkpoint {
                        events.push((
                            *room_id,
                            *player_id,
                            "checkpoint",
                            p.world.vehicle.checkpoint as u32,
                        ));
                    }
                    if races && p.world.vehicle.completed_laps != old_laps {
                        events.push((*room_id, *player_id, "lap", p.world.vehicle.completed_laps));
                    }
                    if races && !was_finished && p.world.vehicle.finished {
                        events.push((
                            *room_id,
                            *player_id,
                            "finish",
                            p.world.vehicle.completed_laps,
                        ));
                    }
                }
                let mut bodies = player_ids
                    .iter()
                    .filter_map(|player_id| {
                        let player = room.players.get(player_id)?;
                        Some(VehicleContactBody {
                            stable_id: *player_id,
                            vehicle: player.world.vehicle,
                            previous_position: *previous_positions.get(player_id)?,
                            radius: player.contact_radius,
                            min_z: player.contact_min_z,
                            max_z: player.contact_max_z,
                        })
                    })
                    .collect::<Vec<_>>();
                let (ai_radius, ai_min_z, ai_max_z) = opponents::contact_profile();
                let mut ai_post_step = HashMap::new();
                if let Some(ai) = &room.ai {
                    for car in &ai.opponents.cars {
                        let before = ai_before
                            .iter()
                            .find(|(id, _)| *id == car.id)
                            .map_or(car.world.vehicle.position, |(_, p)| *p);
                        ai_post_step.insert(car.id, car.world.vehicle.position);
                        bodies.push(VehicleContactBody {
                            stable_id: car.id,
                            vehicle: car.world.vehicle,
                            previous_position: before,
                            radius: ai_radius,
                            min_z: ai_min_z,
                            max_z: ai_max_z,
                        });
                    }
                }
                if resolve_pairwise_vehicle_contacts(&mut bodies).is_ok() {
                    for body in bodies {
                        if opponents::is_ai_id(body.stable_id) {
                            if let Some(car) = room.ai.as_mut().and_then(|ai| {
                                ai.opponents
                                    .cars
                                    .iter_mut()
                                    .find(|c| c.id == body.stable_id)
                            }) {
                                let previous = ai_post_step
                                    .get(&body.stable_id)
                                    .copied()
                                    .unwrap_or(body.vehicle.position);
                                let displaced = previous != body.vehicle.position;
                                car.world.vehicle = body.vehicle;
                                if displaced {
                                    car.world.constrain_after_vehicle_contact(previous);
                                }
                            }
                        } else if let Some(player) = room.players.get_mut(&body.stable_id) {
                            let previous = *post_step_positions
                                .get(&body.stable_id)
                                .unwrap_or(&body.vehicle.position);
                            let displaced = previous != body.vehicle.position;
                            player.world.vehicle = body.vehicle;
                            if displaced {
                                player.world.constrain_after_vehicle_contact(previous);
                            }
                        }
                    }
                }
                // Publish application only after the input's world step and
                // the room's post-step contact constraints have completed.
                for player_id in &player_ids {
                    let player = room.players.get_mut(player_id).expect("player exists");
                    if let Some(sequence) = step_sequences.get(player_id).copied().flatten() {
                        player.applied_sequence = Some(sequence);
                        player.applied_server_tick = Some(server_tick);
                    }
                }
                if room.players.values().all(|p| p.world.vehicle.finished) {
                    room.summary.status = RaceStatus::Finished;
                    changed.push(*room_id);
                }
            }
            _ => {}
        }
    }
    if hub.tick_schedule_recording_enabled {
        if let (Some(identity), Some(before_boundary), Some(racing_rooms), Some(after_boundary)) = (
            identity,
            before_boundary,
            racing_rooms,
            hub.replay_boundary_identity().ok(),
        ) {
            hub.tick_schedules.push_back(HubTickSchedule {
                tick: server_tick,
                identity,
                before_boundary,
                after_boundary,
                racing_rooms,
            });
            while hub.tick_schedules.len() > HUB_REPLAY_HISTORY_TICKS {
                hub.tick_schedules.pop_front();
            }
        }
    }
    if publish_updates {
        for (room_id, player_id, event, value) in events {
            broadcast_event(&hub, room_id, player_id, event, value);
        }
        for id in hub.rooms.keys().copied().collect::<Vec<_>>() {
            send_room_snapshot(&hub, id);
        }
        if !changed.is_empty() {
            send_lobby(&hub, None);
        }
    }
}

async fn tick_loop(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs_f64(1.0 / 60.0));
    loop {
        interval.tick().await;
        let mut hub = state.0.lock().await;
        advance_authoritative_tick(&mut hub);
    }
}

fn contact_sweep_origin(before: Vec2, after: Vec2, reset_requested: bool) -> Vec2 {
    if reset_requested { after } else { before }
}

fn broadcast_event(hub: &Hub, room_id: u32, player_id: u32, event: &str, value: u32) {
    let Some(room) = hub.rooms.get(&room_id) else {
        return;
    };
    let message = ServerMessage::ReliableEvent {
        race_id: room_id,
        event: event.into(),
        player_id: Some(player_id),
        value,
    };
    for recipient in room.players.keys().copied() {
        send(hub, recipient, message.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_validation_accepts_colour_ids_and_legacy_aliases_only() {
        assert!(valid_paint("paint-colour-0"));
        assert!(valid_paint("paint-colour-87"));
        assert!(valid_paint("paint-project-red"));
        assert!(!valid_paint("paint-colour-88"));
        assert!(!valid_paint("paint-colour-07"));
        assert!(!valid_paint("paint-project-pink"));
        assert!(!valid_paint(""));
    }
    use racer_sim::{PreviewCollisionWorld, PreviewGroundTriangle, PreviewWallSegment};

    #[test]
    fn every_player_world_uses_the_classic_parameters_of_the_chosen_car() {
        let track = project_tracks::project_gameplay_track("track-profile-rcp-red-mesa").unwrap();
        let car = track.cars.get(PROJECT_VEHICLE_PROFILE_ID).cloned().unwrap();
        let mut tops = Vec::new();
        for model in ["quill-coupe", "brisk-roadster", "tumble-rally", "shard-gt"] {
            let expected = project_classic_config(model).unwrap();
            let world =
                new_room_world(Some(&track), 1, false, Some(&car), &stub_models(), 0, model)
                    .unwrap();
            assert_eq!(world.vehicle_model(), VehicleModel::Classic);
            assert_eq!(
                world.classic_parameters(),
                Some(&expected),
                "{model} on a track"
            );
            let circuit = new_room_world(None, 1, false, None, &stub_models(), 0, model).unwrap();
            assert_eq!(
                circuit.classic_parameters(),
                Some(&expected),
                "{model} on the circuit"
            );
            tops.push(expected.top_speed_mps.to_bits());
        }
        tops.sort_unstable();
        tops.dedup();
        assert_eq!(tops.len(), 4);
        // No choice (or an unknown id) drives the default demo car.
        let default = default_project_classic_config();
        for model in ["", "no-such-car"] {
            let world =
                new_room_world(Some(&track), 1, false, Some(&car), &stub_models(), 0, model)
                    .unwrap();
            assert_eq!(world.classic_parameters(), Some(&default));
        }
    }

    #[test]
    fn two_players_with_different_cars_get_different_worlds_in_one_room() {
        let track = project_tracks::project_gameplay_track("track-profile-rcp-red-mesa").unwrap();
        let car = track.cars.get(PROJECT_VEHICLE_PROFILE_ID).cloned().unwrap();
        let world_for = |model: &str, slot| {
            new_room_world(
                Some(&track),
                1,
                false,
                Some(&car),
                &stub_models(),
                slot,
                model,
            )
            .unwrap()
        };
        let (mut a, mut b) = (world_for("tumble-rally", 0), world_for("shard-gt", 1));
        for _ in 0..(8 * 60) {
            let go = DriverInput {
                throttle: 1.0,
                ..DriverInput::default()
            };
            a.step(go);
            b.step(go);
        }
        // The same inputs give the quicker-accelerating car more speed.
        assert!(a.vehicle.speed() > b.vehicle.speed() + 3.0);
    }

    fn synthetic_open_drive_state() -> (AppState, Vec<mpsc::Receiver<ServerMessage>>) {
        let catalog = GameplayDataStore::synthetic_for_tests();
        let track = Arc::clone(catalog.track("track-profile-0001").unwrap());
        let mut players = HashMap::new();
        for (player_id, car_id, name) in [
            (1, "vehicle-profile-0001", "Host"),
            (2, "vehicle-profile-0002", "Guest"),
        ] {
            let car = track.cars.get(car_id).unwrap();
            let world = new_world(TrackRuntime::open_preview(), Some(car), &stub_models()).unwrap();
            players.insert(
                player_id,
                new_player(
                    world,
                    name.into(),
                    car.profile_id.clone(),
                    car.source_fingerprint.clone(),
                    "paint-project-red".into(),
                    (String::new(), 0),
                    Some(car),
                ),
            );
        }
        let mut hub = Hub::default();
        hub.vehicle_models = Arc::new(stub_registry());
        hub.imported_default_model = Some(STUB_DEFAULT.into());
        hub.rooms.insert(
            1,
            Room {
                summary: RaceSummary {
                    race_id: 1,
                    host_player_id: 1,
                    name: "Synthetic imported room".into(),
                    player_count: 2,
                    max_players: MAX_PLAYERS_PER_ROOM as u8,
                    laps: 0,
                    status: RaceStatus::Lobby,
                    content_identity: "owner-open-drive-v1".into(),
                    track_profile_id: track.profile_id.clone(),
                    track_fingerprint: track.source_fingerprint.clone(),
                    open_drive: true,
                    selection_revision: 1,
                    ai_count: 0,
                    ai_skill: 1,
                    ai_catch_up: true,
                },
                owner_player_id: 1,
                gameplay_track: Some(track),
                players,
                countdown: 0,
                ai: None,
            },
        );
        let mut receivers = Vec::new();
        for player_id in [1, 2] {
            let (reliable, receiver) = mpsc::channel(64);
            let (snapshots, _) = watch::channel(None);
            let (close, _) = watch::channel(false);
            hub.clients.insert(
                player_id,
                ClientChannel {
                    reliable,
                    snapshots,
                    close,
                },
            );
            receivers.push(receiver);
        }
        (
            AppState(Arc::new(Mutex::new(hub)), None, Some(Arc::new(catalog))),
            receivers,
        )
    }

    fn add_synthetic_room_two(hub: &mut Hub) {
        let first = hub.rooms.get(&1).unwrap();
        let track = Arc::clone(first.gameplay_track.as_ref().unwrap());
        let mut players = HashMap::new();
        for (player_id, car_id, name) in [
            (3, "vehicle-profile-0001", "Third"),
            (4, "vehicle-profile-0002", "Fourth"),
        ] {
            let car = track.cars.get(car_id).unwrap();
            let world = new_world(TrackRuntime::open_preview(), Some(car), &stub_models()).unwrap();
            players.insert(
                player_id,
                new_player(
                    world,
                    name.into(),
                    car.profile_id.clone(),
                    car.source_fingerprint.clone(),
                    "paint-project-blue".into(),
                    (String::new(), 0),
                    Some(car),
                ),
            );
        }
        hub.rooms.insert(
            2,
            Room {
                summary: RaceSummary {
                    race_id: 2,
                    host_player_id: 3,
                    name: "Second synthetic room".into(),
                    player_count: 2,
                    max_players: MAX_PLAYERS_PER_ROOM as u8,
                    laps: 0,
                    status: RaceStatus::Lobby,
                    content_identity: "owner-open-drive-v1".into(),
                    track_profile_id: track.profile_id.clone(),
                    track_fingerprint: track.source_fingerprint.clone(),
                    open_drive: true,
                    selection_revision: 1,
                    ai_count: 0,
                    ai_skill: 1,
                    ai_catch_up: true,
                },
                owner_player_id: 3,
                gameplay_track: Some(track),
                players,
                countdown: 0,
                ai: None,
            },
        );
        hub.next_player = 5;
        hub.next_room = 3;
    }

    fn error_code(receiver: &mut mpsc::Receiver<ServerMessage>) -> String {
        match receiver.try_recv().unwrap() {
            ServerMessage::Error { code, .. } => code,
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[test]
    fn origin_check_rejects_cross_site_and_accepts_same_origin() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "localhost:3000".parse().unwrap());
        headers.insert(ORIGIN, "http://localhost:3000".parse().unwrap());
        assert!(same_origin(&headers));
        headers.insert(ORIGIN, "http://evil.example".parse().unwrap());
        assert!(!same_origin(&headers));
        headers.remove(ORIGIN);
        assert!(!same_origin(&headers));
    }

    #[test]
    fn rate_limits_mutations_and_input_bursts() {
        let mut limiter = RateLimiter::default();
        for _ in 0..INPUTS_PER_SECOND {
            assert!(limiter.allow(&ClientMessage::DriverInput {
                sequence: 0,
                client_tick: 0,
                throttle: 0.0,
                brake: 0.0,
                steer: 0.0,
                handbrake: false,
                shift_up: false,
                shift_down: false,
                reset_request: false
            }));
        }
        assert!(!limiter.allow(&ClientMessage::DriverInput {
            sequence: 0,
            client_tick: 0,
            throttle: 0.0,
            brake: 0.0,
            steer: 0.0,
            handbrake: false,
            shift_up: false,
            shift_down: false,
            reset_request: false
        }));
    }

    #[tokio::test]
    async fn protocol_two_hello_is_rejected_with_protocol_mismatch() {
        let (state, mut receivers) = synthetic_open_drive_state();
        assert_ne!(PROTOCOL_MAJOR, 2);
        process_message(
            &state,
            1,
            ClientMessage::Hello {
                protocol_major: 2,
                build_id: "stale".into(),
                content_capabilities: Vec::new(),
            },
        )
        .await;
        assert_eq!(error_code(&mut receivers[0]), "protocol_mismatch");
    }

    #[tokio::test]
    async fn open_drive_lobby_changes_are_host_scoped_and_readiness_is_roster_exact() {
        let (state, mut receivers) = synthetic_open_drive_state();
        let guest_track = state
            .2
            .as_ref()
            .unwrap()
            .track("track-profile-0003")
            .unwrap();
        process_message(
            &state,
            2,
            ClientMessage::ChangeTrack {
                race_id: 1,
                selection_revision: 1,
                content_identity: "owner-open-drive-v1".into(),
                track_profile_id: guest_track.profile_id.clone(),
                track_fingerprint: guest_track.source_fingerprint.clone(),
                laps: 0,
            },
        )
        .await;
        assert_eq!(error_code(&mut receivers[1]), "host_only");

        process_message(
            &state,
            1,
            ClientMessage::ChangeTrack {
                race_id: 1,
                selection_revision: 1,
                content_identity: "owner-open-drive-v1".into(),
                track_profile_id: guest_track.profile_id.clone(),
                track_fingerprint: guest_track.source_fingerprint.clone(),
                laps: 0,
            },
        )
        .await;
        {
            let hub = state.0.lock().await;
            let room = hub.rooms.get(&1).unwrap();
            assert_eq!(room.summary.selection_revision, 2);
            assert_eq!(room.summary.track_profile_id, "track-profile-0003");
            assert!(room.players.values().all(|player| !player.ready));
        }
        for receiver in &mut receivers {
            while receiver.try_recv().is_ok() {}
        }

        process_message(
            &state,
            2,
            ClientMessage::ReadyState {
                ready: true,
                selection_revision: 1,
                track_fingerprint: guest_track.source_fingerprint.clone(),
                vehicle_templates: Vec::new(),
            },
        )
        .await;
        assert_eq!(error_code(&mut receivers[1]), "stale_revision");

        let host_car = guest_track.cars.get("vehicle-profile-0001").unwrap();
        let guest_car = guest_track.cars.get("vehicle-profile-0002").unwrap();
        let host_template = protocol::AvailableCarProfile {
            profile_id: host_car.profile_id.clone(),
            source_fingerprint: host_car.source_fingerprint.clone(),
        };
        let guest_template = protocol::AvailableCarProfile {
            profile_id: guest_car.profile_id.clone(),
            source_fingerprint: guest_car.source_fingerprint.clone(),
        };
        let ready = |templates| ClientMessage::ReadyState {
            ready: true,
            selection_revision: 2,
            track_fingerprint: guest_track.source_fingerprint.clone(),
            vehicle_templates: templates,
        };
        process_message(&state, 2, ready(vec![host_template.clone()])).await;
        assert_eq!(error_code(&mut receivers[1]), "render_profile_missing");
        process_message(
            &state,
            2,
            ready(vec![
                host_template.clone(),
                guest_template.clone(),
                host_template.clone(),
            ]),
        )
        .await;
        assert_eq!(error_code(&mut receivers[1]), "render_profile_missing");
        let wrong = protocol::AvailableCarProfile {
            profile_id: guest_template.profile_id.clone(),
            source_fingerprint: "f".repeat(64),
        };
        process_message(&state, 2, ready(vec![host_template.clone(), wrong])).await;
        assert_eq!(error_code(&mut receivers[1]), "render_profile_missing");

        process_message(
            &state,
            2,
            ready(vec![host_template.clone(), guest_template.clone()]),
        )
        .await;
        assert!(matches!(
            receivers[1].try_recv().unwrap(),
            ServerMessage::ReadyAccepted {
                race_id: 1,
                ready: true,
                selection_revision: 2
            }
        ));
        {
            let hub = state.0.lock().await;
            assert!(hub.rooms.get(&1).unwrap().players.get(&2).unwrap().ready);
        }

        process_message(
            &state,
            2,
            ClientMessage::ChangeVehicle {
                race_id: 1,
                selection_revision: 2,
                vehicle_profile_id: "vehicle-profile-0001".into(),
                vehicle_fingerprint: host_car.source_fingerprint.clone(),
                paint_id: "paint-project-blue".into(),
                car_model: String::new(),
                car_variant: 0,
            },
        )
        .await;
        {
            let hub = state.0.lock().await;
            let room = hub.rooms.get(&1).unwrap();
            assert_eq!(room.summary.selection_revision, 3);
            assert_eq!(room.players[&2].vehicle_profile_id, "vehicle-profile-0001");
            assert_eq!(room.players[&1].vehicle_profile_id, "vehicle-profile-0001");
            assert!(room.players.values().all(|player| !player.ready));
        }
        for receiver in &mut receivers {
            while receiver.try_recv().is_ok() {}
        }

        process_message(
            &state,
            2,
            ClientMessage::ChangeVehicle {
                race_id: 1,
                selection_revision: 3,
                vehicle_profile_id: "vehicle-profile-missing".into(),
                vehicle_fingerprint: "a".repeat(64),
                paint_id: "paint-project-blue".into(),
                car_model: String::new(),
                car_variant: 0,
            },
        )
        .await;
        assert_eq!(error_code(&mut receivers[1]), "profile_unavailable");

        {
            let mut hub = state.0.lock().await;
            hub.rooms.get_mut(&1).unwrap().summary.status = RaceStatus::Countdown { ticks_left: 4 };
        }
        process_message(
            &state,
            1,
            ClientMessage::ChangeTrack {
                race_id: 1,
                selection_revision: 3,
                content_identity: "owner-open-drive-v1".into(),
                track_profile_id: "track-profile-0001".into(),
                track_fingerprint: "3".repeat(64),
                laps: 0,
            },
        )
        .await;
        assert_eq!(error_code(&mut receivers[0]), "race_started");
        process_message(
            &state,
            1,
            ClientMessage::ChangeVehicle {
                race_id: 1,
                selection_revision: 3,
                vehicle_profile_id: "vehicle-profile-0002".into(),
                vehicle_fingerprint: "2".repeat(64),
                paint_id: "paint-project-red".into(),
                car_model: String::new(),
                car_variant: 0,
            },
        )
        .await;
        assert_eq!(error_code(&mut receivers[0]), "race_started");
        {
            let mut hub = state.0.lock().await;
            hub.rooms.get_mut(&1).unwrap().summary.status = RaceStatus::Racing;
        }
        process_message(
            &state,
            1,
            ClientMessage::ChangeTrack {
                race_id: 1,
                selection_revision: 3,
                content_identity: "owner-open-drive-v1".into(),
                track_profile_id: "track-profile-0001".into(),
                track_fingerprint: "3".repeat(64),
                laps: 0,
            },
        )
        .await;
        assert_eq!(error_code(&mut receivers[0]), "race_started");
    }

    #[tokio::test]
    async fn leaving_host_transfers_lobby_authority_and_invalidates_readiness() {
        let (state, _) = synthetic_open_drive_state();
        {
            let mut hub = state.0.lock().await;
            let room = hub.rooms.get_mut(&1).unwrap();
            room.players.get_mut(&1).unwrap().ready = true;
            room.players.get_mut(&2).unwrap().ready = true;
            leave_room(&mut hub, 1);
        }
        let hub = state.0.lock().await;
        let room = hub.rooms.get(&1).unwrap();
        assert_eq!(room.owner_player_id, 2);
        assert_eq!(room.summary.host_player_id, 2);
        assert_eq!(room.summary.player_count, 1);
        assert!(!room.players[&2].ready);
    }

    #[test]
    fn only_finite_intent_enters_simulation() {
        assert!(normalize_input(f32::NAN, 0.0, 0.0, false, (false, false), false).is_none());
        let shifted = normalize_input(0.0, 0.0, 0.0, false, (true, false), false).unwrap();
        assert!(shifted.shift_up && !shifted.shift_down);
        let input = normalize_input(5.0, -2.0, -5.0, false, (false, false), false).unwrap();
        assert_eq!(input.throttle, 1.0);
        assert_eq!(input.brake, 0.0);
        assert_eq!(input.steer, -1.0);
    }

    #[test]
    fn duplicate_and_stale_inputs_are_rejected_but_sequence_wrap_is_supported() {
        assert!(!sequence_is_newer(7, 7));
        assert!(!sequence_is_newer(6, 7));
        assert!(sequence_is_newer(8, 7));
        assert!(sequence_is_newer(0, u32::MAX));
        assert!(!sequence_is_newer(u32::MAX, 0));
    }

    #[tokio::test]
    async fn applied_input_ack_is_post_step_and_tracks_latest_accepted_sequence() {
        let (state, _) = synthetic_open_drive_state();
        let (snapshot_tx, mut snapshot_rx) = watch::channel(None);
        {
            let mut hub = state.0.lock().await;
            hub.rooms.get_mut(&1).unwrap().summary.status = RaceStatus::Racing;
            hub.clients.get_mut(&1).unwrap().snapshots = snapshot_tx;
        }

        for sequence in [u32::MAX, u32::MAX, u32::MAX - 1, 0, u32::MAX, 1, 2] {
            process_message(
                &state,
                1,
                ClientMessage::DriverInput {
                    sequence,
                    client_tick: 99,
                    throttle: 0.5,
                    brake: 0.0,
                    steer: 0.0,
                    handbrake: false,
                    shift_up: false,
                    shift_down: false,
                    reset_request: false,
                },
            )
            .await;
        }
        {
            let hub = state.0.lock().await;
            let player = &hub.rooms[&1].players[&1];
            assert_eq!(player.ack_sequence, Some(2));
            assert_eq!(player.applied_sequence, None);
            assert_eq!(player.applied_server_tick, None);
            assert_eq!(player.world.tick, 0);
        }

        let task = tokio::spawn(tick_loop(state.clone()));
        tokio::time::timeout(Duration::from_secs(1), snapshot_rx.changed())
            .await
            .expect("racing tick should publish a snapshot")
            .unwrap();
        let snapshot = snapshot_rx.borrow_and_update().clone().unwrap();
        let ServerMessage::StateSnapshot {
            server_tick,
            ack_input_sequence,
            ack_applied_input_sequence,
            ack_applied_server_tick,
            ..
        } = snapshot
        else {
            panic!("expected state snapshot")
        };
        assert_eq!(ack_input_sequence, Some(2));
        assert_eq!(ack_applied_input_sequence, Some(2));
        assert_eq!(ack_applied_server_tick, Some(server_tick));
        task.abort();
    }

    #[tokio::test]
    async fn snapshot_reports_race_position_only_for_non_open_drive_rooms() {
        for open_drive in [true, false] {
            let (state, _) = synthetic_open_drive_state();
            let (snapshot_tx, mut snapshot_rx) = watch::channel(None);
            {
                let mut hub = state.0.lock().await;
                let room = hub.rooms.get_mut(&1).unwrap();
                room.summary.status = RaceStatus::Racing;
                room.summary.open_drive = open_drive;
                hub.clients.get_mut(&1).unwrap().snapshots = snapshot_tx;
            }
            let task = tokio::spawn(tick_loop(state.clone()));
            tokio::time::timeout(Duration::from_secs(1), snapshot_rx.changed())
                .await
                .expect("racing tick should publish a snapshot")
                .unwrap();
            let ServerMessage::StateSnapshot { vehicles, .. } =
                snapshot_rx.borrow_and_update().clone().unwrap()
            else {
                panic!("expected state snapshot")
            };
            task.abort();
            let vehicle = &vehicles[0];
            assert_eq!(vehicle.race_position, if open_drive { 0 } else { 1 });
            assert_eq!(vehicle.last_lap_ticks, None);
            assert_eq!(vehicle.best_lap_ticks, None);
            assert_eq!(vehicle.finish_ticks, None);
        }
    }

    fn next_car_model_ack(receiver: &mut mpsc::Receiver<ServerMessage>) -> (bool, String) {
        loop {
            if let ServerMessage::CarModelAck {
                accepted, reason, ..
            } = receiver.try_recv().expect("a car model ack")
            {
                return (accepted, reason);
            }
        }
    }

    async fn model_of(state: &AppState, player_id: u32) -> String {
        let hub = state.0.lock().await;
        let world = &hub.rooms[&1].players[&player_id].world;
        world
            .custom_model_id()
            .map_or_else(|| format!("{:?}", world.vehicle_model()), str::to_string)
    }

    /// Test double for a private provider: a point-mass model under another id.
    struct StubModel(&'static str);
    impl racer_sim::model_ext::VehicleModelProvider for StubModel {
        fn id(&self) -> &str {
            self.0
        }
        fn wire_code(&self) -> u8 {
            if self.0 == STUB_EXTERNAL {
                STUB_EXTERNAL_CODE
            } else {
                STUB_DEFAULT_CODE
            }
        }
        /// Only the vehicle profile named by the parameters is affected.
        fn applies_to(&self, params: &[u8], vehicle_profile_id: &str) -> bool {
            self.0 != STUB_EXTERNAL || params == vehicle_profile_id.as_bytes()
        }
        fn create(
            &self,
            _params: &[u8],
        ) -> Result<Box<dyn racer_sim::model_ext::VehicleDynamics>, String> {
            racer_example_model::ExampleProvider.create(b"{}")
        }
    }

    const STUB_DEFAULT: &str = "stub.default";
    const STUB_EXTERNAL: &str = "stub.external";
    const STUB_DEFAULT_CODE: u8 = 6;
    const STUB_EXTERNAL_CODE: u8 = 7;

    /// Registry with the stub providers an imported-car flow selects.
    fn stub_registry() -> VehicleModelRegistry {
        let mut registry = VehicleModelRegistry::new();
        for id in [STUB_DEFAULT, STUB_EXTERNAL] {
            registry.register(Box::new(StubModel(id))).unwrap();
        }
        registry
    }

    fn stub_models() -> ModelSet {
        ModelSet {
            registry: Arc::new(stub_registry()),
            imported_default: Some(STUB_DEFAULT.into()),
        }
    }

    /// Selection message whose parameters name the vehicle profile it is for.
    fn select_external(profile: &str) -> ClientMessage {
        ClientMessage::SetCarModel {
            model_id: STUB_EXTERNAL.into(),
            params: profile.as_bytes().to_vec(),
        }
    }

    #[tokio::test]
    async fn external_model_applies_only_to_the_matching_vehicle_profile() {
        let (state, mut receivers) = synthetic_open_drive_state();

        // A malformed selection is rejected and nothing is stored.
        process_message(
            &state,
            1,
            ClientMessage::SetCarModel {
                model_id: "Bad Id".into(),
                params: Vec::new(),
            },
        )
        .await;
        assert!(!next_car_model_ack(&mut receivers[0]).0);
        assert!(state.0.lock().await.external_cars.is_empty());

        // Parameters for another vehicle profile are stored but not applied.
        process_message(&state, 1, select_external("vehicle-profile-0002")).await;
        assert!(next_car_model_ack(&mut receivers[0]).0);
        assert_eq!(model_of(&state, 1).await, STUB_DEFAULT);

        // Matching parameters activate the model for player 1 only.
        process_message(&state, 1, select_external("vehicle-profile-0001")).await;
        assert!(next_car_model_ack(&mut receivers[0]).0);
        assert_eq!(model_of(&state, 1).await, STUB_EXTERNAL);
        assert_eq!(model_of(&state, 2).await, STUB_DEFAULT);

        // Changing to another car drops to the default; changing back restores it.
        let revision = state.0.lock().await.rooms[&1].summary.selection_revision;
        let change = |profile: &str, fingerprint: String, revision| ClientMessage::ChangeVehicle {
            race_id: 1,
            selection_revision: revision,
            vehicle_profile_id: profile.into(),
            vehicle_fingerprint: fingerprint,
            paint_id: "paint-project-red".into(),
            car_model: String::new(),
            car_variant: 0,
        };
        process_message(
            &state,
            1,
            change("vehicle-profile-0002", "2".repeat(64), revision),
        )
        .await;
        assert_eq!(model_of(&state, 1).await, STUB_DEFAULT);
        let revision = state.0.lock().await.rooms[&1].summary.selection_revision;
        process_message(
            &state,
            1,
            change("vehicle-profile-0001", "1".repeat(64), revision),
        )
        .await;
        assert_eq!(model_of(&state, 1).await, STUB_EXTERNAL);

        // Racing: snapshots carry the per-vehicle model code.
        let (snapshot_tx, mut snapshot_rx) = watch::channel(None);
        {
            let mut hub = state.0.lock().await;
            hub.rooms.get_mut(&1).unwrap().summary.status = RaceStatus::Racing;
            hub.clients.get_mut(&2).unwrap().snapshots = snapshot_tx;
        }
        let task = tokio::spawn(tick_loop(state.clone()));
        tokio::time::timeout(Duration::from_secs(1), snapshot_rx.changed())
            .await
            .expect("racing tick should publish a snapshot")
            .unwrap();
        task.abort();
        let ServerMessage::StateSnapshot { vehicles, .. } =
            snapshot_rx.borrow_and_update().clone().unwrap()
        else {
            panic!("expected state snapshot")
        };
        let id_of = |player: u32| vehicles.iter().find(|v| v.player_id == player).unwrap();
        assert_eq!(id_of(1).handling_model, STUB_EXTERNAL_CODE);
        assert_eq!(id_of(2).handling_model, STUB_DEFAULT_CODE);
    }

    #[tokio::test]
    async fn external_selection_is_dropped_on_disconnect() {
        let (state, mut receivers) = synthetic_open_drive_state();
        process_message(&state, 2, select_external("vehicle-profile-0002")).await;
        assert!(next_car_model_ack(&mut receivers[1]).0);
        assert!(state.0.lock().await.external_cars.contains_key(&2));
        disconnect(&state, 2).await;
        assert!(state.0.lock().await.external_cars.is_empty());
    }

    #[tokio::test]
    async fn external_player_is_captured_and_replayed_by_the_hub_schedule() {
        let make = || async {
            let (state, receivers) = synthetic_open_drive_state();
            process_message(&state, 1, select_external("vehicle-profile-0001")).await;
            (state, receivers)
        };
        let (source, _source_receivers) = make().await;
        let (target, _target_receivers) = make().await;
        let checkpoint = {
            let mut hub = source.0.lock().await;
            hub.set_tick_schedule_recording(true);
            let room = hub.rooms.get_mut(&1).unwrap();
            room.summary.status = RaceStatus::Racing;
            for player in room.players.values_mut() {
                player.ready = true;
            }
            hub.capture_simulation_frame().unwrap()
        };
        target.0.lock().await.set_tick_schedule_recording(true);
        for sequence in 1..=3 {
            process_message(
                &source,
                1,
                ClientMessage::DriverInput {
                    sequence,
                    client_tick: 0,
                    throttle: 1.0,
                    brake: 0.0,
                    steer: 0.2,
                    handbrake: false,
                    shift_up: sequence == 2,
                    shift_down: false,
                    reset_request: false,
                },
            )
            .await;
            let mut hub = source.0.lock().await;
            for _ in 0..40 {
                advance_authoritative_tick(&mut hub);
            }
        }
        let schedules = source.0.lock().await.capture_tick_schedule(0, 120).unwrap();
        assert_eq!(schedules.len(), 120);
        let mut target_hub = target.0.lock().await;
        target_hub.restore_simulation_frame(&checkpoint).unwrap();
        target_hub.replay_tick_schedule(&schedules).unwrap();
        let source_hub = source.0.lock().await;
        let replayed = target_hub.capture_simulation_frame().unwrap();
        assert_eq!(replayed, source_hub.capture_simulation_frame().unwrap());
        let external_frame = replayed.rooms[0]
            .participants
            .iter()
            .find(|p| p.player_id == 1)
            .unwrap();
        assert!(
            external_frame.world.custom.is_some(),
            "the provider-model state is captured"
        );
        assert!(
            replayed.rooms[0]
                .participants
                .iter()
                .all(|p| p.world.custom.is_some())
        );
    }

    #[tokio::test]
    async fn finished_driver_noop_step_does_not_advance_applied_ack() {
        let (state, _) = synthetic_open_drive_state();
        let (snapshot_tx, mut snapshot_rx) = watch::channel(None);
        {
            let mut hub = state.0.lock().await;
            let room = hub.rooms.get_mut(&1).unwrap();
            room.summary.status = RaceStatus::Racing;
            let player = room.players.get_mut(&1).unwrap();
            player.world.vehicle.finished = true;
            player.applied_sequence = Some(10);
            player.applied_server_tick = Some(0);
            hub.clients.get_mut(&1).unwrap().snapshots = snapshot_tx;
        }
        process_message(
            &state,
            1,
            ClientMessage::DriverInput {
                sequence: 11,
                client_tick: 3,
                throttle: 1.0,
                brake: 0.0,
                steer: 0.0,
                handbrake: false,
                shift_up: false,
                shift_down: false,
                reset_request: false,
            },
        )
        .await;
        let task = tokio::spawn(tick_loop(state.clone()));
        tokio::time::timeout(Duration::from_secs(1), snapshot_rx.changed())
            .await
            .expect("racing tick should publish a snapshot")
            .unwrap();
        let ServerMessage::StateSnapshot {
            ack_input_sequence,
            ack_applied_input_sequence,
            ack_applied_server_tick,
            ..
        } = snapshot_rx.borrow_and_update().clone().unwrap()
        else {
            panic!("expected state snapshot")
        };
        assert_eq!(ack_input_sequence, Some(11));
        assert_eq!(ack_applied_input_sequence, Some(10));
        assert_eq!(ack_applied_server_tick, Some(0));
        task.abort();
    }

    #[tokio::test]
    async fn lobby_and_countdown_never_claim_input_application() {
        for status in [RaceStatus::Lobby, RaceStatus::Countdown { ticks_left: 7 }] {
            let (state, _) = synthetic_open_drive_state();
            {
                let mut hub = state.0.lock().await;
                let room = hub.rooms.get_mut(&1).unwrap();
                if let RaceStatus::Countdown { ticks_left } = status {
                    room.countdown = ticks_left;
                    room.summary.status = RaceStatus::Countdown { ticks_left };
                } else {
                    room.summary.status = RaceStatus::Lobby;
                }
            }
            process_message(
                &state,
                1,
                ClientMessage::DriverInput {
                    sequence: 7,
                    client_tick: 1,
                    throttle: 1.0,
                    brake: 0.0,
                    steer: 0.0,
                    handbrake: false,
                    shift_up: false,
                    shift_down: false,
                    reset_request: false,
                },
            )
            .await;
            let task = tokio::spawn(tick_loop(state.clone()));
            tokio::time::sleep(Duration::from_millis(40)).await;
            task.abort();
            let hub = state.0.lock().await;
            let player = &hub.rooms[&1].players[&1];
            assert_eq!(player.world.tick, 0);
            assert_eq!(player.ack_sequence, None);
            assert_eq!(player.applied_sequence, None);
            assert_eq!(player.applied_server_tick, None);
        }
    }

    #[tokio::test]
    async fn track_and_vehicle_world_replacement_clears_ack_markers() {
        let (state, _) = synthetic_open_drive_state();
        let mut hub = state.0.lock().await;
        let room = hub.rooms.get_mut(&1).unwrap();
        let track = Arc::clone(room.gameplay_track.as_ref().unwrap());
        for player in room.players.values_mut() {
            player.ack_sequence = Some(12);
            player.applied_sequence = Some(11);
            player.applied_server_tick = Some(42);
        }
        change_room_track(
            room,
            track.profile_id.clone(),
            track.source_fingerprint.clone(),
            "owner-open-drive-v1".into(),
            0,
            Some(Arc::clone(&track)),
            true,
            &stub_models(),
        )
        .unwrap();
        assert!(room.players.values().all(|player| {
            player.ack_sequence.is_none()
                && player.applied_sequence.is_none()
                && player.applied_server_tick.is_none()
                && player.world.tick == 0
        }));

        let guest_car = track.cars.get("vehicle-profile-0002").unwrap();
        let guest_fingerprint = guest_car.source_fingerprint.clone();
        change_player_vehicle(
            room,
            1,
            guest_car.profile_id.clone(),
            guest_fingerprint,
            "paint-project-blue".into(),
            (String::new(), 0),
            &stub_models(),
        )
        .unwrap();
        assert!(room.players[&1].ack_sequence.is_none());
        assert!(room.players[&1].applied_sequence.is_none());
        assert!(room.players[&1].applied_server_tick.is_none());
        assert_eq!(room.players[&1].world.tick, 0);
    }

    #[tokio::test]
    async fn hub_frame_restores_and_replays_actual_multicar_tick_suffix() {
        let (source, _) = synthetic_open_drive_state();
        let (target, _) = synthetic_open_drive_state();
        let frame = {
            let mut hub = source.0.lock().await;
            let room = hub.rooms.get_mut(&1).unwrap();
            room.summary.status = RaceStatus::Racing;
            for player in room.players.values_mut() {
                player.ready = true;
            }
            room.players.get_mut(&1).unwrap().input =
                normalize_input(0.6, 0.0, 0.25, false, (false, false), false).unwrap();
            room.players.get_mut(&1).unwrap().ack_sequence = Some(10);
            room.players.get_mut(&2).unwrap().input =
                normalize_input(0.4, 0.0, -0.2, false, (false, false), false).unwrap();
            room.players.get_mut(&2).unwrap().ack_sequence = Some(20);
            for _ in 0..4 {
                advance_authoritative_tick(&mut hub);
            }
            let room = hub.rooms.get_mut(&1).unwrap();
            let player = room.players.get_mut(&1).unwrap();
            player.input = normalize_input(0.8, 0.0, 0.5, false, (false, false), false).unwrap();
            player.ack_sequence = Some(11);
            let frame = hub.capture_simulation_frame().unwrap();
            assert_eq!(frame.authoritative_tick, 4);
            assert_eq!(frame.rooms[0].participants[0].applied_sequence, Some(10));
            assert_eq!(frame.rooms[0].participants[0].received_sequence, Some(11));
            frame
        };

        {
            let mut hub = target.0.lock().await;
            let player = hub.rooms.get_mut(&1).unwrap().players.get_mut(&1).unwrap();
            player
                .world
                .step(normalize_input(1.0, 0.0, -1.0, false, (false, false), false).unwrap());
            hub.restore_simulation_frame(&frame).unwrap();
            assert_eq!(hub.capture_simulation_frame().unwrap(), frame);
        }

        let mut source_hub = source.0.lock().await;
        let mut target_hub = target.0.lock().await;
        for _ in 0..12 {
            advance_authoritative_tick(&mut source_hub);
            advance_authoritative_tick(&mut target_hub);
        }
        assert_eq!(
            source_hub.capture_simulation_frame().unwrap(),
            target_hub.capture_simulation_frame().unwrap()
        );
    }

    #[tokio::test]
    async fn hub_frame_rejects_malformed_state_transactionally() {
        let (source, _) = synthetic_open_drive_state();
        let frame = {
            let mut hub = source.0.lock().await;
            let room = hub.rooms.get_mut(&1).unwrap();
            room.summary.status = RaceStatus::Racing;
            for player in room.players.values_mut() {
                player.ready = true;
            }
            let player = hub.rooms.get_mut(&1).unwrap().players.get_mut(&1).unwrap();
            player.input = normalize_input(0.5, 0.0, 0.25, false, (false, false), false).unwrap();
            player.ack_sequence = Some(7);
            hub.capture_simulation_frame().unwrap()
        };

        let mut invalid_frames = Vec::new();
        let mut bad_version = frame.clone();
        bad_version.version = HUB_SIMULATION_FRAME_VERSION + 1;
        invalid_frames.push(bad_version);
        let mut missing_participant = frame.clone();
        missing_participant.rooms[0].participants.pop();
        invalid_frames.push(missing_participant);
        let mut invalid_intent = frame.clone();
        invalid_intent.rooms[0].participants[0]
            .latest_input
            .throttle = f64::NAN;
        invalid_frames.push(invalid_intent);
        let mut partial_ack = frame.clone();
        partial_ack.rooms[0].participants[0].applied_sequence = Some(7);
        invalid_frames.push(partial_ack);
        let mut future_ack = frame.clone();
        future_ack.rooms[0].participants[0].applied_sequence = Some(8);
        future_ack.rooms[0].participants[0].applied_server_tick = Some(0);
        invalid_frames.push(future_ack);
        let mut wrong_world = frame.clone();
        wrong_world.rooms[0].participants[0].world.world_identity[0] ^= 1;
        invalid_frames.push(wrong_world);
        let mut changed_contact = frame.clone();
        changed_contact.rooms[0].participants[0].contact_radius += 0.25;
        invalid_frames.push(changed_contact);
        let mut changed_config = frame.clone();
        changed_config.rooms[0].track_profile_id.push_str("-other");
        invalid_frames.push(changed_config);
        let mut lobby_all_ready = frame.clone();
        lobby_all_ready.rooms[0].status = RaceStatus::Lobby;
        invalid_frames.push(lobby_all_ready);
        let mut lobby_with_schedule = frame.clone();
        lobby_with_schedule.rooms[0].status = RaceStatus::Lobby;
        for participant in &mut lobby_with_schedule.rooms[0].participants {
            participant.ready = false;
        }
        invalid_frames.push(lobby_with_schedule);
        let mut countdown_without_ready_roster = frame.clone();
        countdown_without_ready_roster.rooms[0].countdown = 1;
        countdown_without_ready_roster.rooms[0].status = RaceStatus::Countdown { ticks_left: 1 };
        countdown_without_ready_roster.rooms[0].participants[1].ready = false;
        invalid_frames.push(countdown_without_ready_roster);
        let mut zero_countdown = frame.clone();
        zero_countdown.rooms[0].status = RaceStatus::Countdown { ticks_left: 0 };
        zero_countdown.rooms[0].countdown = 0;
        invalid_frames.push(zero_countdown);
        let mut oversized_countdown = frame.clone();
        oversized_countdown.rooms[0].status = RaceStatus::Countdown {
            ticks_left: COUNTDOWN_TICKS + 1,
        };
        oversized_countdown.rooms[0].countdown = COUNTDOWN_TICKS + 1;
        invalid_frames.push(oversized_countdown);
        let mut race_not_ready = frame.clone();
        race_not_ready.rooms[0].participants[1].ready = false;
        invalid_frames.push(race_not_ready);

        for invalid in invalid_frames {
            let (target, _) = synthetic_open_drive_state();
            let mut hub = target.0.lock().await;
            let before = hub.capture_simulation_frame().unwrap();
            assert!(hub.restore_simulation_frame(&invalid).is_err());
            assert_eq!(hub.capture_simulation_frame().unwrap(), before);
        }

        let (changed_target, _) = synthetic_open_drive_state();
        let mut hub = changed_target.0.lock().await;
        hub.rooms
            .get_mut(&1)
            .unwrap()
            .players
            .get_mut(&1)
            .unwrap()
            .contact_radius += 0.25;
        let before = hub.capture_simulation_frame().unwrap();
        assert!(hub.restore_simulation_frame(&frame).is_err());
        assert_eq!(hub.capture_simulation_frame().unwrap(), before);
    }

    #[tokio::test]
    async fn hub_frame_accepts_reachable_single_player_finished_after_leave() {
        let (state, _) = synthetic_open_drive_state();
        let mut hub = state.0.lock().await;
        let room = hub.rooms.get_mut(&1).unwrap();
        room.summary.status = RaceStatus::Racing;
        for player in room.players.values_mut() {
            player.ready = true;
        }
        let player = room.players.get_mut(&1).unwrap();
        player.input = normalize_input(0.5, 0.0, 0.25, false, (false, false), false).unwrap();
        player.ack_sequence = Some(7);
        for _ in 0..3 {
            advance_authoritative_tick(&mut hub);
        }
        assert_eq!(hub.rooms[&1].players[&1].applied_sequence, Some(7));

        // This is the production disconnect/leave transition: a racing room
        // with one participant becomes Finished and clears the survivor's ready
        // flag, while retaining the already applied latest input markers.
        leave_room(&mut hub, 2);
        let frame = hub.capture_simulation_frame().unwrap();
        assert_eq!(frame.rooms[0].status, RaceStatus::Finished);
        assert_eq!(frame.rooms[0].participants.len(), 1);
        assert!(!frame.rooms[0].participants[0].ready);
        assert_eq!(frame.rooms[0].participants[0].applied_sequence, Some(7));
    }

    #[tokio::test]
    async fn hub_tick_schedule_replays_exact_coalesced_and_wrapping_inputs() {
        let (source, _) = synthetic_open_drive_state();
        let (target, mut target_receivers) = synthetic_open_drive_state();
        let (snapshot_tx_1, snapshot_rx_1) = watch::channel(None);
        let (snapshot_tx_2, snapshot_rx_2) = watch::channel(None);
        {
            let mut hub = target.0.lock().await;
            hub.set_tick_schedule_recording(true);
            hub.clients.get_mut(&1).unwrap().snapshots = snapshot_tx_1;
            hub.clients.get_mut(&2).unwrap().snapshots = snapshot_tx_2;
        }
        let checkpoint = {
            let mut hub = source.0.lock().await;
            hub.set_tick_schedule_recording(true);
            let room = hub.rooms.get_mut(&1).unwrap();
            room.summary.status = RaceStatus::Racing;
            for player in room.players.values_mut() {
                player.ready = true;
            }
            hub.capture_simulation_frame().unwrap()
        };

        for (player_id, sequence, throttle, steer) in [
            (1, 5, 0.2, -0.1),
            (1, 6, 0.8, 0.4),
            (1, 5, 0.1, -0.8), // stale after 6: latest value must remain 6
            (2, u32::MAX, 0.3, -0.2),
            (2, 0, 0.6, 0.25), // valid wrapping successor
        ] {
            process_message(
                &source,
                player_id,
                ClientMessage::DriverInput {
                    sequence,
                    client_tick: 0, // server never uses the client tick for application order
                    throttle,
                    brake: 0.0,
                    steer,
                    handbrake: false,
                    shift_up: false,
                    shift_down: false,
                    reset_request: false,
                },
            )
            .await;
        }

        {
            let mut hub = source.0.lock().await;
            advance_authoritative_tick(&mut hub);
        }
        process_message(
            &source,
            1,
            ClientMessage::DriverInput {
                sequence: 7,
                client_tick: 0,
                throttle: 0.4,
                brake: 0.1,
                steer: -0.3,
                handbrake: true,
                shift_up: false,
                shift_down: false,
                reset_request: false,
            },
        )
        .await;
        process_message(
            &source,
            2,
            ClientMessage::DriverInput {
                sequence: u32::MAX,
                client_tick: 999_999,
                throttle: 0.9,
                brake: 0.0,
                steer: 1.0,
                handbrake: false,
                shift_up: false,
                shift_down: false,
                reset_request: false,
            },
        )
        .await;
        {
            let mut hub = source.0.lock().await;
            for _ in 0..3 {
                advance_authoritative_tick(&mut hub);
            }
        }

        let schedules = {
            let hub = source.0.lock().await;
            let schedules = hub.capture_tick_schedule(0, 4).unwrap();
            assert_eq!(schedules.len(), 4);
            assert_eq!(
                schedules[0].racing_rooms[0].participants[0].received_sequence,
                Some(6)
            );
            assert_eq!(
                schedules[0].racing_rooms[0].participants[0].applied_sequence,
                Some(6)
            );
            assert_eq!(
                schedules[0].racing_rooms[0].participants[0].input,
                normalize_input(0.8, 0.0, 0.4, false, (false, false), false).unwrap()
            );
            assert_eq!(
                schedules[0].racing_rooms[0].participants[1].received_sequence,
                Some(0)
            );
            assert_eq!(
                schedules[1].racing_rooms[0].participants[0].received_sequence,
                Some(7)
            );
            assert_eq!(
                schedules[1].racing_rooms[0].participants[1].received_sequence,
                Some(0)
            );
            schedules
        };

        let mut target_hub = target.0.lock().await;
        target_hub.restore_simulation_frame(&checkpoint).unwrap();
        let unchanged_frame = target_hub.capture_simulation_frame().unwrap();
        let unchanged_history = target_hub.tick_schedules.clone();
        let mut bad_after_boundary = schedules.clone();
        bad_after_boundary[0].after_boundary[0] ^= 1;
        assert!(
            target_hub
                .replay_tick_schedule(&bad_after_boundary)
                .is_err()
        );
        assert_eq!(
            target_hub.capture_simulation_frame().unwrap(),
            unchanged_frame
        );
        assert_eq!(target_hub.tick_schedules, unchanged_history);
        target_hub.replay_tick_schedule(&schedules).unwrap();
        let source_hub = source.0.lock().await;
        assert_eq!(
            target_hub.capture_simulation_frame().unwrap(),
            source_hub.capture_simulation_frame().unwrap()
        );
        assert_eq!(target_hub.tick_schedules, schedules);
        assert!(snapshot_rx_1.borrow().is_none());
        assert!(snapshot_rx_2.borrow().is_none());
        assert!(
            target_receivers
                .iter_mut()
                .all(|receiver| receiver.try_recv().is_err())
        );
    }

    #[tokio::test]
    async fn hub_tick_schedule_fails_closed_for_eviction_and_lifecycle_changes() {
        let (state, _) = synthetic_open_drive_state();
        {
            let mut hub = state.0.lock().await;
            hub.set_tick_schedule_recording(true);
            let room = hub.rooms.get_mut(&1).unwrap();
            room.summary.status = RaceStatus::Racing;
            for player in room.players.values_mut() {
                player.ready = true;
            }
            for _ in 0..=HUB_REPLAY_HISTORY_TICKS {
                advance_authoritative_tick(&mut hub);
            }
            assert!(hub.capture_tick_schedule(0, 1).is_err());
            assert_eq!(
                hub.capture_tick_schedule(1, HUB_REPLAY_HISTORY_TICKS as u64 + 1)
                    .unwrap()
                    .len(),
                HUB_REPLAY_HISTORY_TICKS
            );
        }

        let (source, _) = synthetic_open_drive_state();
        let (target, _) = synthetic_open_drive_state();
        let checkpoint = {
            let mut hub = source.0.lock().await;
            hub.set_tick_schedule_recording(true);
            let room = hub.rooms.get_mut(&1).unwrap();
            room.summary.status = RaceStatus::Racing;
            for player in room.players.values_mut() {
                player.ready = true;
            }
            hub.capture_simulation_frame().unwrap()
        };
        target.0.lock().await.set_tick_schedule_recording(true);
        {
            let mut hub = source.0.lock().await;
            leave_room(&mut hub, 2);
            advance_authoritative_tick(&mut hub);
        }
        let schedules = source.0.lock().await.capture_tick_schedule(0, 1).unwrap();
        let mut target_hub = target.0.lock().await;
        target_hub.restore_simulation_frame(&checkpoint).unwrap();
        let before = target_hub.capture_simulation_frame().unwrap();
        let history_before = target_hub.tick_schedules.clone();
        assert!(target_hub.replay_tick_schedule(&schedules).is_err());
        assert_eq!(target_hub.capture_simulation_frame().unwrap(), before);
        assert_eq!(target_hub.tick_schedules, history_before);
    }

    #[tokio::test]
    async fn normal_ticks_skip_world_identity_hashes_even_with_large_synthetic_geometry() {
        let (state, _) = synthetic_open_drive_state();
        let mut hub = state.0.lock().await;
        let track = hub.rooms[&1].gameplay_track.as_ref().unwrap();
        let mut large_geometry = track.cars["vehicle-profile-0001"].collision.clone();
        let base_triangle = large_geometry.ground[0];
        large_geometry.ground.clear();
        large_geometry.ground.push(base_triangle);
        for index in 1..2048 {
            let offset_x = index as f64 * 400.0;
            let offset_y = 10_000.0;
            let mut vertices = base_triangle.vertices;
            for vertex in &mut vertices {
                vertex[0] += offset_x;
                vertex[1] += offset_y;
            }
            large_geometry
                .ground
                .push(racer_sim::PreviewGroundTriangle { vertices });
        }
        let room = hub.rooms.get_mut(&1).unwrap();
        room.summary.status = RaceStatus::Racing;
        for player in room.players.values_mut() {
            player.ready = true;
            player
                .world
                .install_preview_collision(large_geometry.clone())
                .unwrap();
        }

        hub.identity_build_count.store(0, Ordering::Relaxed);
        for _ in 0..4 {
            advance_authoritative_tick(&mut hub);
        }
        assert!(!hub.tick_schedule_recording_enabled);
        assert!(hub.tick_schedules.is_empty());
        assert_eq!(hub.identity_build_count.load(Ordering::Relaxed), 0);

        hub.set_tick_schedule_recording(true);
        advance_authoritative_tick(&mut hub);
        assert!(!hub.tick_schedules.is_empty());
        assert!(hub.identity_build_count.load(Ordering::Relaxed) > 0);
    }

    #[tokio::test]
    async fn hub_identity_and_frame_order_are_canonical() {
        let (first, _) = synthetic_open_drive_state();
        let (second, _) = synthetic_open_drive_state();
        let mut first = first.0.lock().await;
        let mut second = second.0.lock().await;
        add_synthetic_room_two(&mut first);
        add_synthetic_room_two(&mut second);
        assert_eq!(
            first.simulation_identity().unwrap(),
            second.simulation_identity().unwrap()
        );
        let first_frame = first.capture_simulation_frame().unwrap();
        let rooms = second.rooms.drain().collect::<Vec<_>>();
        for (room_id, room) in rooms.into_iter().rev() {
            second.rooms.insert(room_id, room);
        }
        assert_eq!(
            first.simulation_identity().unwrap(),
            second.simulation_identity().unwrap()
        );
        assert_eq!(first_frame, second.capture_simulation_frame().unwrap());
    }

    #[test]
    fn reset_contact_origin_does_not_sweep_teleport_path() {
        let before = Vec2::new(-100.0, 0.0);
        let reset_spawn = Vec2::new(0.0, 0.0);
        assert_eq!(contact_sweep_origin(before, reset_spawn, true), reset_spawn);
        assert_eq!(contact_sweep_origin(before, reset_spawn, false), before);

        let reset_world = new_world(TrackRuntime::open_preview(), None, &stub_models()).unwrap();
        let mut reset_vehicle = reset_world.vehicle;
        reset_vehicle.position = reset_spawn;
        let mut peer_world = new_world(TrackRuntime::open_preview(), None, &stub_models()).unwrap();
        peer_world.vehicle.position = before;
        let mut bodies = vec![
            VehicleContactBody {
                stable_id: 1,
                vehicle: reset_vehicle,
                previous_position: contact_sweep_origin(before, reset_spawn, true),
                radius: 1.0,
                min_z: -0.5,
                max_z: 0.5,
            },
            VehicleContactBody {
                stable_id: 2,
                vehicle: peer_world.vehicle,
                previous_position: before,
                radius: 1.0,
                min_z: -0.5,
                max_z: 0.5,
            },
        ];
        assert_eq!(resolve_pairwise_vehicle_contacts(&mut bodies), Ok(0));
    }

    #[test]
    fn identical_open_drive_start_positions_settle_with_bounded_contacts() {
        let mut worlds = [
            new_world(TrackRuntime::open_preview(), None, &stub_models()).unwrap(),
            new_world(TrackRuntime::open_preview(), None, &stub_models()).unwrap(),
        ];
        assert_eq!(worlds[0].vehicle.position, worlds[1].vehicle.position);
        for _ in 0..8 {
            let mut bodies = worlds
                .iter()
                .enumerate()
                .map(|(id, world)| VehicleContactBody {
                    stable_id: id as u32 + 1,
                    vehicle: world.vehicle,
                    previous_position: world.vehicle.position,
                    radius: 1.0,
                    min_z: -0.5,
                    max_z: 0.5,
                })
                .collect::<Vec<_>>();
            resolve_pairwise_vehicle_contacts(&mut bodies).unwrap();
            for (world, body) in worlds.iter_mut().zip(bodies) {
                let previous = world.vehicle.position;
                world.vehicle = body.vehicle;
                world.constrain_after_vehicle_contact(previous);
            }
        }
        let delta = (worlds[0].vehicle.position.x - worlds[1].vehicle.position.x)
            .hypot(worlds[0].vehicle.position.y - worlds[1].vehicle.position.y);
        assert!(
            delta.is_finite() && delta >= 1.99,
            "start separation={delta}"
        );
        assert!(
            worlds
                .iter()
                .all(|world| world.vehicle.position_z.is_finite())
        );
    }

    #[test]
    fn three_car_contact_near_wall_preserves_supported_world_bounds() {
        let collision = PreviewCollisionWorld {
            ground: vec![
                PreviewGroundTriangle {
                    vertices: [[-10.0, -10.0, 0.0], [10.0, -10.0, 0.0], [10.0, 10.0, 0.0]],
                },
                PreviewGroundTriangle {
                    vertices: [[-10.0, -10.0, 0.0], [10.0, 10.0, 0.0], [-10.0, 10.0, 0.0]],
                },
            ],
            walls: vec![PreviewWallSegment {
                start: Vec2::new(10.0, -10.0),
                end: Vec2::new(10.0, 10.0),
                min_z: 0.0,
                max_z: 2.0,
            }],
            vehicle_radius: 1.0,
            vehicle_min_z: -0.5,
            vehicle_max_z: 0.5,
            initial_ground_z: 0.0,
            units_per_sim_unit: 1.0,
            edge_rows: Vec::new(),
        };
        let mut worlds = (0..3)
            .map(|_| {
                let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
                world.enable_shared_preview_handling();
                world.install_preview_collision(collision.clone()).unwrap();
                world
            })
            .collect::<Vec<_>>();
        for world in &mut worlds {
            world.vehicle.position = Vec2::new(8.2, 0.0);
            world.vehicle.position_z = 0.0;
        }

        for _ in 0..8 {
            let mut bodies = worlds
                .iter()
                .enumerate()
                .map(|(id, world)| VehicleContactBody {
                    stable_id: id as u32 + 10,
                    vehicle: world.vehicle,
                    previous_position: world.vehicle.position,
                    radius: 1.0,
                    min_z: -0.5,
                    max_z: 0.5,
                })
                .collect::<Vec<_>>();
            resolve_pairwise_vehicle_contacts(&mut bodies).unwrap();
            assert!(
                bodies.iter().any(|body| body.vehicle.position.x > 9.0),
                "overlap correction should reach the synthetic wall"
            );
            for (world, body) in worlds.iter_mut().zip(bodies) {
                let previous = world.vehicle.position;
                world.vehicle = body.vehicle;
                world.constrain_after_vehicle_contact(previous);
            }
        }
        assert!(worlds.iter().all(|world| {
            world.vehicle.position.x <= 9.001
                && world.vehicle.position_z.is_finite()
                && world.preview_ground_normal().is_some()
        }));
    }

    #[test]
    fn snapshots_coalesce_and_reliable_overflow_closes_client() {
        let hub = Hub::default();
        let (tx, _rx) = mpsc::channel(1);
        let (snapshots, snapshot_rx) = watch::channel(None);
        let (close, close_rx) = watch::channel(false);
        let player = 4;
        let mut hub = hub;
        hub.clients.insert(
            player,
            ClientChannel {
                reliable: tx.clone(),
                snapshots,
                close,
            },
        );
        tx.try_send(ServerMessage::LeftRace).unwrap();
        send(
            &hub,
            player,
            ServerMessage::StateSnapshot {
                race_id: 1,
                server_tick: 1,
                ack_input_sequence: None,
                ack_applied_input_sequence: None,
                ack_applied_server_tick: None,
                status: RaceStatus::Racing,
                selection_revision: 1,
                vehicles: vec![],
            },
        );
        send(
            &hub,
            player,
            ServerMessage::StateSnapshot {
                race_id: 1,
                server_tick: 2,
                ack_input_sequence: None,
                ack_applied_input_sequence: None,
                ack_applied_server_tick: None,
                status: RaceStatus::Racing,
                selection_revision: 1,
                vehicles: vec![],
            },
        );
        assert!(matches!(
            &*snapshot_rx.borrow(),
            Some(ServerMessage::StateSnapshot { server_tick: 2, .. })
        ));
        assert!(!*close_rx.borrow());
        send(&hub, player, ServerMessage::LeftRace);
        assert!(close_rx.has_changed().unwrap());
    }

    // ---- Imported tracks that race: laps, grid, opponents with their own cars ----

    /// A circular road of radius `radius` (about 1.6 km at 250 m) starting at
    /// the origin heading along +X, so the start pose is the simulation origin.
    fn ring_road(radius: f64) -> Vec<racer_sim::TrackEdgeRow> {
        let step = 4.0 / radius;
        (0..(std::f64::consts::TAU / step) as usize)
            .map(|i| {
                let a = i as f64 * step;
                // Centre of the circle is (0, radius); a = 0 is the origin.
                racer_sim::TrackEdgeRow {
                    x: radius * a.sin(),
                    y: radius - radius * a.cos(),
                    z: 0.0,
                    forward: Vec2::new(a.cos(), a.sin()),
                    right: Vec2::new(a.sin(), -a.cos()),
                    left_distance: 8.0,
                    right_distance: 8.0,
                    block: 0,
                }
            })
            .collect()
    }

    fn racing_catalog_state() -> (AppState, Vec<mpsc::Receiver<ServerMessage>>) {
        let catalog = GameplayDataStore::synthetic_catalog_with_road(
            &["track-profile-0001", "track-profile-0002"],
            &["vehicle-profile-0001", "vehicle-profile-0002"],
            ring_road(250.0),
        );
        let mut hub = Hub::default();
        hub.vehicle_models = Arc::new(stub_registry());
        hub.imported_default_model = Some(STUB_DEFAULT.into());
        let mut receivers = Vec::new();
        for player_id in [1, 2] {
            let (reliable, receiver) = mpsc::channel(256);
            let (snapshots, _) = watch::channel(None);
            let (close, _) = watch::channel(false);
            hub.clients.insert(
                player_id,
                ClientChannel {
                    reliable,
                    snapshots,
                    close,
                },
            );
            receivers.push(receiver);
        }
        (
            AppState(Arc::new(Mutex::new(hub)), None, Some(Arc::new(catalog))),
            receivers,
        )
    }

    async fn create_owner_room(state: &AppState, laps: u8, ai_count: u8) {
        let track = state
            .2
            .as_ref()
            .unwrap()
            .track("track-profile-0001")
            .unwrap()
            .clone();
        let car = track.cars.get("vehicle-profile-0001").unwrap();
        process_message(
            state,
            1,
            ClientMessage::CreateRace {
                name: "Owner race".into(),
                laps,
                content_identity: "owner-open-drive-v1".into(),
                track_profile_id: track.profile_id.clone(),
                track_fingerprint: track.source_fingerprint.clone(),
                vehicle_profile_id: car.profile_id.clone(),
                vehicle_fingerprint: car.source_fingerprint.clone(),
                driver_name: "Host".into(),
                paint_id: "paint-colour-1".into(),
                car_model: String::new(),
                car_variant: 0,
                ai_count,
                ai_skill: 1,
                ai_catch_up: true,
            },
        )
        .await;
        let car = track.cars.get("vehicle-profile-0002").unwrap();
        process_message(
            state,
            2,
            ClientMessage::JoinRace {
                race_id: 1,
                content_identity: "owner-open-drive-v1".into(),
                track_profile_id: track.profile_id.clone(),
                track_fingerprint: track.source_fingerprint.clone(),
                vehicle_profile_id: car.profile_id.clone(),
                vehicle_fingerprint: car.source_fingerprint.clone(),
                driver_name: "Guest".into(),
                paint_id: "paint-colour-3".into(),
                car_model: String::new(),
                car_variant: 0,
            },
        )
        .await;
    }

    #[tokio::test]
    async fn owner_track_with_a_road_races_with_laps_and_opponents_in_their_own_cars() {
        let (state, mut receivers) = racing_catalog_state();
        create_owner_room(&state, 1, 3).await;
        {
            let hub = state.0.lock().await;
            let room = hub.rooms.get(&1).unwrap();
            assert!(room.summary.open_drive && room.races());
            assert_eq!((room.summary.laps, room.summary.ai_count), (1, 3));
            // Two humans plus three computer cars start one race.
            let course = room.owner_course().expect("the ring is a circuit");
            assert!(course.closed);
        }
        // Ready needs the car models of the computer drivers too.
        let track = state.2.as_ref().unwrap().track("track-profile-0001").unwrap().clone();
        let template = |id: &str| protocol::AvailableCarProfile {
            profile_id: id.into(),
            source_fingerprint: track.cars.get(id).unwrap().source_fingerprint.clone(),
        };
        let both = vec![template("vehicle-profile-0001"), template("vehicle-profile-0002")];
        let ready = |templates: Vec<protocol::AvailableCarProfile>, revision| ClientMessage::ReadyState {
            ready: true,
            selection_revision: revision,
            track_fingerprint: track.source_fingerprint.clone(),
            vehicle_templates: templates,
        };
        let revision = state.0.lock().await.rooms[&1].summary.selection_revision;
        // Both cars of the track are used by the humans, so the roster is exact with both.
        process_message(&state, 1, ready(both.clone(), revision)).await;
        process_message(&state, 2, ready(both.clone(), revision)).await;
        for receiver in &mut receivers {
            while receiver.try_recv().is_ok() {}
        }
        let (snapshot_tx, snapshot_rx) = watch::channel(None);
        {
            let mut hub = state.0.lock().await;
            assert!(matches!(
                hub.rooms[&1].summary.status,
                RaceStatus::Countdown { .. }
            ));
            hub.clients.get_mut(&1).unwrap().snapshots = snapshot_tx;
            // Humans stand on the course grid with opponents ahead of them.
            let room = hub.rooms.get(&1).unwrap();
            let ai = room.ai.as_ref().expect("opponents were created");
            assert_eq!(ai.opponents.cars.len(), 3);
            let human_x: Vec<f64> = room.players.values().map(|p| p.world.vehicle.position.x).collect();
            assert!(ai
                .opponents
                .cars
                .iter()
                .all(|c| human_x.iter().all(|x| c.world.vehicle.position.x > *x)));
            assert!(room.players.values().all(|p| p.world.track.laps_to_finish == 1
                && p.world.track.checkpoints.len() >= 3));
            for _ in 0..(COUNTDOWN_TICKS + 1) {
                advance_authoritative_tick(&mut hub);
            }
            assert!(matches!(hub.rooms[&1].summary.status, RaceStatus::Racing));
            // Let the computer cars race one lap (about 1.6 km).
            for _ in 0..(60 * 120) {
                advance_authoritative_tick(&mut hub);
                let room = &hub.rooms[&1];
                if room.ai.as_ref().is_some_and(|ai| {
                    ai.opponents.cars.iter().all(|c| c.world.vehicle.finished)
                }) {
                    break;
                }
            }
            send_room_snapshot(&hub, 1);
        }
        let ServerMessage::StateSnapshot { vehicles, .. } =
            snapshot_rx.borrow().clone().expect("snapshot")
        else {
            panic!("expected a snapshot")
        };
        let ai: Vec<_> = vehicles.iter().filter(|v| v.is_ai).collect();
        assert_eq!(ai.len(), 3);
        // The field alternates between the track's cars, starting after the host's.
        let profiles: Vec<&str> = ai.iter().map(|v| v.vehicle_profile_id.as_str()).collect();
        assert_eq!(
            profiles,
            ["vehicle-profile-0002", "vehicle-profile-0001", "vehicle-profile-0002"]
        );
        assert!(ai.iter().all(|v| v.paint_id == "paint-colour-1"), "opponents wear the host's paint");
        assert!(ai.iter().all(|v| v.finished && v.completed_laps == 1 && v.race_position >= 1));
        // Positions are distinct over everybody in the room.
        let mut positions: Vec<u8> = vehicles.iter().map(|v| v.race_position).collect();
        positions.sort_unstable();
        positions.dedup();
        assert_eq!(positions.len(), vehicles.len());
    }

    #[tokio::test]
    async fn owner_room_without_a_road_course_stays_free_drive_and_without_opponents() {
        // The synthetic default catalogue has no road rows.
        let (state, _) = synthetic_open_drive_state();
        process_message(
            &state,
            1,
            ClientMessage::ConfigureAi {
                race_id: 1,
                selection_revision: 1,
                ai_count: 2,
                ai_skill: 1,
                ai_catch_up: true,
            },
        )
        .await;
        let hub = state.0.lock().await;
        let room = hub.rooms.get(&1).unwrap();
        assert!(!room.races());
        assert_eq!(room.summary.ai_count, 0);
    }

    #[tokio::test]
    async fn owner_laps_are_bounded_and_free_drive_clears_opponents() {
        let (state, _) = racing_catalog_state();
        create_owner_room(&state, 9, 2).await;
        assert!(state.0.lock().await.rooms.is_empty(), "more than five laps is invalid");
        create_owner_room(&state, 5, 2).await;
        {
            let hub = state.0.lock().await;
            assert_eq!(hub.rooms[&1].summary.laps, 5);
        }
        let track = state.2.as_ref().unwrap().track("track-profile-0001").unwrap().clone();
        process_message(
            &state,
            1,
            ClientMessage::ChangeTrack {
                race_id: 1,
                selection_revision: 1,
                content_identity: "owner-open-drive-v1".into(),
                track_profile_id: track.profile_id.clone(),
                track_fingerprint: track.source_fingerprint.clone(),
                laps: 0,
            },
        )
        .await;
        let hub = state.0.lock().await;
        let room = hub.rooms.get(&1).unwrap();
        assert!(!room.races());
        assert_eq!((room.summary.laps, room.summary.ai_count), (0, 0));
    }

    #[tokio::test]
    async fn concurrent_selection_changes_do_not_conflict() {
        let (state, mut receivers) = synthetic_open_drive_state();
        let other = state
            .2
            .as_ref()
            .unwrap()
            .track("track-profile-0003")
            .unwrap()
            .clone();
        // Host changes the track and, from the same revision, the guest changes car.
        process_message(
            &state,
            1,
            ClientMessage::ChangeTrack {
                race_id: 1,
                selection_revision: 1,
                content_identity: "owner-open-drive-v1".into(),
                track_profile_id: other.profile_id.clone(),
                track_fingerprint: other.source_fingerprint.clone(),
                laps: 0,
            },
        )
        .await;
        let car = other.cars.get("vehicle-profile-0001").unwrap();
        process_message(
            &state,
            2,
            ClientMessage::ChangeVehicle {
                race_id: 1,
                selection_revision: 1,
                vehicle_profile_id: car.profile_id.clone(),
                vehicle_fingerprint: car.source_fingerprint.clone(),
                paint_id: "paint-colour-5".into(),
                car_model: String::new(),
                car_variant: 0,
            },
        )
        .await;
        // And the other way round: the host changes track again from a stale revision.
        let first = state
            .2
            .as_ref()
            .unwrap()
            .track("track-profile-0001")
            .unwrap()
            .clone();
        process_message(
            &state,
            1,
            ClientMessage::ChangeTrack {
                race_id: 1,
                selection_revision: 2,
                content_identity: "owner-open-drive-v1".into(),
                track_profile_id: first.profile_id.clone(),
                track_fingerprint: first.source_fingerprint.clone(),
                laps: 0,
            },
        )
        .await;
        let hub = state.0.lock().await;
        let room = hub.rooms.get(&1).unwrap();
        assert_eq!(room.summary.track_profile_id, "track-profile-0001");
        assert_eq!(room.players[&2].vehicle_profile_id, "vehicle-profile-0001");
        assert_eq!(room.players[&2].paint_id, "paint-colour-5");
        assert_eq!(room.summary.selection_revision, 4);
        for receiver in &mut receivers {
            while let Ok(message) = receiver.try_recv() {
                assert!(
                    !matches!(message, ServerMessage::Error { .. }),
                    "no change may be rejected: {message:?}"
                );
            }
        }
    }
}
