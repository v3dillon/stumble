mod support;

use axum::{routing::get, Json, Router};
use chrono::Utc;
use serde_json::json;
use stumble_core::*;
use stumble_mcp::{serve_stdio, McpToolCall, McpToolRouter};
use support::{EphemeralHttpServer, McpClient, McpToolResult, PersistentNode, ScopedHarness};

#[derive(Clone)]
struct FixtureOrigin {
    snapshot: std::sync::Arc<std::sync::RwLock<FederationPodSnapshot>>,
}

async fn start_fixture_origin(
    snapshot: FederationPodSnapshot,
) -> (FixtureOrigin, EphemeralHttpServer) {
    let fixture = FixtureOrigin {
        snapshot: std::sync::Arc::new(std::sync::RwLock::new(snapshot)),
    };
    let well_known = fixture.clone();
    let manifest = fixture.clone();
    let events = fixture.clone();
    let app = Router::new()
        .route(
            "/.well-known/stumble-node",
            get(move || {
                let snapshot = well_known
                    .snapshot
                    .read()
                    .expect("fixture snapshot")
                    .clone();
                async move {
                    Json(WellKnownNode {
                        protocol: snapshot.node.supported_protocol_version.clone(),
                        node: snapshot.node,
                        endpoints: Default::default(),
                    })
                }
            }),
        )
        .route(
            "/federation/pods/subscriber-mcp/manifest",
            get(move || {
                let value = manifest
                    .snapshot
                    .read()
                    .expect("fixture snapshot")
                    .manifest
                    .clone();
                async move { Json(value) }
            }),
        )
        .route(
            "/federation/pods/subscriber-mcp/events",
            get(move || {
                let value = events
                    .snapshot
                    .read()
                    .expect("fixture snapshot")
                    .events
                    .clone();
                async move { Json(value) }
            }),
        );
    (fixture, EphemeralHttpServer::start(app).await)
}

fn publish_item(origin: &AgentTools, pod: Option<&Pod>, source_url: &str, key: &str) -> Pod {
    let curator = ScopedHarness::register(
        origin,
        &format!("origin curator {key}"),
        vec![HarnessCapability::PodCuration],
        None,
    );
    let curator = origin
        .authenticate_token(curator.token())
        .expect("authenticate curator")
        .expect("current curator token");
    let pod = match pod {
        Some(pod) => pod.clone(),
        None => {
            let private_pod = origin
                .create_pod(
                    &curator,
                    CreatePodRequest {
                        name: "Subscriber MCP Pod".into(),
                        slug: "subscriber-mcp".into(),
                        description: "Direct subscription test Pod".into(),
                        visibility: Visibility::Private,
                    },
                )
                .expect("create private Pod");
            let approver = ScopedHarness::register(
                origin,
                "origin publication approver",
                vec![HarnessCapability::Approval],
                None,
            );
            let approver = origin
                .authenticate_token(approver.token())
                .expect("authenticate approver")
                .expect("current approver token");
            let proposal = match origin
                .request_set_pod_visibility(
                    &curator,
                    private_pod.id,
                    Visibility::Public,
                    Utc::now(),
                )
                .expect("request public visibility")
            {
                PodVisibilityOutcome::PendingApproval(proposal) => proposal,
                PodVisibilityOutcome::Updated(_) => panic!("publication requires approval"),
            };
            origin
                .approve_pending_proposal(&approver, proposal.id, Utc::now())
                .expect("approve public visibility");
            origin
                .pod_by_slug("subscriber-mcp", None)
                .expect("public Pod")
        }
    };
    origin
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .expect("set manual curation");
    let submitter = ScopedHarness::register(
        origin,
        &format!("origin submitter {key}"),
        vec![HarnessCapability::CandidateSubmission],
        None,
    );
    let submitter = origin
        .authenticate_token(submitter.token())
        .expect("authenticate submitter")
        .expect("current submitter token");
    let submitted = origin
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                evidence: CandidateSubmissionEvidence {
                    source_url: source_url.into(),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(format!("Subscriber MCP item {key}")),
                        author: None,
                        published_at: None,
                    },
                    permitted_excerpt: None,
                    summary: Some("Signed federation fixture".into()),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec!["federation".into()],
                    provenance: CandidateProvenance {
                        discovered_at: Utc::now(),
                        discovery_method: "test_harness".into(),
                        referrer_url: None,
                    },
                    proposed_placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Subscriber MCP behavior".into(),
                        confidence: CandidateConfidence::new(1.0).expect("valid confidence"),
                    }],
                    task_context: None,
                    harness_idempotency_key: format!("harness-{key}"),
                    client_idempotency_key: format!("client-{key}"),
                },
            },
        )
        .expect("submit Candidate");
    origin
        .curate_candidate(&curator, submitted.candidate.id, Utc::now())
        .expect("curate Candidate");
    origin
        .review_candidate_placement(
            &curator,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .expect("accept Placement");
    pod
}

