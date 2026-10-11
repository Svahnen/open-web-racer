use futures_util::{SinkExt, StreamExt};
use racer_protocol::{
    self as protocol, ClientMessage, MAX_MESSAGE_BYTES, PROJECT_CONTENT_ID, PROTOCOL_MAJOR,
    ServerMessage,
};
use std::time::Duration;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header::ORIGIN},
};

async fn start_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = racer_server::router("web");
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("ws://{address}/ws"), task)
}

async fn open(
    url: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
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

async fn next_server(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> ServerMessage {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
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

async fn send(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    msg: ClientMessage,
) {
    let bytes = protocol::encode_client(&msg).unwrap();
    socket.send(Message::Binary(bytes.into())).await.unwrap();
}

fn hello(major: u16) -> ClientMessage {
    ClientMessage::Hello {
        protocol_major: major,
        build_id: "test".into(),
        content_capabilities: vec![PROJECT_CONTENT_ID.into()],
    }
}

#[tokio::test]
async fn websocket_requires_matching_hello_before_mutation() {
    let (url, server) = start_server().await;
    let mut socket = open(&url).await;
    assert!(matches!(
        next_server(&mut socket).await,
        ServerMessage::Welcome { .. }
    ));
    assert!(matches!(
        next_server(&mut socket).await,
        ServerMessage::LobbySnapshot { .. }
    ));
    send(
        &mut socket,
        ClientMessage::CreateRace {
            name: "should not exist".into(),
            laps: 1,
            content_identity: PROJECT_CONTENT_ID.into(),
            track_profile_id: "track-profile-project-circuit".into(),
            track_fingerprint: "0".repeat(64),
            vehicle_profile_id: "vehicle-profile-project-default".into(),
            vehicle_fingerprint: "0".repeat(64),
            driver_name: "Host".into(),
            paint_id: "paint-project-red".into(),
            car_model: String::new(),
            car_variant: 0,
            ai_count: 0,
            ai_skill: 1,
            ai_catch_up: true,
        },
    )
    .await;
    assert!(
        matches!(next_server(&mut socket).await, ServerMessage::Error { code, .. } if code == "hello_required")
    );
    send(&mut socket, hello(PROTOCOL_MAJOR)).await;
    assert!(
        matches!(next_server(&mut socket).await, ServerMessage::LobbySnapshot { races, .. } if races.is_empty())
    );
    send(&mut socket, ClientMessage::ListRaces).await;
    assert!(
        matches!(next_server(&mut socket).await, ServerMessage::LobbySnapshot { races, .. } if races.is_empty())
    );
    socket.close(None).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn major_mismatch_closes_without_creating_a_room() {
    let (url, server) = start_server().await;
    let mut socket = open(&url).await;
    let _ = next_server(&mut socket).await;
    let _ = next_server(&mut socket).await;
    send(&mut socket, hello(PROTOCOL_MAJOR + 1)).await;
    assert!(
        matches!(next_server(&mut socket).await, ServerMessage::Error { code, .. } if code == "protocol_mismatch")
    );
    let bytes = protocol::encode_client(&ClientMessage::CreateRace {
        name: "blocked".into(),
        laps: 1,
        content_identity: PROJECT_CONTENT_ID.into(),
        track_profile_id: "track-profile-project-circuit".into(),
        track_fingerprint: "0".repeat(64),
        vehicle_profile_id: "vehicle-profile-project-default".into(),
        vehicle_fingerprint: "0".repeat(64),
        driver_name: "Host".into(),
        paint_id: "paint-project-red".into(),
        car_model: String::new(),
        car_variant: 0,
        ai_count: 0,
        ai_skill: 1,
        ai_catch_up: true,
    })
    .unwrap();
    let _ = socket.send(Message::Binary(bytes.into())).await;
    let mut observer = open(&url).await;
    let _ = next_server(&mut observer).await;
    let _ = next_server(&mut observer).await;
    send(&mut observer, hello(PROTOCOL_MAJOR)).await;
    assert!(
        matches!(next_server(&mut observer).await, ServerMessage::LobbySnapshot { races, .. } if races.is_empty())
    );
    let _ = observer.close(None).await;
    server.abort();
}

#[tokio::test]
async fn rooms_are_isolated_and_leave_removes_an_empty_room() {
    let (url, server) = start_server().await;
    let mut host = open(&url).await;
    let _ = next_server(&mut host).await;
    let _ = next_server(&mut host).await;
    send(&mut host, hello(PROTOCOL_MAJOR)).await;
    let _ = next_server(&mut host).await;
    send(
        &mut host,
        ClientMessage::CreateRace {
            name: "First room".into(),
            laps: 1,
            content_identity: PROJECT_CONTENT_ID.into(),
            track_profile_id: "track-profile-project-circuit".into(),
            track_fingerprint: "0".repeat(64),
            vehicle_profile_id: "vehicle-profile-project-default".into(),
            vehicle_fingerprint: "0".repeat(64),
            driver_name: "Host".into(),
            paint_id: "paint-project-red".into(),
            car_model: String::new(),
            car_variant: 0,
            ai_count: 0,
            ai_skill: 1,
            ai_catch_up: true,
        },
    )
    .await;
    let first_id = loop {
        if let ServerMessage::RaceCreated { race_id } = next_server(&mut host).await {
            break race_id;
        }
    };
    let mut guest = open(&url).await;
    let _ = next_server(&mut guest).await;
    let _ = next_server(&mut guest).await;
    send(&mut guest, hello(PROTOCOL_MAJOR)).await;
    let _ = next_server(&mut guest).await;
    send(
        &mut guest,
        ClientMessage::CreateRace {
            name: "Second room".into(),
            laps: 1,
            content_identity: PROJECT_CONTENT_ID.into(),
            track_profile_id: "track-profile-project-circuit".into(),
            track_fingerprint: "0".repeat(64),
            vehicle_profile_id: "vehicle-profile-project-default".into(),
            vehicle_fingerprint: "0".repeat(64),
            driver_name: "Guest".into(),
            paint_id: "paint-project-blue".into(),
            car_model: String::new(),
            car_variant: 0,
            ai_count: 0,
            ai_skill: 1,
            ai_catch_up: true,
        },
    )
    .await;
    let second_id = loop {
        if let ServerMessage::RaceCreated { race_id } = next_server(&mut guest).await {
            break race_id;
        }
    };
    assert_ne!(first_id, second_id);
    send(&mut host, ClientMessage::ListRaces).await;
    let races = loop {
        if let ServerMessage::LobbySnapshot { races, .. } = next_server(&mut host).await {
            if races.len() == 2 {
                break races;
            }
        }
    };
    assert_eq!(races.len(), 2);
    send(
        &mut host,
        ClientMessage::JoinRace {
            race_id: second_id,
            content_identity: PROJECT_CONTENT_ID.into(),
            track_profile_id: "track-profile-project-circuit".into(),
            track_fingerprint: "0".repeat(64),
            vehicle_profile_id: "vehicle-profile-project-default".into(),
            vehicle_fingerprint: "0".repeat(64),
            driver_name: "Host".into(),
            paint_id: "paint-project-red".into(),
            car_model: String::new(),
            car_variant: 0,
        },
    )
    .await;
    let rejected = loop {
        if let ServerMessage::JoinRejected { reason } = next_server(&mut host).await {
            break reason;
        }
    };
    assert_eq!(rejected, "already_joined");
    send(&mut host, ClientMessage::LeaveRace).await;
    loop {
        if matches!(next_server(&mut host).await, ServerMessage::LeftRace) {
            break;
        }
    }
    let races = loop {
        if let ServerMessage::LobbySnapshot { races, .. } = next_server(&mut guest).await {
            if races.len() == 1 && races[0].race_id == second_id {
                break races;
            }
        }
    };
    assert_eq!(races.len(), 1);
    assert_eq!(races[0].race_id, second_id);
    let _ = host.close(None).await;
    let _ = guest.close(None).await;
    server.abort();
}

#[tokio::test]
async fn two_player_race_ranks_the_driving_player_ahead() {
    let (url, server) = start_server().await;
    let mut host = open(&url).await;
    let mut guest = open(&url).await;
    for socket in [&mut host, &mut guest] {
        let _ = next_server(socket).await;
        let _ = next_server(socket).await;
        send(socket, hello(PROTOCOL_MAJOR)).await;
        let _ = next_server(socket).await;
    }
    send(
        &mut host,
        ClientMessage::CreateRace {
            name: "Ranking room".into(),
            laps: 1,
            content_identity: PROJECT_CONTENT_ID.into(),
            track_profile_id: "track-profile-project-circuit".into(),
            track_fingerprint: "0".repeat(64),
            vehicle_profile_id: "vehicle-profile-project-default".into(),
            vehicle_fingerprint: "0".repeat(64),
            driver_name: "Host".into(),
            paint_id: "paint-project-red".into(),
            car_model: String::new(),
            car_variant: 0,
            ai_count: 0,
            ai_skill: 1,
            ai_catch_up: true,
        },
    )
    .await;
    let race_id = loop {
        if let ServerMessage::RaceCreated { race_id } = next_server(&mut host).await {
            break race_id;
        }
    };
    send(
        &mut guest,
        ClientMessage::JoinRace {
            race_id,
            content_identity: PROJECT_CONTENT_ID.into(),
            track_profile_id: "track-profile-project-circuit".into(),
            track_fingerprint: "0".repeat(64),
            vehicle_profile_id: "vehicle-profile-project-default".into(),
            vehicle_fingerprint: "0".repeat(64),
            driver_name: "Guest".into(),
            paint_id: "paint-project-blue".into(),
            car_model: String::new(),
            car_variant: 0,
        },
    )
    .await;
    let (guest_id, revision) = loop {
        if let ServerMessage::JoinAccepted {
            player_id,
            selection_revision,
            ..
        } = next_server(&mut guest).await
        {
            break (player_id, selection_revision);
        }
    };
    for socket in [&mut host, &mut guest] {
        send(
            socket,
            ClientMessage::ReadyState {
                ready: true,
                selection_revision: revision,
                track_fingerprint: "0".repeat(64),
                vehicle_templates: vec![protocol::AvailableCarProfile {
                    profile_id: "vehicle-profile-project-default".into(),
                    source_fingerprint: "0".repeat(64),
                }],
            },
        )
        .await;
    }
    // The countdown runs in real time; once racing, only the guest drives.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    let mut sequence = 0u32;
    let mut ranked = None;
    let mut racing = false;
    while tokio::time::Instant::now() < deadline && ranked.is_none() {
        let message = tokio::time::timeout(Duration::from_secs(2), guest.next())
            .await
            .expect("guest should keep receiving server messages")
            .unwrap()
            .unwrap();
        let Message::Binary(bytes) = message else {
            continue;
        };
        if let ServerMessage::StateSnapshot {
            status, vehicles, ..
        } = protocol::decode_server(&bytes).unwrap()
        {
            if matches!(status, protocol::RaceStatus::Racing) {
                racing = true;
                let guest_rank = vehicles
                    .iter()
                    .find(|v| v.player_id == guest_id)
                    .map(|v| v.race_position);
                let host_rank = vehicles
                    .iter()
                    .find(|v| v.player_id != guest_id)
                    .map(|v| v.race_position);
                if vehicles.len() == 2 && guest_rank == Some(1) && host_rank == Some(2) {
                    ranked = Some((guest_rank, host_rank));
                }
            }
        }
        if racing {
            sequence += 1;
            send(
                &mut guest,
                ClientMessage::DriverInput {
                    sequence,
                    client_tick: u64::from(sequence),
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
        }
    }
    assert!(racing, "race should leave the countdown");
    assert_eq!(
        ranked,
        Some((Some(1), Some(2))),
        "the throttling guest must outrank the idle host with distinct positions"
    );
    let _ = host.close(None).await;
    let _ = guest.close(None).await;
    server.abort();
}

type TestSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Reads server messages until `pick` returns a value.
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
