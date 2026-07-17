use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("stumble-legacy-migration-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        // Best-effort test cleanup; a failure must not hide migration evidence.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn legacy_json_migration_preserves_identity_placements_feedback_and_events() {
    let data_dir = TestDataDir::new();
    let legacy_tools = AgentTools::new(seed_store());
    let legacy_context = legacy_tools.default_auth_context().unwrap();
    legacy_tools
        .create_pod(
            &legacy_context,
            CreatePodRequest {
                name: "Legacy Pod".into(),
                slug: "legacy-pod".into(),
                description: "Legacy persisted placement".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    legacy_tools
        .submit_link_to_pod(
            &legacy_context,
            "legacy-pod",
            SubmitLinkRequest {
                url: "https://legacy.example/canonical-item".into(),
                title: Some("Legacy canonical item".into()),
                description: None,
                note: Some("Legacy placement evidence".into()),
                tags: vec!["legacy".into()],
                discovered_by_crawler: true,
            },
        )
        .unwrap();
    let mut legacy = legacy_tools.store().read().unwrap().clone();
    let user_id = uuid::Uuid::now_v7();
    legacy.users.insert(
        user_id,
        User {
            id: user_id,
            display_name: "Legacy User".into(),
            created_at: chrono::Utc::now(),
        },
    );
    let submission_id = *legacy.submissions.keys().next().unwrap();
    legacy.feedback_events.push(FeedbackEvent {
        user_id,
        tenant_id: None,
        submission_id,
        event_type: FeedbackKind::Saved,
        reason: Some("legacy feedback evidence".into()),
        created_at: chrono::Utc::now(),
        local_only: true,
    });
    let content_ids = legacy.submissions.keys().copied().collect::<Vec<_>>();
    let placements = legacy.submission_pods.clone();
    let feedback = legacy.feedback_events.clone();
    let events = legacy.event_log.clone();
    assert!(!content_ids.is_empty());
    assert!(!placements.is_empty());
    assert!(!feedback.is_empty());
    assert!(!events.is_empty());
    save_store_snapshot(&legacy, &data_dir.0.join("store.json")).unwrap();

    let migrated = AgentTools::open_home_node(&data_dir.0, InMemoryStore::default).unwrap();
    let store = migrated.store();
    let store = store.read().unwrap();

    assert!(content_ids
        .iter()
        .all(|id| store.submissions.contains_key(id)));
    assert_eq!(
        serde_json::to_value(&store.submission_pods).unwrap(),
        serde_json::to_value(placements).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&store.feedback_events).unwrap(),
        serde_json::to_value(feedback).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&store.event_log).unwrap(),
        serde_json::to_value(events).unwrap()
    );
}
