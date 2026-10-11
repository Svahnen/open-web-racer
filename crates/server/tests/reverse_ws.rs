//! Reverse gear over the real WebSocket path: two clients in a room, shift
//! taps of different lengths (including a tap that is released before the
//! server's next 32 Hz step), then throttle must drive the car backward.

use futures_util::{SinkExt, StreamExt};
use racer_content::project_tracks::{
    PROJECT_TRACK_CONTENT_ID, project_track_catalog, project_track_profile_id,
};
use racer_protocol::{
    self as protocol, ClientMessage, MAX_MESSAGE_BYTES, PROJECT_CONTENT_ID, PROTOCOL_MAJOR,
    ServerMessage, VehicleSnapshot,
};
use std::time::Duration;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header::ORIGIN},
};

type TestSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const GEAR_REVERSE: u8 = 250;
const GEAR_NEUTRAL: u8 = 251;

async fn start_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = racer_server::router("web");
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("ws://{address}/ws"), task)
}

async fn open(url: &str) -> TestSocket {
    let mut request = url.into_client_request().unwrap();
    let origin = url
        .strip_prefix("ws://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    request
        .headers_mut()
        .insert(ORIGIN, format!("http://{origin}").parse().unwrap());
    connect_async(request).await.unwrap().0
}

async fn next_server(socket: &mut TestSocket) -> ServerMessage {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Binary(bytes) = message {
            assert!(bytes.len() <= MAX_MESSAGE_BYTES);
            return protocol::decode_server(&bytes).unwrap();
        }
    }
}

async fn send(socket: &mut TestSocket, msg: ClientMessage) {
    let bytes = protocol::encode_client(&msg).unwrap();
    socket.send(Message::Binary(bytes.into())).await.unwrap();
}

async fn wait_for<T>(
    socket: &mut TestSocket,
    mut pick: impl FnMut(ServerMessage) -> Option<T>,
) -> T {
    loop {
        if let Some(value) = pick(next_server(socket).await) {
            return value;
        }
    }
}

fn hello() -> ClientMessage {
    ClientMessage::Hello {
        protocol_major: PROTOCOL_MAJOR,
        build_id: "test".into(),
        content_capabilities: vec![PROJECT_CONTENT_ID.into()],
    }
}

fn create(profile: &str, fingerprint: &str, content: &str) -> ClientMessage {
    ClientMessage::CreateRace {
        name: "Reverse room".into(),
        laps: 2,
        content_identity: content.into(),
        track_profile_id: profile.into(),
        track_fingerprint: fingerprint.into(),
        vehicle_profile_id: "vehicle-profile-project-default".into(),
        vehicle_fingerprint: "0".repeat(64),
        driver_name: "Host".into(),
        paint_id: "paint-project-red".into(),
        car_model: String::new(),
        car_variant: 0,
        ai_count: 0,
        ai_skill: 1,
        ai_catch_up: true,
    }
}

fn join(race_id: u32, profile: &str, fingerprint: &str, content: &str) -> ClientMessage {
    ClientMessage::JoinRace {
        race_id,
        content_identity: content.into(),
        track_profile_id: profile.into(),
        track_fingerprint: fingerprint.into(),
        vehicle_profile_id: "vehicle-profile-project-default".into(),
        vehicle_fingerprint: "0".repeat(64),
        driver_name: "Guest".into(),
        paint_id: "paint-project-blue".into(),
        car_model: String::new(),
        car_variant: 0,
    }
}

struct Driver {
    socket: TestSocket,
    player_id: u32,
    sequence: u32,
}

impl Driver {
    async fn input(&mut self, throttle: f32, brake: f32, up: bool, down: bool) {
        self.sequence += 1;
        let sequence = self.sequence;
        send(
            &mut self.socket,
            ClientMessage::DriverInput {
                sequence,
                client_tick: u64::from(sequence),
                throttle,
                brake,
                steer: 0.0,
                handbrake: false,
                shift_up: up,
                shift_down: down,
                reset_request: false,
            },
        )
        .await;
    }

