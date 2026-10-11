//! Deterministic generator for the project-owned demo cars.
//!
//! [`generate`] returns every file of the demo-car content folder
//! (`content/demo/cars`): an RCP v1 pack (`pack.json`, `cars/`, `vehicles/`,
//! `buffers/`) and one glTF 2.0 file per car under `gltf/`. Everything is
//! computed from the parameters in [`design`]; no external model or texture is
//! used and the output is byte-identical on every run.

pub mod design;
pub mod geom;
pub mod gltf;
pub mod mesh;
pub mod palette;
pub mod rcp;

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub use design::{DESIGNS, Design};

/// Path to a file inside the content folder (forward slashes) and its bytes.
pub type Files = BTreeMap<String, Vec<u8>>;

/// All generated files, keyed by path relative to the content folder.
pub fn generate() -> Files {
    let pack = rcp::pack();
    pack.validate().expect("generated pack validates");
    let mut files = pack.to_files().expect("pack serializes");
    // Text files end with a newline.
    for (path, bytes) in &mut files {
        if path.ends_with(".json") && bytes.last() != Some(&b'\n') {
            bytes.push(b'\n');
        }
    }
    for d in DESIGNS {
        files.insert(format!("gltf/{}.gltf", d.id), gltf::export(d).into_bytes());
    }
    files
}

/// SHA-256 over all paths and contents (sorted by path), as lowercase hex.
pub fn digest(files: &Files) -> String {
    let mut h = Sha256::new();
    for (path, bytes) in files {
        h.update((path.len() as u64).to_le_bytes());
        h.update(path.as_bytes());
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(bytes);
    }
    hex(&h.finalize())
}

/// SHA-256 of one file, as lowercase hex.
pub fn file_digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
