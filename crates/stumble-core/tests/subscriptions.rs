use chrono::{Duration, TimeZone, Utc};
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-subscriptions-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        // Test cleanup is best effort; a failed assertion should remain the primary failure.
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
    let private_pod = tools
        .create_pod(
            &proposer,
            CreatePodRequest {
                name: "Remote systems".into(),
                slug: slug.into(),
                description: "Accepted references from the Origin Node".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let now = Utc::now();
    let proposal = tools
        .create_pending_proposal(
            &proposer,
            SensitiveChange::PublishPod {
                pod_id: private_pod.id,
            },
            now,
            now + Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

fn accept_item(tools: &AgentTools, pod: &Pod) {
    let submitter = harness(
        tools,
        "origin submitter",
        vec![HarnessCapability::CandidateSubmission],
    );
    let curator = harness(
        tools,
        "origin curator",
        vec![HarnessCapability::PodCuration],
    );
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let candidate = tools
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                evidence: CandidateSubmissionEvidence {
                    source_url: "https://reference.example/remote-report?utm_source=origin".into(),
                    source_metadata: CandidateSourceMetadata {
                        title: Some("Remote report".into()),
                        author: Some("Reference author".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("A permitted excerpt".into()),
                    summary: Some("An accepted remote Content Reference".into()),
                    content_type: CandidateContentType::Article,
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
                    harness_idempotency_key: "origin-worker-1".into(),
                    client_idempotency_key: "origin-client-1".into(),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&curator, candidate.candidate.id, Utc::now())
        .unwrap();
    tools
        .review_candidate_placement(
            &curator,
            candidate.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap();
}

#[test]
fn incompatible_protocol_is_rejected_before_events_are_projected() {
    let origin = AgentTools::new(seed_store());
    let home = AgentTools::new(seed_store());
    let pod = create_public_pod(&origin, "future-event-shapes");
    accept_item(&origin, &pod);
    let mut snapshot = origin
        .federation_pod_snapshot(&origin.default_auth_context().unwrap(), &pod.slug, None)
        .unwrap();
    snapshot.node.supported_protocol_version = "stumble/0.1".into();
    let subscriber = harness(
        &home,
        "protocol negotiator",
        vec![HarnessCapability::SubscriptionManagement],
    );

    let result = home.subscribe_public_pod(
        &subscriber,
        SubscribePublicPodRequest::new(
            "https://origin.example/federation/pods/future-event-shapes",
            snapshot,
        ),
        Utc::now(),
    );

    assert!(matches!(
        result,
        Err(AgentToolsError::IncompatibleProtocol { received, supported })
            if received == "stumble/0.1" && supported == CURRENT_PROTOCOL_VERSION
    ));
    assert!(home.pod_by_slug(&pod.slug, subscriber.tenant_id).is_err());
}

#[test]
fn subscribed_home_node_synchronizes_incrementally_and_reads_remote_content_offline() {
    // Arrange: publish accepted content on a reachable Origin Node.
    let origin_dir = TestDataDir::new("origin");
    let home_dir = TestDataDir::new("home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "remote-systems");
    accept_item(&origin, &pod);
    let origin_owner = origin.default_auth_context().unwrap();
    let snapshot = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let replayed_snapshot = snapshot.clone();
    let subscriber = harness(
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
}

#[test]
fn invalid_signed_artifacts_are_rejected_before_any_remote_projection() {
    // Arrange
    let origin_dir = TestDataDir::new("invalid-origin");
    let home_dir = TestDataDir::new("invalid-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "tampered-pod");
    let mut snapshot = origin
        .federation_pod_snapshot(&origin.default_auth_context().unwrap(), &pod.slug, None)
        .unwrap();
    snapshot.events[0].payload_json["pod"]["description"] =
        serde_json::json!("an attacker changed signed metadata");
    let subscriber = harness(
        &home,
        "careful subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );

    // Act
    let result = home.subscribe_public_pod(
        &subscriber,
        SubscribePublicPodRequest::new(
            "https://origin.example/federation/pods/tampered-pod",
            snapshot,
        ),
        Utc::now(),
    );

    // Assert
    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::InvalidSignature))
    ));
    assert!(home.pod_by_slug("tampered-pod", None).is_err());
}

#[test]
fn malformed_but_validly_signed_event_is_rejected_before_projection() {
    // Arrange
    let origin_dir = TestDataDir::new("malformed-origin");
    let home_dir = TestDataDir::new("malformed-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "malformed-pod");
    let mut snapshot = origin
        .federation_pod_snapshot(&origin.default_auth_context().unwrap(), &pod.slug, None)
        .unwrap();
    let node = origin.store().read().unwrap().default_node().unwrap();
    let malformed = sign_public_event(
        &node,
        "content_item_placed",
        &pod.slug,
        serde_json::json!({
            "content_item": "not a Content Item",
            "accepted_placement": "not an Accepted Placement"
        }),
        snapshot.manifest.latest_known_event_hash.clone(),
    )
    .unwrap();
    snapshot.manifest.latest_known_event_hash = Some(malformed.content_hash.clone());
    snapshot.events.push(malformed);
    let subscriber = harness(
        &home,
        "strict subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );

    // Act
    let result = home.subscribe_public_pod(
        &subscriber,
        SubscribePublicPodRequest::new(
            "https://origin.example/federation/pods/malformed-pod",
            snapshot,
        ),
        Utc::now(),
    );

    // Assert
    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::Validation(_)))
    ));
    assert!(home.pod_by_slug("malformed-pod", None).is_err());
}

#[test]
fn unaccepted_legacy_submission_is_absent_from_synchronization_artifacts() {
    // Arrange
    let origin_dir = TestDataDir::new("unaccepted-origin");
    let home_dir = TestDataDir::new("unaccepted-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "accepted-only");
    let curator = harness(
        &origin,
        "legacy submitter",
        vec![
            HarnessCapability::PodCuration,
            HarnessCapability::CandidateSubmission,
        ],
    );
    origin
        .submit_link_to_pod(
            &curator,
            &pod.slug,
            SubmitLinkRequest {
                url: "https://private-candidate.example/unaccepted".into(),
                title: Some("Unaccepted legacy submission".into()),
                description: Some("must not synchronize".into()),
                note: Some("private submitter note".into()),
                tags: vec!["private-candidate".into()],
                discovered_by_crawler: false,
            },
        )
        .unwrap();

    // Act
    let snapshot = origin
        .federation_pod_snapshot(&origin.default_auth_context().unwrap(), &pod.slug, None)
        .unwrap();
    let artifacts = serde_json::to_string(&snapshot).unwrap();
    let subscriber = harness(
        &home,
        "accepted-only subscriber",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::FeedRead,
        ],
    );
    home.subscribe_public_pod(
        &subscriber,
        SubscribePublicPodRequest::new(
            "https://origin.example/federation/pods/accepted-only",
            snapshot,
        ),
        Utc::now(),
    )
    .unwrap();

    // Assert
    assert!(!artifacts.contains("private-candidate.example"));
    assert!(!artifacts.contains("private submitter note"));
    assert!(home
        .get_feed_batch(&subscriber, FeedBatchRequest::new(1).unwrap(), Utc::now())
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn later_accepted_placement_syncs_from_cursor_without_exporting_home_feedback() {
    // Arrange: subscribe before the Origin Node accepts any Content Item.
    let origin_dir = TestDataDir::new("incremental-origin");
    let home_dir = TestDataDir::new("incremental-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "incremental-pod");
    let origin_owner = origin.default_auth_context().unwrap();
    let initial = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let user = harness(
        &home,
        "private Home Node user",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::FeedRead,
            HarnessCapability::Feedback,
        ],
    );
    let subscribed = home
        .subscribe_public_pod(
            &user,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/incremental-pod",
                initial,
            ),
            Utc::now(),
        )
        .unwrap();
    assert_eq!(subscribed.imported_events, 2);
    accept_item(&origin, &pod);

    // Act: resume from the stored cursor and apply only the new accepted placement.
    let incremental = origin
        .federation_pod_snapshot(
            &origin_owner,
            &pod.slug,
            subscribed.subscription.last_event_hash.as_deref(),
        )
        .unwrap();
    assert_eq!(incremental.events.len(), 1);
    let synchronized = home
        .synchronize_subscription(&user, subscribed.subscription.id, incremental)
        .unwrap();

    // Assert: the permitted reference is delivered, while feedback stays Home-Node local.
    assert_eq!(synchronized.imported_events, 1);
    let feed = home
        .get_feed_batch(&user, FeedBatchRequest::new(1).unwrap(), Utc::now())
        .unwrap();
    let item = &feed.items[0];
    assert_eq!(item.content_reference.title, "Remote report");
    assert_eq!(
        item.content_reference.permitted_description.as_deref(),
        Some("A permitted excerpt")
    );
    home.record_feed_feedback(
        &user,
        item.content_reference.content_item_id,
        FeedbackKind::Interesting,
        None,
        Some("locally useful".into()),
        Utc::now(),
    )
    .unwrap();
    let mut private_taste = UpdateTasteProfileRequest::default();
    private_taste.interests = Some(vec!["secret-home-interest".into()]);
    home.update_taste_profile(&user, private_taste).unwrap();
    assert!(home
        .taste_profile(&user)
        .unwrap()
        .explicit
        .interests
        .contains(&"secret-home-interest".to_string()));
    let public_artifacts = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let public_json = serde_json::to_string(&public_artifacts).unwrap();
    assert!(!public_json.contains("locally useful"));
    assert!(!public_json.contains("feedback"));
    let home_public_json = serde_json::to_string(&(
        home.node_info(&home.default_auth_context().unwrap())
            .unwrap(),
        home.list_public_pods(&home.default_auth_context().unwrap())
            .unwrap(),
    ))
    .unwrap();
    assert!(!home_public_json.contains("secret-home-interest"));
    assert!(!home_public_json.contains("locally useful"));
    assert!(!home_public_json.contains("feedback"));
    assert!(home
        .federation_pod_snapshot(&home.default_auth_context().unwrap(), &pod.slug, None)
        .is_err());
}

#[test]
fn a_remote_slug_collision_cannot_mutate_a_local_private_pod() {
    let origin_dir = TestDataDir::new("collision-origin");
    let home_dir = TestDataDir::new("collision-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let remote_pod = create_public_pod(&origin, "collision-pod");
    let local_owner = harness(
        &home,
        "local Pod owner",
        vec![HarnessCapability::PodCuration],
    );
    let local_pod = home
        .create_pod(
            &local_owner,
            CreatePodRequest {
                name: "Private local Pod".into(),
                slug: "collision-pod".into(),
                description: "must remain local".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let local_package = home
        .get_pod_package_version(
            &local_owner,
            &local_pod.slug,
            PackageVersion::new(1).unwrap(),
        )
        .unwrap();
    let snapshot = origin
        .federation_pod_snapshot(
            &origin.default_auth_context().unwrap(),
            &remote_pod.slug,
            None,
        )
        .unwrap();
    let subscriber = harness(
        &home,
        "collision subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );

    let result = home.subscribe_public_pod(
        &subscriber,
        SubscribePublicPodRequest::new(
            "https://origin.example/federation/pods/collision-pod",
            snapshot,
        ),
        Utc::now(),
    );

    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::Duplicate(_)))
    ));
    let unchanged = home.pod_by_slug("collision-pod", None).unwrap();
    assert_eq!(unchanged.id, local_pod.id);
    assert_eq!(unchanged.visibility, Visibility::Private);
    assert_eq!(unchanged.origin_node_id, local_pod.origin_node_id);
    assert_eq!(
        home.get_pod_package_version(
            &local_owner,
            &local_pod.slug,
            PackageVersion::new(1).unwrap(),
        )
        .unwrap()
        .context_md,
        local_package.context_md
    );
}

#[test]
fn a_remote_pod_id_collision_cannot_overwrite_an_unrelated_local_pod() {
    let origin_dir = TestDataDir::new("id-collision-origin");
    let home_dir = TestDataDir::new("id-collision-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let remote_pod = create_public_pod(&origin, "remote-id-collision");
    let local_owner = harness(
        &home,
        "ID collision owner",
        vec![HarnessCapability::PodCuration],
    );
    let mut local_pod = home
        .create_pod(
            &local_owner,
            CreatePodRequest {
                name: "Unrelated private Pod".into(),
                slug: "unrelated-private-pod".into(),
                description: "must not be overwritten".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    {
        let store = home.store();
        let mut store = store.write().unwrap();
        store.pods.remove(&local_pod.id);
        local_pod.id = remote_pod.id;
        store.pods.insert(local_pod.id, local_pod.clone());
    }
    let snapshot = origin
        .federation_pod_snapshot(
            &origin.default_auth_context().unwrap(),
            &remote_pod.slug,
            None,
        )
        .unwrap();
    let subscriber = harness(
        &home,
        "ID collision subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );

    let result = home.subscribe_public_pod(
        &subscriber,
        SubscribePublicPodRequest::new(
            "https://origin.example/federation/pods/remote-id-collision",
            snapshot,
        ),
        Utc::now(),
    );

    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::Duplicate(_)))
    ));
    assert_eq!(
        home.pod_by_slug("unrelated-private-pod", None).unwrap().id,
        remote_pod.id
    );
    assert!(home.pod_by_slug("remote-id-collision", None).is_err());
}

#[test]
fn anonymous_context_cannot_synchronize_an_owned_subscription() {
    let origin_dir = TestDataDir::new("anonymous-origin");
    let home_dir = TestDataDir::new("anonymous-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "owned-subscription");
    let origin_owner = origin.default_auth_context().unwrap();
    let initial = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let subscriber = harness(
        &home,
        "subscription owner",
        vec![HarnessCapability::SubscriptionManagement],
    );
    let subscribed = home
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/owned-subscription",
                initial,
            ),
            Utc::now(),
        )
        .unwrap();
    let empty = origin
        .federation_pod_snapshot(
            &origin_owner,
            &pod.slug,
            subscribed.subscription.last_event_hash.as_deref(),
        )
        .unwrap();
    let mut anonymous = subscriber.clone();
    anonymous.user_id = None;
    anonymous.harness_id = None;

    let result = home.synchronize_subscription(&anonymous, subscribed.subscription.id, empty);

    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::Validation(_)))
    ));
}

