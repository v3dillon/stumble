mod support;

use chrono::Utc;
use stumble_core::*;
use support::*;

fn enrichment_request(
    pod_id: PodId,
    key: &str,
    media_references: Vec<MediaReference>,
) -> CandidateSubmissionRequest {
    CandidateSubmissionRequest {
        evidence: CandidateSubmissionEvidence {
            source_url: "https://reference.example/remote-report".into(),
            source_metadata: CandidateSourceMetadata {
                title: None,
                author: None,
                published_at: None,
            },
            permitted_excerpt: None,
            summary: None,
            content_type: CandidateContentType::Article,
            media_references,
            tags: vec![],
            provenance: CandidateProvenance {
                discovered_at: Utc::now(),
                discovery_method: "later_browser_evidence".into(),
                referrer_url: None,
            },
            proposed_placements: vec![ProposedCandidatePlacement {
                pod_id,
                reason: "Corroborates the accepted reference".into(),
                confidence: CandidateConfidence::new(0.8).unwrap(),
            }],
            task_context: None,
            harness_idempotency_key: format!("{key}-worker"),
            client_idempotency_key: format!("{key}-client"),
        },
    }
}

#[test]
fn subscribed_home_node_synchronizes_incrementally_and_reads_remote_content_offline() {
    // Arrange: publish accepted content on a reachable Origin Node.
    let origin_dir = TestDataDir::new("origin");
    let home_dir = TestDataDir::new("home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "remote-systems");
    submit_and_accept_placement(&origin, &pod);
    let origin_owner = origin.default_auth_context().unwrap();
    let snapshot = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let signed_content_item: ContentItem = serde_json::from_value(
        snapshot
            .events
            .iter()
            .find(|event| event.event_type == "content_item_placed")
            .unwrap()
            .payload_json["content_item"]
            .clone(),
    )
    .unwrap();
    assert_eq!(
        signed_content_item.media_references(),
        &[media_reference(
            MediaReferenceType::Image,
            "https://media.reference.example/remote-report/diagram.jpg",
        )]
    );
    let replayed_snapshot = snapshot.clone();
    let subscriber = register_authenticated_harness(
        &home,
        "outbound-only subscriber",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::FeedRead,
        ],
    );

    // Act: subscribe by the canonical public Pod URL and project verified artifacts.
    let synchronized = home
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/remote-systems",
                snapshot,
            ),
            Utc::now(),
        )
        .unwrap();

    // Assert: accepted remote content is Feed-eligible and the cursor is idempotent.
    assert_eq!(synchronized.imported_events, 3);
    let cursor = synchronized.subscription.last_event_hash.clone();
    assert_eq!(
        home.synchronize_subscription(
            &subscriber,
            synchronized.subscription.id,
            replayed_snapshot,
        )
        .unwrap()
        .imported_events,
        0
    );
    let no_changes = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, cursor.as_deref())
        .unwrap();
    assert!(no_changes.events.is_empty());
    assert_eq!(
        home.synchronize_subscription(&subscriber, synchronized.subscription.id, no_changes)
            .unwrap()
            .imported_events,
        0
    );
    let feed = home
        .get_feed_batch(&subscriber, FeedBatchRequest::new(10).unwrap(), Utc::now())
        .unwrap();
    assert_eq!(feed.items.len(), 1);
    assert_eq!(
        feed.items[0].content_reference.canonical_url,
        "https://reference.example/remote-report"
    );
    assert_eq!(
        feed.items[0].content_reference.media_references,
        vec![media_reference(
            MediaReferenceType::Image,
            "https://media.reference.example/remote-report/diagram.jpg",
        )]
    );
    let synchronized_pod = home
        .pod_by_slug("remote-systems", subscriber.tenant_id)
        .unwrap();
    let pod_content = home
        .pod_content_stream(&subscriber, synchronized_pod.id)
        .unwrap();
    assert_eq!(
        pod_content[0].content_item.media_references(),
        feed.items[0].content_reference.media_references
    );
    assert_eq!(feed.items[0].placements.len(), 1);
    assert_eq!(
        feed.items[0].placements[0].origin_node_id,
        synchronized.subscription.origin_node_id
    );
    assert_eq!(
        home.get_pod_package_version(
            &subscriber,
            "remote-systems",
            PackageVersion::new(1).unwrap(),
        )
        .unwrap()
        .version,
        1
    );

    // The Origin Node can now be unavailable; SQLite retains the synchronized projection.
    drop(origin);
    drop(home);
    let reopened = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    assert_eq!(
        reopened
            .subscription(&subscriber, synchronized.subscription.id)
            .unwrap()
            .last_event_hash,
        cursor
    );
    let offline_feed = reopened
        .get_feed_batch(&subscriber, FeedBatchRequest::new(10).unwrap(), Utc::now())
        .unwrap();
    assert_eq!(offline_feed.items.len(), 1);
    assert_eq!(
        offline_feed.items[0].content_reference.media_references,
        feed.items[0].content_reference.media_references
    );
}

