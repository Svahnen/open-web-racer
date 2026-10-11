//! Generated project tracks as selectable room tracks over the real WebSocket path.

use futures_util::{SinkExt, StreamExt};
use racer_content::project_tracks::{
    PROJECT_TRACK_CONTENT_ID, project_track_catalog, project_track_profile_id,
};
use racer_protocol::{
    self as protocol, ClientMessage, MAX_MESSAGE_BYTES, PROJECT_CONTENT_ID, PROTOCOL_MAJOR,
    ServerMessage,
};
use std::time::Duration;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header::ORIGIN},
};

type TestSocket =
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

fn selection(track: &str) -> (String, String) {
    let info = project_track_catalog()
        .into_iter()
        .find(|t| t.id == track)
        .expect("catalog track");
    (project_track_profile_id(&info.id), info.fingerprint)
}

fn create_on(profile: &str, fingerprint: &str, content: &str, laps: u8) -> ClientMessage {
    ClientMessage::CreateRace {
        name: "Generated room".into(),
        laps,
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

#[tokio::test]
async fn generated_track_rooms_validate_selection_and_race_with_gates() {
    tokio::task::spawn_blocking(racer_server::warm_project_tracks)
        .await
        .unwrap();
    let (url, server) = start_server().await;
    let mut host = open(&url).await;
    let mut guest = open(&url).await;
    for socket in [&mut host, &mut guest] {
        let _ = next_server(socket).await;
        let _ = next_server(socket).await;
        send(socket, hello()).await;
        let _ = next_server(socket).await;
    }
    let (profile, fingerprint) = selection("red-mesa");
    let content = PROJECT_TRACK_CONTENT_ID;

    // wrong fingerprint, wrong content identity, unknown track and bad laps are refused
    let wrong_print = "f".repeat(64);
    for (message, expected) in [
        (
            create_on(&profile, &wrong_print, content, 2),
            "content_mismatch",
        ),
        (
            create_on(&profile, &fingerprint, PROJECT_CONTENT_ID, 2),
            "content_mismatch",
        ),
        (
            create_on("track-profile-rcp-nowhere", &fingerprint, content, 2),
            "profile_unavailable",
        ),
        (
            create_on(&profile, &fingerprint, content, 0),
            "content_mismatch",
        ),
    ] {
        send(&mut host, message).await;
        let code = wait_for(&mut host, |m| match m {
            ServerMessage::Error { code, .. } => Some(code),
            _ => None,
        })
        .await;
        assert_eq!(code, expected);
    }

    send(&mut host, create_on(&profile, &fingerprint, content, 2)).await;
    let race_id = wait_for(&mut host, |m| match m {
        ServerMessage::RaceCreated { race_id } => Some(race_id),
        _ => None,
    })
    .await;
    // a guest with the circuit identity cannot join a generated-track room
    send(
        &mut guest,
        join(
            race_id,
            "track-profile-project-circuit",
            &"0".repeat(64),
            PROJECT_CONTENT_ID,
        ),
    )
    .await;
    let rejected = wait_for(&mut guest, |m| match m {
        ServerMessage::JoinRejected { reason } => Some(reason),
        _ => None,
    })
    .await;
    assert_eq!(rejected, "content_mismatch");
    send(&mut guest, join(race_id, &profile, &fingerprint, content)).await;
    let (guest_id, revision) = wait_for(&mut guest, |m| match m {
        ServerMessage::JoinAccepted {
            player_id,
            selection_revision,
            content_requirement,
            track_profile_id,
            ..
        } => {
            assert_eq!(content_requirement, PROJECT_TRACK_CONTENT_ID);
            assert!(track_profile_id.starts_with("track-profile-rcp-"));
            Some((player_id, selection_revision))
        }
        _ => None,
    })
    .await;
    for socket in [&mut host, &mut guest] {
        send(
            socket,
            ClientMessage::ReadyState {
                ready: true,
                selection_revision: revision,
                track_fingerprint: fingerprint.clone(),
                vehicle_templates: vec![protocol::AvailableCarProfile {
                    profile_id: "vehicle-profile-project-default".into(),
                    source_fingerprint: "0".repeat(64),
                }],
            },
        )
        .await;
    }
    // Real-time countdown, then only the guest drives along the start straight.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(40);
    let mut sequence = 0u32;
    let mut racing = false;
    let mut outcome = None;
    let mut start_positions = None;
    while tokio::time::Instant::now() < deadline && outcome.is_none() {
        let message = tokio::time::timeout(Duration::from_secs(5), guest.next())
            .await
            .expect("guest keeps receiving server messages")
            .unwrap()
            .unwrap();
        let Message::Binary(bytes) = message else {
            continue;
        };
        if let ServerMessage::StateSnapshot {
            status, vehicles, ..
        } = protocol::decode_server(&bytes).unwrap()
        {
            if vehicles.len() == 2 && start_positions.is_none() {
                let mut v: Vec<_> = vehicles.iter().map(|v| (v.player_id, v.x, v.y)).collect();
                v.sort_by_key(|e| e.0);
                start_positions = Some(v);
            }
            if matches!(status, protocol::RaceStatus::Racing) {
                racing = true;
                if let Some(me) = vehicles.iter().find(|v| v.player_id == guest_id) {
                    if me.speed > 25.0 && me.x > 60.0 {
                        let other = vehicles.iter().find(|v| v.player_id != guest_id).unwrap();
                        outcome = Some((
                            me.race_position,
                            other.race_position,
                            me.z,
                            me.ground_normal[2],
                        ));
                    }
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
    assert!(racing, "the generated-track race leaves the countdown");
    let positions = start_positions.expect("both cars are in the snapshot");
    assert_ne!(
        (positions[0].1, positions[0].2),
        (positions[1].1, positions[1].2),
        "players start in different grid slots"
    );
    let (guest_rank, host_rank, z, normal_z) =
        outcome.expect("guest accelerates down the start straight");
    assert_eq!((guest_rank, host_rank), (1, 2));
    assert!(z > 10.0, "grounded on the plateau road (z {z})");
    assert!(normal_z > 0.9);
    let _ = host.close(None).await;
    let _ = guest.close(None).await;
    server.abort();
}

#[tokio::test]
async fn computer_opponents_drive_a_generated_track_room() {
    tokio::task::spawn_blocking(racer_server::warm_project_tracks)
        .await
        .unwrap();
    let (url, server) = start_server().await;
    let mut host = open(&url).await;
    let _ = next_server(&mut host).await;
    let _ = next_server(&mut host).await;
    send(&mut host, hello()).await;
    let _ = next_server(&mut host).await;
    let (profile, fingerprint) = selection("night-harbour");
    let mut create = create_on(&profile, &fingerprint, PROJECT_TRACK_CONTENT_ID, 1);
    if let ClientMessage::CreateRace { ai_count, .. } = &mut create {
        *ai_count = 2;
    }
    send(&mut host, create).await;
    let revision = wait_for(&mut host, |m| match m {
        ServerMessage::JoinAccepted {
            selection_revision, ..
        } => Some(selection_revision),
        _ => None,
    })
    .await;
    send(
        &mut host,
        ClientMessage::ReadyState {
            ready: true,
            selection_revision: revision,
            track_fingerprint: fingerprint.clone(),
            vehicle_templates: vec![protocol::AvailableCarProfile {
                profile_id: "vehicle-profile-project-default".into(),
                source_fingerprint: "0".repeat(64),
            }],
        },
    )
    .await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(40);
    let mut moving = None;
    while tokio::time::Instant::now() < deadline && moving.is_none() {
        let message = tokio::time::timeout(Duration::from_secs(5), host.next())
            .await
            .expect("snapshots keep arriving")
            .unwrap()
            .unwrap();
        let Message::Binary(bytes) = message else {
            continue;
        };
        if let ServerMessage::StateSnapshot {
            status, vehicles, ..
        } = protocol::decode_server(&bytes).unwrap()
        {
            if matches!(status, protocol::RaceStatus::Racing) && vehicles.len() == 3 {
                let fast = vehicles
                    .iter()
                    .filter(|v| v.player_id >= 1_000_000_000 && v.speed > 8.0)
                    .count();
                if fast == 2 {
                    moving = Some(vehicles.iter().map(|v| v.z).fold(f64::MAX, f64::min));
                }
            }
        }
    }
    let min_z = moving.expect("two computer opponents accelerate on the generated track");
    assert!(
        min_z > 1.0,
        "opponents are grounded on the road (z {min_z})"
    );
    let _ = host.close(None).await;
    server.abort();
}

fn with_car(mut message: ClientMessage, model: &str, variant: u8) -> ClientMessage {
    match &mut message {
        ClientMessage::CreateRace {
            car_model,
            car_variant,
            ..
        }
        | ClientMessage::JoinRace {
            car_model,
            car_variant,
            ..
        } => {
            *car_model = model.into();
            *car_variant = variant;
        }
        _ => unreachable!(),
    }
    message
}

async fn expect_error(socket: &mut TestSocket) -> String {
    wait_for(socket, |m| match m {
        ServerMessage::Error { code, .. } => Some(code),
        _ => None,
    })
    .await
}

#[tokio::test]
async fn every_player_sees_the_cars_everybody_picked_and_the_servers_opponent_cars() {
    use racer_content::project_cars::project_cars;
    tokio::task::spawn_blocking(racer_server::warm_project_tracks)
        .await
        .unwrap();
    let (url, server) = start_server().await;
    let mut host = open(&url).await;
    let mut guest = open(&url).await;
    for socket in [&mut host, &mut guest] {
        let _ = next_server(socket).await;
        let _ = next_server(socket).await;
        send(socket, hello()).await;
        let _ = next_server(socket).await;
    }
    let (profile, fingerprint) = selection("night-harbour");
    let content = PROJECT_TRACK_CONTENT_ID;

    // Unknown models and paint schemes outside the catalogue are refused.
    let too_long = "a".repeat(protocol::MAX_CAR_MODEL_BYTES + 1);
    for (model, variant) in [
        ("no-such-car", 0u8),
        ("shard-gt", 200),
        ("", 3),
        (too_long.as_str(), 0),
    ] {
        send(
            &mut host,
            with_car(
                create_on(&profile, &fingerprint, content, 1),
                model,
                variant,
            ),
        )
        .await;
        assert_eq!(expect_error(&mut host).await, "invalid_settings");
    }

    let mut create = with_car(create_on(&profile, &fingerprint, content, 1), "shard-gt", 5);
    if let ClientMessage::CreateRace { ai_count, .. } = &mut create {
        *ai_count = 3;
    }
    send(&mut host, create).await;
    let race_id = wait_for(&mut host, |m| match m {
        ServerMessage::RaceCreated { race_id } => Some(race_id),
        _ => None,
    })
    .await;
    send(
        &mut guest,
        with_car(
            join(race_id, &profile, &fingerprint, content),
            "tumble-rally",
            1,
        ),
    )
    .await;
    let (guest_id, revision) = wait_for(&mut guest, |m| match m {
        ServerMessage::JoinAccepted {
            player_id,
            selection_revision,
            ..
        } => Some((player_id, selection_revision)),
        _ => None,
    })
    .await;

    // The guest changes its mind in the lobby; the change is validated too.
    let change = |model: &str, variant: u8, revision: u32| ClientMessage::ChangeVehicle {
        race_id,
        selection_revision: revision,
        vehicle_profile_id: "vehicle-profile-project-default".into(),
        vehicle_fingerprint: "0".repeat(64),
        paint_id: "paint-project-blue".into(),
        car_model: model.into(),
        car_variant: variant,
    };
    send(&mut guest, change("nope", 0, revision)).await;
    assert_eq!(expect_error(&mut guest).await, "invalid_settings");
    send(&mut guest, change("brisk-roadster", 4, revision)).await;
    let revision = wait_for(&mut guest, |m| match m {
        ServerMessage::StateSnapshot {
            selection_revision,
            vehicles,
            ..
        } if selection_revision != revision
            && vehicles.iter().any(|v| v.car_model == "brisk-roadster") =>
        {
            Some(selection_revision)
        }
        _ => None,
    })
    .await;

    for socket in [&mut host, &mut guest] {
        send(
            socket,
            ClientMessage::ReadyState {
                ready: true,
                selection_revision: revision,
                track_fingerprint: fingerprint.clone(),
                vehicle_templates: vec![protocol::AvailableCarProfile {
                    profile_id: "vehicle-profile-project-default".into(),
                    source_fingerprint: "0".repeat(64),
                }],
            },
        )
        .await;
    }
    // Both players receive identical car assignments for all five cars.
    let mut seen = Vec::new();
    for socket in [&mut host, &mut guest] {
        let vehicles = wait_for(socket, |m| match m {
            ServerMessage::StateSnapshot { vehicles, .. } if vehicles.len() == 5 => Some(vehicles),
            _ => None,
        })
        .await;
        let mut picks: Vec<_> = vehicles
            .iter()
            .map(|v| (v.player_id, v.car_model.clone(), v.car_variant))
            .collect();
        picks.sort();
        seen.push(picks);
    }
    assert_eq!(seen[0], seen[1], "all clients agree on every car");
    let picks = &seen[0];
    let guest_pick = picks.iter().find(|p| p.0 == guest_id).unwrap();
    assert_eq!((guest_pick.1.as_str(), guest_pick.2), ("brisk-roadster", 4));
    let humans: Vec<_> = picks.iter().filter(|p| p.0 < 1_000_000_000).collect();
    assert_eq!(humans.len(), 2);
    assert!(
        humans
            .iter()
            .any(|p| (p.1.as_str(), p.2) == ("shard-gt", 5))
    );

    // Opponent cars follow the host's pick through the catalogue rule.
    let cars = project_cars();
    let base = (cars.index_of("shard-gt").unwrap(), 5);
    let ai: Vec<_> = picks.iter().filter(|p| p.0 >= 1_000_000_000).collect();
    assert_eq!(ai.len(), 3);
    for (k, pick) in ai.iter().enumerate() {
        let (car, paint) = cars.opponent_pick(base, k);
        assert_eq!(
            (pick.1.as_str(), usize::from(pick.2)),
            (cars.cars()[car].id.as_str(), paint)
        );
    }
    let _ = host.close(None).await;
    let _ = guest.close(None).await;
    server.abort();
}
