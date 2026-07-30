//! Authoritative SQLite persistence: schema application, tri-state open,
//! optimistic-concurrency change persistence, and the migrating loader.

use super::migrations::{
    migrate_record_value, persist_migrated_pod_relationships, persist_migrated_records,
    rewrite_collection,
};
use super::registry::{
    collection_key_spec, is_positional_key, store_records, KeySpec, PersistedStore, StoreRecords,
    STORE_COLLECTIONS,
};
use super::{InMemoryStore, StorePersistenceError};
use rusqlite::OptionalExtension;
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

const SQLITE_STORE_SCHEMA: &str =
    include_str!("../../migrations/sqlite/0002_authoritative_store.sql");
const SQLITE_DROP_LEGACY_HUB: &str =
    include_str!("../../migrations/sqlite/0003_drop_legacy_hub.sql");
const SQLITE_SEARCH_INDEX_SCHEMA: &str =
    include_str!("../../migrations/sqlite/0004_search_index.sql");

/// Reports whether a SQLite path contains an initialized Stumble store without
/// creating the file when it is absent.
pub fn sqlite_home_node_is_initialized(
    database_path: &Path,
) -> Result<bool, StorePersistenceError> {
    if !database_path.is_file() {
        return Ok(false);
    }
    let connection = rusqlite::Connection::open_with_flags(
        database_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    if !store_schema_exists(&connection)? {
        return Ok(false);
    }
    Ok(sqlite_store_state(&connection)? == SqliteStoreState::Initialized)
}

/// Whether the store tables exist; read-only connections cannot rely on
/// [`apply_sqlite_schema`] having created them.
fn store_schema_exists(connection: &rusqlite::Connection) -> Result<bool, StorePersistenceError> {
    Ok(connection.query_row(
        "SELECT
           EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'stumble_store_metadata')
           AND EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'stumble_store_records')",
        [],
        |row| row.get(0),
    )?)
}

/// Opens the authoritative SQLite store, seeding only when the database is empty.
pub fn load_or_initialize_sqlite_store(
    database_path: &Path,
    seed: impl FnOnce() -> InMemoryStore,
) -> Result<InMemoryStore, StorePersistenceError> {
    if let Some(parent) = database_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut connection = open_sqlite_store(database_path)?;
    match sqlite_store_state(&connection)? {
        SqliteStoreState::Initialized => return load_sqlite_store_from_connection(&mut connection),
        SqliteStoreState::PopulatedWithoutMetadata => {
            return Err(StorePersistenceError::PopulatedUninitializedDatabase)
        }
        SqliteStoreState::Empty => {}
    }

    let store = seed();
    initialize_sqlite_store(&mut connection, &store)?;
    Ok(store)
}

pub fn load_sqlite_store(database_path: &Path) -> Result<InMemoryStore, StorePersistenceError> {
    let mut connection = open_sqlite_store(database_path)?;
    load_sqlite_store_from_connection(&mut connection)
}

/// Applies only changed domain records in one SQLite transaction.
///
/// `previous` is the record snapshot from the last successful persist (or
/// load), so unchanged records cost nothing beyond the in-memory diff. On
/// success the returned records become the caller's next baseline.
pub fn persist_sqlite_store_changes(
    database_path: &Path,
    previous: &StoreRecords,
    current: &InMemoryStore,
) -> Result<(i64, StoreRecords), StorePersistenceError> {
    let mut connection = open_sqlite_store(database_path)?;
    let current_records = store_records(current)?;
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

    for (collection_and_key, previous_value) in previous.iter().filter(|(key, value)| {
        current_records
            .get(*key)
            .is_none_or(|current| current != *value)
    }) {
        ensure_record_unchanged(&transaction, collection_and_key, Some(previous_value))?;
    }
    for collection_and_key in current_records
        .keys()
        .filter(|key| !previous.contains_key(*key))
    {
        ensure_record_unchanged(&transaction, collection_and_key, None)?;
    }

    for (collection_and_key, _) in previous
        .iter()
        .filter(|(collection_and_key, _)| !current_records.contains_key(*collection_and_key))
    {
        transaction.execute(
            "DELETE FROM stumble_store_records WHERE collection = ?1 AND record_key = ?2",
            rusqlite::params![collection_and_key.0, collection_and_key.1],
        )?;
    }
    for ((collection, record_key), value_json) in current_records
        .iter()
        .filter(|(collection_and_key, value)| previous.get(*collection_and_key) != Some(*value))
    {
        transaction.execute(
            "INSERT INTO stumble_store_records (collection, record_key, value_json) VALUES (?1, ?2, ?3)
             ON CONFLICT (collection, record_key) DO UPDATE SET value_json = excluded.value_json",
            rusqlite::params![collection, record_key, value_json],
        )?;
    }
    transaction.execute(
        "INSERT INTO stumble_store_metadata (key, value) VALUES ('initialized', '1')
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [],
    )?;
    transaction.execute(
        "INSERT INTO stumble_store_metadata (key, value) VALUES ('generation', '1')
         ON CONFLICT (key) DO UPDATE SET value = CAST(CAST(stumble_store_metadata.value AS INTEGER) + 1 AS TEXT)",
        [],
    )?;
    let generation: i64 = transaction.query_row(
        "SELECT CAST(value AS INTEGER) FROM stumble_store_metadata WHERE key = 'generation'",
        [],
        |row| row.get(0),
    )?;
    transaction.commit()?;
    Ok((generation, current_records))
}

/// Reads the monotonically increasing store generation used by long-lived
/// processes to detect writes from other processes. Zero for fresh stores.
pub fn read_store_generation(database_path: &Path) -> Result<i64, StorePersistenceError> {
    let connection = open_sqlite_store(database_path)?;
    let generation = connection
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM stumble_store_metadata WHERE key = 'generation'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(generation.unwrap_or(0))
}

