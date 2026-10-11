//! A separate crate's vehicle model provider plugs into the stock server and
//! is selected over the generic `SetCarModel` message.

use futures_util::{SinkExt, StreamExt};
use racer_protocol::{
    self as protocol, ClientMessage, PROJECT_CONTENT_ID, PROTOCOL_MAJOR, ServerMessage,
};
use racer_server::{ServerConfig, VehicleModelRegistry};
use std::time::Duration;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::header::ORIGIN},
};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn start_server() -> (String, tokio::task::JoinHandle<()>) {
    let mut models = VehicleModelRegistry::new();
    racer_example_model::register(&mut models).unwrap();
    let app = ServerConfig::new()
        .with_vehicle_models(models)
        .router("web")
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
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
            return protocol::decode_server(&bytes).unwrap();
        }
    }
}

async fn send(socket: &mut Socket, msg: ClientMessage) {
    let bytes = protocol::encode_client(&msg).unwrap();
    socket.send(Message::Binary(bytes.into())).await.unwrap();
}

async fn ack(socket: &mut Socket) -> (String, bool, String) {
    loop {
        if let ServerMessage::CarModelAck {
            model_id,
            accepted,
            reason,
        } = next_server(socket).await
        {
            return (model_id, accepted, reason);
        }
    }
}

#[tokio::test]
async fn registered_provider_model_is_accepted_and_reported_as_external() {
    let (url, server) = start_server().await;
    let mut host = open(&url).await;
    let host_id = match next_server(&mut host).await {
        ServerMessage::Welcome { player_id, .. } => player_id,
        other => panic!("expected welcome, got {other:?}"),
    };
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

    // Before joining: unknown ids and bad params are declined, a good request is stored.
    send(
        &mut host,
        ClientMessage::SetCarModel {
            model_id: "missing.model".into(),
            params: Vec::new(),
        },
    )
    .await;
    let (id, accepted, reason) = ack(&mut host).await;
    assert_eq!(id, "missing.model");
    assert!(!accepted);
    assert!(!reason.is_empty());
    send(
        &mut host,
        ClientMessage::SetCarModel {
            model_id: racer_example_model::MODEL_ID.into(),
            params: b"{\"accel\": -3}".to_vec(),
        },
    )
    .await;
    assert!(!ack(&mut host).await.1);
    send(
        &mut host,
        ClientMessage::SetCarModel {
            model_id: racer_example_model::MODEL_ID.into(),
            params: b"{\"max_speed\": 30}".to_vec(),
        },
    )
    .await;
    let (id, accepted, reason) = ack(&mut host).await;
    assert_eq!(id, racer_example_model::MODEL_ID);
    assert!(accepted, "{reason}");

    send(
        &mut host,
        ClientMessage::CreateRace {
            name: "External model room".into(),
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
            ai_count: 1,
            ai_skill: 1,
            ai_catch_up: true,
        },
    )
    .await;
    let revision = loop {
        if let ServerMessage::LobbySnapshot { races, .. } = next_server(&mut host).await {
            if let Some(r) = races.first() {
                break r.selection_revision;
            }
        }
    };
    send(
        &mut host,
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

    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    let mut model = None;
    let mut sequence = 0u32;
    while tokio::time::Instant::now() < deadline && model.is_none() {
        match next_server(&mut host).await {
            ServerMessage::StateSnapshot {
                status, vehicles, ..
            } if matches!(status, protocol::RaceStatus::Racing) => {
                model = vehicles
                    .iter()
                    .find(|v| v.player_id == host_id)
                    .map(|v| v.handling_model);
                sequence += 1;
                send(
                    &mut host,
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
            _ => {}
        }
    }
    assert_eq!(model, Some(protocol::HANDLING_EXTERNAL));
    let _ = host.close(None).await;
    server.abort();
}