#[tokio::test]
async fn subscription_management_harness_subscribes_and_refreshes_from_a_real_origin() {
    let origin = PersistentNode::open("subscriber-http-origin");
    let home = PersistentNode::open("subscriber-http-home");
    let pod = publish_item(
        &origin.tools,
        None,
        "https://example.com/subscriber-mcp/first",
        "first",
    );
    let origin_http = EphemeralHttpServer::start_origin(origin.tools.clone()).await;
    let subscriber = home.harness(
        "subscriber MCP Harness",
        vec![HarnessCapability::SubscriptionManagement],
        None,
    );
    let denied = home.harness(
        "feed-only direct caller",
        vec![HarnessCapability::FeedRead],
        None,
    );
    let denied_context = home
        .tools
        .authenticate_token(denied.token())
        .expect("authenticate denied caller")
        .expect("current denied caller token");
    let denied_router = McpToolRouter::new(home.tools.clone(), denied_context);
    let public_pod_url = format!("{}/federation/pods/subscriber-mcp", origin_http.base_url);
    let denied_subscription = denied_router
        .call_async(McpToolCall {
            tool: "subscribe_public_pod".into(),
            arguments: json!({"public_pod_url": public_pod_url}),
        })
        .await
        .expect_err("core denies direct subscription call");
    assert!(denied_subscription
        .to_string()
        .contains("harness grant lacks subscription_management"));
    let mcp = home.mcp(&subscriber);

    let names = mcp.list_tool_names(1).await;
    assert!(names.iter().any(|name| name == "subscribe_public_pod"));
    assert!(names.iter().any(|name| name == "synchronize_subscription"));

    let subscribed = mcp
        .call_tool(
            2,
            "subscribe_public_pod",
            json!({"public_pod_url": public_pod_url}),
        )
        .await;
    assert!(!subscribed.is_error());
    assert_eq!(subscribed.value()["imported_events"], 3);
    let subscription_id = subscribed.value()["subscription"]["id"]
        .as_str()
        .expect("Subscription identity");
    let first_cursor = subscribed.value()["subscription"]["last_event_hash"]
        .as_str()
        .expect("verified cursor");

    publish_item(
        &origin.tools,
        Some(&pod),
        "https://example.com/subscriber-mcp/second",
        "second",
    );
    let denied_refresh = denied_router
        .call_async(McpToolCall {
            tool: "synchronize_subscription".into(),
            arguments: json!({"subscription_id": subscription_id}),
        })
        .await
        .expect_err("core denies direct synchronization call");
    assert!(denied_refresh
        .to_string()
        .contains("harness grant lacks subscription_management"));
    let refreshed = mcp
        .call_tool(
            3,
            "synchronize_subscription",
            json!({"subscription_id": subscription_id}),
        )
        .await;
    assert!(!refreshed.is_error());
    assert_eq!(refreshed.value()["imported_events"], 1);
    assert_ne!(
        refreshed.value()["subscription"]["last_event_hash"],
        first_cursor
    );
}

#[tokio::test]
async fn stdio_dispatches_direct_subscription_and_incremental_synchronization() {
    let origin = PersistentNode::open("subscriber-stdio-origin");
    let home = PersistentNode::open("subscriber-stdio-home");
    let pod = publish_item(
        &origin.tools,
        None,
        "https://example.com/subscriber-mcp/stdio-first",
        "stdio-first",
    );
    let origin_http = EphemeralHttpServer::start_origin(origin.tools.clone()).await;
    let subscriber = home.harness(
        "stdio subscriber",
        vec![HarnessCapability::SubscriptionManagement],
        None,
    );
    let context = home
        .tools
        .authenticate_token(subscriber.token())
        .expect("authenticate stdio subscriber")
        .expect("current stdio subscriber token");

    let subscribe_request = json!({
        "jsonrpc": "2.0",
        "id": "subscribe",
        "method": "tools/call",
        "params": {
            "name": "subscribe_public_pod",
            "arguments": {
                "public_pod_url": format!("{}/federation/pods/subscriber-mcp", origin_http.base_url)
            }
        }
    });
    let mut subscribe_output = Vec::new();
    let stdio_home = home.tools.clone();
    let stdio_context = context.clone();
    serve_stdio(
        move || Ok((stdio_home.clone(), stdio_context.clone())),
        std::io::Cursor::new(format!("{subscribe_request}\n")),
        &mut subscribe_output,
    )
    .await
    .expect("serve stdio subscription call");
    let subscribed = McpToolResult::from_json(
        serde_json::from_slice(&subscribe_output).expect("stdio JSON response"),
    );
    assert!(!subscribed.is_error());
    let subscription_id = subscribed.value()["subscription"]["id"]
        .as_str()
        .expect("Subscription identity");

    publish_item(
        &origin.tools,
        Some(&pod),
        "https://example.com/subscriber-mcp/stdio-second",
        "stdio-second",
    );
    let synchronize_request = json!({
        "jsonrpc": "2.0",
        "id": "synchronize",
        "method": "tools/call",
        "params": {
            "name": "synchronize_subscription",
            "arguments": {"subscription_id": subscription_id}
        }
    });
    let mut synchronize_output = Vec::new();
    let stdio_home = home.tools.clone();
    let stdio_context = context.clone();
    serve_stdio(
        move || Ok((stdio_home.clone(), stdio_context.clone())),
        std::io::Cursor::new(format!("{synchronize_request}\n")),
        &mut synchronize_output,
    )
    .await
    .expect("serve stdio synchronization call");
    let synchronized = McpToolResult::from_json(
        serde_json::from_slice(&synchronize_output).expect("stdio JSON response"),
    );
    assert!(!synchronized.is_error());
    assert_eq!(synchronized.value()["imported_events"], 1);
}