pub(crate) fn open_sqlite_store(
    path: &Path,
) -> Result<rusqlite::Connection, StorePersistenceError> {
    let connection = rusqlite::Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    apply_sqlite_schema(&connection)?;
    Ok(connection)
}

/// Applies the full idempotent schema, including forward migrations, so every
/// connection — file-backed or in-memory — sees the same database shape.
///
/// There is no schema_version row: this scheme relies on every batch here
/// staying idempotent and being reapplied on each open. A future migration
/// that *transforms* rows (rather than creating or dropping structures) must
/// introduce version metadata instead of joining this list.
pub(crate) fn apply_sqlite_schema(
    connection: &rusqlite::Connection,
) -> Result<(), rusqlite::Error> {
    connection.execute_batch(SQLITE_STORE_SCHEMA)?;
    // Forward migration: drop non-authoritative legacy Hub caches without
    // transforming their contents. Idempotent for new and existing databases.
    connection.execute_batch(SQLITE_DROP_LEGACY_HUB)?;
    connection.execute_batch(SQLITE_SEARCH_INDEX_SCHEMA)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SqliteStoreState {
    Empty,
    Initialized,
    PopulatedWithoutMetadata,
}

fn sqlite_store_state(
    connection: &rusqlite::Connection,
) -> Result<SqliteStoreState, StorePersistenceError> {
    let (initialized, populated): (bool, bool) = connection.query_row(
        "SELECT
           EXISTS(SELECT 1 FROM stumble_store_metadata WHERE key = 'initialized'),
           EXISTS(SELECT 1 FROM stumble_store_records)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(match (initialized, populated) {
        (true, _) => SqliteStoreState::Initialized,
        (false, true) => SqliteStoreState::PopulatedWithoutMetadata,
        (false, false) => SqliteStoreState::Empty,
    })
}

fn ensure_record_unchanged(
    transaction: &rusqlite::Transaction<'_>,
    collection_and_key: &(String, String),
    expected: Option<&String>,
) -> Result<(), StorePersistenceError> {
    let actual = transaction
        .query_row(
            "SELECT value_json FROM stumble_store_records WHERE collection = ?1 AND record_key = ?2",
            rusqlite::params![collection_and_key.0, collection_and_key.1],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if actual.as_ref() != expected {
        return Err(StorePersistenceError::ConcurrentWriteConflict {
            collection: collection_and_key.0.clone(),
            record_key: collection_and_key.1.clone(),
        });
    }
    Ok(())
}

pub(super) fn initialize_sqlite_store(
    connection: &mut rusqlite::Connection,
    store: &InMemoryStore,
) -> Result<(), StorePersistenceError> {
    let records = store_records(store)?;
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for ((collection, record_key), value_json) in records {
        transaction.execute(
            "INSERT INTO stumble_store_records (collection, record_key, value_json) VALUES (?1, ?2, ?3)",
            rusqlite::params![collection, record_key, value_json],
        )?;
    }
    transaction.execute(
        "INSERT INTO stumble_store_metadata (key, value) VALUES ('initialized', '1')",
        [],
    )?;
    transaction.execute(
        "INSERT INTO stumble_store_metadata (key, value) VALUES ('generation', '1')
         ON CONFLICT (key) DO NOTHING",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

fn load_sqlite_store_from_connection(
    connection: &mut rusqlite::Connection,
) -> Result<InMemoryStore, StorePersistenceError> {
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    // Rows whose stored shape is legacy and must be rewritten canonically.
    let mut legacy_rows: Vec<(String, String)> = Vec::new();
    // Positional-key collections still stored under a legacy key scheme.
    let mut rekey_collections: BTreeSet<&'static str> = BTreeSet::new();
    let mut collections = serde_json::Map::new();
    for collection in STORE_COLLECTIONS {
        collections.insert(
            (*collection).to_string(),
            serde_json::Value::Array(Vec::new()),
        );
    }
    collections.insert("version".to_string(), serde_json::json!(1));

    let mut statement = transaction.prepare(
        "SELECT collection, record_key, value_json FROM stumble_store_records
         ORDER BY collection, record_key",
    )?;
    let records = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for record in records {
        let (collection, record_key, value_json) = record?;
        let mut value: serde_json::Value = serde_json::from_str(&value_json)?;
        if migrate_record_value(&collection, &mut value)? {
            legacy_rows.push((collection.clone(), record_key.clone()));
        }
        if let Some(canonical) = STORE_COLLECTIONS
            .iter()
            .find(|canonical| **canonical == collection)
        {
            if matches!(collection_key_spec(canonical), Some(KeySpec::Positional))
                && !is_positional_key(&record_key)
            {
                rekey_collections.insert(canonical);
            }
        }
        if let Some(serde_json::Value::Array(values)) = collections.get_mut(&collection) {
            values.push(value);
        }
    }
    drop(statement);
    let snapshot: PersistedStore = serde_json::from_value(serde_json::Value::Object(collections))?;
    if snapshot.version != 1 {
        return Err(StorePersistenceError::UnsupportedVersion(snapshot.version));
    }
    let had_legacy_pod_memberships = !snapshot.pod_memberships.is_empty();
    let store: InMemoryStore = snapshot.try_into()?;
    if !legacy_rows.is_empty() || had_legacy_pod_memberships || !rekey_collections.is_empty() {
        let records = store_records(&store)?;
        persist_migrated_records(&transaction, &records, &legacy_rows)?;
        if had_legacy_pod_memberships {
            persist_migrated_pod_relationships(&transaction, &records)?;
        }
        for collection in rekey_collections {
            rewrite_collection(&transaction, collection, &records)?;
        }
    }
    transaction.commit()?;
    Ok(store)
}
