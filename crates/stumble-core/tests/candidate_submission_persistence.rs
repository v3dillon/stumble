mod support;

use stumble_core::*;
use support::{
    candidate_harness, candidate_submission_request, create_candidate_test_pod, TestDataDir,
};

#[test]
fn changed_input_cannot_reuse_either_idempotency_key() {
    let tools = AgentTools::new(seed_store());
    let pod = create_candidate_test_pod(&tools, "conflicting-candidates");
    let harness = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let request = candidate_submission_request(&[pod.id]);
    tools.submit_candidate(&harness, request.clone()).unwrap();
    let mut changed = request;
    changed.evidence.summary = Some("changed evidence".into());

    assert!(matches!(
        tools.submit_candidate(&harness, changed),
        Err(AgentToolsError::CandidateIdempotencyConflict)
    ));
}

#[test]
fn concurrent_idempotent_submissions_cannot_create_duplicate_candidates() {
    let data_dir = TestDataDir::new("concurrent-candidate-submissions");
    let setup = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let pod = create_candidate_test_pod(&setup, "concurrent-candidates");
    let issued = setup
        .register_agent_harness(
            &setup.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "concurrent candidate worker".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::CandidateSubmission],
                pod_ids: Some(vec![pod.id]),
            },
        )
        .unwrap();
    let token = issued.token.expose().to_string();
    drop(setup);

    let authenticator = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let ctx = authenticator.authenticate_token(&token).unwrap().unwrap();
    drop(authenticator);
    let first = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let second = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let request = candidate_submission_request(&[pod.id]);
    let submitted = first.submit_candidate(&ctx, request.clone()).unwrap();
    assert!(matches!(
        second.submit_candidate(&ctx, request.clone()),
        Err(AgentToolsError::Persistence(
            StorePersistenceError::ConcurrentWriteConflict { .. }
        ))
    ));
    drop(first);
    drop(second);

    let reopened = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let ctx = reopened.authenticate_token(&token).unwrap().unwrap();
    let retry = reopened.submit_candidate(&ctx, request).unwrap();
    assert_eq!(retry.submission.id, submitted.submission.id);
    assert_eq!(
        reopened
            .inspect_candidate(&ctx, submitted.candidate.id)
            .unwrap()
            .submissions
            .len(),
        1
    );
}

