mod support;

use chrono::Utc;
use stumble_core::*;
use support::{
    candidate_harness, candidate_submission_request as candidate_request,
    create_candidate_test_pod as create_test_pod, media_reference as media,
};

fn set_task_context(request: &mut CandidateSubmissionRequest, context: CandidateTaskContext) {
    let CandidateSubmissionRequestTarget::PodPlacements { task_context, .. } = &mut request.target
    else {
        panic!("test request must target Pod placements");
    };
    *task_context = Some(context);
}

fn task_context_mut(request: &mut CandidateSubmissionRequest) -> &mut CandidateTaskContext {
    let CandidateSubmissionRequestTarget::PodPlacements { task_context, .. } = &mut request.target
    else {
        panic!("test request must target Pod placements");
    };
    task_context.as_mut().unwrap()
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
    assert_eq!(submitted.submission.target.placements().len(), 2);
    assert_eq!(
        submitted.submission.target.acquisition_origin(),
        CandidateAcquisitionOrigin::AgentDiscovery
    );
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
        submitted.submission.evidence.media_references,
        vec![
            media(
                MediaReferenceType::Image,
                "https://cdn.example.com/report/diagram.png"
            ),
            media(
                MediaReferenceType::Video,
                "https://cdn.example.com/report/demo.mp4"
            ),
        ]
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
fn omitted_media_references_default_to_an_empty_reference_list() {
    let mut request = serde_json::to_value(candidate_request(&[uuid::Uuid::now_v7()])).unwrap();
    request.as_object_mut().unwrap().remove("media_references");

    let request: CandidateSubmissionRequest = serde_json::from_value(request).unwrap();

    assert!(request.evidence.media_references.is_empty());
}

#[test]
fn interactive_harness_kind_alone_never_marks_a_pod_operation_as_user_learning() {
    let tools = AgentTools::new(seed_store());
    let pod = create_test_pod(&tools, "explicit-user-operation");
    let submitter = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let submitted = tools
        .submit_candidate(&submitter, candidate_request(&[pod.id]))
        .unwrap();

    assert_eq!(
        submitted.submission.target.acquisition_origin(),
        CandidateAcquisitionOrigin::AgentDiscovery
    );
    assert!(!submitted.submission.target.learning_enabled());

    let profile_reader = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Feedback],
        None,
    );
    assert_eq!(
        tools
            .taste_profile(&profile_reader)
            .unwrap()
            .interest_seed_evidence
            .active_seed_count,
        0
    );
}

#[test]
fn pod_target_rejects_an_empty_placement_collection() {
    let tools = AgentTools::new(seed_store());
    let submitter = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        None,
    );
    let mut request = candidate_request(&[]);
    request.target = CandidateSubmissionRequestTarget::PodPlacements {
        placements: Vec::new(),
        task_context: None,
    };

    assert!(matches!(
        tools.submit_candidate(&submitter, request),
        Err(AgentToolsError::Store(StoreError::Validation(message)))
            if message.contains("at least one placement")
    ));
}

#[test]
fn candidate_submission_rejects_unknown_media_reference_types() {
    let mut request = serde_json::to_value(candidate_request(&[uuid::Uuid::now_v7()])).unwrap();
    request["media_references"][0]["media_type"] = serde_json::json!("document");

    let result = serde_json::from_value::<CandidateSubmissionRequest>(request);

    assert!(result.is_err());
}

#[test]
fn candidate_submission_rejects_media_references_that_are_not_permitted_web_urls() {
    let tools = AgentTools::new(seed_store());
    let pod = create_test_pod(&tools, "invalid-media-reference");
    let harness = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let request = serde_json::json!({
        "media_type": "image",
        "url": "file:///tmp/archived-image.png"
    });
    assert!(serde_json::from_value::<MediaReference>(request).is_err());
    assert!(tools.list_candidates(&harness).unwrap().is_empty());
}

