use chrono::Utc;
use stumble_core::*;

use crate::common::*;

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