#[test]
fn invalid_package_version_rejects_the_whole_increment_before_projection() {
    let origin_dir = TestDataDir::new("package-origin");
    let home_dir = TestDataDir::new("package-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "package-version-pod");
    let origin_owner = origin.default_auth_context().unwrap();
    let initial = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let subscriber = harness(
        &home,
        "package subscriber",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::FeedRead,
        ],
    );
    let subscribed = home
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/package-version-pod",
                initial,
            ),
            Utc::now(),
        )
        .unwrap();
    accept_item(&origin, &pod);
    let mut incremental = origin
        .federation_pod_snapshot(
            &origin_owner,
            &pod.slug,
            subscribed.subscription.last_event_hash.as_deref(),
        )
        .unwrap();
    let mut invalid_package = origin
        .get_pod_package_version(&origin_owner, &pod.slug, PackageVersion::new(1).unwrap())
        .unwrap();
    invalid_package.version = 0;
    let node = origin.store().read().unwrap().default_node().unwrap();
    let invalid_event = sign_public_event(
        &node,
        "pod_skill_pack_updated",
        &pod.slug,
        serde_json::json!({"package": invalid_package}),
        incremental.manifest.latest_known_event_hash.clone(),
    )
    .unwrap();
    incremental.manifest.skill_pack_version = 0;
    incremental.manifest.latest_known_event_hash = Some(invalid_event.content_hash.clone());
    incremental.events.push(invalid_event);

    let result =
        home.synchronize_subscription(&subscriber, subscribed.subscription.id, incremental);

    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::Validation(_)))
    ));
    assert!(home
        .get_feed_batch(&subscriber, FeedBatchRequest::new(1).unwrap(), Utc::now())
        .unwrap()
        .items
        .is_empty());
    assert_eq!(
        home.subscription(&subscriber, subscribed.subscription.id)
            .unwrap()
            .last_event_hash,
        subscribed.subscription.last_event_hash
    );
}