#[test]
fn media_reference_boundary_canonicalizes_equivalent_web_urls() {
    let reference = MediaReference::new(
        MediaReferenceType::Image,
        "HTTPS://CDN.EXAMPLE.COM:443/report/diagram.png?utm_source=feed&b=2&a=1#preview",
    )
    .unwrap();

    assert_eq!(
        reference.url(),
        "https://cdn.example.com/report/diagram.png?a=1&b=2"
    );
    assert!(MediaReference::new(MediaReferenceType::Image, "file:///tmp/image.png").is_err());
    assert_eq!(
        canonicalize_url("ftp://EXAMPLE.COM/report?a=1#section").unwrap(),
        "ftp://example.com/report?a=1"
    );
    assert!(serde_json::from_value::<MediaReference>(serde_json::json!({
        "media_type": "image",
        "url": "javascript:alert(1)"
    }))
    .is_err());
}

#[test]
fn canonical_media_identity_deduplicates_and_rejects_type_conflicts() {
    let tools = AgentTools::new(seed_store());
    let pod = create_test_pod(&tools, "canonical-media-evidence");
    let harness = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let mut first = candidate_request(&[pod.id]);
    first.evidence.media_references = vec![MediaReference::new(
        MediaReferenceType::Image,
        "https://CDN.example.com:443/report/diagram.png?b=2&a=1#first",
    )
    .unwrap()];
    let submitted = tools.submit_candidate(&harness, first).unwrap();

    let mut duplicate = candidate_request(&[pod.id]);
    duplicate.evidence.source_url = "https://example.com/report".into();
    duplicate.evidence.harness_idempotency_key = "canonical-duplicate-worker".into();
    duplicate.evidence.client_idempotency_key = "canonical-duplicate-client".into();
    duplicate.evidence.media_references = vec![MediaReference::new(
        MediaReferenceType::Image,
        "https://cdn.example.com/report/diagram.png?a=1&b=2",
    )
    .unwrap()];
    tools.submit_candidate(&harness, duplicate).unwrap();

    let mut conflict = candidate_request(&[pod.id]);
    conflict.evidence.source_url = "https://example.com/report".into();
    conflict.evidence.harness_idempotency_key = "canonical-conflict-worker".into();
    conflict.evidence.client_idempotency_key = "canonical-conflict-client".into();
    conflict.evidence.media_references = vec![MediaReference::new(
        MediaReferenceType::Video,
        "https://cdn.example.com/report/diagram.png?a=1&b=2",
    )
    .unwrap()];

    assert!(matches!(
        tools.submit_candidate(&harness, conflict),
        Err(AgentToolsError::Store(StoreError::Validation(message)))
            if message.contains("conflicting media types")
    ));
    assert_eq!(
        tools
            .inspect_candidate(&harness, submitted.candidate.id)
            .unwrap()
            .submissions
            .len(),
        2
    );
}

