//! Legacy JSON snapshot format.
//!
//! SQLite is the authoritative store; this whole-store JSON file exists to
//! read and write pre-SQLite snapshots (today exercised only by tests). It
//! shares [`migrate_record_value`] with the SQLite loader so value migrations
//! are written once.

use super::migrations::migrate_record_value;
use super::registry::PersistedStore;
use super::{InMemoryStore, StorePersistenceError};
use std::path::Path;

pub fn save_store_snapshot(
    store: &InMemoryStore,
    path: &Path,
) -> Result<(), StorePersistenceError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let snapshot = PersistedStore::from(store);
    let bytes = serde_json::to_vec_pretty(&snapshot)?;
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, bytes)?;
    std::fs::rename(tmp_path, path)?;
    Ok(())
}

pub fn load_store_snapshot(path: &Path) -> Result<InMemoryStore, StorePersistenceError> {
    let bytes = std::fs::read(path)?;
    let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let mut migrated = false;
    if let Some(collections) = value.as_object_mut() {
        for (collection, values) in collections.iter_mut() {
            let Some(values) = values.as_array_mut() else {
                continue;
            };
            for value in values {
                migrated |= migrate_record_value(collection, value)?;
            }
        }
    }
    let snapshot: PersistedStore = serde_json::from_value(value)?;
    if snapshot.version != 1 {
        return Err(StorePersistenceError::UnsupportedVersion(snapshot.version));
    }
    let store = snapshot.try_into()?;
    if migrated {
        save_store_snapshot(&store, path)?;
    }
    Ok(store)
}
