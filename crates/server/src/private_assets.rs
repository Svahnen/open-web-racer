//! Optional provider for selected, already-derived render assets.
//!
//! It is disabled unless the server is started with an explicit external root
//! and exact private-client IP allowlist. The manifest maps opaque IDs to
//! relative files; a request can never choose a filesystem path.

use axum::{
    Json,
    body::Body,
    extract::{ConnectInfo, Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    net::{IpAddr, SocketAddr},
    path::{Component, Path as FsPath, PathBuf},
};

const MANIFEST_NAME: &str = "render-assets-v1.json";
const MAX_MANIFEST_BYTES: u64 = 512 * 1024;
const MAX_ASSETS: usize = 512;
const MAX_ASSET_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SCENE_DESCRIPTOR_BYTES: u64 = 48 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct PrivateAssetConfig {
    pub root: PathBuf,
    pub repository_root: PathBuf,
    pub allowed_clients: HashSet<IpAddr>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DiskManifest {
    schema_version: u32,
    #[serde(default)]
    scene_asset_id: Option<String>,
    assets: Vec<DiskAsset>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DiskAsset {
    id: String,
    path: String,
    sha256: String,
    bytes: u64,
    mime: String,
}

#[derive(Clone, Debug, Serialize)]
struct PublicManifest {
    schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    scene_asset_id: Option<String>,
    assets: Vec<PublicAsset>,
}

#[derive(Clone, Debug, Serialize)]
struct PublicAsset {
    id: String,
    sha256: String,
    bytes: u64,
    mime: String,
    url: String,
}

pub(super) struct PrivateAssetStore {
    root: PathBuf,
    allowed_clients: HashSet<IpAddr>,
    assets: HashMap<String, DiskAsset>,
    scene_asset_id: Option<String>,
}

impl PrivateAssetStore {
    pub(super) fn open(config: PrivateAssetConfig) -> Result<Self, String> {
        if config.allowed_clients.is_empty()
            || config
                .allowed_clients
                .iter()
                .any(|ip| !is_private_or_loopback(*ip))
        {
            return Err(
                "private asset provider requires an exact private-client IP allowlist".into(),
            );
        }
        let root_metadata = fs::symlink_metadata(&config.root)
            .map_err(|e| format!("private asset root metadata: {e}"))?;
        if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
            return Err("private asset root must be a real directory".into());
        }
        let root = fs::canonicalize(&config.root)
            .map_err(|e| format!("cannot resolve private asset root: {e}"))?;
        let repo = fs::canonicalize(&config.repository_root)
            .map_err(|e| format!("cannot resolve repository root: {e}"))?;
        if !root.is_dir() || root.starts_with(&repo) {
            return Err("private asset root must be a directory outside the repository".into());
        }
        let manifest_path = root.join(MANIFEST_NAME);
        ensure_regular_no_reparse(&manifest_path)?;
        let metadata =
            fs::metadata(&manifest_path).map_err(|e| format!("manifest metadata: {e}"))?;
        if metadata.len() > MAX_MANIFEST_BYTES {
            return Err("private asset manifest exceeds size limit".into());
        }
        let bytes =
            fs::read(&manifest_path).map_err(|e| format!("read private asset manifest: {e}"))?;
        let disk: DiskManifest = serde_json::from_slice(&bytes)
            .map_err(|e| format!("invalid private asset manifest: {e}"))?;
        if disk.schema_version != 1 || disk.assets.len() > MAX_ASSETS {
            return Err("unsupported or oversized private asset manifest".into());
        }
        let mut assets = HashMap::with_capacity(disk.assets.len());
        for asset in disk.assets {
            validate_asset(&root, &asset)?;
            if assets.insert(asset.id.clone(), asset).is_some() {
                return Err("duplicate private asset ID".into());
            }
        }
        if let Some(scene_id) = &disk.scene_asset_id {
            if !valid_id(scene_id) {
                return Err("invalid private scene asset ID".into());
            }
            let scene = assets
                .get(scene_id)
                .ok_or("private scene asset ID is not in the allowlist")?;
            if scene.mime != "application/json" || scene.bytes > MAX_SCENE_DESCRIPTOR_BYTES {
                return Err("private scene asset must be a bounded JSON descriptor".into());
            }
        }
        Ok(Self {
            root,
            allowed_clients: config.allowed_clients,
            assets,
            scene_asset_id: disk.scene_asset_id,
        })
    }

    fn authorized(&self, peer: SocketAddr) -> bool {
        let ip = match peer.ip() {
            IpAddr::V6(ip) => ip
                .to_ipv4_mapped()
                .map(IpAddr::V4)
                .unwrap_or(IpAddr::V6(ip)),
            ip => ip,
        };
        self.allowed_clients.contains(&ip)
    }

    fn public_manifest(&self) -> PublicManifest {
        let mut assets = self
            .assets
            .values()
            .map(|asset| PublicAsset {
                id: asset.id.clone(),
                sha256: asset.sha256.clone(),
                bytes: asset.bytes,
                mime: asset.mime.clone(),
                url: format!("/private-assets/{}", asset.id),
            })
            .collect::<Vec<_>>();
        assets.sort_by(|a, b| a.id.cmp(&b.id));
        PublicManifest {
            schema_version: 1,
            scene_asset_id: self.scene_asset_id.clone(),
            assets,
        }
    }

    fn read_verified(&self, id: &str) -> Result<(Vec<u8>, String), StatusCode> {
        let asset = self.assets.get(id).ok_or(StatusCode::NOT_FOUND)?;
        let path = self.root.join(&asset.path);
        ensure_no_reparse_path(&self.root, FsPath::new(&asset.path))
            .map_err(|_| StatusCode::FORBIDDEN)?;
        let canonical = fs::canonicalize(&path).map_err(|_| StatusCode::NOT_FOUND)?;
        if !canonical.starts_with(&self.root) {
            return Err(StatusCode::FORBIDDEN);
        }
        let metadata = fs::metadata(&canonical).map_err(|_| StatusCode::NOT_FOUND)?;
        if metadata.len() != asset.bytes || metadata.len() > MAX_ASSET_BYTES {
            return Err(StatusCode::CONFLICT);
        }
        let bytes = fs::read(canonical).map_err(|_| StatusCode::NOT_FOUND)?;
        if sha256_hex(&bytes) != asset.sha256 {
            return Err(StatusCode::CONFLICT);
        }
        Ok((bytes, asset.mime.clone()))
    }
}

pub(super) async fn manifest(
    State(state): State<super::AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    let Some(store) = &state.1 else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !store.authorized(peer) {
        return StatusCode::FORBIDDEN.into_response();
    }
    Json(store.public_manifest()).into_response()
}

pub(super) async fn asset(
    State(state): State<super::AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
) -> Response {
    let Some(store) = &state.1 else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !store.authorized(peer) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !valid_id(&id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match store.read_verified(&id) {
        Ok((bytes, mime)) => {
            let mut response = Response::new(Body::from(bytes));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(&mime).expect("MIME validated while loading manifest"),
            );
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=3600, immutable"),
            );
            response.headers_mut().insert(
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            );
            response
        }
        Err(status) => status.into_response(),
    }
}

fn validate_asset(root: &FsPath, asset: &DiskAsset) -> Result<(), String> {
    if !valid_id(&asset.id)
        || asset.bytes == 0
        || asset.bytes > MAX_ASSET_BYTES
        || !valid_sha256(&asset.sha256)
        || !valid_mime_path(&asset.mime, &asset.path)
    {
        return Err("invalid private asset record".into());
    }
    let relative = FsPath::new(&asset.path);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("asset paths must be relative and traversal-free".into());
    }
    let path = root.join(relative);
    ensure_no_reparse_path(root, relative)?;
    let canonical = fs::canonicalize(&path).map_err(|e| format!("asset path: {e}"))?;
    if !canonical.starts_with(root) {
        return Err("asset path resolves outside private root".into());
    }
    let metadata = fs::metadata(canonical).map_err(|e| format!("asset metadata: {e}"))?;
    if !metadata.is_file() || metadata.len() != asset.bytes {
        return Err("asset type or size does not match manifest".into());
    }
    Ok(())
}