    /// Next own vehicle from the stream of snapshots.
    async fn own(&mut self) -> VehicleSnapshot {
        let id = self.player_id;
        wait_for(&mut self.socket, |m| match m {
            ServerMessage::StateSnapshot { vehicles, .. } => {
                vehicles.into_iter().find(|v| v.player_id == id)
            }
            _ => None,
        })
        .await
    }

    async fn gear_is(&mut self, expected: u8) -> bool {
        for _ in 0..40 {
            if self.own().await.gear == expected {
                return true;
            }
        }
        false
    }
}

fn forward_speed(v: &VehicleSnapshot) -> f64 {
    v.velocity_x * v.heading.cos() + v.velocity_y * v.heading.sin()
}

/// Both players ready, wait for the race to go live; returns (host, guest).
async fn start_room(
    url: &str,
    profile: &str,
    fingerprint: &str,
    content: &str,
) -> (Driver, Driver) {
    start_room_with(
        url,
        profile,
        fingerprint,
        content,
        protocol::Transmission::Automatic,
    )
    .await
}

/// Like `start_room`; the guest chooses its gearbox mode in the lobby.
async fn start_room_with(
    url: &str,
    profile: &str,
    fingerprint: &str,
    content: &str,
    guest_transmission: protocol::Transmission,
) -> (Driver, Driver) {
    let mut host = open(url).await;
    let mut guest = open(url).await;
    for socket in [&mut host, &mut guest] {
        let _ = next_server(socket).await;
        let _ = next_server(socket).await;
        send(socket, hello()).await;
        let _ = next_server(socket).await;
    }
    send(&mut host, create(profile, fingerprint, content)).await;
    let race_id = wait_for(&mut host, |m| match m {
        ServerMessage::RaceCreated { race_id } => Some(race_id),
        _ => None,
    })
    .await;
    send(&mut guest, join(race_id, profile, fingerprint, content)).await;
    let (guest_id, revision) = wait_for(&mut guest, |m| match m {
        ServerMessage::JoinAccepted {
            player_id,
            selection_revision,
            ..
        } => Some((player_id, selection_revision)),
        _ => None,
    })
    .await;
    send(
        &mut guest,
        ClientMessage::SetTransmission {
            transmission: guest_transmission,
            auto_reverse: false,
        },
    )
    .await;
    for socket in [&mut host, &mut guest] {
        send(
            socket,
            ClientMessage::ReadyState {
                ready: true,
                selection_revision: revision,
                track_fingerprint: fingerprint.to_string(),
                vehicle_templates: vec![protocol::AvailableCarProfile {
                    profile_id: "vehicle-profile-project-default".into(),
                    source_fingerprint: "0".repeat(64),
                }],
            },
        )
        .await;
    }
    // The countdown runs in real time.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(40);
    loop {
        assert!(tokio::time::Instant::now() < deadline, "race never started");
        let live = wait_for(&mut guest, |m| match m {
            ServerMessage::StateSnapshot { status, .. } => {
                Some(matches!(status, protocol::RaceStatus::Racing))
            }
            _ => None,
        })
        .await;
        if live {
            break;
        }
    }
    (
        Driver {
            socket: host,
            player_id: 0,
            sequence: 0,
        },
        Driver {
            socket: guest,
            player_id: guest_id,
            sequence: 0,
        },
    )
}

#[derive(Clone, Copy)]
enum Tap {
    /// Press for 80 ms, release (a normal key press).
    Paced,
    /// Press and release in back-to-back messages (shorter than a server step).
    Burst,
}

