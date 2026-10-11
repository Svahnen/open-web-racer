//! Explicit project-owned WebSocket acceptance server.
//!
//! This feature-gated example never reads the private gameplay data root and is
//! not used by the production server or release/container builds.

use racer_server::{GameplayDataStore, router_with_private_assets_and_gameplay};
use std::{net::SocketAddr, path::PathBuf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind: SocketAddr = std::env::var("RACER_BIND")
        .unwrap_or_else(|_| "127.0.0.1:0".to_owned())
        .parse()?;
    let web_root = std::env::var_os("RACER_WEB_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("web"));
    let app = router_with_private_assets_and_gameplay(
        web_root,
        None,
        Some(GameplayDataStore::synthetic_for_tests()),
    )?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let address = listener.local_addr()?;
    println!("SYNTHETIC_READY=http://{address}");
    axum::serve(listener, app).await?;
    Ok(())
}
