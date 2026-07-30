use super::migrations::{persist_migrated_records, LegacyPodMembership, LegacyPodRole};
use super::sqlite::initialize_sqlite_store;
use super::*;
use std::time::Duration;

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
        harness_id: None,
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

/// Two byte-identical feedback events plus a distinct third, to prove ordered
/// log collections keep duplicates and order through persistence. Returns the
/// resulting total count.
fn push_duplicate_feedback(store: &mut InMemoryStore) -> usize {
    let user_id = *store.users.keys().next().unwrap();
    let submission_id = *store.submissions.keys().next().unwrap();
    let created_at = chrono::Utc::now();
    let event = FeedbackEvent {
        user_id,
        tenant_id: None,
        submission_id,
        event_type: FeedbackKind::Interesting,
        reason: Some("duplicate on purpose".to_string()),
        created_at,
        local_only: true,
    };
    store.feedback_events.push(event.clone());
    store.feedback_events.push(event);
    store.feedback_events.push(FeedbackEvent {
        user_id,
        tenant_id: None,
        submission_id,
        event_type: FeedbackKind::Dismissed,
        reason: None,
        created_at,
        local_only: true,
    });
    store.feedback_events.len()
}

#[test]
fn sqlite_home_node_initializes_and_restarts() {
    let dir = temp_store_dir("sqlite-restart");
    let database_path = dir.join("stumble.sqlite3");

    let first =
        load_or_initialize_sqlite_store(&database_path, || crate::seeds::seed_store()).unwrap();
    let first_node_id = first
        .node_identities
        .values()
        .find(|node| node.tenant_id.is_none())
        .unwrap()
        .id;
    let restarted =
        load_or_initialize_sqlite_store(&database_path, InMemoryStore::default).unwrap();

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
fn sqlite_round_trips_a_populated_store_exactly() {
    let dir = temp_store_dir("sqlite-full-round-trip");
    std::fs::create_dir_all(&dir).unwrap();
    let database_path = dir.join("stumble.sqlite3");
    let mut store = populated_legacy_store();
    let feedback_count = push_duplicate_feedback(&mut store);
    assert!(!store.pods.is_empty());
    assert!(!store.submissions.is_empty());
    assert!(!store.event_log.is_empty());

    let mut connection = open_sqlite_store(&database_path).unwrap();
    initialize_sqlite_store(&mut connection, &store).unwrap();
    drop(connection);
    let loaded = load_sqlite_store(&database_path).unwrap();

    // Canonical record maps compare every collection, key, and value —
    // including positional order for the log collections.
    assert_eq!(
        store_records(&store).unwrap(),
        store_records(&loaded).unwrap()
    );
    assert_eq!(loaded.feedback_events.len(), feedback_count);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn legacy_content_keyed_log_rows_are_rekeyed_positionally_on_load() {
    let dir = temp_store_dir("sqlite-positional-rekey");
    std::fs::create_dir_all(&dir).unwrap();
    let database_path = dir.join("stumble.sqlite3");
    let mut store = populated_legacy_store();
    let feedback_count = push_duplicate_feedback(&mut store);
    let mut connection = open_sqlite_store(&database_path).unwrap();
    initialize_sqlite_store(&mut connection, &store).unwrap();

    // Rewrite the feedback rows the way the old code keyed them: by their
    // whole serialized value, which also collapses the duplicate pair.
    connection
        .execute(
            "DELETE FROM stumble_store_records WHERE collection = 'feedback_events'",
            [],
        )
        .unwrap();
    for event in &store.feedback_events {
        let value_json = serde_json::to_string(event).unwrap();
        connection
            .execute(
                "INSERT INTO stumble_store_records (collection, record_key, value_json)
                 VALUES ('feedback_events', ?1, ?2)
                 ON CONFLICT (collection, record_key) DO UPDATE SET value_json = excluded.value_json",
                rusqlite::params![value_json, value_json],
            )
            .unwrap();
    }
    drop(connection);

    let loaded = load_sqlite_store(&database_path).unwrap();
    // The duplicate pair collapsed under the legacy keying; the survivors
    // load and the rows are rewritten under canonical positional keys.
    assert_eq!(loaded.feedback_events.len(), feedback_count - 1);
    let connection = open_sqlite_store(&database_path).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT record_key FROM stumble_store_records
             WHERE collection = 'feedback_events' ORDER BY record_key",
        )
        .unwrap();
    let keys: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let expected: Vec<String> = (0..feedback_count - 1)
        .map(|i| format!("{i:020}"))
        .collect();
    assert_eq!(keys, expected);
    drop(statement);
    drop(connection);

    // A second load must not migrate again.
    let reloaded = load_sqlite_store(&database_path).unwrap();
    assert_eq!(
        store_records(&loaded).unwrap(),
        store_records(&reloaded).unwrap()
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn sqlite_transactions_preserve_writes_from_separate_home_node_instances() {
    let dir = temp_store_dir("sqlite-concurrent-writes");
    let database_path = dir.join("stumble.sqlite3");
    let first_store =
        load_or_initialize_sqlite_store(&database_path, crate::seeds::seed_store).unwrap();
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
        harness_id: None,
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
    let first_store =
        load_or_initialize_sqlite_store(&database_path, crate::seeds::seed_store).unwrap();
    let user_id = *first_store.users.keys().next().unwrap();
    let local_node_id = first_store.default_node().unwrap().id;
    let second_store = load_sqlite_store(&database_path).unwrap();
    let first = crate::AgentTools::new_sqlite_persistent(first_store, &database_path);
    let second = crate::AgentTools::new_sqlite_persistent(second_store, &database_path);
    let ctx = AuthContext {
        user_id: Some(user_id),
        tenant_id: None,
        node_id: local_node_id,
        harness_id: None,
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
        crate::AgentToolsError::Persistence(StorePersistenceError::ConcurrentWriteConflict { .. })
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
fn two_connections_prevent_stale_discovery_task_migration_overwrite() {
    let dir = temp_store_dir("sqlite-discovery-task-migration-race");
    let database_path = dir.join("stumble.sqlite3");
    let mut store = crate::seeds::seed_store();
    let pod_id = Uuid::now_v7();
    let now = chrono::Utc::now();
    let task = DiscoveryTask {
        id: Uuid::now_v7().into(),
        target: DiscoveryTaskTarget::Pod {
            pod_id,
            package_version: PackageVersion::new(1).unwrap(),
        },
        origin: DiscoveryTaskOrigin::Scheduled {
            source_rule_index: 0,
        },
        due_at: now,
        state: DiscoveryTaskState::Pending,
        attempts: Vec::new(),
        created_at: now,
    };
    store.discovery_tasks.insert(task.id, task.clone());
    load_or_initialize_sqlite_store(&database_path, || store.clone()).unwrap();

    let records = store_records(&store).unwrap();
    let ((_, record_key), canonical_json) = records
        .iter()
        .find(|((collection, _), _)| collection == "discovery_tasks")
        .unwrap();
    let mut legacy_task: serde_json::Value = serde_json::from_str(canonical_json).unwrap();
    legacy_task
        .as_object_mut()
        .unwrap()
        .remove("target")
        .unwrap();
    let legacy_json = serde_json::to_string(&legacy_task).unwrap();
    let mut first = rusqlite::Connection::open(&database_path).unwrap();
    first
        .execute(
            "UPDATE stumble_store_records SET value_json = ?1
                 WHERE collection = 'discovery_tasks' AND record_key = ?2",
            rusqlite::params![legacy_json, record_key],
        )
        .unwrap();

    let mut lifecycle_update = legacy_task;
    lifecycle_update["state"] = serde_json::json!({"status": "completed"});
    let lifecycle_json = serde_json::to_string(&lifecycle_update).unwrap();
    let second = rusqlite::Connection::open(&database_path).unwrap();
    second.busy_timeout(Duration::from_millis(1)).unwrap();
    let transaction = first
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let competing_write = second.execute(
        "UPDATE stumble_store_records SET value_json = ?1
                 WHERE collection = 'discovery_tasks' AND record_key = ?2",
        rusqlite::params![lifecycle_json, record_key],
    );
    assert!(matches!(
        competing_write,
        Err(rusqlite::Error::SqliteFailure(error, _))
            if matches!(
                error.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    ));

    persist_migrated_records(
        &transaction,
        &records,
        &[("discovery_tasks".to_string(), record_key.clone())],
    )
    .unwrap();
    transaction.commit().unwrap();
    second
        .execute(
            "UPDATE stumble_store_records SET value_json = ?1
                 WHERE collection = 'discovery_tasks' AND record_key = ?2",
            rusqlite::params![lifecycle_json, record_key],
        )
        .unwrap();
    let persisted: String = second
        .query_row(
            "SELECT value_json FROM stumble_store_records
                 WHERE collection = 'discovery_tasks' AND record_key = ?1",
            [record_key],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(persisted, lifecycle_json);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn legacy_sqlite_pod_memberships_rewrite_once_before_restart() {
    let dir = temp_store_dir("sqlite-pod-relationship-migration");
    let database_path = dir.join("stumble.sqlite3");
    std::fs::create_dir_all(&dir).unwrap();
    let original = populated_legacy_store();
    let user_id = *original.users.keys().next().unwrap();
    let pod_id = original.pod_by_slug("legacy-pod", None).unwrap().id;
    let created_at = original
        .pod_roles
        .iter()
        .find(|assignment| assignment.user_id == user_id && assignment.pod_id == pod_id)
        .unwrap()
        .created_at;
    let legacy_membership = LegacyPodMembership {
        user_id,
        pod_id,
        role: LegacyPodRole::Moderator,
        is_priority: true,
        created_at,
    };
    let mut connection = open_sqlite_store(&database_path).unwrap();
    initialize_sqlite_store(&mut connection, &original).unwrap();
    connection
        .execute(
            "DELETE FROM stumble_store_records WHERE collection = 'pod_roles'",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO stumble_store_records (collection, record_key, value_json)
                 VALUES ('pod_memberships', ?1, ?2)",
            rusqlite::params![
                serde_json::to_string(&[user_id, pod_id]).unwrap(),
                serde_json::to_string(&legacy_membership).unwrap()
            ],
        )
        .unwrap();
    drop(connection);

    let migrated = load_sqlite_store(&database_path).unwrap();
    assert!(migrated.pod_roles.iter().any(|assignment| {
        assignment.user_id == user_id
            && assignment.pod_id == pod_id
            && assignment.role == PodRole::Curator
    }));
    assert!(migrated.subscriptions.values().any(|subscription| {
        subscription.user_id == user_id
            && subscription.local_pod_id == pod_id
            && subscription.is_priority
    }));
    let connection = open_sqlite_store(&database_path).unwrap();
    let legacy_rows: usize = connection
        .query_row(
            "SELECT COUNT(*) FROM stumble_store_records WHERE collection = 'pod_memberships'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(legacy_rows, 0);
    drop(connection);

    let restarted = load_sqlite_store(&database_path).unwrap();
    assert_eq!(restarted.pod_roles, migrated.pod_roles);
    assert_eq!(restarted.subscriptions, migrated.subscriptions);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn refuses_a_populated_database_without_migration_metadata() {
    let dir = temp_store_dir("sqlite-uninitialized-populated");
    let database_path = dir.join("stumble.sqlite3");
    std::fs::create_dir_all(&dir).unwrap();
    let connection = open_sqlite_store(&database_path).unwrap();
    connection
        .execute(
            "INSERT INTO stumble_store_records (collection, record_key, value_json)
                 VALUES ('unknown', 'existing', '{\"preserve\":true}')",
            [],
        )
        .unwrap();

    assert!(matches!(
        load_or_initialize_sqlite_store(&database_path, InMemoryStore::default),
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
fn snapshot_round_trips_a_populated_store_exactly() {
    let mut store = populated_legacy_store();
    let feedback_count = push_duplicate_feedback(&mut store);
    assert!(!store.pods.is_empty());
    let dir = std::env::temp_dir().join(format!("stumble-store-test-{}", Uuid::now_v7()));
    let path = dir.join("store.json");

    save_store_snapshot(&store, &path).unwrap();
    let loaded = load_store_snapshot(&path).unwrap();

    assert_eq!(
        store_records(&store).unwrap(),
        store_records(&loaded).unwrap()
    );
    assert_eq!(loaded.feedback_events.len(), feedback_count);

    let _ = std::fs::remove_dir_all(dir);
}