#[test]
fn canonical_deduplication_never_promotes_private_user_evidence_to_a_pod() {
    let tools = AgentTools::new(seed_store());
    let user = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        None,
    );
    let mut private_request = candidate_request(&[]);
    private_request.evidence.source_url =
        "https://example.com/report?utm_source=private-secret#personal".into();
    private_request.evidence.source_metadata.title = Some("private-title-needle".into());
    private_request.evidence.media_references = vec![media(
        MediaReferenceType::Image,
        "https://private.example/private-media-needle.png",
    )];
    let private = tools.submit_candidate(&user, private_request).unwrap();

    let pod = create_test_pod(&tools, "canonical-url-privacy");
    let pod_harness = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let mut pod_request = candidate_request(&[pod.id]);
    pod_request.evidence.source_url = "https://example.com/report".into();
    pod_request.evidence.harness_idempotency_key = "pod-visible-worker".into();
    pod_request.evidence.client_idempotency_key = "pod-visible-client".into();
    let pod_submission = tools.submit_candidate(&pod_harness, pod_request).unwrap();

    assert_eq!(pod_submission.candidate.id, private.candidate.id);
    let inspected = tools
        .inspect_candidate(&pod_harness, private.candidate.id)
        .unwrap();
    assert_eq!(inspected.candidate.source_url, "https://example.com/report");
    assert_eq!(inspected.submissions.len(), 1);
    assert!(!serde_json::to_string(&inspected)
        .unwrap()
        .contains("private-secret"));

    let curator = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        Some(vec![pod.id]),
    );
    tools
        .curate_candidate(&curator, private.candidate.id, Utc::now())
        .unwrap();
    tools
        .review_candidate_placement(
            &curator,
            private.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            Some(CurationRationale::new("Pod evidence reviewed").unwrap()),
            Utc::now(),
        )
        .unwrap();

    let mut later_private = candidate_request(&[]);
    later_private.evidence.source_url =
        "https://example.com/report?utm_source=later-private".into();
    later_private.evidence.source_metadata.title = Some("later-private-title".into());
    later_private.evidence.media_references = vec![media(
        MediaReferenceType::Image,
        "https://private.example/later-private-media.png",
    )];
    later_private.evidence.harness_idempotency_key = "later-private-worker".into();
    later_private.evidence.client_idempotency_key = "later-private-client".into();
    tools.submit_candidate(&user, later_private).unwrap();

    let public_projection = serde_json::to_string(&(
        tools.list_content_items_for_pod(&curator, pod.id).unwrap(),
        tools.federation_pod_events(&curator, &pod.slug).unwrap(),
    ))
    .unwrap();
    for private_needle in [
        "private-secret",
        "private-title-needle",
        "private-media-needle",
        "later-private",
    ] {
        assert!(!public_projection.contains(private_needle));
    }
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
    let CandidateSubmissionRequestTarget::PodPlacements { placements, .. } =
        &mut independent_request.target
    else {
        panic!("test request must target Pod placements");
    };
    placements[0].reason = "independent corroboration".into();
    placements[0].confidence = CandidateConfidence::new(0.6).unwrap();
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
    assert!(inspected.submissions.iter().all(|submission| {
        submission.evidence.media_references
            == vec![
                media(
                    MediaReferenceType::Image,
                    "https://cdn.example.com/report/diagram.png",
                ),
                media(
                    MediaReferenceType::Video,
                    "https://cdn.example.com/report/demo.mp4",
                ),
            ]
    }));
    assert!(inspected.submissions.iter().any(|submission| {
        submission.target.placements()[0].reason == "independent corroboration"
            && submission.target.placements()[0].confidence
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
    set_task_context(
        &mut request,
        CandidateTaskContext {
            task_id: task.id,
            package_version: task.target.pod().unwrap().1,
        },
    );
    assert!(matches!(
        tools.submit_candidate(&other, request.clone()),
        Err(AgentToolsError::CandidateTaskLeaseRequired)
    ));
    task_context_mut(&mut request).package_version = PackageVersion::new(2).unwrap();
    assert!(matches!(
        tools.submit_candidate(&worker, request.clone()),
        Err(AgentToolsError::CandidatePackageVersionMismatch)
    ));
    task_context_mut(&mut request).package_version = task.target.pod().unwrap().1;

    let submitted = tools.submit_candidate(&worker, request).unwrap();
    assert_eq!(
        submitted.submission.target.task_context().unwrap().task_id,
        task.id
    );
    assert_eq!(
        submitted.submission.target.acquisition_origin(),
        CandidateAcquisitionOrigin::AgentDiscovery
    );
    assert!(!submitted.submission.target.learning_enabled());
    let profile_reader = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Feedback],
        None,
    );
    assert_eq!(
        tools
            .taste_profile(&profile_reader)
            .unwrap()
            .interest_seed_evidence
            .active_seed_count,
        0
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
fn revoked_candidate_context_propagates_authorization_failure() {
    let tools = AgentTools::new(seed_store());
    let pod = create_test_pod(&tools, "revoked-candidate-context");
    let harness = candidate_harness(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let submitted = tools
        .submit_candidate(&harness, candidate_request(&[pod.id]))
        .unwrap();
    tools
        .revoke_agent_harness(
            &tools.default_auth_context().unwrap(),
            harness.harness_id.unwrap(),
        )
        .unwrap();

    for result in [
        tools.list_candidates(&harness).map(|_| ()),
        tools
            .inspect_candidate(&harness, submitted.candidate.id)
            .map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(AgentToolsError::Forbidden { reason })
                if reason == "harness grant is revoked or missing"
        ));
    }
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
    set_task_context(
        &mut request,
        CandidateTaskContext {
            task_id: task.id,
            package_version: task.target.pod().unwrap().1,
        },
    );
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
    set_task_context(
        &mut request,
        CandidateTaskContext {
            task_id: task.id,
            package_version: task.target.pod().unwrap().1,
        },
    );

    assert!(matches!(
        tools.submit_candidate(&worker, request),
        Err(AgentToolsError::CandidateTaskLeaseRequired)
    ));
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
