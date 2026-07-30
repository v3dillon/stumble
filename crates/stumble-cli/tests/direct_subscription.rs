use axum::{body::Body, http::Request};
use chrono::Utc;
use stumble_api::{router, router_with_base_url};
use stumble_core::*;
use tower::ServiceExt;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-direct-subscription-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        // Test cleanup is best effort; assertion failures remain primary.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn harness(tools: &AgentTools, label: &str, capabilities: Vec<HarnessCapability>) -> AuthContext {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
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

fn accepted_public_item(origin: &AgentTools) -> Pod {
    let proposer = harness(
        origin,
        "origin proposer",
        vec![HarnessCapability::PodCuration],
    );
    let approver = harness(origin, "origin approver", vec![HarnessCapability::Approval]);
    let private_pod = origin
        .create_pod(
            &proposer,
            CreatePodRequest {
                name: "Reachable Origin Pod".into(),
                slug: "reachable-origin".into(),
                description: "Direct-address acceptance".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let now = Utc::now();
    let proposal = match origin
        .request_set_pod_visibility(&proposer, private_pod.id, Visibility::Public, now)
        .unwrap()
    {
        PodVisibilityOutcome::PendingApproval(proposal) => proposal,
        PodVisibilityOutcome::Updated(_) => panic!("publication must require approval"),
    };
    origin
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    let pod = origin.pod_by_slug("reachable-origin", None).unwrap();
    origin
        .set_pod_curation_policy(&proposer, pod.id, CurationPolicy::Manual, now)
        .unwrap();
    let submitter = harness(
        origin,
        "origin candidate submitter",
        vec![HarnessCapability::CandidateSubmission],
    );
    let submitted = origin
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Proves direct addressing".into(),
                        confidence: CandidateConfidence::new(0.95).unwrap(),
                    }],
                    task_context: None,
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: "https://reference.example/outbound-only".into(),
                    source_metadata: CandidateSourceMetadata {
                        title: Some("Outbound-only report".into()),
                        author: None,
                        published_at: None,
                    },
                    permitted_excerpt: Some("Permitted HTTP acceptance excerpt".into()),
                    summary: Some("Fetched from a reachable Origin Node".into()),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec!["outbound".into()],
                    provenance: CandidateProvenance {
                        discovered_at: now,
                        discovery_method: "browser_search".into(),
                        referrer_url: None,
                    },
                    harness_idempotency_key: "direct-http-worker".into(),
                    client_idempotency_key: "direct-http-client".into(),
                },
            },
        )
        .unwrap();
    origin
        .curate_candidate(&proposer, submitted.candidate.id, now)
        .unwrap();
    origin
        .review_candidate_placement(
            &proposer,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            now,
        )
        .unwrap();
    pod
}

#[tokio::test]
async fn private_home_node_subscribes_outbound_through_the_direct_pod_url() {
    // Arrange: only the Origin Node receives an inbound listener.
    let origin_dir = TestDataDir::new("origin");
    let home_dir = TestDataDir::new("home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    accepted_public_item(&origin);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let base_url = format!("http://{address}");
    let origin_router = router_with_base_url(origin.clone(), &base_url);
    let origin_server = tokio::spawn(async move { axum::serve(listener, origin_router).await });
    let subscriber = harness(
        &home,
        "outbound-only Home Node",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::FeedRead,
            HarnessCapability::Feedback,
        ],
    );

    // Act: the Home Node resolves and fetches the direct Pod URL over HTTP loopback.
    let synchronized = stumble_sync::subscribe_pod_from_url(
        &home,
        &subscriber,
        &format!("{base_url}/federation/pods/reachable-origin"),
    )
    .await
    .unwrap();

    // Assert: remote accepted content is local and public Home responses stay private.
    assert_eq!(synchronized.imported_events, 2);
    let feed = home
        .get_feed_batch(&subscriber, FeedBatchRequest::new(1).unwrap(), Utc::now())
        .unwrap();
    assert_eq!(
        feed.items[0].content_reference.canonical_url,
        "https://reference.example/outbound-only"
    );
    home.record_feed_feedback(
        &subscriber,
        feed.items[0].content_reference.content_item_id,
        FeedbackKind::Interesting,
        None,
        Some("private-home-feedback".into()),
        Utc::now(),
    )
    .unwrap();
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec!["private-home-interest".into()]);
    home.update_taste_profile(&subscriber, taste).unwrap();

    let public_home = router(home.clone());
    let mut public_bodies = String::new();
    for path in [
        "/federation/node",
        "/federation/pods",
        "/federation/pods/reachable-origin/manifest",
        "/federation/pods/reachable-origin/events",
    ] {
        let response = public_home
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        public_bodies.push_str(&String::from_utf8_lossy(&bytes));
    }
    assert!(!public_bodies.contains("private-home-feedback"));
    assert!(!public_bodies.contains("private-home-interest"));
    assert!(!public_bodies.contains("reference.example/outbound-only"));

    // No Home listener was needed; synchronized SQLite content survives Origin outage.
    origin_server.abort();
    let offline = home
        .get_feed_batch(&subscriber, FeedBatchRequest::new(1).unwrap(), Utc::now())
        .unwrap();
    assert_eq!(offline.id, feed.id);
}

#[tokio::test]
async fn disallowed_plain_http_is_rejected_before_any_origin_request() {
    let home_dir = TestDataDir::new("http-policy-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let subscriber = harness(
        &home,
        "URL policy subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );

    let result = stumble_sync::subscribe_pod_from_url(
        &home,
        &subscriber,
        "http://192.0.2.1/federation/pods/disallowed",
    )
    .await;

    assert!(matches!(
        result,
        Err(stumble_sync::DirectSubscriptionError::InvalidAddress(_))
    ));
}