#[test]
fn a_changed_package_cannot_reuse_an_immutable_version() {
    let origin_dir = TestDataDir::new("reused-package-origin");
    let home_dir = TestDataDir::new("reused-package-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "immutable-package-pod");
    let origin_owner = origin.default_auth_context().unwrap();
    let initial = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let subscriber = harness(
        &home,
        "immutable package subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );
    let subscribed = home
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/immutable-package-pod",
                initial,
            ),
            Utc::now(),
        )
        .unwrap();
    let mut changed = origin
        .get_pod_package_version(&origin_owner, &pod.slug, PackageVersion::new(1).unwrap())
        .unwrap();
    changed
        .context_md
        .push_str("\nChanged while reusing version one.\n");
    let node = origin.store().read().unwrap().default_node().unwrap();
    let event = sign_public_event(
        &node,
        "pod_skill_pack_updated",
        &pod.slug,
        serde_json::json!({"package": changed}),
        subscribed.subscription.last_event_hash.clone(),
    )
    .unwrap();
    let snapshot = FederationPodSnapshot::new(
        origin.node_info(&origin_owner).unwrap(),
        PodManifest {
            pod,
            latest_known_event_hash: Some(event.content_hash.clone()),
            skill_pack_version: 1,
            public_source_summary: vec![],
        },
        vec![event],
    );

    let result = home.synchronize_subscription(&subscriber, subscribed.subscription.id, snapshot);

    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::Validation(_)))
    ));
}