async fn tap(d: &mut Driver, down: bool, kind: Tap) {
    d.input(0.0, 0.0, !down, down).await;
    if matches!(kind, Tap::Paced) {
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    d.input(0.0, 0.0, false, false).await;
    tokio::time::sleep(Duration::from_millis(70)).await;
}

async fn reverse_sequence(guest: &mut Driver, kind: Tap) {
    // The automatic box starts in first gear (HUD "1"): Z gives N, Z gives R,
    // the third press at R changes nothing.
    assert!(guest.gear_is(1).await, "race starts in first gear");
    tap(guest, true, kind).await;
    assert!(guest.gear_is(GEAR_NEUTRAL).await, "Z from 1 selects N");
    tap(guest, true, kind).await;
    assert!(guest.gear_is(GEAR_REVERSE).await, "Z from N selects R");
    tap(guest, true, kind).await;
    assert!(guest.gear_is(GEAR_REVERSE).await, "second Z stays in R");
    let mut best = 0.0f64;
    for _ in 0..80 {
        guest.input(1.0, 0.0, false, false).await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        let v = guest.own().await;
        best = best.min(forward_speed(&v));
        if best < -3.0 {
            break;
        }
    }
    assert!(best < -3.0, "reverse speed {best}");
    assert!(guest.gear_is(GEAR_REVERSE).await, "no auto-shift out of R");
}

async fn run_case(profile: &str, fingerprint: &str, content: &str, kind: Tap) {
    let (url, server) = start_server().await;
    let (host, mut guest) = start_room(&url, profile, fingerprint, content).await;
    reverse_sequence(&mut guest, kind).await;
    drop(host);
    server.abort();
}

#[tokio::test]
async fn circuit_room_reverses_with_paced_shift_taps() {
    run_case(
        "track-profile-project-circuit",
        &"0".repeat(64),
        PROJECT_CONTENT_ID,
        Tap::Paced,
    )
    .await;
}

#[tokio::test]
async fn circuit_room_reverses_with_burst_shift_taps() {
    run_case(
        "track-profile-project-circuit",
        &"0".repeat(64),
        PROJECT_CONTENT_ID,
        Tap::Burst,
    )
    .await;
}

#[tokio::test]
async fn generated_track_room_reverses_with_burst_shift_taps() {
    tokio::task::spawn_blocking(racer_server::warm_project_tracks)
        .await
        .unwrap();
    let info = project_track_catalog().into_iter().next().unwrap();
    run_case(
        &project_track_profile_id(&info.id),
        &info.fingerprint,
        PROJECT_TRACK_CONTENT_ID,
        Tap::Burst,
    )
    .await;
}

/// The gearbox mode is a per-player lobby setting: the manual player starts in
/// neutral and the throttle alone does not move the car; A walks 1, 2; the
/// automatic host starts in first gear and drives away.
#[tokio::test]
async fn manual_transmission_setting_reaches_the_players_world() {
    let (url, server) = start_server().await;
    let (_host, mut guest) = start_room_with(
        &url,
        "track-profile-project-circuit",
        &"0".repeat(64),
        PROJECT_CONTENT_ID,
        protocol::Transmission::Manual,
    )
    .await;
    assert!(
        guest.gear_is(GEAR_NEUTRAL).await,
        "manual starts in neutral"
    );
    for _ in 0..20 {
        guest.input(1.0, 0.0, false, false).await;
        let v = guest.own().await;
        assert!(forward_speed(&v).abs() < 0.01, "moved in neutral");
    }
    tap(&mut guest, false, Tap::Paced).await;
    assert!(guest.gear_is(1).await, "A selects first gear");
    let mut best = 0.0f64;
    for _ in 0..60 {
        guest.input(1.0, 0.0, false, false).await;
        best = best.max(forward_speed(&guest.own().await));
    }
    assert!(best > 3.0, "manual car drives away in first gear: {best}");
    tap(&mut guest, false, Tap::Paced).await;
    assert!(guest.gear_is(2).await, "A selects second gear");
    server.abort();
}

/// A manual setting is rejected once the race has started.
#[tokio::test]
async fn transmission_cannot_change_after_the_race_started() {
    let (url, server) = start_server().await;
    let (_host, mut guest) = start_room(
        &url,
        "track-profile-project-circuit",
        &"0".repeat(64),
        PROJECT_CONTENT_ID,
    )
    .await;
    send(
        &mut guest.socket,
        ClientMessage::SetTransmission {
            transmission: protocol::Transmission::Manual,
            auto_reverse: false,
        },
    )
    .await;
    let code = wait_for(&mut guest.socket, |m| match m {
        ServerMessage::Error { code, .. } => Some(code),
        _ => None,
    })
    .await;
    assert_eq!(code, "race_started");
    assert!(guest.gear_is(1).await, "still automatic in first gear");
    server.abort();
}
