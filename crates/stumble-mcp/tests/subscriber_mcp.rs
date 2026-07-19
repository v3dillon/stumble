use axum::{body::Body, http::Request, routing::get, Json, Router};
use chrono::Utc;
use serde_json::{json, Value};
use stumble_api::router_with_base_url;
use stumble_core::*;
use stumble_mcp::{serve_stdio, streamable_http_router, McpToolCall, McpToolRouter};
use tower::ServiceExt;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-subscriber-mcp-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).expect("create test Home Node directory");
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
struct FixtureOrigin {
    snapshot: std::sync::Arc<std::sync::RwLock<FederationPodSnapshot>>,
}

async fn start_fixture_origin(
    snapshot: FederationPodSnapshot,
) -> (
    String,
    FixtureOrigin,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
) {
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture Origin");
    let base_url = format!("http://{}", listener.local_addr().expect("fixture address"));
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    (base_url, fixture, server)
}

fn harness(
    tools: &AgentTools,
    label: &str,
    capabilities: Vec<HarnessCapability>,
) -> RegisterAgentHarnessResponse {
    tools
        .register_agent_harness(
            &tools.default_auth_context().expect("owner context"),
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind: AgentHarnessKind::Interactive,
                capabilities,
                pod_ids: None,
            },
        )
        .expect("register Harness")
}

