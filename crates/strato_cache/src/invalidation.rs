//! Cache invalidation primitives.

use std::collections::BTreeMap;

use crate::manifest::{CacheManifest, StorageKey};

/// Deterministic cache invalidation result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheInvalidation {
    /// Keys absent from the current manifest.
    pub removed: Vec<StorageKey>,
    /// Keys whose content hash changed.
    pub changed: Vec<StorageKey>,
    /// Keys absent from the previous manifest.
    pub added: Vec<StorageKey>,
}

impl CacheInvalidation {
    /// Compare an old manifest with current content hashes.
    #[must_use]
    pub fn between(old: &CacheManifest, current: &BTreeMap<StorageKey, String>) -> Self {
        let removed = old
            .entries
            .keys()
            .filter(|key| !current.contains_key(*key))
            .cloned()
            .collect();
        let changed = current
            .iter()
            .filter(|(key, hash)| {
                old.entries
                    .get(*key)
                    .is_some_and(|old_hash| old_hash != *hash)
            })
            .map(|(key, _)| key.clone())
            .collect();
        let added = current
            .keys()
            .filter(|key| !old.entries.contains_key(*key))
            .cloned()
            .collect();

        Self {
            removed,
            changed,
            added,
        }
    }
}
