use crate::domain::*;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("duplicate: {0}")]
    Duplicate(String),
    #[error("tenant boundary violation")]
    TenantBoundary,
    #[error("untrusted peer")]
    UntrustedPeer,
    #[error("invalid event signature")]
    InvalidSignature,
    #[error("validation failed: {0}")]
    Validation(String),
}

#[derive(Debug, Error)]
pub enum StorePersistenceError {
    #[error("storage io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("storage json failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("storage sqlite failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("unsupported store version: {0}")]
    UnsupportedVersion(u32),
    #[error("concurrent write conflict in {collection} record {record_key}")]
    ConcurrentWriteConflict {
        collection: String,
        record_key: String,
    },
    #[error("refusing to initialize a populated SQLite database without migration metadata")]
    PopulatedUninitializedDatabase,
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryStore {
    pub tenants: HashMap<TenantId, Tenant>,
    pub users: HashMap<UserId, User>,
    pub tenant_users: Vec<TenantUser>,
    pub api_tokens: HashMap<Uuid, ApiToken>,
    pub node_identities: HashMap<NodeIdentityId, NodeIdentity>,
    pub trusted_peers: HashMap<PeerId, TrustedPeer>,
    pub pods: HashMap<PodId, Pod>,
    pub pod_memberships: Vec<PodMembership>,
    pub pod_rules: HashMap<PodId, PodRules>,
    pub pod_skill_packs: HashMap<PodId, PodSkillPack>,
    pub event_log: Vec<EventLog>,
    pub submissions: HashMap<SubmissionId, Submission>,
    pub submission_pods: Vec<SubmissionPod>,
    pub submission_assets: HashMap<Uuid, SubmissionAsset>,
    pub crawler_sources: HashMap<Uuid, CrawlerSource>,
    pub crawl_candidates: HashMap<Uuid, CrawlCandidate>,
    pub user_preferences: HashMap<(UserId, Option<TenantId>), UserPreferences>,
    pub feedback_events: Vec<FeedbackEvent>,
    pub briefs: HashMap<Uuid, Brief>,
    pub saves: HashSet<(UserId, SubmissionId)>,
    pub private_notes: BTreeMap<(UserId, SubmissionId), String>,
    pub reading_history: HashSet<(UserId, SubmissionId)>,
    pub hub_nodes: HashMap<NodeIdentityId, HubRegisteredNode>,
    pub hub_pods: HashMap<(NodeIdentityId, String), HubRegisteredPod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedStore {
    version: u32,
    tenants: Vec<Tenant>,
    users: Vec<User>,
    tenant_users: Vec<TenantUser>,
    api_tokens: Vec<ApiToken>,
    node_identities: Vec<NodeIdentity>,
    trusted_peers: Vec<TrustedPeer>,
    pods: Vec<Pod>,
    pod_memberships: Vec<PodMembership>,
    pod_rules: Vec<PodRules>,
    pod_skill_packs: Vec<PodSkillPack>,
    event_log: Vec<EventLog>,
    submissions: Vec<Submission>,
    submission_pods: Vec<SubmissionPod>,
    submission_assets: Vec<SubmissionAsset>,
    crawler_sources: Vec<CrawlerSource>,
    crawl_candidates: Vec<CrawlCandidate>,
    user_preferences: Vec<UserPreferences>,
    feedback_events: Vec<FeedbackEvent>,
    briefs: Vec<Brief>,
    saves: Vec<PersistedUserSubmission>,
    private_notes: Vec<PersistedPrivateNote>,
    reading_history: Vec<PersistedUserSubmission>,
    hub_nodes: Vec<HubRegisteredNode>,
    hub_pods: Vec<HubRegisteredPod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedUserSubmission {
    user_id: UserId,
    submission_id: SubmissionId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedPrivateNote {
    user_id: UserId,
    submission_id: SubmissionId,
    body: String,
}

impl From<&InMemoryStore> for PersistedStore {
    fn from(store: &InMemoryStore) -> Self {
        Self {
            version: 1,
            tenants: store.tenants.values().cloned().collect(),
            users: store.users.values().cloned().collect(),
            tenant_users: store.tenant_users.clone(),
            api_tokens: store.api_tokens.values().cloned().collect(),
            node_identities: store.node_identities.values().cloned().collect(),
            trusted_peers: store.trusted_peers.values().cloned().collect(),
            pods: store.pods.values().cloned().collect(),
            pod_memberships: store.pod_memberships.clone(),
            pod_rules: store.pod_rules.values().cloned().collect(),
            pod_skill_packs: store.pod_skill_packs.values().cloned().collect(),
            event_log: store.event_log.clone(),
            submissions: store.submissions.values().cloned().collect(),
            submission_pods: store.submission_pods.clone(),
            submission_assets: store.submission_assets.values().cloned().collect(),
            crawler_sources: store.crawler_sources.values().cloned().collect(),
            crawl_candidates: store.crawl_candidates.values().cloned().collect(),
            user_preferences: store.user_preferences.values().cloned().collect(),
            feedback_events: store.feedback_events.clone(),
            briefs: store.briefs.values().cloned().collect(),
            saves: store
                .saves
                .iter()
                .map(|(user_id, submission_id)| PersistedUserSubmission {
                    user_id: *user_id,
                    submission_id: *submission_id,
                })
                .collect(),
            private_notes: store
                .private_notes
                .iter()
                .map(|((user_id, submission_id), body)| PersistedPrivateNote {
                    user_id: *user_id,
                    submission_id: *submission_id,
                    body: body.clone(),
                })
                .collect(),
            reading_history: store
                .reading_history
                .iter()
                .map(|(user_id, submission_id)| PersistedUserSubmission {
                    user_id: *user_id,
                    submission_id: *submission_id,
                })
                .collect(),
            hub_nodes: store.hub_nodes.values().cloned().collect(),
            hub_pods: store.hub_pods.values().cloned().collect(),
        }
    }
}

impl From<PersistedStore> for InMemoryStore {
    fn from(snapshot: PersistedStore) -> Self {
        Self {
            tenants: snapshot
                .tenants
                .into_iter()
                .map(|tenant| (tenant.id, tenant))
                .collect(),
            users: snapshot
                .users
                .into_iter()
                .map(|user| (user.id, user))
                .collect(),
            tenant_users: snapshot.tenant_users,
            api_tokens: snapshot
                .api_tokens
                .into_iter()
                .map(|token| (token.id, token))
                .collect(),
            node_identities: snapshot
                .node_identities
                .into_iter()
                .map(|node| (node.id, node))
                .collect(),
            trusted_peers: snapshot
                .trusted_peers
                .into_iter()
                .map(|peer| (peer.id, peer))
                .collect(),
            pods: snapshot.pods.into_iter().map(|pod| (pod.id, pod)).collect(),
            pod_memberships: snapshot.pod_memberships,
            pod_rules: snapshot
                .pod_rules
                .into_iter()
                .map(|rules| (rules.pod_id, rules))
                .collect(),
            pod_skill_packs: snapshot
                .pod_skill_packs
                .into_iter()
                .map(|pack| (pack.pod_id, pack))
                .collect(),
            event_log: snapshot.event_log,
            submissions: snapshot
                .submissions
                .into_iter()
                .map(|submission| (submission.id, submission))
                .collect(),
            submission_pods: snapshot.submission_pods,
            submission_assets: snapshot
                .submission_assets
                .into_iter()
                .map(|asset| (asset.id, asset))
                .collect(),
            crawler_sources: snapshot
                .crawler_sources
                .into_iter()
                .map(|source| (source.id, source))
                .collect(),
            crawl_candidates: snapshot
                .crawl_candidates
                .into_iter()
                .map(|candidate| (candidate.id, candidate))
                .collect(),
            user_preferences: snapshot
                .user_preferences
                .into_iter()
                .map(|prefs| ((prefs.user_id, prefs.tenant_id), prefs))
                .collect(),
            feedback_events: snapshot.feedback_events,
            briefs: snapshot
                .briefs
                .into_iter()
                .map(|brief| (brief.id, brief))
                .collect(),
            saves: snapshot
                .saves
                .into_iter()
                .map(|save| (save.user_id, save.submission_id))
                .collect(),
            private_notes: snapshot
                .private_notes
                .into_iter()
                .map(|note| ((note.user_id, note.submission_id), note.body))
                .collect(),
            reading_history: snapshot
                .reading_history
                .into_iter()
                .map(|history| (history.user_id, history.submission_id))
                .collect(),
            hub_nodes: snapshot
                .hub_nodes
                .into_iter()
                .map(|node| (node.node_id, node))
                .collect(),
            hub_pods: snapshot
                .hub_pods
                .into_iter()
                .map(|pod| ((pod.node_id, pod.pod_slug.clone()), pod))
                .collect(),
        }
    }
}

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
    let snapshot: PersistedStore = serde_json::from_slice(&bytes)?;
    if snapshot.version != 1 {
        return Err(StorePersistenceError::UnsupportedVersion(snapshot.version));
    }
    Ok(snapshot.into())
}

pub fn load_or_seed_store_snapshot(
    path: &Path,
    seed: impl FnOnce() -> InMemoryStore,
) -> Result<InMemoryStore, StorePersistenceError> {
    if path.exists() {
        load_store_snapshot(path)
    } else {
        let store = seed();
        save_store_snapshot(&store, path)?;
        Ok(store)
    }
}

const SQLITE_STORE_SCHEMA: &str =
    include_str!("../../../migrations/sqlite/0002_authoritative_store.sql");
const STORE_COLLECTIONS: &[&str] = &[
    "tenants",
    "users",
    "tenant_users",
    "api_tokens",
    "node_identities",
    "trusted_peers",
    "pods",
    "pod_memberships",
    "pod_rules",
    "pod_skill_packs",
    "event_log",
    "submissions",
    "submission_pods",
    "submission_assets",
    "crawler_sources",
    "crawl_candidates",
    "user_preferences",
    "feedback_events",
    "briefs",
    "saves",
    "private_notes",
    "reading_history",
    "hub_nodes",
    "hub_pods",
];

type StoreRecords = BTreeMap<(String, String), String>;

/// Opens the authoritative SQLite store, importing a legacy JSON snapshot only
/// when the database has never been initialized.
pub fn load_or_initialize_sqlite_store(
    database_path: &Path,
    legacy_path: &Path,
    seed: impl FnOnce() -> InMemoryStore,
) -> Result<InMemoryStore, StorePersistenceError> {
    if let Some(parent) = database_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut connection = open_sqlite_store(database_path)?;
    match sqlite_store_state(&connection)? {
        SqliteStoreState::Initialized => return load_sqlite_store_from_connection(&connection),
        SqliteStoreState::PopulatedWithoutMetadata => {
            return Err(StorePersistenceError::PopulatedUninitializedDatabase)
        }
        SqliteStoreState::Empty => {}
    }

    let store = if legacy_path.exists() {
        let store = load_store_snapshot(legacy_path)?;
        let backup_path = legacy_path.with_extension("json.migrated.bak");
        if !backup_path.exists() {
            std::fs::copy(legacy_path, backup_path)?;
        }
        store
    } else {
        seed()
    };
    initialize_sqlite_store(&mut connection, &store)?;
    Ok(store)
}

pub fn load_sqlite_store(database_path: &Path) -> Result<InMemoryStore, StorePersistenceError> {
    let connection = open_sqlite_store(database_path)?;
    load_sqlite_store_from_connection(&connection)
}

/// Applies only changed domain records in one SQLite transaction.
pub fn persist_sqlite_store_changes(
    database_path: &Path,
    previous: &InMemoryStore,
    current: &InMemoryStore,
) -> Result<(), StorePersistenceError> {
    let mut connection = open_sqlite_store(database_path)?;
    let previous_records = store_records(previous)?;
    let current_records = store_records(current)?;
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

    for (collection_and_key, previous_value) in previous_records.iter().filter(|(key, value)| {
        current_records
            .get(*key)
            .is_none_or(|current| current != *value)
    }) {
        ensure_record_unchanged(&transaction, collection_and_key, Some(previous_value))?;
    }
    for collection_and_key in current_records
        .keys()
        .filter(|key| !previous_records.contains_key(*key))
    {
        ensure_record_unchanged(&transaction, collection_and_key, None)?;
    }

    for (collection_and_key, _) in previous_records
        .iter()
        .filter(|(collection_and_key, _)| !current_records.contains_key(*collection_and_key))
    {
        transaction.execute(
            "DELETE FROM stumble_store_records WHERE collection = ?1 AND record_key = ?2",
            rusqlite::params![collection_and_key.0, collection_and_key.1],
        )?;
    }
    for ((collection, record_key), value_json) in
        current_records
            .iter()
            .filter(|(collection_and_key, value)| {
                previous_records.get(*collection_and_key) != Some(*value)
            })
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
    transaction.commit()?;
    Ok(())
}

fn open_sqlite_store(path: &Path) -> Result<rusqlite::Connection, StorePersistenceError> {
    let connection = rusqlite::Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(SQLITE_STORE_SCHEMA)?;
    Ok(connection)
}

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

fn initialize_sqlite_store(
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
    transaction.commit()?;
    Ok(())
}

fn load_sqlite_store_from_connection(
    connection: &rusqlite::Connection,
) -> Result<InMemoryStore, StorePersistenceError> {
    let mut collections = serde_json::Map::new();
    for collection in STORE_COLLECTIONS {
        collections.insert(
            (*collection).to_string(),
            serde_json::Value::Array(Vec::new()),
        );
    }
    collections.insert("version".to_string(), serde_json::json!(1));

    let mut statement = connection.prepare(
        "SELECT collection, value_json FROM stumble_store_records ORDER BY collection, record_key",
    )?;
    let records = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for record in records {
        let (collection, value_json) = record?;
        let value = serde_json::from_str(&value_json)?;
        if let Some(serde_json::Value::Array(values)) = collections.get_mut(&collection) {
            values.push(value);
        }
    }
    let snapshot: PersistedStore = serde_json::from_value(serde_json::Value::Object(collections))?;
    if snapshot.version != 1 {
        return Err(StorePersistenceError::UnsupportedVersion(snapshot.version));
    }
    Ok(snapshot.into())
}

fn store_records(store: &InMemoryStore) -> Result<StoreRecords, StorePersistenceError> {
    let snapshot_value = serde_json::to_value(PersistedStore::from(store))?;
    let snapshot = snapshot_value
        .as_object()
        .expect("PersistedStore serializes as an object");
    let mut records = BTreeMap::new();
    for collection in STORE_COLLECTIONS {
        let values = snapshot
            .get(*collection)
            .and_then(serde_json::Value::as_array)
            .expect("PersistedStore collections serialize as arrays");
        for value in values {
            let record_key = record_key(collection, value)?;
            records.insert(
                ((*collection).to_string(), record_key),
                serde_json::to_string(value)?,
            );
        }
    }
    Ok(records)
}

fn record_key(
    collection: &str,
    value: &serde_json::Value,
) -> Result<String, StorePersistenceError> {
    let fields: &[&str] = match collection {
        "tenant_users" => &["tenant_id", "user_id"],
        "pod_memberships" => &["user_id", "pod_id"],
        "submission_pods" => &["submission_id", "pod_id"],
        "user_preferences" => &["user_id", "tenant_id"],
        "saves" | "private_notes" | "reading_history" => &["user_id", "submission_id"],
        "hub_pods" => &["node_id", "pod_slug"],
        "hub_nodes" => &["node_id"],
        "pod_rules" | "pod_skill_packs" => &["pod_id"],
        "event_log" => &["event_id"],
        "feedback_events" => return Ok(serde_json::to_string(value)?),
        _ => &["id"],
    };
    let mut key = Vec::with_capacity(fields.len());
    for field in fields {
        key.push(
            value
                .get(*field)
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        );
    }
    Ok(serde_json::to_string(&key)?)
}

impl InMemoryStore {
    pub fn default_node(&self) -> Result<NodeIdentity, StoreError> {
        self.node_identities
            .values()
            .find(|node| node.tenant_id.is_none())
            .or_else(|| self.node_identities.values().next())
            .cloned()
            .ok_or_else(|| StoreError::NotFound("node identity".to_string()))
    }

    pub fn node_for_tenant(&self, tenant_id: Option<TenantId>) -> Result<NodeIdentity, StoreError> {
        self.node_identities
            .values()
            .find(|node| node.tenant_id == tenant_id)
            .or_else(|| {
                self.node_identities
                    .values()
                    .find(|node| node.tenant_id.is_none())
            })
            .cloned()
            .ok_or_else(|| StoreError::NotFound("node identity".to_string()))
    }

    pub fn pod_by_slug(&self, slug: &str, tenant_id: Option<TenantId>) -> Result<Pod, StoreError> {
        self.pods
            .values()
            .find(|pod| pod.slug == slug && pod.tenant_id == tenant_id)
            .or_else(|| {
                self.pods
                    .values()
                    .find(|pod| pod.slug == slug && pod.tenant_id.is_none())
            })
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("pod {slug}")))
    }

    pub fn tenant_by_slug(&self, slug: &str) -> Result<Tenant, StoreError> {
        self.tenants
            .values()
            .find(|tenant| tenant.slug == slug)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("tenant {slug}")))
    }

    pub fn assert_tenant(
        &self,
        actual: Option<TenantId>,
        expected: Option<TenantId>,
    ) -> Result<(), StoreError> {
        if actual == expected || actual.is_none() {
            Ok(())
        } else {
            Err(StoreError::TenantBoundary)
        }
    }

    pub fn submissions_for_pod(&self, pod_id: PodId) -> Vec<&Submission> {
        let ids: HashSet<_> = self
            .submission_pods
            .iter()
            .filter(|link| link.pod_id == pod_id)
            .map(|link| link.submission_id)
            .collect();
        self.submissions
            .values()
            .filter(|submission| ids.contains(&submission.id))
            .collect()
    }

    pub fn public_events_for_pod(&self, pod_slug: &str) -> Vec<EventLog> {
        self.event_log
            .iter()
            .filter(|event| event.pod_slug == pod_slug && !is_private_event(&event.event_type))
            .cloned()
            .collect()
    }

    pub fn latest_event_hash(&self, pod_slug: &str) -> Option<String> {
        self.event_log
            .iter()
            .rev()
            .find(|event| event.pod_slug == pod_slug)
            .map(|event| event.content_hash.clone())
    }
}

