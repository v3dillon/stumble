use chrono::Utc;
use stumble_core::*;

use crate::common::*;

#[test]
fn manual_curation_queues_every_placement_until_authorized_review() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let pod = public_pod(&tools, "manual-curation");
    let curator = harness(
        &tools,
        "manual curator",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        Some(vec![pod.id]),
    );
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let submitter = harness(
        &tools,
        "candidate submitter",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let submitted = tools
        .submit_candidate(&submitter, candidate_request(pod.id, 1.0))
        .unwrap();
    let mut corroborating = candidate_request(pod.id, 0.8);
    corroborating.evidence.source_url = "https://example.com/curation".into();
    corroborating.evidence.harness_idempotency_key = "corroborating-worker".into();
    corroborating.evidence.client_idempotency_key = "corroborating-client".into();
    corroborating.evidence.media_references = vec![
        media(
            MediaReferenceType::Image,
            "HTTPS://MEDIA.EXAMPLE.COM:443/curation/preview.jpg#corroborated",
        ),
        media(
            MediaReferenceType::Video,
            "https://media.example.com/curation/demo.mp4",
        ),
    ];
    let corroborated = tools.submit_candidate(&submitter, corroborating).unwrap();
    assert_eq!(corroborated.candidate.id, submitted.candidate.id);

    let curated = tools
        .curate_candidate(&curator, submitted.candidate.id, Utc::now())
        .unwrap();

    assert!(curated.content_item.is_none());
    assert_eq!(curated.placements.len(), 1);
    assert_eq!(curated.placements[0].status, PodPlacementStatus::Pending);
    assert!(tools
        .list_content_items_for_pod(&curator, pod.id)
        .unwrap()
        .is_empty());

    let reviewed = tools
        .review_candidate_placement(
            &curator,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            Some(rationale("Reviewed against the Pod boundary")),
            Utc::now(),
        )
        .unwrap();

    assert_eq!(reviewed.status, PodPlacementStatus::Accepted);
    assert_eq!(reviewed.curation_path, CurationPath::ManualReview);
    assert_eq!(reviewed.audit_history.len(), 2);
    let accepted = tools.list_content_items_for_pod(&curator, pod.id).unwrap();
    assert_eq!(accepted.len(), 1);
    assert_eq!(
        accepted[0].media_references(),
        &[
            media(
                MediaReferenceType::Video,
                "https://media.example.com/curation/demo.mp4"
            ),
            media(
                MediaReferenceType::Image,
                "https://media.example.com/curation/preview.jpg"
            )
        ]
    );

    let mut later = candidate_request(pod.id, 0.7);
    later.evidence.source_url = "https://example.com/curation".into();
    later.evidence.harness_idempotency_key = "failed-enrichment-worker".into();
    later.evidence.client_idempotency_key = "failed-enrichment-client".into();
    later.evidence.media_references = vec![media(
        MediaReferenceType::Image,
        "https://media.example.com/curation/later.jpg",
    )];
    let staged = tools.store().read().unwrap().clone();
    // Point persistence at a directory so SQLite open fails.
    let failing = AgentTools::new_sqlite_persistent(staged, &data_dir.0);
    let events_before = failing
        .federation_pod_events(&curator, &pod.slug)
        .unwrap()
        .len();

    assert!(matches!(
        failing.submit_candidate(&submitter, later),
        Err(AgentToolsError::Persistence(StorePersistenceError::Sqlite(
            _
        )))
    ));
    assert_eq!(
        failing
            .list_content_items_for_pod(&curator, pod.id)
            .unwrap()[0]
            .media_references(),
        accepted[0].media_references()
    );
    assert_eq!(
        failing
            .federation_pod_events(&curator, &pod.slug)
            .unwrap()
            .len(),
        events_before
    );
}

