//! Extension point for content ingestion.
//!
//! A [`ContentProvider`] turns a set of files (a directory listing, an archive
//! already unpacked in memory, or browser-selected files) into a validated
//! [`racer_pack_schema::Pack`]. The engine ships one provider for native
//! Racer Content Packs ([`RcpProvider`]); external crates can register more
//! without the engine knowing anything about their source formats.

use racer_pack_schema::{MANIFEST_FILE, Pack, PackError};
use std::collections::BTreeMap;

/// Read-only view of candidate input files. Paths use forward slashes and are relative.
pub trait FileSource {
    fn paths(&self) -> Vec<String>;
    fn read(&self, path: &str) -> Option<Vec<u8>>;
}

/// In-memory [`FileSource`].
impl FileSource for BTreeMap<String, Vec<u8>> {
    fn paths(&self) -> Vec<String> {
        self.keys().cloned().collect()
    }

    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.get(path).cloned()
    }
}

#[derive(Debug)]
pub enum IngestError {
    /// The provider does not recognise the input.
    Unsupported(String),
    /// Input was recognised but could not be converted.
    Failed(String),
    Pack(PackError),
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(m) => write!(f, "unsupported input: {m}"),
            Self::Failed(m) => write!(f, "ingest failed: {m}"),
            Self::Pack(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for IngestError {}

impl From<PackError> for IngestError {
    fn from(e: PackError) -> Self {
        Self::Pack(e)
    }
}

pub trait ContentProvider: Send + Sync {
    /// Stable provider identifier.
    fn id(&self) -> &str;

    /// Cheap check whether this provider can handle the files.
    fn detect(&self, files: &dyn FileSource) -> bool;

    /// Convert the files into a pack. Implementations should call
    /// [`Pack::validate`] before returning.
    fn ingest(&self, files: &dyn FileSource) -> Result<Pack, IngestError>;
}

/// Ordered list of providers; the first whose `detect` succeeds wins.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: Vec<Box<dyn ContentProvider>>,
}

impl ProviderRegistry {
    /// Registry preloaded with the native pack provider.
    pub fn with_native() -> Self {
        let mut r = Self::default();
        r.providers.push(Box::new(RcpProvider));
        r
    }

    pub fn register(&mut self, provider: Box<dyn ContentProvider>) -> Result<(), String> {
        if self.providers.iter().any(|p| p.id() == provider.id()) {
            return Err(format!(
                "content provider {:?} already registered",
                provider.id()
            ));
        }
        self.providers.push(provider);
        Ok(())
    }

    pub fn ids(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.id()).collect()
    }

    pub fn ingest(&self, files: &dyn FileSource) -> Result<Pack, IngestError> {
        let provider = self
            .providers
            .iter()
            .find(|p| p.detect(files))
            .ok_or_else(|| IngestError::Unsupported("no provider recognised the files".into()))?;
        provider.ingest(files)
    }
}

/// Provider for native `rcp/1` packs (a `pack.json` plus its documents and buffers).
pub struct RcpProvider;

impl ContentProvider for RcpProvider {
    fn id(&self) -> &str {
        "rcp"
    }

    fn detect(&self, files: &dyn FileSource) -> bool {
        files.paths().iter().any(|p| p == MANIFEST_FILE)
    }

    fn ingest(&self, files: &dyn FileSource) -> Result<Pack, IngestError> {
        let map: BTreeMap<String, Vec<u8>> = files
            .paths()
            .into_iter()
            .filter_map(|p| files.read(&p).map(|b| (p, b)))
            .collect();
        let pack = Pack::from_files(&map)?;
        pack.validate()
            .map_err(|e| IngestError::Pack(PackError::Invalid(e)))?;
        Ok(pack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use racer_pack_schema::{DocumentIndex, Manifest, RCP_SCHEMA_ID};

    fn minimal() -> BTreeMap<String, Vec<u8>> {
        Pack {
            manifest: Some(Manifest {
                schema: RCP_SCHEMA_ID.into(),
                id: "empty".into(),
                name: "Empty".into(),
                version: None,
                license: "CC0-1.0".into(),
                author: "Test".into(),
                description: None,
                documents: DocumentIndex::default(),
            }),
            ..Pack::default()
        }
        .to_files()
        .unwrap()
    }

    #[test]
    fn native_provider_ingests_valid_pack() {
        let r = ProviderRegistry::with_native();
        assert_eq!(r.ids(), vec!["rcp"]);
        let pack = r.ingest(&minimal()).unwrap();
        assert_eq!(pack.manifest.unwrap().id, "empty");
    }

    #[test]
    fn unknown_files_are_unsupported_and_duplicates_rejected() {
        let mut r = ProviderRegistry::with_native();
        let files = BTreeMap::from([("x.bin".to_string(), vec![1u8])]);
        assert!(matches!(r.ingest(&files), Err(IngestError::Unsupported(_))));
        assert!(r.register(Box::new(RcpProvider)).is_err());
    }

    #[test]
    fn invalid_native_pack_is_reported() {
        let mut files = minimal();
        files.insert(
            "pack.json".into(),
            br#"{"schema":"rcp/1","id":"Bad Id","name":"n","license":"MIT","author":"a"}"#.to_vec(),
        );
        let err = ProviderRegistry::with_native().ingest(&files).unwrap_err();
        assert!(err.to_string().contains("manifest.id"), "{err}");
    }
}
