use chrono::{TimeZone, Utc};
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-candidate-submissions-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn candidate_request(pod_ids: &[PodId]) -> CandidateSubmissionRequest {
    CandidateSubmissionRequest {
        evidence: CandidateSubmissionEvidence {
            source_url: "https://example.com/report?utm_source=feed#section".into(),
            source_metadata: CandidateSourceMetadata {
                title: Some("A careful incident report".into()),
                author: Some("Example Engineering".into()),
                published_at: Some(Utc.with_ymd_and_hms(2026, 7, 16, 12, 0, 0).unwrap()),
            },
            permitted_excerpt: Some("A short, permitted excerpt.".into()),
            summary: Some("How the team diagnosed and repaired the incident.".into()),
            content_type: CandidateContentType::Article,
            tags: vec!["reliability".into(), "incident-review".into()],
            provenance: CandidateProvenance {
                discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 9, 0, 0).unwrap(),
                discovery_method: "browser_search".into(),
                referrer_url: Some("https://search.example/results?q=incident".into()),
            },
            proposed_placements: pod_ids
                .iter()
                .enumerate()
                .map(|(index, pod_id)| ProposedCandidatePlacement {
                    pod_id: *pod_id,
                    reason: format!("placement reason {index}"),
                    confidence: CandidateConfidence::new(0.8 - index as f32 * 0.1).unwrap(),
                })
                .collect(),
            task_context: None,
            harness_idempotency_key: "worker-run-42".into(),
            client_idempotency_key: "client-request-42".into(),
        },
    }
}

fn candidate_harness(
    tools: &AgentTools,
    kind: AgentHarnessKind,
    capabilities: Vec<HarnessCapability>,
    pod_ids: Option<Vec<PodId>>,
) -> AuthContext {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "candidate worker".into(),
                kind,
                capabilities,
                pod_ids,
            },
        )
        .unwrap();
    tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap()
}

fn create_test_pod(tools: &AgentTools, slug: &str) -> Pod {
    tools
        .create_pod(
            &tools.default_auth_context().unwrap(),
            CreatePodRequest {
                name: slug.into(),
                slug: slug.into(),
                description: "Candidate acceptance Pod".into(),
                visibility: Visibility::Public,
            },
        )
        .unwrap()
}

#[test]
fn interactive_harness_submits_structured_private_multi_pod_candidate() {
    let tools = AgentTools::new(seed_store());
    let first = create_test_pod(&tools, "first-candidates");
    let second = create_test_pod(&tools, "second-candidates");
    let harness = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![first.id, second.id]),
    );
    let events_before = tools.federation_pod_events(&harness, &first.slug).unwrap();

    let submitted = tools
        .submit_candidate(&harness, candidate_request(&[first.id, second.id]))
        .unwrap();
    let inspected = tools
        .inspect_candidate(&harness, submitted.candidate.id)
        .unwrap();

    assert_eq!(
        submitted.candidate.canonical_url,
        "https://example.com/report"
    );
    assert_eq!(
        submitted.candidate.review_state,
        CandidateReviewState::Pending
    );
    assert_eq!(submitted.submission.evidence.proposed_placements.len(), 2);
    assert_eq!(
        submitted
            .submission
            .evidence
            .source_metadata
            .author
            .as_deref(),
        Some("Example Engineering")
    );
    assert_eq!(
        submitted.submission.evidence.permitted_excerpt.as_deref(),
        Some("A short, permitted excerpt.")
    );
    assert_eq!(
        submitted.submission.evidence.content_type,
        CandidateContentType::Article
    );
    assert_eq!(
        submitted.allowed_actions,
        vec![CandidateAllowedAction::InspectCandidate]
    );
    assert_eq!(
        inspected.allowed_actions,
        vec![CandidateAllowedAction::SubmitCandidateEvidence]
    );
    let owner_inspection = tools
        .inspect_candidate(
            &tools.default_auth_context().unwrap(),
            submitted.candidate.id,
        )
        .unwrap();
    assert!(owner_inspection.allowed_actions.is_empty());
    assert_eq!(inspected.submissions, vec![submitted.submission.clone()]);
    let events_after = tools.federation_pod_events(&harness, &first.slug).unwrap();
    assert_eq!(events_after.len(), events_before.len());
    assert_eq!(
        events_after.last().map(|event| event.event_id),
        events_before.last().map(|event| event.event_id),
        "Candidates must not create federation events"
    );
    let federation_artifact = serde_json::to_string(&serde_json::json!({
        "manifest": tools.federation_pod_manifest(&harness, &first.slug).unwrap(),
        "events": events_after,
    }))
    .unwrap();
    assert!(!federation_artifact.contains("https://example.com/report"));
    assert!(!federation_artifact.contains("review_state"));
    assert!(!federation_artifact.contains("candidate_id"));
}

