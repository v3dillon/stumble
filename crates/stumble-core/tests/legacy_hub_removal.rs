//! Acceptance coverage for ticket 09: remove the legacy Hub completely.

use rusqlite::Connection;
use serde_json::json;
use std::path::PathBuf;
use stumble_core::*;
use uuid::Uuid;

struct TestDataDir(PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-legacy-hub-removal-{label}-{}",
            Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn database_path(&self) -> PathBuf {
        self.0.join("stumble.sqlite3")
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn owner_context(tools: &AgentTools) -> AuthContext {
    tools
        .local_owner_auth_context()
        .expect("local owner context")
}

#[test]
fn forward_sqlite_migration_drops_hub_caches_and_preserves_unrelated_state() {
    let data_dir = TestDataDir::new("drop-hub");
    let tools = AgentTools::initialize_home_node(&data_dir.0, seed_store).unwrap();
    let ctx = owner_context(&tools);
    let node_id = ctx.node_id;
    let user_id = ctx.user_id.expect("seed user");
    tools
        .create_pod(
            &ctx,
            CreatePodRequest {
                name: "Keep Me".into(),
                slug: "keep-me".into(),
                description: "must survive hub drop".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let before = tools.store().read().unwrap().clone();
    assert!(before.pods.values().any(|pod| pod.slug == "keep-me"));
    assert!(before.node_identities.contains_key(&node_id));
    assert!(before.users.contains_key(&user_id));
    let event_count = before.event_log.len();
    let subscription_count = before.subscriptions.len();
    let feedback_count = before.feedback_events.len();
    drop(tools);

    // Reintroduce legacy Hub SQL tables and document collections after initialize.
    // The forward migration must drop these without transforming contents.
    {
        let connection = Connection::open(data_dir.database_path()).unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE hub_registered_nodes (
                  node_id TEXT PRIMARY KEY,
                  display_name TEXT NOT NULL,
                  base_url TEXT NOT NULL,
                  public_key TEXT NOT NULL,
                  protocol_version TEXT NOT NULL,
                  registered_at TEXT NOT NULL,
                  last_seen_at TEXT NOT NULL
                );
                CREATE TABLE hub_registered_pods (
                  id TEXT PRIMARY KEY,
                  node_id TEXT NOT NULL,
                  node_base_url TEXT NOT NULL,
                  pod_slug TEXT NOT NULL,
                  pod_name TEXT NOT NULL,
                  description TEXT NOT NULL,
                  tags TEXT NOT NULL DEFAULT '[]',
                  skill_pack_version INTEGER NOT NULL,
                  latest_event_hash TEXT NULL,
                  manifest_url TEXT NOT NULL,
                  events_url TEXT NOT NULL,
                  registered_at TEXT NOT NULL,
                  updated_at TEXT NOT NULL
                );
                INSERT INTO hub_registered_nodes (
                  node_id, display_name, base_url, public_key, protocol_version,
                  registered_at, last_seen_at
                ) VALUES (
                  'legacy-node', 'Legacy', 'https://legacy.example', 'pk',
                  'stumble/1.0', '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z'
                );
                INSERT INTO hub_registered_pods (
                  id, node_id, node_base_url, pod_slug, pod_name, description, tags,
                  skill_pack_version, latest_event_hash, manifest_url, events_url,
                  registered_at, updated_at
                ) VALUES (
                  'legacy-pod', 'legacy-node', 'https://legacy.example', 'legacy',
                  'Legacy Pod', 'cache only', '[]', 1, NULL,
                  'https://legacy.example/manifest',
                  'https://legacy.example/events',
                  '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z'
                );
                ",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO stumble_store_records (collection, record_key, value_json)
                 VALUES ('hub_nodes', ?1, ?2)",
                rusqlite::params![
                    json!([node_id]).to_string(),
                    json!({
                        "node_id": node_id,
                        "display_name": "Legacy Cache Node",
                        "base_url": "https://legacy.example",
                        "public_key": "pk",
                        "protocol_version": "stumble/1.0",
                        "registered_at": "2020-01-01T00:00:00Z",
                        "last_seen_at": "2020-01-01T00:00:00Z",
                    })
                    .to_string()
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO stumble_store_records (collection, record_key, value_json)
                 VALUES ('hub_pods', ?1, ?2)",
                rusqlite::params![
                    json!([node_id, "legacy"]).to_string(),
                    json!({
                        "id": Uuid::now_v7(),
                        "node_id": node_id,
                        "node_base_url": "https://legacy.example",
                        "pod_slug": "legacy",
                        "pod_name": "Legacy",
                        "description": "cache",
                        "tags": [],
                        "skill_pack_version": 1,
                        "latest_event_hash": null,
                        "manifest_url": "https://legacy.example/m",
                        "events_url": "https://legacy.example/e",
                        "registered_at": "2020-01-01T00:00:00Z",
                        "updated_at": "2020-01-01T00:00:00Z",
                    })
                    .to_string()
                ],
            )
            .unwrap();
    }

    // Re-open through the normal Home Node path; open_sqlite_store re-applies 0003.
    let migrated = AgentTools::open_home_node(&data_dir.0, InMemoryStore::default).unwrap();
    let store = migrated.store().read().unwrap().clone();

    assert_eq!(store.default_node().unwrap().id, node_id);
    assert!(store.users.contains_key(&user_id));
    assert!(store.pods.values().any(|pod| pod.slug == "keep-me"));
    assert_eq!(store.event_log.len(), event_count);
    assert_eq!(store.subscriptions.len(), subscription_count);
    assert_eq!(store.feedback_events.len(), feedback_count);
    assert_eq!(store.api_tokens.len(), before.api_tokens.len());

    let connection = Connection::open(data_dir.database_path()).unwrap();
    let hub_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table'
               AND name IN ('hub_registered_nodes', 'hub_registered_pods')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(hub_tables, 0);
    let hub_records: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM stumble_store_records
             WHERE collection IN ('hub_nodes', 'hub_pods')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(hub_records, 0);
    let marker: String = connection
        .query_row(
            "SELECT value FROM stumble_store_metadata WHERE key = 'schema_legacy_hub_removed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(marker, "1");
}

#[test]
fn new_sqlite_deployments_never_create_hub_tables() {
    let data_dir = TestDataDir::new("new-deploy");
    AgentTools::initialize_home_node(&data_dir.0, seed_store).unwrap();
    let connection = Connection::open(data_dir.database_path()).unwrap();
    let hub_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table'
               AND name IN ('hub_registered_nodes', 'hub_registered_pods')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(hub_tables, 0);
    let hub_records: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM stumble_store_records WHERE collection LIKE 'hub_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(hub_records, 0);
}

#[test]
fn well_known_metadata_contains_no_hub_terminology() {
    let tools = AgentTools::new(seed_store());
    let ctx = owner_context(&tools);
    let well_known = tools.well_known_node(&ctx, "https://node.example").unwrap();
    let encoded = serde_json::to_string(&well_known).unwrap();
    assert!(!encoded.to_lowercase().contains("hub"));
    assert!(!well_known.endpoints.contains_key("hub_search_pods"));
    assert!(well_known.endpoints.contains_key("node"));
    assert!(well_known.endpoints.contains_key("pods"));
}

#[test]
fn explore_surface_uses_substrate_types_without_hub_fields() {
    let tools = AgentTools::new(seed_store());
    let ctx = owner_context(&tools);
    let response = tools
        .explore_public_pods(&ctx, ExploreRequest::new("systems", 5, 0).unwrap())
        .unwrap();
    let encoded = serde_json::to_string(&response).unwrap();
    assert!(!encoded.to_lowercase().contains("hub"));
    assert_eq!(response.query, "systems");
}

#[test]
fn authoritative_store_schema_source_contains_no_hub_tables() {
    let schema = include_str!("../../../migrations/sqlite/0002_authoritative_store.sql");
    assert!(!schema.contains("hub_registered_nodes"));
    assert!(!schema.contains("hub_registered_pods"));
    assert!(!schema.to_lowercase().contains("hub_"));
    // Forward cleanup may name hub tables only as DROP targets.
    let drop_legacy = include_str!("../../../migrations/sqlite/0003_drop_legacy_hub.sql");
    assert!(drop_legacy.contains("DROP TABLE IF EXISTS hub_registered_nodes"));
    assert!(drop_legacy.contains("DROP TABLE IF EXISTS hub_registered_pods"));
}