#[tokio::test]
async fn subscription_tools_are_hidden_without_subscription_management() {
    let home = AgentTools::new(seed_store());
    let feed_reader = ScopedHarness::register(
        &home,
        "feed-only subscriber",
        vec![HarnessCapability::FeedRead],
        None,
    );

    let names = McpClient::new(
        stumble_mcp::streamable_http_router(home),
        feed_reader.token(),
    )
    .list_tool_names(1)
    .await;

    assert!(!names.iter().any(|name| name == "subscribe_public_pod"));
    assert!(!names.iter().any(|name| name == "synchronize_subscription"));
}

#[tokio::test]
async fn direct_subscription_preserves_the_canonical_public_pod_url_error() {
    let home = AgentTools::new(seed_store());
    let subscriber = ScopedHarness::register(
        &home,
        "canonical URL subscriber",
        vec![HarnessCapability::SubscriptionManagement],
        None,
    );

    let response = McpClient::new(
        stumble_mcp::streamable_http_router(home),
        subscriber.token(),
    )
    .call_tool(
        1,
        "subscribe_public_pod",
        json!({"public_pod_url": "https://example.com/not-a-public-pod-address"}),
    )
    .await;

    assert!(response.is_error());
    assert_eq!(response.error_text(), "invalid public Pod address");
}

#[tokio::test]
async fn subscription_mcp_preserves_origin_identity_signature_and_chain_errors() {
    let origin = AgentTools::new(seed_store());
    let pod = publish_item(
        &origin,
        None,
        "https://example.com/subscriber-mcp/error-fixture",
        "error-fixture",
    );
    let origin_context = origin.default_auth_context().expect("Origin context");
    let valid_snapshot = origin
        .federation_pod_snapshot(&origin_context, &pod.slug, None)
        .expect("valid signed snapshot");

    let mut invalid_signature = valid_snapshot.clone();
    invalid_signature
        .events
        .last_mut()
        .expect("signed event")
        .signature = "invalid-signature".into();
    let (_, signature_server) = start_fixture_origin(invalid_signature).await;
    assert_subscription_error(
        &signature_server.base_url,
        "invalid signature",
        "signature failure subscriber",
    )
    .await;

    let mut broken_chain = valid_snapshot.clone();
    broken_chain.events.remove(1);
    let (_, chain_server) = start_fixture_origin(broken_chain).await;
    assert_subscription_error(
        &chain_server.base_url,
        "validation failed: signed Pod Event chain is discontinuous",
        "chain failure subscriber",
    )
    .await;

    let (fixture, identity_server) = start_fixture_origin(valid_snapshot).await;
    let home = AgentTools::new(seed_store());
    let subscriber = ScopedHarness::register(
        &home,
        "identity change subscriber",
        vec![HarnessCapability::SubscriptionManagement],
        None,
    );
    let mcp = McpClient::new(
        stumble_mcp::streamable_http_router(home),
        subscriber.token(),
    );
    let subscribed = mcp
        .call_tool(
            1,
            "subscribe_public_pod",
            json!({
                "public_pod_url": format!("{}/federation/pods/subscriber-mcp", identity_server.base_url)
            }),
        )
        .await;
    let subscription_id = subscribed.value()["subscription"]["id"]
        .as_str()
        .expect("Subscription identity");
    let replacement_origin = AgentTools::new(seed_store());
    fixture.snapshot.write().expect("fixture snapshot").node = replacement_origin
        .node_info(
            &replacement_origin
                .default_auth_context()
                .expect("replacement Origin context"),
        )
        .expect("replacement Origin identity");

    let refreshed = mcp
        .call_tool(
            2,
            "synchronize_subscription",
            json!({"subscription_id": subscription_id}),
        )
        .await;
    assert!(refreshed.is_error());
    assert_eq!(
        refreshed.error_text(),
        "validation failed: synchronization artifacts do not match the Subscription"
    );
}

async fn assert_subscription_error(base_url: &str, expected: &str, label: &str) {
    let home = AgentTools::new(seed_store());
    let subscriber = ScopedHarness::register(
        &home,
        label,
        vec![HarnessCapability::SubscriptionManagement],
        None,
    );
    let response = McpClient::new(
        stumble_mcp::streamable_http_router(home),
        subscriber.token(),
    )
    .call_tool(
        1,
        "subscribe_public_pod",
        json!({
            "public_pod_url": format!("{base_url}/federation/pods/subscriber-mcp")
        }),
    )
    .await;
    assert!(response.is_error());
    assert_eq!(response.error_text(), expected);
}
