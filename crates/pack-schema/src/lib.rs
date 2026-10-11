//! Racer Content Pack (RCP) v1: schema types, JSON Schema export and validation.
//!
//! A pack is a directory (or zip) containing `pack.json` plus JSON documents
//! (tracks, cars, vehicles, audio, HUD) and little-endian binary buffer files.
//! See `docs/CONTENT_PACK.md` for the human-readable specification.

pub mod ai;
pub mod audio;
pub mod buffer;
pub mod car;
pub mod common;
pub mod hud;
pub mod track;
pub mod validate;
pub mod vehicle;

pub use ai::AiSet;
pub use audio::AudioSet;
pub use buffer::{AccessorId, BufferBytes, BufferTable};
pub use car::Car;
pub use hud::Hud;
pub use track::Track;
pub use validate::{Issues, ValidationError};
pub use vehicle::{ArcadeVehicleParams, ClassicVehicleParams, EngineSynthParams, VehicleDef};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Value of `pack.json` `schema` for this version.
pub const RCP_SCHEMA_ID: &str = "rcp/1";
/// Name of the manifest file at the pack root.
pub const MANIFEST_FILE: &str = "pack.json";

/// Paths (relative to the pack root) of the documents a pack contains.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentIndex {
    #[serde(default)]
    pub tracks: Vec<String>,
    #[serde(default)]
    pub cars: Vec<String>,
    #[serde(default)]
    pub vehicles: Vec<String>,
    #[serde(default)]
    pub audio: Vec<String>,
    #[serde(default)]
    pub hud: Vec<String>,
    #[serde(default)]
    pub ai: Vec<String>,
}

/// Contents of `pack.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Always `"rcp/1"`.
    pub schema: String,
    /// Lowercase identifier: `a-z`, `0-9`, `-`, `_`, `.`.
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// SPDX licence expression covering the pack contents.
    pub license: String,
    pub author: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub documents: DocumentIndex,
}

/// A fully loaded pack.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pack {
    pub manifest: Option<Manifest>,
    pub tracks: Vec<Track>,
    pub cars: Vec<Car>,
    pub vehicles: Vec<VehicleDef>,
    pub audio: Vec<AudioSet>,
    pub hud: Vec<Hud>,
    pub ai: Vec<AiSet>,
    pub buffers: BufferBytes,
}

/// Failure to read or write a pack.
#[derive(Debug)]
pub enum PackError {
    Missing(String),
    Json(String, serde_json::Error),
    Invalid(ValidationError),
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(p) => write!(f, "pack file {p:?} is missing"),
            Self::Json(p, e) => write!(f, "pack file {p:?} is not valid: {e}"),
            Self::Invalid(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PackError {}

impl Manifest {
    pub fn validate(&self, out: &mut Issues) {
        if self.schema != RCP_SCHEMA_ID {
            out.add(
                "manifest.schema",
                format!("expected {RCP_SCHEMA_ID:?}, found {:?}", self.schema),
            );
        }
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 64
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_.".contains(c));
        if !id_ok {
            out.add("manifest.id", "id must be 1..=64 chars of a-z 0-9 - _ .");
        }
        for (field, v) in [
            ("name", &self.name),
            ("license", &self.license),
            ("author", &self.author),
        ] {
            if v.trim().is_empty() {
                out.add(&format!("manifest.{field}"), "must not be empty");
            }
        }
        let d = &self.documents;
        for (kind, list) in [
            ("tracks", &d.tracks),
            ("cars", &d.cars),
            ("vehicles", &d.vehicles),
            ("audio", &d.audio),
            ("hud", &d.hud),
            ("ai", &d.ai),
        ] {
            for (i, p) in list.iter().enumerate() {
                if !validate::is_safe_relative_path(p) || !p.ends_with(".json") {
                    out.add(
                        &format!("manifest.documents.{kind}[{i}]"),
                        format!("{p:?} must be a relative .json path without '..'"),
                    );
                }
            }
        }
    }
}

impl Pack {
    /// Validate the whole pack, including cross references and buffer ranges.
    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut out = Issues::default();
        match &self.manifest {
            Some(m) => m.validate(&mut out),
            None => out.add("manifest", "pack has no manifest"),
        }
        let b = Some(&self.buffers);
        let mut ids = std::collections::BTreeSet::new();
        for (i, t) in self.tracks.iter().enumerate() {
            if !ids.insert(format!("track:{}", t.id)) {
                out.add(
                    &format!("tracks[{i}].id"),
                    format!("duplicate track id {:?}", t.id),
                );
            }
            t.validate(&format!("tracks[{i}]"), b, &mut out);
        }
        for (i, c) in self.cars.iter().enumerate() {
            if !ids.insert(format!("car:{}", c.id)) {
                out.add(
                    &format!("cars[{i}].id"),
                    format!("duplicate car id {:?}", c.id),
                );
            }
            c.validate(&format!("cars[{i}]"), b, &mut out);
        }
        for (i, v) in self.vehicles.iter().enumerate() {
            if !ids.insert(format!("vehicle:{}", v.id)) {
                out.add(
                    &format!("vehicles[{i}].id"),
                    format!("duplicate vehicle id {:?}", v.id),
                );
            }
            v.validate(&format!("vehicles[{i}]"), &mut out);
            if let Some(car) = &v.car {
                if !self.cars.iter().any(|c| &c.id == car) {
                    out.add(
                        &format!("vehicles[{i}].car"),
                        format!("unknown car {car:?}"),
                    );
                }
            }
        }
        for (i, a) in self.audio.iter().enumerate() {
            a.validate(&format!("audio[{i}]"), b, &mut out);
        }
        for (i, h) in self.hud.iter().enumerate() {
            h.validate(&format!("hud[{i}]"), b, &mut out);
        }
        for (i, a) in self.ai.iter().enumerate() {
            if !ids.insert(format!("ai:{}", a.id)) {
                out.add(
                    &format!("ai[{i}].id"),
                    format!("duplicate ai set id {:?}", a.id),
                );
            }
            a.validate(&format!("ai[{i}]"), &mut out);
        }
        out.finish()
    }

    /// Serialize to `path -> bytes` (pack.json, documents, buffers). Document paths are
    /// regenerated from ids (`tracks/<id>.json`, `audio/<n>.json`, ...).
    pub fn to_files(&self) -> Result<BTreeMap<String, Vec<u8>>, PackError> {
        let mut manifest = self
            .manifest
            .clone()
            .ok_or_else(|| PackError::Missing(MANIFEST_FILE.into()))?;
        let mut files: BTreeMap<String, Vec<u8>> = self.buffers.clone();
        let mut docs = DocumentIndex::default();
        fn put<T: Serialize>(
            files: &mut BTreeMap<String, Vec<u8>>,
            list: &mut Vec<String>,
            path: String,
            v: &T,
        ) -> Result<(), PackError> {
            let bytes =
                serde_json::to_vec_pretty(v).map_err(|e| PackError::Json(path.clone(), e))?;
            files.insert(path.clone(), bytes);
            list.push(path);
            Ok(())
        }
        for t in &self.tracks {
            put(
                &mut files,
                &mut docs.tracks,
                format!("tracks/{}.json", t.id),
                t,
            )?;
        }
        for c in &self.cars {
            put(&mut files, &mut docs.cars, format!("cars/{}.json", c.id), c)?;
        }
        for v in &self.vehicles {
            put(
                &mut files,
                &mut docs.vehicles,
                format!("vehicles/{}.json", v.id),
                v,
            )?;
        }
        for (i, a) in self.audio.iter().enumerate() {
            put(&mut files, &mut docs.audio, format!("audio/{i}.json"), a)?;
        }
        for (i, h) in self.hud.iter().enumerate() {
            put(&mut files, &mut docs.hud, format!("hud/{i}.json"), h)?;
        }
        for a in &self.ai {
            put(&mut files, &mut docs.ai, format!("ai/{}.json", a.id), a)?;
        }
        manifest.documents = docs;
        let bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|e| PackError::Json(MANIFEST_FILE.into(), e))?;
        files.insert(MANIFEST_FILE.into(), bytes);
        Ok(files)
    }