#[test]
fn retries_are_idempotent_and_canonical_deduplication_keeps_independent_evidence() {
    let tools = AgentTools::new(seed_store());
    let pod = create_test_pod(&tools, "deduplicated-candidates");
    let first_harness = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let second_harness = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let request = candidate_request(&[pod.id]);

    let first = tools
        .submit_candidate(&first_harness, request.clone())
        .unwrap();
    let retry = tools.submit_candidate(&first_harness, request).unwrap();
    let mut independent_request = candidate_request(&[pod.id]);
    independent_request.evidence.source_url = "https://example.com/report".into();
    independent_request.evidence.harness_idempotency_key = "another-worker-run".into();
    independent_request.evidence.client_idempotency_key = "another-client-request".into();
    independent_request.evidence.proposed_placements[0].reason = "independent corroboration".into();
    independent_request.evidence.proposed_placements[0].confidence =
        CandidateConfidence::new(0.6).unwrap();
    let independent = tools
        .submit_candidate(&second_harness, independent_request)
        .unwrap();

    assert_eq!(retry.submission.id, first.submission.id);
    assert_eq!(independent.candidate.id, first.candidate.id);
    assert_ne!(independent.submission.id, first.submission.id);
    let inspected = tools
        .inspect_candidate(&first_harness, first.candidate.id)
        .unwrap();
    assert_eq!(inspected.submissions.len(), 2);
    assert!(inspected.submissions.iter().any(|submission| {
        submission.evidence.proposed_placements[0].reason == "independent corroboration"
            && submission.evidence.proposed_placements[0].confidence
                == CandidateConfidence::new(0.6).unwrap()
    }));
}

#[test]
fn task_submission_requires_the_owning_lease_and_pinned_package_version() {
    let tools = AgentTools::new(seed_store());
    let pod = create_test_pod(&tools, "task-candidates");
    let worker = candidate_harness(
        &tools,
        AgentHarnessKind::Unattended,
        vec![
            HarnessCapability::CandidateSubmission,
            HarnessCapability::DiscoveryTasks,
        ],
        Some(vec![pod.id]),
    );
    let other = candidate_harness(
        &tools,
        AgentHarnessKind::Unattended,
        vec![
            HarnessCapability::CandidateSubmission,
            HarnessCapability::DiscoveryTasks,
        ],
        Some(vec![pod.id]),
    );
    let now = Utc::now();
    let task = tools
        .create_immediate_discovery_task(
            &worker,
            CreateImmediateDiscoveryTaskRequest {
                pod_id: pod.id,
                instructions: "find an incident review".into(),
                idempotency_key: "task-1".into(),
            },
            now,
        )
        .unwrap();
    tools
        .claim_discovery_task(
            &worker,
            task.id,
            now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    let mut request = candidate_request(&[pod.id]);
    assert!(matches!(
        tools.submit_candidate(&worker, request.clone()),
        Err(AgentToolsError::CandidateTaskRequired)
    ));
    request.evidence.task_context = Some(CandidateTaskContext {
        task_id: task.id,
        package_version: task.package_version,
    });
    assert!(matches!(
        tools.submit_candidate(&other, request.clone()),
        Err(AgentToolsError::CandidateTaskLeaseRequired)
    ));
    request
        .evidence
        .task_context
        .as_mut()
        .unwrap()
        .package_version = PackageVersion::new(2).unwrap();
    assert!(matches!(
        tools.submit_candidate(&worker, request.clone()),
        Err(AgentToolsError::CandidatePackageVersionMismatch)
    ));
    request
        .evidence
        .task_context
        .as_mut()
        .unwrap()
        .package_version = task.package_version;

    let submitted = tools.submit_candidate(&worker, request).unwrap();
    assert_eq!(
        submitted.submission.evidence.task_context.unwrap().task_id,
        task.id
    );
}

#[test]
fn placement_authorization_and_confidence_validation_are_enforced() {
    let tools = AgentTools::new(seed_store());
    let allowed = create_test_pod(&tools, "allowed-candidates");
    let denied = create_test_pod(&tools, "denied-candidates");
    let harness = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![allowed.id]),
    );

    assert!(CandidateConfidence::new(f32::NAN).is_err());
    assert!(CandidateConfidence::new(1.01).is_err());
    assert!(matches!(
        tools.submit_candidate(&harness, candidate_request(&[allowed.id, denied.id])),
        Err(AgentToolsError::Forbidden { .. })
    ));
}