fn publish_item(origin: &AgentTools, pod: Option<&Pod>, source_url: &str, key: &str) -> Pod {
    let curator = harness(
        origin,
        &format!("origin curator {key}"),
        vec![HarnessCapability::PodCuration],
    );
    let curator = origin
        .authenticate_token(curator.token.expose())
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
            let approver = harness(
                origin,
                "origin publication approver",
                vec![HarnessCapability::Approval],
            );
            let approver = origin
                .authenticate_token(approver.token.expose())
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
    let submitter = harness(
        origin,
        &format!("origin submitter {key}"),
        vec![HarnessCapability::CandidateSubmission],
    );
    let submitter = origin
        .authenticate_token(submitter.token.expose())
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
    let origin_dir = TestDataDir::new("http-origin");
    let home_dir = TestDataDir::new("http-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).expect("open Origin Node");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).expect("open Home Node");
    let pod = publish_item(
        &origin,
        None,
        "https://example.com/subscriber-mcp/first",
        "first",
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind Origin listener");
    let origin_base_url = format!("http://{}", listener.local_addr().expect("Origin address"));
    let origin_app = router_with_base_url(origin.clone(), &origin_base_url);
    let origin_server = tokio::spawn(async move { axum::serve(listener, origin_app).await });
    let subscriber = harness(
        &home,
        "subscriber MCP Harness",
        vec![HarnessCapability::SubscriptionManagement],
    );
    let denied = harness(
        &home,
        "feed-only direct caller",
        vec![HarnessCapability::FeedRead],
    );
    let denied_context = home
        .authenticate_token(denied.token.expose())
        .expect("authenticate denied caller")
        .expect("current denied caller token");
    let denied_router = McpToolRouter::new(home.clone(), denied_context);
    let public_pod_url = format!("{origin_base_url}/federation/pods/subscriber-mcp");
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
    let mcp = streamable_http_router(home);

    let catalog = call_mcp(
        mcp.clone(),
        subscriber.token.expose(),
        "tools/list",
        json!({}),
    )
    .await;
    let names = catalog["result"]["tools"]
        .as_array()
        .expect("tool descriptors")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"subscribe_public_pod"));
    assert!(names.contains(&"synchronize_subscription"));

    let subscribed = call_tool(
        mcp.clone(),
        subscriber.token.expose(),
        "subscribe_public_pod",
        json!({"public_pod_url": public_pod_url}),
    )
    .await;
    assert_eq!(subscribed["result"]["isError"], false);
    assert_eq!(
        subscribed["result"]["structuredContent"]["value"]["imported_events"],
        3
    );
    let subscription_id = subscribed["result"]["structuredContent"]["value"]["subscription"]["id"]
        .as_str()
        .expect("Subscription identity");
    let first_cursor = subscribed["result"]["structuredContent"]["value"]["subscription"]
        ["last_event_hash"]
        .as_str()
        .expect("verified cursor");

    publish_item(
        &origin,
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
    let refreshed = call_tool(
        mcp,
        subscriber.token.expose(),
        "synchronize_subscription",
        json!({"subscription_id": subscription_id}),
    )
    .await;
    assert_eq!(refreshed["result"]["isError"], false);
    assert_eq!(
        refreshed["result"]["structuredContent"]["value"]["imported_events"],
        1
    );
    assert_ne!(
        refreshed["result"]["structuredContent"]["value"]["subscription"]["last_event_hash"],
        first_cursor
    );

    origin_server.abort();
}

#[tokio::test]
async fn stdio_dispatches_direct_subscription_and_incremental_synchronization() {
    let origin_dir = TestDataDir::new("stdio-origin");
    let home_dir = TestDataDir::new("stdio-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).expect("open Origin Node");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).expect("open Home Node");
    let pod = publish_item(
        &origin,
        None,
        "https://example.com/subscriber-mcp/stdio-first",
        "stdio-first",
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind Origin listener");
    let origin_base_url = format!("http://{}", listener.local_addr().expect("Origin address"));
    let origin_app = router_with_base_url(origin.clone(), &origin_base_url);
    let origin_server = tokio::spawn(async move { axum::serve(listener, origin_app).await });
    let subscriber = harness(
        &home,
        "stdio subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );
    let context = home
        .authenticate_token(subscriber.token.expose())
        .expect("authenticate stdio subscriber")
        .expect("current stdio subscriber token");

    let subscribe_request = json!({
        "jsonrpc": "2.0",
        "id": "subscribe",
        "method": "tools/call",
        "params": {
            "name": "subscribe_public_pod",
            "arguments": {
                "public_pod_url": format!("{origin_base_url}/federation/pods/subscriber-mcp")
            }
        }
    });
    let mut subscribe_output = Vec::new();
    serve_stdio(
        || Ok((home.clone(), context.clone())),
        std::io::Cursor::new(format!("{subscribe_request}\n")),
        &mut subscribe_output,
    )
    .await
    .expect("serve stdio subscription call");
    let subscribed: Value = serde_json::from_slice(&subscribe_output).expect("stdio JSON response");
    assert_eq!(subscribed["result"]["isError"], false);
    let subscription_id = subscribed["result"]["structuredContent"]["value"]["subscription"]["id"]
        .as_str()
        .expect("Subscription identity");

    publish_item(
        &origin,
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
    serve_stdio(
        || Ok((home.clone(), context.clone())),
        std::io::Cursor::new(format!("{synchronize_request}\n")),
        &mut synchronize_output,
    )
    .await
    .expect("serve stdio synchronization call");
    let synchronized: Value =
        serde_json::from_slice(&synchronize_output).expect("stdio JSON response");
    assert_eq!(synchronized["result"]["isError"], false);
    assert_eq!(
        synchronized["result"]["structuredContent"]["value"]["imported_events"],
        1
    );
    origin_server.abort();
}

#[tokio::test]
async fn subscription_tools_are_hidden_without_subscription_management() {
    let home = AgentTools::new(seed_store());
    let feed_reader = harness(
        &home,
        "feed-only subscriber",
        vec![HarnessCapability::FeedRead],
    );

    let catalog = call_mcp(
        streamable_http_router(home),
        feed_reader.token.expose(),
        "tools/list",
        json!({}),
    )
    .await;
    let names = catalog["result"]["tools"]
        .as_array()
        .expect("tool descriptors")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();

    assert!(!names.contains(&"subscribe_public_pod"));
    assert!(!names.contains(&"synchronize_subscription"));
}

#[tokio::test]
async fn direct_subscription_preserves_the_canonical_public_pod_url_error() {
    let home = AgentTools::new(seed_store());
    let subscriber = harness(
        &home,
        "canonical URL subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );

    let response = call_tool(
        streamable_http_router(home),
        subscriber.token.expose(),
        "subscribe_public_pod",
        json!({"public_pod_url": "https://example.com/not-a-public-pod-address"}),
    )
    .await;

    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["content"][0]["text"],
        "invalid public Pod address"
    );
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
    let (signature_base_url, _, signature_server) = start_fixture_origin(invalid_signature).await;
    assert_subscription_error(
        &signature_base_url,
        "invalid signature",
        "signature failure subscriber",
    )
    .await;
    signature_server.abort();

    let mut broken_chain = valid_snapshot.clone();
    broken_chain.events.remove(1);
    let (chain_base_url, _, chain_server) = start_fixture_origin(broken_chain).await;
    assert_subscription_error(
        &chain_base_url,
        "validation failed: signed Pod Event chain is discontinuous",
        "chain failure subscriber",
    )
    .await;
    chain_server.abort();

    let (identity_base_url, fixture, identity_server) = start_fixture_origin(valid_snapshot).await;
    let home = AgentTools::new(seed_store());
    let subscriber = harness(
        &home,
        "identity change subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );
    let mcp = streamable_http_router(home);
    let subscribed = call_tool(
        mcp.clone(),
        subscriber.token.expose(),
        "subscribe_public_pod",
        json!({
            "public_pod_url": format!("{identity_base_url}/federation/pods/subscriber-mcp")
        }),
    )
    .await;
    let subscription_id = subscribed["result"]["structuredContent"]["value"]["subscription"]["id"]
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

    let refreshed = call_tool(
        mcp,
        subscriber.token.expose(),
        "synchronize_subscription",
        json!({"subscription_id": subscription_id}),
    )
    .await;
    assert_eq!(refreshed["result"]["isError"], true);
    assert_eq!(
        refreshed["result"]["content"][0]["text"],
        "validation failed: synchronization artifacts do not match the Subscription"
    );
    identity_server.abort();
}

async fn assert_subscription_error(base_url: &str, expected: &str, label: &str) {
    let home = AgentTools::new(seed_store());
    let subscriber = harness(
        &home,
        label,
        vec![HarnessCapability::SubscriptionManagement],
    );
    let response = call_tool(
        streamable_http_router(home),
        subscriber.token.expose(),
        "subscribe_public_pod",
        json!({
            "public_pod_url": format!("{base_url}/federation/pods/subscriber-mcp")
        }),
    )
    .await;
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(response["result"]["content"][0]["text"], expected);
}

async fn call_tool(app: axum::Router, token: &str, name: &str, arguments: Value) -> Value {
    call_mcp(
        app,
        token,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    )
    .await
}

async fn call_mcp(app: axum::Router, token: &str, method: &str, params: Value) -> Value {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .header("mcp-protocol-version", "2025-06-18")
                .body(Body::from(
                    json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
                        .to_string(),
                ))
                .expect("MCP request"),
        )
        .await
        .expect("MCP response");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("MCP response body");
    serde_json::from_slice(&body).expect("MCP JSON response")
}
