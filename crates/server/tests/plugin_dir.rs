//! The server serves a configured browser-plugin directory at `/plugins/ext/`
//! and advertises it through `/plugins.json`; without one nothing is served.

use racer_server::{ServerConfig, plugin_manifest_json};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::PathBuf,
};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("racer-plugin-dir-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn serve(config: ServerConfig, web_root: PathBuf) -> SocketAddr {
    let app = config.router(web_root).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    address
}

async fn get(address: SocketAddr, path: &'static str) -> (u16, String, String) {
    tokio::task::spawn_blocking(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        let (head, body) = raw.split_once("\r\n\r\n").unwrap();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, head.to_ascii_lowercase(), body.to_owned())
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn configured_plugin_directory_is_served_and_advertised() {
    let web = scratch("web");
    let plugins = scratch("plugins");
    std::fs::write(plugins.join("plugin.json"), r#"{"plugins":["main.js"]}"#).unwrap();
    std::fs::write(plugins.join("main.js"), "export default () => null;\n").unwrap();
    let address = serve(ServerConfig::new().plugin_dir(Some(plugins)), web).await;

    let (status, head, body) = get(address, "/plugins.json").await;
    assert_eq!(status, 200);
    assert!(head.contains("application/json"));
    assert_eq!(body, r#"{"plugins":["/plugins/ext/main.js"]}"#);

    let (status, head, body) = get(address, "/plugins/ext/main.js").await;
    assert_eq!(status, 200);
    assert!(
        head.contains("javascript"),
        "module scripts need a JavaScript type: {head}"
    );
    assert!(body.contains("export default"));

    let (status, _, _) = get(address, "/plugins/ext/missing.js").await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn without_a_plugin_directory_nothing_is_served() {
    let web = scratch("web-none");
    let address = serve(ServerConfig::new().plugin_dir(None), web).await;
    assert_eq!(get(address, "/plugins.json").await.0, 404);
    assert_eq!(get(address, "/plugins/ext/main.js").await.0, 404);
}

#[test]
fn manifest_falls_back_to_index_and_rejects_unsafe_names() {
    let dir = scratch("manifest");
    assert_eq!(plugin_manifest_json(&dir).unwrap(), r#"{"plugins":[]}"#);
    std::fs::write(dir.join("index.js"), "").unwrap();
    assert_eq!(
        plugin_manifest_json(&dir).unwrap(),
        r#"{"plugins":["/plugins/ext/index.js"]}"#
    );
    for bad in ["../x.js", "/abs.js", "a b.js", "a.txt", "a//b.js"] {
        std::fs::write(
            dir.join("plugin.json"),
            format!(r#"{{"plugins":["{bad}"]}}"#),
        )
        .unwrap();
        assert!(
            plugin_manifest_json(&dir).is_err(),
            "{bad} must be rejected"
        );
    }
    std::fs::write(dir.join("plugin.json"), r#"{"plugins":["sub/a.mjs"]}"#).unwrap();
    assert_eq!(
        plugin_manifest_json(&dir).unwrap(),
        r#"{"plugins":["/plugins/ext/sub/a.mjs"]}"#
    );
    assert!(plugin_manifest_json(&dir.join("nope")).is_err());
}
