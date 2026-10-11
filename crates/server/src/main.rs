use racer_server::{PrivateAssetConfig, ServerConfig};
use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address: SocketAddr = std::env::var("RACER_BIND")
        .unwrap_or_else(|_| "127.0.0.1:4173".to_owned())
        .parse()?;
    std::thread::spawn(racer_server::warm_project_tracks);
    let app = ServerConfig::new()
        .private_assets(private_asset_config()?)
        .router("web")?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    let address = listener.local_addr()?;
    println!("racer-server listening on http://{address}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn private_asset_config() -> Result<Option<PrivateAssetConfig>, Box<dyn std::error::Error>> {
    let Some(root) = std::env::var_os("RACER_PRIVATE_ASSET_ROOT") else {
        return Ok(None);
    };
    let clients = std::env::var("RACER_PRIVATE_ASSET_CLIENTS")?;
    let allowed_clients = clients
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::parse::<IpAddr>)
        .collect::<Result<HashSet<_>, _>>()?;
    Ok(Some(PrivateAssetConfig {
        root: root.into(),
        repository_root: std::env::current_dir()?,
        allowed_clients,
    }))
}