    /// Parse a pack from `path -> bytes`. Does not run [`Pack::validate`].
    pub fn from_files(files: &BTreeMap<String, Vec<u8>>) -> Result<Self, PackError> {
        fn get<T: serde::de::DeserializeOwned>(
            files: &BTreeMap<String, Vec<u8>>,
            path: &str,
        ) -> Result<T, PackError> {
            let bytes = files
                .get(path)
                .ok_or_else(|| PackError::Missing(path.into()))?;
            serde_json::from_slice(bytes).map_err(|e| PackError::Json(path.into(), e))
        }
        let manifest: Manifest = get(files, MANIFEST_FILE)?;
        if manifest.schema != RCP_SCHEMA_ID {
            let mut o = Issues::default();
            manifest.validate(&mut o);
            return Err(PackError::Invalid(o.finish().unwrap_err()));
        }
        let d = manifest.documents.clone();
        let mut pack = Pack::default();
        let mut doc_paths: Vec<&String> = Vec::new();
        for p in &d.tracks {
            pack.tracks.push(get(files, p)?);
            doc_paths.push(p);
        }
        for p in &d.cars {
            pack.cars.push(get(files, p)?);
            doc_paths.push(p);
        }
        for p in &d.vehicles {
            pack.vehicles.push(get(files, p)?);
            doc_paths.push(p);
        }
        for p in &d.audio {
            pack.audio.push(get(files, p)?);
            doc_paths.push(p);
        }
        for p in &d.hud {
            pack.hud.push(get(files, p)?);
            doc_paths.push(p);
        }
        for p in &d.ai {
            pack.ai.push(get(files, p)?);
            doc_paths.push(p);
        }
        for (path, bytes) in files {
            if path != MANIFEST_FILE && !doc_paths.contains(&path) {
                pack.buffers.insert(path.clone(), bytes.clone());
            }
        }
        pack.manifest = Some(manifest);
        Ok(pack)
    }
}

/// JSON Schemas for each RCP v1 document type, keyed by file role.
pub fn json_schemas() -> BTreeMap<&'static str, serde_json::Value> {
    fn s<T: JsonSchema>() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(T)).expect("schema serializes")
    }
    BTreeMap::from([
        ("manifest", s::<Manifest>()),
        ("track", s::<Track>()),
        ("car", s::<Car>()),
        ("vehicle", s::<VehicleDef>()),
        ("audio", s::<AudioSet>()),
        ("hud", s::<Hud>()),
        ("ai", s::<AiSet>()),
    ])
}

#[cfg(test)]
mod tests;
