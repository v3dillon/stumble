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
    let preference_key = *legacy.user_preferences.keys().next().unwrap();
    legacy
        .user_preferences
        .get_mut(&preference_key)
        .unwrap()
        .blocked_sources = vec!["legacy.example".into()];
    let content_ids = legacy.submissions.keys().copied().collect::<Vec<_>>();
    let candidate_id: CandidateId = uuid::Uuid::now_v7().into();
    legacy.candidates.insert(
        candidate_id,
        Candidate {
            id: candidate_id,
            tenant_id: None,
            source_url: "https://legacy.example/candidate".into(),
            canonical_url: "https://legacy.example/candidate".into(),
            review_state: CandidateReviewState::Pending,
            created_at: chrono::Utc::now(),
        },
    );
    let placements = legacy.submission_pods.clone();
    let feedback = legacy.feedback_events.clone();
    let events = legacy.event_log.clone();
    assert!(!content_ids.is_empty());
    assert!(!placements.is_empty());
    assert!(!feedback.is_empty());
    assert!(!events.is_empty());
    let legacy_path = data_dir.0.join("store.json");
    save_store_snapshot(&legacy, &legacy_path).unwrap();
    let mut legacy_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&legacy_path).unwrap()).unwrap();
    let legacy_candidate = legacy_json["candidates"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|candidate| candidate["id"] == candidate_id.to_string())
        .unwrap();
    legacy_candidate["source_url"] =
        serde_json::json!("https://legacy.example/candidate?utm_source=private-migration#secret");
    for preferences in legacy_json["user_preferences"].as_array_mut().unwrap() {
        preferences
            .as_object_mut()
            .unwrap()
            .remove("blocked_source_affinities");
    }
    std::fs::write(
        &legacy_path,
        serde_json::to_vec_pretty(&legacy_json).unwrap(),
    )
    .unwrap();

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
    assert_eq!(
        store.candidates[&candidate_id].source_url,
        "https://legacy.example/candidate"
    );
    assert_eq!(
        store.user_preferences[&preference_key].blocked_sources,
        vec!["legacy.example"]
    );
    assert!(store.user_preferences[&preference_key]
        .blocked_source_affinities
        .is_empty());
}
