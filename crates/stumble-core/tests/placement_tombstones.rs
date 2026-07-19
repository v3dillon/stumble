use chrono::{Duration, TimeZone, Utc};
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-placement-tombstones-{label}-{}",
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

fn harness(tools: &AgentTools, label: &str, capabilities: Vec<HarnessCapability>) -> AuthContext {
    let owner = tools.default_auth_context().unwrap();
    let issued = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind: AgentHarnessKind::Interactive,
                capabilities,
                pod_ids: None,
            },
        )
        .unwrap();
    tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap()
}

fn create_public_pod(tools: &AgentTools, slug: &str) -> Pod {
    let proposer = harness(
        tools,
        "public Pod proposer",
        vec![HarnessCapability::PodCuration],
    );
    let approver = harness(
        tools,
        "public Pod approver",
        vec![HarnessCapability::Approval],
    );
    let pod = tools
        .create_pod(
            &proposer,
            CreatePodRequest {
                name: "Remote systems".into(),
                slug: slug.into(),
                description: "Origin-curated references".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let now = Utc::now();
    let proposal = tools
        .create_pending_proposal(
            &proposer,
            SensitiveChange::PublishPod { pod_id: pod.id },
            now,
            now + Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

fn accept_item(tools: &AgentTools, pod: &Pod, suffix: &str) -> ContentItemId {
    let submitter = harness(tools, suffix, vec![HarnessCapability::CandidateSubmission]);
    let curator = harness(tools, suffix, vec![HarnessCapability::PodCuration]);
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let candidate = tools
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                evidence: CandidateSubmissionEvidence {
                    source_url: format!("https://reference.example/{suffix}"),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(format!("Reference {suffix}")),
                        author: Some("Reference author".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("A permitted excerpt".into()),
                    summary: Some("An accepted remote Content Reference".into()),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec!["systems".into()],
                    provenance: CandidateProvenance {
                        discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
                        discovery_method: "browser_search".into(),
                        referrer_url: Some("https://search.example/results".into()),
                    },
                    proposed_placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Directly concerns distributed systems".into(),
                        confidence: CandidateConfidence::new(0.95).unwrap(),
                    }],
                    task_context: None,
                    harness_idempotency_key: format!("origin-worker-{suffix}"),
                    client_idempotency_key: format!("origin-client-{suffix}"),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&curator, candidate.candidate.id, Utc::now())
        .unwrap();
    let placement = tools
        .review_candidate_placement(
            &curator,
            candidate.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap();
    placement.content_item_id.unwrap()
}

fn approve_withdrawal(tools: &AgentTools, pod: &Pod, content_item_id: ContentItemId) {
    let proposer = harness(
        tools,
        "withdrawal proposer",
        vec![HarnessCapability::PodCuration],
    );
    let approver = harness(
        tools,
        "withdrawal approver",
        vec![HarnessCapability::Approval],
    );
    let now = Utc::now();
    let outcome = tools
        .request_remove_submission_from_pod(&proposer, &pod.slug, content_item_id.into(), now)
        .unwrap();
    let RemoveSubmissionOutcome::PendingApproval(proposal) = outcome else {
        panic!("public placement withdrawal must require approval");
    };
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
}

#[test]
fn signed_tombstones_stop_origin_delivery_without_erasing_local_curation() {
    // Phase 1 — Arrange independent Origin and Home Node curation.
    let origin_dir = TestDataDir::new("origin");
    let home_dir = TestDataDir::new("home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let origin_pod = create_public_pod(&origin, "withdrawals");
    let saved_origin_id = accept_item(&origin, &origin_pod, "saved-reference");
    let added_origin_id = accept_item(&origin, &origin_pod, "added-reference");
    let origin_curator = harness(
        &origin,
        "Origin Node independent curator",
        vec![HarnessCapability::PodCuration],
    );
    let origin_local_pod = origin
        .create_pod(
            &origin_curator,
            CreatePodRequest {
                name: "Origin local notes".into(),
                slug: "origin-local-notes".into(),
                description: "Independent Origin Node curation".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let origin_local_placement = origin
        .add_content_item_to_pod(
            &origin_curator,
            AddContentItemToPodRequest::new(
                added_origin_id,
                origin_local_pod.id,
                Some("Keep independently at the Origin Node".into()),
            )
            .unwrap(),
            Utc::now(),
        )
        .unwrap();
    let origin_owner = origin.default_auth_context().unwrap();
    let subscriber = harness(
        &home,
        "Home Node user",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::FeedRead,
            HarnessCapability::Feedback,
            HarnessCapability::PodCuration,
        ],
    );
    let snapshot = origin
        .federation_pod_snapshot(&origin_owner, &origin_pod.slug, None)
        .unwrap();
    let subscribed = home
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/withdrawals",
                snapshot,
            ),
            Utc::now(),
        )
        .unwrap();
    let initial_feed = home
        .get_feed_batch(&subscriber, FeedBatchRequest::new(10).unwrap(), Utc::now())
        .unwrap();
    let saved_local_id = initial_feed
        .items
        .iter()
        .find(|item| {
            item.content_reference
                .canonical_url
                .ends_with("saved-reference")
        })
        .unwrap()
        .content_reference
        .content_item_id;
    let added_local_id = initial_feed
        .items
        .iter()
        .find(|item| {
            item.content_reference
                .canonical_url
                .ends_with("added-reference")
        })
        .unwrap()
        .content_reference
        .content_item_id;
    home.save_link(&subscriber, saved_local_id.into()).unwrap();
    let local_pod = home
        .create_pod(
            &subscriber,
            CreatePodRequest {
                name: "My systems notes".into(),
                slug: "my-systems-notes".into(),
                description: "Independent local curation".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    home.join_pod(&subscriber, &local_pod.slug).unwrap();
    let local_placement = home
        .add_content_item_to_pod(
            &subscriber,
            AddContentItemToPodRequest::new(
                added_local_id,
                local_pod.id,
                Some("Keep this independently".into()),
            )
            .unwrap(),
            Utc::now(),
        )
        .unwrap();

    // Phase 1 — Assert Add to Pod captured the original placement provenance.
    assert_eq!(local_placement.origin_placements.len(), 1);
    assert_eq!(
        local_placement.origin_placements[0].origin_node_id,
        subscribed.subscription.origin_node_id
    );

    // Phase 2 — Act: independently approve both Origin Pod withdrawals.
    approve_withdrawal(&origin, &origin_pod, saved_origin_id);
    approve_withdrawal(&origin, &origin_pod, added_origin_id);

    // Phase 2 — Assert the Origin retained local curation and appended signed history.
    assert_eq!(
        origin
            .pod_placement(
                &origin_curator,
                origin_local_placement.candidate_id,
                origin_local_pod.id,
            )
            .unwrap()
            .origin_withdrawals
            .len(),
        1
    );
    let incremental = origin
        .federation_pod_snapshot(
            &origin_owner,
            &origin_pod.slug,
            subscribed.subscription.last_event_hash.as_deref(),
        )
        .unwrap();
    assert_eq!(
        incremental
            .events
            .iter()
            .filter(|event| event.event_type == "placement_tombstoned")
            .count(),
        2
    );
    // Phase 3 — Act: incrementally project the verified tombstones at the Home Node.
    let synchronized = home
        .synchronize_subscription(&subscriber, subscribed.subscription.id, incremental)
        .unwrap();

    // Phase 3 — Assert only the Origin placements lost eligibility.
    assert_eq!(synchronized.imported_events, 2);
    let saved = home.saved_content_references(&subscriber).unwrap();
    let saved = saved
        .iter()
        .find(|saved| saved.content_reference.content_item_id == saved_local_id)
        .unwrap();
    assert_eq!(saved.origin_withdrawals.len(), 1);
    let retained = home
        .pod_placement(&subscriber, local_placement.candidate_id, local_pod.id)
        .unwrap();
    assert_eq!(retained.status, PodPlacementStatus::Accepted);
    assert_eq!(
        retained.origin_placements,
        local_placement.origin_placements
    );
    assert_eq!(retained.origin_withdrawals.len(), 1);

    // Phase 4 — Act: reverse and re-add the independent local placement.
    home.reverse_pod_placement(
        &subscriber,
        retained.candidate_id,
        local_pod.id,
        CurationRationale::new("Temporarily remove from my Pod").unwrap(),
        Utc::now(),
    )
    .unwrap();
    let readded = home
        .add_content_item_to_pod(
            &subscriber,
            AddContentItemToPodRequest::new(
                added_local_id,
                local_pod.id,
                Some("Restore my independent placement".into()),
            )
            .unwrap(),
            Utc::now(),
        )
        .unwrap();

    // Phase 4 — Assert re-adding did not erase origin history.
    assert_eq!(readded.origin_placements, local_placement.origin_placements);
    assert_eq!(readded.origin_withdrawals.len(), 1);

    // Phase 5 — Act: request a later Feed Batch after recurrence decay.
    let feed = home
        .complete_feed_batch(&subscriber, initial_feed.id, Utc::now())
        .and_then(|_| {
            home.get_feed_batch(
                &subscriber,
                FeedBatchRequest::new(10).unwrap(),
                Utc::now() + Duration::days(31),
            )
        })
        .unwrap();

    // Phase 5 — Assert the saved-only item is ineligible and local curation remains eligible.
    assert_eq!(feed.items.len(), 1);
    assert_eq!(
        feed.items[0].content_reference.content_item_id,
        added_local_id
    );
    assert_eq!(feed.items[0].placements.len(), 1);
    assert_eq!(feed.items[0].placements[0].pod_id, local_pod.id);

    // Phase 6 — Act: retrieve the complete Origin Pod event chain.
    let origin_history = origin
        .federation_pod_snapshot(&origin_owner, &origin_pod.slug, None)
        .unwrap();

    // Phase 6 — Assert acceptance and withdrawal history are both append-only.
    assert!(origin_history.events.iter().any(|event| {
        event.event_type == "content_item_placed"
            && event
                .payload_json
                .to_string()
                .contains(&saved_origin_id.to_string())
    }));
    assert!(origin_history.events.iter().any(|event| {
        event.event_type == "placement_tombstoned"
            && event
                .payload_json
                .to_string()
                .contains(&saved_origin_id.to_string())
    }));

    // Phase 7 — Act: restart the SQLite-backed Home Node.
    drop(home);
    let reopened = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();

    // Phase 7 — Assert private curation and withdrawal provenance survived restart.
    assert_eq!(
        reopened
            .saved_content_references(&subscriber)
            .unwrap()
            .into_iter()
            .find(|saved| saved.content_reference.content_item_id == saved_local_id)
            .unwrap()
            .origin_withdrawals
            .len(),
        1
    );
    assert_eq!(
        reopened
            .pod_placement(&subscriber, local_placement.candidate_id, local_pod.id)
            .unwrap()
            .origin_withdrawals
            .len(),
        1
    );
}

#[test]
fn tombstone_signature_and_cursor_are_verified_before_projection() {
    // Arrange
    let origin_dir = TestDataDir::new("verified-origin");
    let home_dir = TestDataDir::new("verified-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "verified-withdrawals");
    let origin_content_item_id = accept_item(&origin, &pod, "verified-reference");
    let origin_owner = origin.default_auth_context().unwrap();
    let subscriber = harness(
        &home,
        "verifying Home Node user",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::FeedRead,
        ],
    );
    let initial = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let subscribed = home
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/verified-withdrawals",
                initial,
            ),
            Utc::now(),
        )
        .unwrap();
    approve_withdrawal(&origin, &pod, origin_content_item_id);
    let incremental = origin
        .federation_pod_snapshot(
            &origin_owner,
            &pod.slug,
            subscribed.subscription.last_event_hash.as_deref(),
        )
        .unwrap();
    let mut tampered = incremental.clone();
    tampered.events[0].payload_json["placement_tombstone"]["withdrawn_at"] =
        serde_json::json!("2020-01-01T00:00:00Z");

    // Act
    let rejected = home.synchronize_subscription(&subscriber, subscribed.subscription.id, tampered);

    // Assert
    assert!(matches!(
        rejected,
        Err(AgentToolsError::Store(StoreError::InvalidSignature))
    ));
    assert_eq!(
        home.subscription(&subscriber, subscribed.subscription.id)
            .unwrap()
            .last_event_hash,
        subscribed.subscription.last_event_hash
    );
    assert_eq!(
        home.get_feed_batch(&subscriber, FeedBatchRequest::new(1).unwrap(), Utc::now())
            .unwrap()
            .items
            .len(),
        1
    );

    // Phase 2 — Act: apply the valid event once, then replay the same segment.
    let synchronized = home
        .synchronize_subscription(&subscriber, subscribed.subscription.id, incremental.clone())
        .unwrap();
    let replayed = home
        .synchronize_subscription(&subscriber, subscribed.subscription.id, incremental)
        .unwrap();

    // Phase 2 — Assert valid projection is incremental and replay is idempotent.
    assert_eq!(synchronized.imported_events, 1);
    assert_eq!(replayed.imported_events, 0);
}