pub fn is_private_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "link_saved_private"
            | "link_dismissed_private"
            | "private_note_added"
            | "user_preference_updated"
            | "source_blocked_private"
            | "topic_blocked_private"
            | "reading_history_recorded"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store_dir(test_name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("stumble-{test_name}-{}", Uuid::now_v7()))
    }

    fn populated_legacy_store() -> InMemoryStore {
        let store = crate::seeds::seed_store();
        let local_node_id = store
            .node_identities
            .values()
            .find(|node| node.tenant_id.is_none())
            .unwrap()
            .id;
        let user_id = *store.users.keys().next().unwrap();
        let tools = crate::AgentTools::new(store);
        let ctx = AuthContext {
            user_id: Some(user_id),
            tenant_id: None,
            node_id: local_node_id,
        };
        tools
            .create_pod(
                &ctx,
                CreatePodRequest {
                    name: "Legacy Pod".to_string(),
                    slug: "legacy-pod".to_string(),
                    description: "Existing curation".to_string(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();
        let submission = tools
            .submit_link_to_pod(
                &ctx,
                "legacy-pod",
                SubmitLinkRequest {
                    url: "https://example.com/legacy".to_string(),
                    title: Some("Legacy Item".to_string()),
                    description: Some("Existing submission".to_string()),
                    note: Some("Keep this provenance".to_string()),
                    tags: vec!["legacy".to_string()],
                    discovered_by_crawler: false,
                },
            )
            .unwrap();
        tools.save_link(&ctx, submission.id).unwrap();
        tools
            .generate_brief(
                &ctx,
                GenerateBriefRequest {
                    pod_slugs: vec!["legacy-pod".to_string()],
                    query: Some("legacy".to_string()),
                    user_id: Some(user_id),
                },
            )
            .unwrap();
        tools.store().read().unwrap().clone()
    }

    #[test]
    fn sqlite_home_node_initializes_and_restarts() {
        let dir = temp_store_dir("sqlite-restart");
        let database_path = dir.join("stumble.sqlite3");
        let legacy_path = dir.join("store.json");

        let first = load_or_initialize_sqlite_store(&database_path, &legacy_path, || {
            crate::seeds::seed_store()
        })
        .unwrap();
        let first_node_id = first
            .node_identities
            .values()
            .find(|node| node.tenant_id.is_none())
            .unwrap()
            .id;
        let restarted =
            load_or_initialize_sqlite_store(&database_path, &legacy_path, InMemoryStore::default)
                .unwrap();

        assert!(database_path.exists());
        assert_eq!(
            restarted
                .node_identities
                .values()
                .find(|node| node.tenant_id.is_none())
                .unwrap()
                .id,
            first_node_id
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sqlite_transactions_preserve_writes_from_separate_home_node_instances() {
        let dir = temp_store_dir("sqlite-concurrent-writes");
        let database_path = dir.join("stumble.sqlite3");
        let legacy_path = dir.join("store.json");
        let first_store =
            load_or_initialize_sqlite_store(&database_path, &legacy_path, crate::seeds::seed_store)
                .unwrap();
        let second_store = load_sqlite_store(&database_path).unwrap();
        let first = crate::AgentTools::new_sqlite_persistent(first_store, &database_path);
        let second = crate::AgentTools::new_sqlite_persistent(second_store, &database_path);
        let local_node_id = first
            .store()
            .read()
            .unwrap()
            .node_identities
            .values()
            .find(|node| node.tenant_id.is_none())
            .unwrap()
            .id;
        let first_ctx = AuthContext {
            user_id: None,
            tenant_id: None,
            node_id: local_node_id,
        };
        let second_ctx = first_ctx.clone();

        first
            .create_pod(
                &first_ctx,
                CreatePodRequest {
                    name: "First Pod".to_string(),
                    slug: "first-pod".to_string(),
                    description: "Written by the first process".to_string(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();
        second
            .create_pod(
                &second_ctx,
                CreatePodRequest {
                    name: "Second Pod".to_string(),
                    slug: "second-pod".to_string(),
                    description: "Written by the second process".to_string(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();

        let restarted = load_sqlite_store(&database_path).unwrap();
        assert!(restarted.pod_by_slug("first-pod", None).is_ok());
        assert!(restarted.pod_by_slug("second-pod", None).is_ok());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sqlite_rejects_a_stale_write_to_the_same_record() {
        let dir = temp_store_dir("sqlite-conflicting-writes");
        let database_path = dir.join("stumble.sqlite3");
        let legacy_path = dir.join("store.json");
        let first_store =
            load_or_initialize_sqlite_store(&database_path, &legacy_path, crate::seeds::seed_store)
                .unwrap();
        let user_id = *first_store.users.keys().next().unwrap();
        let local_node_id = first_store.default_node().unwrap().id;
        let second_store = load_sqlite_store(&database_path).unwrap();
        let first = crate::AgentTools::new_sqlite_persistent(first_store, &database_path);
        let second = crate::AgentTools::new_sqlite_persistent(second_store, &database_path);
        let ctx = AuthContext {
            user_id: Some(user_id),
            tenant_id: None,
            node_id: local_node_id,
        };

        first
            .update_preferences(
                &ctx,
                UpdatePreferencesRequest {
                    interests: Some(vec!["first writer".to_string()]),
                    blocked_topics: None,
                    blocked_sources: None,
                    preferred_brief_length: None,
                    preferred_discovery_mode: None,
                },
            )
            .unwrap();
        let error = second
            .update_preferences(
                &ctx,
                UpdatePreferencesRequest {
                    interests: Some(vec!["stale writer".to_string()]),
                    blocked_topics: None,
                    blocked_sources: None,
                    preferred_brief_length: None,
                    preferred_discovery_mode: None,
                },
            )
            .unwrap_err();

        assert!(matches!(
            error,
            crate::AgentToolsError::Persistence(
                StorePersistenceError::ConcurrentWriteConflict { .. }
            )
        ));
        assert_eq!(
            second
                .store()
                .read()
                .unwrap()
                .user_preferences
                .get(&(user_id, None))
                .unwrap()
                .interests,
            vec!["first writer"]
        );
        second
            .update_preferences(
                &ctx,
                UpdatePreferencesRequest {
                    interests: Some(vec!["retried writer".to_string()]),
                    blocked_topics: None,
                    blocked_sources: None,
                    preferred_brief_length: None,
                    preferred_discovery_mode: None,
                },
            )
            .unwrap();
        let restarted = load_sqlite_store(&database_path).unwrap();
        assert_eq!(
            restarted
                .user_preferences
                .get(&(user_id, None))
                .unwrap()
                .interests,
            vec!["retried writer"]
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_json_imports_once_with_a_recoverable_backup() {
        let dir = temp_store_dir("sqlite-legacy-import");
        let database_path = dir.join("stumble.sqlite3");
        let legacy_path = dir.join("store.json");
        let original = populated_legacy_store();
        save_store_snapshot(&original, &legacy_path).unwrap();

        let imported =
            load_or_initialize_sqlite_store(&database_path, &legacy_path, InMemoryStore::default)
                .unwrap();
        assert_eq!(
            store_records(&imported).unwrap(),
            store_records(&original).unwrap()
        );
        assert!(legacy_path.with_extension("json.migrated.bak").exists());

        save_store_snapshot(&InMemoryStore::default(), &legacy_path).unwrap();
        let restarted =
            load_or_initialize_sqlite_store(&database_path, &legacy_path, InMemoryStore::default)
                .unwrap();
        assert_eq!(
            store_records(&restarted).unwrap(),
            store_records(&original).unwrap()
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_legacy_json_leaves_sqlite_empty_and_can_be_retried() {
        let dir = temp_store_dir("sqlite-malformed-legacy");
        let database_path = dir.join("stumble.sqlite3");
        let legacy_path = dir.join("store.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&legacy_path, b"{ not valid json").unwrap();

        assert!(matches!(
            load_or_initialize_sqlite_store(&database_path, &legacy_path, InMemoryStore::default),
            Err(StorePersistenceError::Json(_))
        ));
        assert!(store_records(&load_sqlite_store(&database_path).unwrap())
            .unwrap()
            .is_empty());

        let recoverable = populated_legacy_store();
        save_store_snapshot(&recoverable, &legacy_path).unwrap();
        let imported =
            load_or_initialize_sqlite_store(&database_path, &legacy_path, InMemoryStore::default)
                .unwrap();
        assert_eq!(
            store_records(&imported).unwrap(),
            store_records(&recoverable).unwrap()
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_import_refuses_a_populated_database_without_migration_metadata() {
        let dir = temp_store_dir("sqlite-uninitialized-populated");
        let database_path = dir.join("stumble.sqlite3");
        let legacy_path = dir.join("store.json");
        std::fs::create_dir_all(&dir).unwrap();
        save_store_snapshot(&populated_legacy_store(), &legacy_path).unwrap();
        let connection = open_sqlite_store(&database_path).unwrap();
        connection
            .execute(
                "INSERT INTO stumble_store_records (collection, record_key, value_json)
                 VALUES ('unknown', 'existing', '{\"preserve\":true}')",
                [],
            )
            .unwrap();

        assert!(matches!(
            load_or_initialize_sqlite_store(&database_path, &legacy_path, InMemoryStore::default),
            Err(StorePersistenceError::PopulatedUninitializedDatabase)
        ));
        let existing: String = connection
            .query_row(
                "SELECT value_json FROM stumble_store_records WHERE record_key = 'existing'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(existing, "{\"preserve\":true}");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn snapshot_round_trips_seeded_store() {
        let store = crate::seeds::seed_store();
        let dir = std::env::temp_dir().join(format!("stumble-store-test-{}", Uuid::now_v7()));
        let path = dir.join("store.json");

        save_store_snapshot(&store, &path).unwrap();
        let loaded = load_store_snapshot(&path).unwrap();

        assert_eq!(loaded.pods.len(), store.pods.len());
        assert_eq!(loaded.node_identities.len(), store.node_identities.len());
        assert_eq!(loaded.user_preferences.len(), store.user_preferences.len());
        assert!(loaded.pods.is_empty());

        let _ = std::fs::remove_dir_all(dir);
    }
}