#[test]
fn later_canonical_evidence_enriches_accepted_content_and_synchronizes_after_restart() {
    let origin_dir = TestDataDir::new("enrichment-origin");
    let home_dir = TestDataDir::new("enrichment-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "enriched-media");
    submit_and_accept_placement(&origin, &pod);
    let origin_owner = origin.default_auth_context().unwrap();
    let subscriber = register_authenticated_harness(
        &home,
        "media enrichment subscriber",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::FeedRead,
        ],
    );
    let subscribed = home
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/enriched-media",
                origin
                    .federation_pod_snapshot(&origin_owner, &pod.slug, None)
                    .unwrap(),
            ),
            Utc::now(),
        )
        .unwrap();
    let cursor = subscribed.subscription.last_event_hash.clone();

    let submitter = register_authenticated_harness(
        &origin,
        "later evidence submitter",
        vec![HarnessCapability::CandidateSubmission],
    );
    let mut later = CandidateSubmissionRequest {
        evidence: CandidateSubmissionEvidence {
            source_url: "HTTPS://REFERENCE.EXAMPLE:443/remote-report#later".into(),
            source_metadata: CandidateSourceMetadata {
                title: Some("Later corroboration".into()),
                author: None,
                published_at: None,
            },
            permitted_excerpt: None,
            summary: None,
            content_type: CandidateContentType::Article,
            media_references: vec![
                media_reference(
                    MediaReferenceType::Image,
                    "https://MEDIA.REFERENCE.EXAMPLE:443/remote-report/diagram.jpg#duplicate",
                ),
                media_reference(
                    MediaReferenceType::Video,
                    "https://media.reference.example/remote-report/walkthrough.mp4",
                ),
            ],
            tags: vec![],
            provenance: CandidateProvenance {
                discovered_at: Utc::now(),
                discovery_method: "later_browser_evidence".into(),
                referrer_url: None,
            },
            proposed_placements: vec![ProposedCandidatePlacement {
                pod_id: pod.id,
                reason: "Corroborates the accepted reference".into(),
                confidence: CandidateConfidence::new(0.8).unwrap(),
            }],
            task_context: None,
            harness_idempotency_key: "later-media-worker".into(),
            client_idempotency_key: "later-media-client".into(),
        },
    };
    origin.submit_candidate(&submitter, later.clone()).unwrap();

    let expected = vec![
        media_reference(
            MediaReferenceType::Image,
            "https://media.reference.example/remote-report/diagram.jpg",
        ),
        media_reference(
            MediaReferenceType::Video,
            "https://media.reference.example/remote-report/walkthrough.mp4",
        ),
    ];
    assert_eq!(
        origin.pod_content_stream(&origin_owner, pod.id).unwrap()[0]
            .content_item
            .media_references(),
        expected
    );
    let remote_pod = home.pod_by_slug(&pod.slug, subscriber.tenant_id).unwrap();

    drop(origin);
    drop(home);
    let origin = AgentTools::open_initialized_home_node(&origin_dir.0).unwrap();
    let home = AgentTools::open_initialized_home_node(&home_dir.0).unwrap();
    let enrichment = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, cursor.as_deref())
        .unwrap();
    assert_eq!(enrichment.events.len(), 1);
    assert_eq!(
        enrichment.events[0].event_type,
        "content_item_metadata_updated"
    );
    assert_eq!(
        home.synchronize_subscription(&subscriber, subscribed.subscription.id, enrichment)
            .unwrap()
            .imported_events,
        1
    );
    assert_eq!(
        home.pod_content_stream(&subscriber, remote_pod.id).unwrap()[0]
            .content_item
            .media_references(),
        expected
    );

    later.evidence.harness_idempotency_key = "conflicting-media-worker".into();
    later.evidence.client_idempotency_key = "conflicting-media-client".into();
    later.evidence.media_references = vec![media_reference(
        MediaReferenceType::Image,
        "https://media.reference.example/remote-report/walkthrough.mp4",
    )];
    let events_before_conflict = origin
        .federation_pod_events(&origin_owner, &pod.slug)
        .unwrap()
        .len();
    assert!(matches!(
        origin.submit_candidate(&submitter, later),
        Err(AgentToolsError::Store(StoreError::Validation(message)))
            if message.contains("conflicting media types")
    ));
    assert_eq!(
        origin
            .federation_pod_events(&origin_owner, &pod.slug)
            .unwrap()
            .len(),
        events_before_conflict
    );

    drop(origin);
    drop(home);
    let origin = AgentTools::open_initialized_home_node(&origin_dir.0).unwrap();
    let home = AgentTools::open_initialized_home_node(&home_dir.0).unwrap();
    assert_eq!(
        origin.pod_content_stream(&origin_owner, pod.id).unwrap()[0]
            .content_item
            .media_references(),
        expected
    );
    assert_eq!(
        home.pod_content_stream(&subscriber, remote_pod.id).unwrap()[0]
            .content_item
            .media_references(),
        expected
    );
}