#[test]
fn candidate_and_idempotency_evidence_survive_sqlite_restart() {
    let data_dir = TestDataDir::new("candidate-submission-persistence");
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let pod = create_candidate_test_pod(&tools, "persistent-candidates");
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "persistent candidate worker".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::CandidateSubmission],
                pod_ids: Some(vec![pod.id]),
            },
        )
        .unwrap();
    let token = issued.token.expose().to_string();
    let request = candidate_submission_request(&[pod.id]);
    let harness = tools.authenticate_token(&token).unwrap().unwrap();
    let submitted = tools.submit_candidate(&harness, request.clone()).unwrap();
    drop(tools);

    let database_path = data_dir.0.join("stumble.sqlite3");
    let connection = rusqlite::Connection::open(&database_path).unwrap();
    let record_key = serde_json::to_string(&[submitted.submission.id]).unwrap();
    let value_json: String = connection
        .query_row(
            "SELECT value_json FROM stumble_store_records
             WHERE collection = 'candidate_submissions' AND record_key = ?1",
            [&record_key],
            |row| row.get(0),
        )
        .unwrap();
    let mut legacy_submission: serde_json::Value = serde_json::from_str(&value_json).unwrap();
    let record = legacy_submission.as_object_mut().unwrap();
    let mut target = record.remove("target").unwrap();
    let placements = target.get_mut("placements").unwrap().take();
    let task_context = target.get_mut("task_context").unwrap().take();
    record.insert("proposed_placements".into(), placements);
    record.insert("task_context".into(), task_context);
    connection
        .execute(
            "UPDATE stumble_store_records SET value_json = ?1
             WHERE collection = 'candidate_submissions' AND record_key = ?2",
            rusqlite::params![
                serde_json::to_string(&legacy_submission).unwrap(),
                record_key
            ],
        )
        .unwrap();
    let candidate_record_key = serde_json::to_string(&[submitted.candidate.id]).unwrap();
    let candidate_value_json: String = connection
        .query_row(
            "SELECT value_json FROM stumble_store_records
             WHERE collection = 'candidates' AND record_key = ?1",
            [&candidate_record_key],
            |row| row.get(0),
        )
        .unwrap();
    let mut legacy_candidate: serde_json::Value =
        serde_json::from_str(&candidate_value_json).unwrap();
    legacy_candidate["source_url"] =
        serde_json::json!("https://example.com/report?utm_source=legacy-private#secret");
    connection
        .execute(
            "UPDATE stumble_store_records SET value_json = ?1
             WHERE collection = 'candidates' AND record_key = ?2",
            rusqlite::params![
                serde_json::to_string(&legacy_candidate).unwrap(),
                candidate_record_key
            ],
        )
        .unwrap();
    drop(connection);

    let reopened = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let harness = reopened.authenticate_token(&token).unwrap().unwrap();
    let retry = reopened.submit_candidate(&harness, request).unwrap();
    let inspected = reopened
        .inspect_candidate(&harness, submitted.candidate.id)
        .unwrap();

    assert_eq!(retry.submission.id, submitted.submission.id);
    assert_eq!(inspected.submissions.len(), 1);
    assert_eq!(inspected.candidate.source_url, "https://example.com/report");
    assert_eq!(
        retry.submission.target.acquisition_origin(),
        CandidateAcquisitionOrigin::AgentDiscovery
    );
    assert!(!retry.submission.target.learning_enabled());
    let connection = rusqlite::Connection::open(database_path).unwrap();
    let migrated: String = connection
        .query_row(
            "SELECT value_json FROM stumble_store_records
             WHERE collection = 'candidate_submissions' AND record_key = ?1",
            [&record_key],
            |row| row.get(0),
        )
        .unwrap();
    assert!(migrated.contains("\"target\""));
    assert!(!migrated.contains("proposed_placements"));
    let migrated_candidate: String = connection
        .query_row(
            "SELECT value_json FROM stumble_store_records
             WHERE collection = 'candidates' AND record_key = ?1",
            [&candidate_record_key],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!migrated_candidate.contains("legacy-private"));

    let user = candidate_harness(
        &reopened,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        None,
    );
    let mut user_request = candidate_submission_request(&[]);
    user_request.evidence.source_url = "https://example.com/report".into();
    user_request.evidence.harness_idempotency_key = "legacy-cross-target-worker".into();
    user_request.evidence.client_idempotency_key = "legacy-cross-target-client".into();
    let cross_target = reopened.submit_candidate(&user, user_request).unwrap();
    assert_eq!(cross_target.candidate.id, submitted.candidate.id);
    assert!(!serde_json::to_string(
        &reopened
            .inspect_candidate(&user, submitted.candidate.id)
            .unwrap()
    )
    .unwrap()
    .contains("legacy-private"));
}

#[test]
fn legacy_candidate_submission_is_persisted_canonically_after_snapshot_load() {
    let data_dir = TestDataDir::new("candidate-submission-snapshot-migration");
    let snapshot_path = data_dir.0.join("store.json");
    let tools = AgentTools::new(seed_store());
    let pod = create_candidate_test_pod(&tools, "snapshot-candidates");
    let harness = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    tools
        .submit_candidate(&harness, candidate_submission_request(&[pod.id]))
        .unwrap();
    save_store_snapshot(&tools.store().read().unwrap(), &snapshot_path).unwrap();

    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&snapshot_path).unwrap()).unwrap();
    let record = snapshot["candidate_submissions"][0]
        .as_object_mut()
        .unwrap();
    let mut target = record.remove("target").unwrap();
    record.insert(
        "proposed_placements".into(),
        target.get_mut("placements").unwrap().take(),
    );
    record.insert(
        "task_context".into(),
        target.get_mut("task_context").unwrap().take(),
    );
    std::fs::write(
        &snapshot_path,
        serde_json::to_vec_pretty(&snapshot).unwrap(),
    )
    .unwrap();

    load_store_snapshot(&snapshot_path).unwrap();

    let migrated: serde_json::Value =
        serde_json::from_slice(&std::fs::read(snapshot_path).unwrap()).unwrap();
    assert_eq!(
        migrated["candidate_submissions"][0]["target"]["kind"],
        "pod_placements"
    );
    assert!(migrated["candidate_submissions"][0]
        .get("proposed_placements")
        .is_none());
}
