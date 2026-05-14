//! Cache storage read/write primitives.

use std::{fs, io, path::Path};

use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;

use crate::manifest::{CacheArtifact, CacheManifest, StorageKey};

/// File-backed cache storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheStorage {
    root: Utf8PathBuf,
}

impl CacheStorage {
    /// Create storage rooted at `root`.
    #[must_use]
    pub fn new(root: Utf8PathBuf) -> Self {
        Self { root }
    }

    /// Write an artifact to storage.
    pub fn write(&self, key: &StorageKey, artifact: &CacheArtifact) -> Result<(), StorageError> {
        let path = self.path_for(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = bincode::serialize(artifact)?;
        fs::write(path, bytes)?;
        Ok(())
    }

    /// Read an artifact from storage.
    pub fn read(&self, key: &StorageKey) -> Result<Option<CacheArtifact>, StorageError> {
        let path = self.path_for(key);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path)?;
        Ok(Some(bincode::deserialize(&bytes)?))
    }

    /// Write the cache manifest.
    pub fn write_manifest(&self, manifest: &CacheManifest) -> Result<(), StorageError> {
        fs::create_dir_all(&self.root)?;
        let bytes = bincode::serialize(manifest)?;
        fs::write(self.root.join("manifest.bin"), bytes)?;
        Ok(())
    }

    /// Read the cache manifest, if present.
    pub fn read_manifest(&self) -> Result<Option<CacheManifest>, StorageError> {
        let path = self.root.join("manifest.bin");
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path)?;
        Ok(Some(bincode::deserialize(&bytes)?))
    }

    /// Delete the cache directory if it exists.
    pub fn clear(&self) -> Result<(), StorageError> {
        if self.root.exists() {
            fs::remove_dir_all(&self.root)?;
        }
        Ok(())
    }

    fn path_for(&self, key: &StorageKey) -> Utf8PathBuf {
        let safe_key = key.key.replace(['/', '\\', ':'], "_");
        self.root
            .join(key.kind.as_str())
            .join(format!("{safe_key}.bin"))
    }
}

/// Cache storage errors.
#[derive(Debug, Error)]
pub enum StorageError {
    /// I/O failure.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// Serialization failure.
    #[error(transparent)]
    Serialize(#[from] Box<bincode::ErrorKind>),
    /// Non-UTF-8 root path.
    #[error("cache path is not valid UTF-8: {0}")]
    NonUtf8Path(String),
}

impl TryFrom<&Path> for CacheStorage {
    type Error = StorageError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        let root = Utf8Path::from_path(path)
            .ok_or_else(|| StorageError::NonUtf8Path(path.display().to_string()))?
            .to_path_buf();
        Ok(Self::new(root))
    }
}