#[test]
fn cross_pod_update_order_cannot_regress_resolved_media_evidence() {
    let origin = AgentTools::new(seed_store());
    let home = AgentTools::new(seed_store());
    let first = create_public_pod(&origin, "media-order-first");
    let second = create_public_pod(&origin, "media-order-second");
    submit_and_accept_placement(&origin, &first);
    let origin_owner = origin.default_auth_context().unwrap();
    let content_item_id = origin.pod_content_stream(&origin_owner, first.id).unwrap()[0]
        .content_item
        .id();
    let curator = register_authenticated_harness(
        &origin,
        "cross Pod curator",
        vec![HarnessCapability::PodCuration],
    );
    origin
        .add_content_item_to_pod(
            &curator,
            AddContentItemToPodRequest::new(content_item_id, second.id, None).unwrap(),
            Utc::now(),
        )
        .unwrap();
    let subscriber = register_authenticated_harness(
        &home,
        "cross Pod subscriber",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::FeedRead,
        ],
    );
    let first_subscription = home
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/media-order-first",
                origin
                    .federation_pod_snapshot(&origin_owner, &first.slug, None)
                    .unwrap(),
            ),
            Utc::now(),
        )
        .unwrap()
        .subscription;
    let second_subscription = home
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/media-order-second",
                origin
                    .federation_pod_snapshot(&origin_owner, &second.slug, None)
                    .unwrap(),
            ),
            Utc::now(),
        )
        .unwrap()
        .subscription;
    let submitter = register_authenticated_harness(
        &origin,
        "cross Pod evidence submitter",
        vec![HarnessCapability::CandidateSubmission],
    );
    origin
        .submit_candidate(
            &submitter,
            enrichment_request(
                first.id,
                "cross-pod-video",
                vec![media_reference(
                    MediaReferenceType::Video,
                    "https://media.reference.example/remote-report/walkthrough.mp4",
                )],
            ),
        )
        .unwrap();
    origin
        .submit_candidate(
            &submitter,
            enrichment_request(
                first.id,
                "cross-pod-image",
                vec![media_reference(
                    MediaReferenceType::Image,
                    "https://media.reference.example/remote-report/final.png",
                )],
            ),
        )
        .unwrap();

    let latest_first = origin
        .federation_pod_snapshot(
            &origin_owner,
            &first.slug,
            first_subscription.last_event_hash.as_deref(),
        )
        .unwrap();
    assert_eq!(latest_first.events.len(), 2);
    home.synchronize_subscription(&subscriber, first_subscription.id, latest_first)
        .unwrap();

    let mut stale_second = origin
        .federation_pod_snapshot(
            &origin_owner,
            &second.slug,
            second_subscription.last_event_hash.as_deref(),
        )
        .unwrap();
    assert_eq!(stale_second.events.len(), 2);
    stale_second.events.truncate(1);
    stale_second.manifest.latest_known_event_hash =
        Some(stale_second.events[0].content_hash.clone());
    home.synchronize_subscription(&subscriber, second_subscription.id, stale_second)
        .unwrap();

    let remote_first = home.pod_by_slug(&first.slug, subscriber.tenant_id).unwrap();
    assert_eq!(
        home.pod_content_stream(&subscriber, remote_first.id)
            .unwrap()[0]
            .content_item
            .media_references(),
        &[
            media_reference(
                MediaReferenceType::Image,
                "https://media.reference.example/remote-report/diagram.jpg",
            ),
            media_reference(
                MediaReferenceType::Image,
                "https://media.reference.example/remote-report/final.png",
            ),
            media_reference(
                MediaReferenceType::Video,
                "https://media.reference.example/remote-report/walkthrough.mp4",
            ),
        ]
    );
}
