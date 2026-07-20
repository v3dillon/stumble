use chrono::{TimeZone, Utc};
use stumble_core::*;

fn media(media_type: MediaReferenceType, url: &str) -> MediaReference {
    MediaReference::new(media_type, url).unwrap()
}

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("stumble-content-curation-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn harness(
    tools: &AgentTools,
    label: &str,
    kind: AgentHarnessKind,
    capabilities: Vec<HarnessCapability>,
    pod_ids: Option<Vec<PodId>>,
) -> AuthContext {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: label.into(),
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

fn private_pod(tools: &AgentTools, slug: &str) -> Pod {
    let curator = harness(
        tools,
        "pod owner",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        None,
    );
    tools
        .create_pod(
            &curator,
            CreatePodRequest {
                name: slug.into(),
                slug: slug.into(),
                description: "Curation acceptance Pod".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap()
}

fn public_pod(tools: &AgentTools, slug: &str) -> Pod {
    let proposer = harness(
        tools,
        "public Pod proposer",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        None,
    );
    let approver = harness(
        tools,
        "public Pod approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
        None,
    );
    let now = Utc::now();
    let proposal = tools
        .create_pending_proposal(
            &proposer,
            SensitiveChange::CreatePublicPod {
                request: CreatePodRequest {
                    name: slug.into(),
                    slug: slug.into(),
                    description: "Public curation acceptance Pod".into(),
                    visibility: Visibility::Public,
                },
            },
            now,
            now + chrono::Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

fn candidate_request(pod_id: PodId, confidence: f32) -> CandidateSubmissionRequest {
    CandidateSubmissionRequest {
        target: CandidateSubmissionRequestTarget::PodPlacements {
            placements: vec![ProposedCandidatePlacement {
                pod_id,
                reason: "Strong topical match".into(),
                confidence: CandidateConfidence::new(confidence).unwrap(),
            }],
            task_context: None,
        },
        evidence: CandidateSubmissionEvidence {
            source_url: "https://example.com/curation?utm_source=test".into(),
            source_metadata: CandidateSourceMetadata {
                title: Some("Curation report".into()),
                author: Some("Example Engineering".into()),
                published_at: None,
            },
            permitted_excerpt: Some("Permitted evidence".into()),
            summary: Some("A report worth curating".into()),
            content_type: CandidateContentType::Article,
            media_references: vec![media(
                MediaReferenceType::Image,
                "https://media.example.com/curation/preview.jpg",
            )],
            tags: vec!["curation".into()],
            provenance: CandidateProvenance {
                discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
                discovery_method: "interactive_search".into(),
                referrer_url: None,
            },
            harness_idempotency_key: format!("worker-{pod_id}"),
            client_idempotency_key: format!("client-{pod_id}"),
        },
    }
}

fn rationale(value: &str) -> CurationRationale {
    CurationRationale::new(value).unwrap()
}

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
    let failing = AgentTools::new_persistent(staged, &data_dir.0);
    let events_before = failing
        .federation_pod_events(&curator, &pod.slug)
        .unwrap()
        .len();

    assert!(matches!(
        failing.submit_candidate(&submitter, later),
        Err(AgentToolsError::Persistence(StorePersistenceError::Io(_)))
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
fn routing_multi_pod_add_to_pod_and_reversal_preserve_identity_and_audit() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let first = private_pod(&tools, "routing-first");
    let second = private_pod(&tools, "routing-second");
    let added = private_pod(&tools, "explicit-add");
    let denied = private_pod(&tools, "routing-denied");
    let curator = harness(
        &tools,
        "routing curator",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        Some(vec![first.id, second.id, added.id]),
    );
    for pod in [&first, &second] {
        tools
            .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
            .unwrap();
    }
    let submitter = harness(
        &tools,
        "routing submitter",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![first.id]),
    );
    let submitted = tools
        .submit_candidate(&submitter, candidate_request(first.id, 0.9))
        .unwrap();
    tools
        .curate_candidate(&curator, submitted.candidate.id, Utc::now())
        .unwrap();
    let first_placement = tools
        .review_candidate_placement(
            &curator,
            submitted.candidate.id,
            first.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap();

    let routed = tools
        .route_candidate_placement(
            &curator,
            submitted.candidate.id,
            RouteCandidatePlacementRequest::new(
                second.id,
                "A second authorized subject match",
                CandidateConfidence::new(0.85).unwrap(),
            )
            .unwrap(),
            Utc::now(),
        )
        .unwrap();
    assert_eq!(routed.status, PodPlacementStatus::Pending);
    assert_eq!(routed.curation_path, CurationPath::RoutingAgent);
    let second_placement = tools
        .review_candidate_placement(
            &curator,
            submitted.candidate.id,
            second.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap();
    assert_eq!(
        second_placement.content_item_id,
        first_placement.content_item_id
    );
    assert!(tools
        .route_candidate_placement(
            &curator,
            submitted.candidate.id,
            RouteCandidatePlacementRequest::new(
                denied.id,
                "Outside the grant",
                CandidateConfidence::new(0.9).unwrap(),
            )
            .unwrap(),
            Utc::now(),
        )
        .is_err());

    let content_item_id = first_placement.content_item_id.unwrap();
    let explicitly_added = tools
        .add_content_item_to_pod(
            &curator,
            AddContentItemToPodRequest::new(
                content_item_id,
                added.id,
                Some("Useful reference for this Pod".into()),
            )
            .unwrap(),
            Utc::now(),
        )
        .unwrap();
    assert_eq!(explicitly_added.status, PodPlacementStatus::Accepted);
    assert_eq!(explicitly_added.curation_path, CurationPath::AddToPod);
    assert_eq!(explicitly_added.content_item_id, Some(content_item_id));
    assert_eq!(explicitly_added.audit_history.len(), 1);

    let reversed = tools
        .reverse_pod_placement(
            &curator,
            submitted.candidate.id,
            second.id,
            rationale("Incorrect cross-Pod route"),
            Utc::now(),
        )
        .unwrap();
    assert_eq!(reversed.status, PodPlacementStatus::Reversed);
    assert_eq!(reversed.curation_path, CurationPath::ManualReview);
    assert_eq!(reversed.audit_history.len(), 3);
    assert!(tools
        .list_content_items_for_pod(&curator, second.id)
        .unwrap()
        .is_empty());
    let readded = tools
        .add_content_item_to_pod(
            &curator,
            AddContentItemToPodRequest::new(
                content_item_id,
                second.id,
                Some("Explicitly restore this placement".into()),
            )
            .unwrap(),
            Utc::now(),
        )
        .unwrap();
    assert_eq!(readded.status, PodPlacementStatus::Accepted);
    assert_eq!(readded.curation_path, CurationPath::AddToPod);
    assert_eq!(readded.audit_history.len(), 4);
    assert_eq!(
        tools
            .list_content_items_for_pod(&curator, first.id)
            .unwrap()[0]
            .id(),
        content_item_id
    );

    drop(tools);
    let reopened = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let owner = reopened.default_auth_context().unwrap();
    assert_eq!(
        reopened
            .list_content_items_for_pod(&owner, added.id)
            .unwrap()[0]
            .id(),
        content_item_id
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

#[test]
fn accepted_content_item_events_project_on_a_subscribing_home_node() {
    let origin_dir = TestDataDir::new();
    let home_dir = TestDataDir::new();
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let pod = public_pod(&origin, "projected-curation");
    let curator = harness(
        &origin,
        "origin curator",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        Some(vec![pod.id]),
    );
    origin
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let submitter = harness(
        &origin,
        "origin submitter",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let submitted = origin
        .submit_candidate(&submitter, candidate_request(pod.id, 0.9))
        .unwrap();
    origin
        .curate_candidate(&curator, submitted.candidate.id, Utc::now())
        .unwrap();
    let accepted = origin
        .review_candidate_placement(
            &curator,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap();
    let events = origin.export_pod_events(&curator, &pod.slug).unwrap();
    let placement_event = events
        .iter()
        .find(|event| event.event_type == "content_item_placed")
        .unwrap();
    let public_payload = serde_json::to_string(&placement_event.payload_json).unwrap();
    for private_field in [
        "candidate_id",
        "source_submission_ids",
        "audit_history",
        "actor",
        "submitted_by",
        "submitter_note",
        "discovered_by_crawler",
    ] {
        assert!(!public_payload.contains(private_field));
    }
    let origin_info = origin.node_info(&curator).unwrap();

    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let local_pod = private_pod(&home, "independent-local-placement");
    let local_curator = harness(
        &home,
        "local curator",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        Some(vec![local_pod.id]),
    );
    home.set_pod_curation_policy(
        &local_curator,
        local_pod.id,
        CurationPolicy::Manual,
        Utc::now(),
    )
    .unwrap();
    let local_submitter = harness(
        &home,
        "local submitter",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![local_pod.id]),
    );
    let local_candidate = home
        .submit_candidate(&local_submitter, candidate_request(local_pod.id, 0.9))
        .unwrap();
    home.curate_candidate(&local_curator, local_candidate.candidate.id, Utc::now())
        .unwrap();
    let local_placement = home
        .review_candidate_placement(
            &local_curator,
            local_candidate.candidate.id,
            local_pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap();
    let administrator = harness(
        &home,
        "home administrator",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Administration],
        None,
    );
    let approver = harness(
        &home,
        "home trust approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
        None,
    );
    let now = Utc::now();
    let trust = home
        .request_add_trusted_peer(
            &administrator,
            "origin".into(),
            "https://origin.example".into(),
            origin_info.public_key,
            now,
        )
        .unwrap();
    home.approve_pending_proposal(&approver, trust.id, now)
        .unwrap();
    let peer_id = home
        .store()
        .read()
        .unwrap()
        .trusted_peers
        .values()
        .find(|peer| peer.base_url == "https://origin.example")
        .unwrap()
        .id;

    home.import_pod_events(&administrator, peer_id, events)
        .unwrap();
    let reader = harness(
        &home,
        "home feed reader",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::FeedRead],
        None,
    );
    let projected = home
        .discover_in_pod(
            &reader,
            &pod.slug,
            DiscoverRequest {
                query: "curation".into(),
                avoid: Vec::new(),
                limit: 10,
                mode: DiscoveryMode::DeepMatch,
                user_id: None,
            },
        )
        .unwrap();

    assert!(projected.iter().any(|item| item.title == "Curation report"));
    let projected_pod = home.pod_by_slug(&pod.slug, None).unwrap();
    let placements = home
        .accepted_placements_for_pod(&reader, projected_pod.id)
        .unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0].reason.as_str(), "Strong topical match");
    assert_eq!(placements[0].curation_path, CurationPath::ManualReview);
    assert_eq!(placements[0].origin_node_id, origin_info.node_id);
    assert_eq!(
        placements[0].content_item_id,
        local_placement.content_item_id.unwrap()
    );

    drop(home);
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let reversal_approver = harness(
        &origin,
        "origin reversal approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
        Some(vec![pod.id]),
    );
    let reversed_at = Utc::now();
    let reversal = match origin
        .request_remove_submission_from_pod(
            &curator,
            &pod.slug,
            accepted.content_item_id.unwrap().into(),
            reversed_at,
        )
        .unwrap()
    {
        RemoveSubmissionOutcome::PendingApproval(proposal) => proposal,
        RemoveSubmissionOutcome::Removed { .. } => panic!("public reversal must require approval"),
    };
    origin
        .approve_pending_proposal(&reversal_approver, reversal.id, reversed_at)
        .unwrap();
    home.import_pod_events(
        &administrator,
        peer_id,
        origin.export_pod_events(&curator, &pod.slug).unwrap(),
    )
    .unwrap();

    assert!(home
        .discover_in_pod(
            &reader,
            &pod.slug,
            DiscoverRequest {
                query: "curation".into(),
                avoid: Vec::new(),
                limit: 10,
                mode: DiscoveryMode::DeepMatch,
                user_id: None,
            },
        )
        .unwrap()
        .is_empty());
    assert_eq!(
        home.list_content_items_for_pod(&local_curator, local_pod.id)
            .unwrap()
            .len(),
        1,
        "independent local placement must survive the remote reversal"
    );
}

#[test]
fn rejected_routes_remain_suppressed_and_private_from_federation() {
    let tools = AgentTools::new(seed_store());
    let pod = public_pod(&tools, "rejected-routing");
    let curator = harness(
        &tools,
        "rejection curator",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        Some(vec![pod.id]),
    );
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let submitter = harness(
        &tools,
        "rejection submitter",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let submitted = tools
        .submit_candidate(&submitter, candidate_request(pod.id, 0.95))
        .unwrap();
    tools
        .curate_candidate(&curator, submitted.candidate.id, Utc::now())
        .unwrap();
    let events_before = tools.federation_pod_events(&curator, &pod.slug).unwrap();

    let rejected = tools
        .review_candidate_placement(
            &curator,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Reject,
            Some(rationale("Outside the Pod boundary")),
            Utc::now(),
        )
        .unwrap();
    let repeated_route = tools
        .route_candidate_placement(
            &curator,
            submitted.candidate.id,
            RouteCandidatePlacementRequest::new(
                pod.id,
                "Try the rejected route again",
                CandidateConfidence::new(1.0).unwrap(),
            )
            .unwrap(),
            Utc::now(),
        )
        .unwrap();

    assert_eq!(rejected.status, PodPlacementStatus::Rejected);
    assert_eq!(repeated_route.status, PodPlacementStatus::Rejected);
    assert!(tools
        .list_content_items_for_pod(&curator, pod.id)
        .unwrap()
        .is_empty());
    let events_after = tools.federation_pod_events(&curator, &pod.slug).unwrap();
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
    let public_artifact = serde_json::to_string(&events_after).unwrap();
    assert!(!public_artifact.contains("Outside the Pod boundary"));
    assert!(!public_artifact.contains("https://example.com/curation"));
}

#[test]
fn approved_public_reversal_updates_audit_and_suppresses_future_routing() {
    let tools = AgentTools::new(seed_store());
    let pod = public_pod(&tools, "public-reversal");
    let curator = harness(
        &tools,
        "public reversal curator",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        Some(vec![pod.id]),
    );
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let submitter = harness(
        &tools,
        "public reversal submitter",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let submitted = tools
        .submit_candidate(&submitter, candidate_request(pod.id, 0.9))
        .unwrap();
    tools
        .curate_candidate(&curator, submitted.candidate.id, Utc::now())
        .unwrap();
    let accepted = tools
        .review_candidate_placement(
            &curator,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap();
    let now = Utc::now();
    let proposal = match tools
        .request_remove_submission_from_pod(
            &curator,
            &pod.slug,
            accepted.content_item_id.unwrap().into(),
            now,
        )
        .unwrap()
    {
        RemoveSubmissionOutcome::PendingApproval(proposal) => proposal,
        RemoveSubmissionOutcome::Removed { .. } => panic!("public reversal must require approval"),
    };
    let approver = harness(
        &tools,
        "public reversal approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
        Some(vec![pod.id]),
    );
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();

    assert!(tools
        .list_content_items_for_pod(&curator, pod.id)
        .unwrap()
        .is_empty());
    let repeated = tools
        .route_candidate_placement(
            &curator,
            submitted.candidate.id,
            RouteCandidatePlacementRequest::new(
                pod.id,
                "Try reversed route",
                CandidateConfidence::new(1.0).unwrap(),
            )
            .unwrap(),
            Utc::now(),
        )
        .unwrap();
    assert_eq!(repeated.status, PodPlacementStatus::Reversed);
    assert_eq!(repeated.curation_path, CurationPath::ManualReview);
    assert_eq!(repeated.audit_history.len(), 3);
}