fn ensure_regular_no_reparse(path: &FsPath) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("path metadata: {e}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("private asset entries must be regular files, not links".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("private asset entries cannot be reparse points".into());
        }
    }
    Ok(())
}

fn ensure_no_reparse_path(root: &FsPath, relative: &FsPath) -> Result<(), String> {
    let mut current = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(part) = component else {
            return Err("asset paths must be traversal-free".into());
        };
        current.push(part);
        let metadata =
            fs::symlink_metadata(&current).map_err(|e| format!("asset path metadata: {e}"))?;
        if metadata.file_type().is_symlink() {
            return Err("private asset paths cannot contain links".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("private asset paths cannot contain reparse points".into());
            }
        }
        let last = index + 1 == components.len();
        if (last && !metadata.is_file()) || (!last && !metadata.is_dir()) {
            return Err("private asset path has an invalid component".into());
        }
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid_mime_path(mime: &str, path: &str) -> bool {
    let extension = FsPath::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        (mime, extension.as_str()),
        ("application/vnd.racer.mesh-v1", "prmesh")
            | ("application/json", "json")
            | ("image/png", "png")
            | ("image/jpeg", "jpg" | "jpeg")
            | ("image/webp", "webp")
    )
}

fn is_private_or_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|v4| v4.is_private() || v4.is_loopback())
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    /// A private-range peer address (built from octets, never written as text).
    fn lan_peer(host: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, host))
    }

    /// A documentation-range address, which is neither private nor loopback.
    fn public_peer() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7))
    }
    use std::{
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new(label: &str) -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("racer-assets-{label}-{stamp}"));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn write(&self, rel: &str, bytes: &[u8]) {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
    }
    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture_record(id: &str, path: &str, bytes: &[u8], mime: &str) -> DiskAsset {
        DiskAsset {
            id: id.into(),
            path: path.into(),
            sha256: sha256_hex(bytes),
            bytes: bytes.len() as u64,
            mime: mime.into(),
        }
    }

    #[test]
    fn manifest_and_asset_are_opaque_and_hash_verified() {
        let root = TestRoot::new("valid");
        let bytes = b"synthetic mesh bytes";
        root.write("render/triangle.prmesh", bytes);
        let record = fixture_record(
            "synthetic-track",
            "render/triangle.prmesh",
            bytes,
            "application/vnd.racer.mesh-v1",
        );
        root.write(
            MANIFEST_NAME,
            &serde_json::to_vec(&DiskManifest {
                schema_version: 1,
                scene_asset_id: None,
                assets: vec![record],
            })
            .unwrap(),
        );
        let store = PrivateAssetStore::open(PrivateAssetConfig {
            root: root.0.clone(),
            repository_root: std::env::current_dir().unwrap(),
            allowed_clients: HashSet::from([lan_peer(5)]),
        })
        .unwrap();
        assert_eq!(
            store.public_manifest().assets[0].url,
            "/private-assets/synthetic-track"
        );
        assert!(
            !serde_json::to_string(&store.public_manifest())
                .unwrap()
                .contains("triangle.prmesh")
        );
        assert_eq!(store.read_verified("synthetic-track").unwrap().0, bytes);
        assert!(store.read_verified("../triangle.prmesh").is_err());
        assert!(!store.authorized(SocketAddr::new(lan_peer(6), 4000)));
        assert!(store.authorized(SocketAddr::new(lan_peer(5), 4000)));
        assert!(store.authorized(SocketAddr::new(
            IpAddr::V6(Ipv4Addr::new(10, 0, 0, 5).to_ipv6_mapped()),
            4000
        )));
    }

    #[test]
    fn private_manifest_exposes_only_an_explicit_bounded_scene_role() {
        let root = TestRoot::new("scene-role");
        let descriptor = b"{}";
        root.write("scene.prscene.json", descriptor);
        root.write("track.png", b"synthetic image");
        let scene = fixture_record(
            "scene-primary",
            "scene.prscene.json",
            descriptor,
            "application/json",
        );
        let texture = fixture_record("track-page", "track.png", b"synthetic image", "image/png");
        root.write(
            MANIFEST_NAME,
            &serde_json::to_vec(&DiskManifest {
                schema_version: 1,
                scene_asset_id: Some("scene-primary".into()),
                assets: vec![scene, texture],
            })
            .unwrap(),
        );
        let store = PrivateAssetStore::open(PrivateAssetConfig {
            root: root.0.clone(),
            repository_root: std::env::current_dir().unwrap(),
            allowed_clients: HashSet::from([lan_peer(5)]),
        })
        .unwrap();
        let public = store.public_manifest();
        assert_eq!(public.scene_asset_id.as_deref(), Some("scene-primary"));
        assert_eq!(public.assets.len(), 2);

        let mut disk: DiskManifest =
            serde_json::from_slice(&fs::read(root.0.join(MANIFEST_NAME)).unwrap()).unwrap();
        disk.scene_asset_id = Some("track-page".into());
        root.write(MANIFEST_NAME, &serde_json::to_vec(&disk).unwrap());
        assert!(
            PrivateAssetStore::open(PrivateAssetConfig {
                root: root.0.clone(),
                repository_root: std::env::current_dir().unwrap(),
                allowed_clients: HashSet::from([lan_peer(5)]),
            })
            .is_err()
        );
        disk.scene_asset_id = Some("missing-scene".into());
        root.write(MANIFEST_NAME, &serde_json::to_vec(&disk).unwrap());
        assert!(
            PrivateAssetStore::open(PrivateAssetConfig {
                root: root.0.clone(),
                repository_root: std::env::current_dir().unwrap(),
                allowed_clients: HashSet::from([lan_peer(5)]),
            })
            .is_err()
        );
    }

    #[test]
    fn changed_bytes_and_source_like_files_are_rejected() {
        let root = TestRoot::new("tamper");
        let bytes = b"first";
        root.write("asset.png", bytes);
        let record = fixture_record("car-texture", "asset.png", bytes, "image/png");
        root.write(
            MANIFEST_NAME,
            &serde_json::to_vec(&DiskManifest {
                schema_version: 1,
                scene_asset_id: None,
                assets: vec![record],
            })
            .unwrap(),
        );
        let store = PrivateAssetStore::open(PrivateAssetConfig {
            root: root.0.clone(),
            repository_root: std::env::current_dir().unwrap(),
            allowed_clients: HashSet::from(["127.0.0.1".parse().unwrap()]),
        })
        .unwrap();
        fs::write(root.0.join("asset.png"), b"other").unwrap();
        assert_eq!(
            store.read_verified("car-texture").unwrap_err(),
            StatusCode::CONFLICT
        );
        assert!(!valid_mime_path("application/json", "disc.iso"));
        assert!(!valid_mime_path("application/json", "source.cue"));
        assert!(!valid_mime_path("application/json", "game.exe"));
    }

    #[test]
    fn configuration_requires_root_outside_repository_and_private_peers() {
        let root = TestRoot::new("config");
        root.write(MANIFEST_NAME, br#"{"schema_version":1,"assets":[]}"#);
        let config = PrivateAssetConfig {
            root: root.0.clone(),
            repository_root: root.0.clone(),
            allowed_clients: HashSet::from([public_peer()]),
        };
        assert!(PrivateAssetStore::open(config).is_err());
        assert!(!is_private_or_loopback(public_peer()));
    }

    #[tokio::test]
    async fn http_manifest_requires_allowlisted_peer_ip() {
        let root = TestRoot::new("http-gate");
        let bytes = b"synthetic";
        root.write("track.prmesh", bytes);
        root.write(
            MANIFEST_NAME,
            &serde_json::to_vec(&DiskManifest {
                schema_version: 1,
                scene_asset_id: None,
                assets: vec![fixture_record(
                    "track",
                    "track.prmesh",
                    bytes,
                    "application/vnd.racer.mesh-v1",
                )],
            })
            .unwrap(),
        );
        let store = Arc::new(
            PrivateAssetStore::open(PrivateAssetConfig {
                root: root.0.clone(),
                repository_root: std::env::current_dir().unwrap(),
                allowed_clients: HashSet::from([lan_peer(5)]),
            })
            .unwrap(),
        );
        let state = crate::AppState(
            Arc::new(tokio::sync::Mutex::new(crate::Hub::default())),
            Some(store),
            None,
        );
        let denied = manifest(
            State(state.clone()),
            ConnectInfo(SocketAddr::new(lan_peer(6), 4000)),
        )
        .await;
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let allowed = manifest(
            State(state),
            ConnectInfo(SocketAddr::new(lan_peer(5), 4000)),
        )
        .await;
        assert_eq!(allowed.status(), StatusCode::OK);
    }
}
