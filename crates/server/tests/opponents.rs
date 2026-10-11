//! Computer opponents in a server room: setup, snapshots, ranking, motion.

use futures_util::{SinkExt, StreamExt};
use racer_protocol::{
    self as protocol, ClientMessage, MAX_MESSAGE_BYTES, PROJECT_CONTENT_ID, PROTOCOL_MAJOR,
    RaceStatus, ServerMessage,
};
use std::time::Duration;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header::ORIGIN},
};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn start_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = racer_server::router("web");
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("ws://{address}/ws"), task)
}

async fn open(url: &str) -> Socket {
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

async fn next_server(socket: &mut Socket) -> ServerMessage {
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

async fn send(socket: &mut Socket, msg: ClientMessage) {
    let bytes = protocol::encode_client(&msg).unwrap();
    socket.send(Message::Binary(bytes.into())).await.unwrap();
}

fn create(ai_count: u8) -> ClientMessage {
    ClientMessage::CreateRace {
        name: "Opponent race".into(),
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
        ai_count,
        ai_skill: 1,
        ai_catch_up: true,
    }
}

fn ready(revision: u32) -> ClientMessage {
    ClientMessage::ReadyState {
        ready: true,
        selection_revision: revision,
        track_fingerprint: "0".repeat(64),
        vehicle_templates: vec![protocol::AvailableCarProfile {
            profile_id: "vehicle-profile-project-default".into(),
            source_fingerprint: "0".repeat(64),
        }],
    }
}

async fn connect_and_create(url: &str, ai_count: u8) -> (Socket, u32, u32) {
    let mut host = open(url).await;
    let _ = next_server(&mut host).await;
    let _ = next_server(&mut host).await;
    send(
        &mut host,
        ClientMessage::Hello {
            protocol_major: PROTOCOL_MAJOR,
            build_id: "test".into(),
            content_capabilities: vec![PROJECT_CONTENT_ID.into()],
        },
    )
    .await;
    let _ = next_server(&mut host).await;
    send(&mut host, create(ai_count)).await;
    loop {
        if let ServerMessage::JoinAccepted {
            race_id,
            selection_revision,
            ..
        } = next_server(&mut host).await
        {
            return (host, race_id, selection_revision);
        }
    }
}

#[tokio::test]
async fn a_solo_host_races_computer_opponents_that_appear_in_snapshots_and_ranking() {
    let (url, server) = start_server().await;
    let (mut host, race_id, revision) = connect_and_create(&url, 3).await;
    // The lobby reports the requested opponents.
    let summary = loop {
        if let ServerMessage::LobbySnapshot { races, .. } = next_server(&mut host).await {
            if let Some(r) = races.into_iter().find(|r| r.race_id == race_id) {
                break r;
            }
        }
    };
    assert_eq!(
        (summary.ai_count, summary.ai_skill, summary.ai_catch_up),
        (3, 1, true)
    );
    // An out-of-range request is rejected without changing the room.
    send(
        &mut host,
        ClientMessage::ConfigureAi {
            race_id,
            selection_revision: revision,
            ai_count: 9,
            ai_skill: 1,
            ai_catch_up: true,
        },
    )
    .await;
    loop {
        if let ServerMessage::Error { code, .. } = next_server(&mut host).await {
            assert_eq!(code, "invalid_settings");
            break;
        }
    }
    // The host lowers the count: the revision moves on and readiness resets.
    send(
        &mut host,
        ClientMessage::ConfigureAi {
            race_id,
            selection_revision: revision,
            ai_count: 2,
            ai_skill: 0,
            ai_catch_up: false,
        },
    )
    .await;
    let summary = loop {
        if let ServerMessage::LobbySnapshot { races, .. } = next_server(&mut host).await {
            if let Some(r) = races
                .into_iter()
                .find(|r| r.race_id == race_id && r.ai_count == 2)
            {
                break r;
            }
        }
    };
    assert_eq!((summary.ai_skill, summary.ai_catch_up), (0, false));
    assert_eq!(summary.selection_revision, revision + 1);
    send(&mut host, ready(summary.selection_revision)).await;
    // One human is enough to start when opponents are requested.
    let first_racing = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let ServerMessage::StateSnapshot {
                status: RaceStatus::Racing,
                vehicles,
                ..
            } = next_server(&mut host).await
            {
                return vehicles;
            }
        }
    })
    .await
    .expect("race starts");
    assert_eq!(first_racing.len(), 3);
    let ai: Vec<_> = first_racing.iter().filter(|v| v.is_ai).collect();
    assert_eq!(ai.len(), 2);
    assert!(
        ai.iter()
            .all(|v| v.handling_model == protocol::HANDLING_SCRIPTED)
    );
    let mut positions: Vec<u8> = first_racing.iter().map(|v| v.race_position).collect();
    positions.sort_unstable();
    assert_eq!(positions, vec![1, 2, 3]);
    let start: Vec<(u32, f64, f64)> = ai.iter().map(|v| (v.player_id, v.x, v.y)).collect();
    // Opponents drive without any input from the host.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let ServerMessage::StateSnapshot { vehicles, .. } = next_server(&mut host).await {
                let moved = start.iter().all(|(id, x, y)| {
                    vehicles
                        .iter()
                        .find(|v| v.player_id == *id)
                        .is_some_and(|v| (v.x - x).hypot(v.y - y) > 3.0)
                });
                if moved {
                    return;
                }
            }
        }
    })
    .await
    .expect("opponents move");
    let _ = host.close(None).await;
    server.abort();
}

#[tokio::test]
async fn rooms_without_opponents_still_need_two_humans() {
    let (url, server) = start_server().await;
    let (mut host, _race_id, revision) = connect_and_create(&url, 0).await;
    send(&mut host, ready(revision)).await;
    // No countdown starts for a lone human without opponents.
    let started = tokio::time::timeout(Duration::from_millis(600), async {
        loop {
            if let ServerMessage::StateSnapshot { status, .. } = next_server(&mut host).await {
                if !matches!(status, RaceStatus::Lobby) {
                    return true;
                }
            }
        }
    })
    .await;
    assert!(started.is_err() || !started.unwrap());
    let _ = host.close(None).await;
    server.abort();
}

#[tokio::test]
async fn invalid_opponent_settings_are_rejected_at_creation() {
    let (url, server) = start_server().await;
    let mut host = open(&url).await;
    let _ = next_server(&mut host).await;
    let _ = next_server(&mut host).await;
    send(
        &mut host,
        ClientMessage::Hello {
            protocol_major: PROTOCOL_MAJOR,
            build_id: "test".into(),
            content_capabilities: vec![PROJECT_CONTENT_ID.into()],
        },
    )
    .await;
    let _ = next_server(&mut host).await;
    send(&mut host, create(8)).await;
    loop {
        if let ServerMessage::Error { code, .. } = next_server(&mut host).await {
            assert_eq!(code, "invalid_settings");
            break;
        }
    }
    let _ = host.close(None).await;
    server.abort();
}