#[test]
fn candidate_and_idempotency_evidence_survive_sqlite_restart() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let pod = create_test_pod(&tools, "persistent-candidates");
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
    let request = candidate_request(&[pod.id]);
    let harness = tools.authenticate_token(&token).unwrap().unwrap();
    let submitted = tools.submit_candidate(&harness, request.clone()).unwrap();
    drop(tools);

    let reopened = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let harness = reopened.authenticate_token(&token).unwrap().unwrap();
    let retry = reopened.submit_candidate(&harness, request).unwrap();
    let inspected = reopened
        .inspect_candidate(&harness, submitted.candidate.id)
        .unwrap();

    assert_eq!(retry.submission.id, submitted.submission.id);
    assert_eq!(inspected.submissions.len(), 1);
}

#[test]
fn changed_input_cannot_reuse_either_idempotency_key() {
    let tools = AgentTools::new(seed_store());
    let pod = create_test_pod(&tools, "conflicting-candidates");
    let harness = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let request = candidate_request(&[pod.id]);
    tools.submit_candidate(&harness, request.clone()).unwrap();
    let mut changed = request;
    changed.evidence.summary = Some("changed evidence".into());

    assert!(matches!(
        tools.submit_candidate(&harness, changed),
        Err(AgentToolsError::CandidateIdempotencyConflict)
    ));
}

#[test]
fn task_submission_retry_remains_safe_after_task_completion() {
    let tools = AgentTools::new(seed_store());
    let pod = create_test_pod(&tools, "completed-task-candidates");
    let worker = candidate_harness(
        &tools,
        AgentHarnessKind::Unattended,
        vec![
            HarnessCapability::CandidateSubmission,
            HarnessCapability::DiscoveryTasks,
        ],
        Some(vec![pod.id]),
    );
    let now = Utc::now();
    let task = tools
        .create_immediate_discovery_task(
            &worker,
            CreateImmediateDiscoveryTaskRequest {
                pod_id: pod.id,
                instructions: "find a report".into(),
                idempotency_key: "completed-task".into(),
            },
            now,
        )
        .unwrap();
    tools
        .claim_discovery_task(
            &worker,
            task.id,
            now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    let mut request = candidate_request(&[pod.id]);
    request.evidence.task_context = Some(CandidateTaskContext {
        task_id: task.id,
        package_version: task.package_version,
    });
    let first = tools.submit_candidate(&worker, request.clone()).unwrap();
    tools
        .complete_discovery_task(&worker, task.id, now)
        .unwrap();

    let retry = tools.submit_candidate(&worker, request).unwrap();
    assert_eq!(retry.submission.id, first.submission.id);
}

#[test]
fn expired_task_lease_cannot_authorize_a_new_candidate_submission() {
    let tools = AgentTools::new(seed_store());
    let pod = create_test_pod(&tools, "expired-task-candidates");
    let worker = candidate_harness(
        &tools,
        AgentHarnessKind::Unattended,
        vec![
            HarnessCapability::CandidateSubmission,
            HarnessCapability::DiscoveryTasks,
        ],
        Some(vec![pod.id]),
    );
    let claimed_at = Utc::now() - chrono::Duration::minutes(10);
    let task = tools
        .create_immediate_discovery_task(
            &worker,
            CreateImmediateDiscoveryTaskRequest {
                pod_id: pod.id,
                instructions: "find an expired report".into(),
                idempotency_key: "expired-task".into(),
            },
            claimed_at,
        )
        .unwrap();
    tools
        .claim_discovery_task(
            &worker,
            task.id,
            claimed_at,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    let mut request = candidate_request(&[pod.id]);
    request.evidence.task_context = Some(CandidateTaskContext {
        task_id: task.id,
        package_version: task.package_version,
    });

    assert!(matches!(
        tools.submit_candidate(&worker, request),
        Err(AgentToolsError::CandidateTaskLeaseRequired)
    ));
}

#[test]
fn concurrent_idempotent_submissions_cannot_create_duplicate_candidates() {
    let data_dir = TestDataDir::new();
    let setup = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let pod = create_test_pod(&setup, "concurrent-candidates");
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
    let request = candidate_request(&[pod.id]);
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
fn imported_remote_pods_cannot_receive_local_candidate_placement_proposals() {
    let tools = AgentTools::new(seed_store());
    let remote = create_test_pod(&tools, "remote-candidate-placement");
    {
        let store = tools.store();
        let mut store = store.write().unwrap();
        store.pods.get_mut(&remote.id).unwrap().origin_node_id = Some(uuid::Uuid::now_v7());
    }
    let harness = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![remote.id]),
    );

    assert!(matches!(
        tools.submit_candidate(&harness, candidate_request(&[remote.id])),
        Err(AgentToolsError::Forbidden { .. })
    ));
}