#[test]
fn assisted_curation_accepts_only_trusted_high_confidence_evidence() {
    let tools = AgentTools::new(seed_store());
    let pod = private_pod(&tools, "assisted-curation");
    let curator = harness(
        &tools,
        "assisted curator",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        Some(vec![pod.id]),
    );
    tools
        .set_pod_curation_policy(
            &curator,
            pod.id,
            CurationPolicy::Assisted {
                confidence_threshold: CandidateConfidence::new(0.8).unwrap(),
            },
            Utc::now(),
        )
        .unwrap();
    let conversational = harness(
        &tools,
        "conversational submitter",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let untrusted = tools
        .submit_candidate(&conversational, candidate_request(pod.id, 0.99))
        .unwrap();
    assert_eq!(
        tools
            .curate_candidate(&curator, untrusted.candidate.id, Utc::now())
            .unwrap()
            .placements[0]
            .status,
        PodPlacementStatus::Pending
    );

    let worker = harness(
        &tools,
        "trusted worker",
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
                instructions: "find a trusted report".into(),
                idempotency_key: "trusted-task".into(),
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
    let mut trusted_low = candidate_request(pod.id, 0.2);
    trusted_low.evidence.source_url = "https://example.com/mixed-trust".into();
    trusted_low.evidence.harness_idempotency_key = "trusted-low-worker".into();
    trusted_low.evidence.client_idempotency_key = "trusted-low-client".into();
    let CandidateSubmissionRequestTarget::PodPlacements { task_context, .. } =
        &mut trusted_low.target
    else {
        panic!("test request must target Pod placements");
    };
    *task_context = Some(CandidateTaskContext {
        task_id: task.id,
        package_version: task.target.pod().unwrap().1,
    });
    let mixed = tools.submit_candidate(&worker, trusted_low).unwrap();
    let mut untrusted_high = candidate_request(pod.id, 1.0);
    untrusted_high.evidence.source_url = "https://example.com/mixed-trust".into();
    untrusted_high.evidence.harness_idempotency_key = "untrusted-high-worker".into();
    untrusted_high.evidence.client_idempotency_key = "untrusted-high-client".into();
    tools
        .submit_candidate(&conversational, untrusted_high)
        .unwrap();
    assert_eq!(
        tools
            .curate_candidate(&curator, mixed.candidate.id, now)
            .unwrap()
            .placements[0]
            .status,
        PodPlacementStatus::Pending,
        "untrusted confidence must not combine with unrelated trusted evidence"
    );

    let mut request = candidate_request(pod.id, 0.9);
    request.evidence.source_url = "https://example.com/trusted-curation".into();
    request.evidence.harness_idempotency_key = "trusted-worker".into();
    request.evidence.client_idempotency_key = "trusted-client".into();
    let CandidateSubmissionRequestTarget::PodPlacements { task_context, .. } = &mut request.target
    else {
        panic!("test request must target Pod placements");
    };
    *task_context = Some(CandidateTaskContext {
        task_id: task.id,
        package_version: task.target.pod().unwrap().1,
    });
    let trusted = tools.submit_candidate(&worker, request).unwrap();

    let curated = tools
        .curate_candidate(&curator, trusted.candidate.id, now)
        .unwrap();

    assert_eq!(curated.placements[0].status, PodPlacementStatus::Accepted);
    assert_eq!(
        curated.placements[0].curation_path,
        CurationPath::AssistedAutomatic
    );
}

#[test]
fn autonomous_curation_requires_independent_sensitive_change_approval() {
    let tools = AgentTools::new(seed_store());
    let pod = private_pod(&tools, "autonomous-curation");
    let proposer = harness(
        &tools,
        "autonomy proposer",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        Some(vec![pod.id]),
    );
    assert!(tools
        .set_pod_curation_policy(
            &proposer,
            pod.id,
            CurationPolicy::Autonomous {
                confidence_threshold: CandidateConfidence::new(0.7).unwrap(),
            },
            Utc::now(),
        )
        .is_err());
    let now = Utc::now();
    let proposal = tools
        .create_pending_proposal(
            &proposer,
            SensitiveChange::EnableAutonomousCuration {
                pod_id: pod.id,
                confidence_threshold: CandidateConfidence::new(0.7).unwrap(),
            },
            now,
            now + chrono::Duration::hours(1),
        )
        .unwrap();
    let approver = harness(
        &tools,
        "autonomy approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
        Some(vec![pod.id]),
    );

    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    let submitter = harness(
        &tools,
        "autonomous submitter",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let submitted = tools
        .submit_candidate(&submitter, candidate_request(pod.id, 0.75))
        .unwrap();
    let curated = tools
        .curate_candidate(&proposer, submitted.candidate.id, now)
        .unwrap();

    assert_eq!(curated.placements[0].status, PodPlacementStatus::Accepted);
    assert_eq!(
        curated.placements[0].curation_path,
        CurationPath::AutonomousAutomatic
    );
}

#[test]
fn curation_rationales_are_validated_at_construction_and_deserialization() {
    assert!(CurationRationale::new("  ").is_err());
    assert!(RouteCandidatePlacementRequest::new(
        uuid::Uuid::now_v7(),
        "",
        CandidateConfidence::new(0.5).unwrap(),
    )
    .is_err());
    assert!(
        serde_json::from_str::<RouteCandidatePlacementRequest>(&format!(
            r#"{{"pod_id":"{}","reason":" ","confidence":0.5}}"#,
            uuid::Uuid::now_v7()
        ))
        .is_err()
    );
}

#[test]
fn mixed_scope_curation_fails_before_any_authoritative_mutation() {
    let tools = AgentTools::new(seed_store());
    let allowed = public_pod(&tools, "preflight-allowed");
    let denied = private_pod(&tools, "preflight-denied");
    let submitter = harness(
        &tools,
        "broad candidate submitter",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![allowed.id, denied.id]),
    );
    let curator = harness(
        &tools,
        "narrow curator",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        Some(vec![allowed.id]),
    );
    let mut request = candidate_request(allowed.id, 0.9);
    let CandidateSubmissionRequestTarget::PodPlacements { placements, .. } = &mut request.target
    else {
        panic!("test request must target Pod placements");
    };
    placements.push(ProposedCandidatePlacement {
        pod_id: denied.id,
        reason: "Unauthorized second route".into(),
        confidence: CandidateConfidence::new(0.9).unwrap(),
    });
    let submitted = tools.submit_candidate(&submitter, request).unwrap();
    let events_before = tools
        .federation_pod_events(&curator, &allowed.slug)
        .unwrap();

    assert!(tools
        .curate_candidate(&curator, submitted.candidate.id, Utc::now())
        .is_err());

    let events_after = tools
        .federation_pod_events(&curator, &allowed.slug)
        .unwrap();
    assert_eq!(
        events_after
            .iter()
            .map(|event| event.event_id)
            .collect::<Vec<_>>(),
        events_before
            .iter()
            .map(|event| event.event_id)
            .collect::<Vec<_>>()
    );
    assert!(tools
        .list_content_items_for_pod(&curator, allowed.id)
        .unwrap()
        .is_empty());
    assert_eq!(
        tools
            .inspect_candidate(&submitter, submitted.candidate.id)
            .unwrap()
            .candidate
            .review_state,
        CandidateReviewState::Pending
    );
    let routed_after_failure = tools
        .route_candidate_placement(
            &curator,
            submitted.candidate.id,
            RouteCandidatePlacementRequest::new(
                allowed.id,
                "Authorized route after failed preflight",
                CandidateConfidence::new(0.9).unwrap(),
            )
            .unwrap(),
            Utc::now(),
        )
        .unwrap();
    assert_eq!(
        routed_after_failure.curation_path,
        CurationPath::RoutingAgent,
        "failed curation must not leave a hidden pending placement"
    );
}
