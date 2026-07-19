use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use stumble_core::{
    seed_store, AgentHarnessKind, AgentTools, CreatePrivatePodWithPackageRequest,
    HarnessCapability, PodPackageContents, RegisterAgentHarnessRequest,
};
use stumble_mcp::streamable_http_router;
use tower::ServiceExt;

#[tokio::test]
async fn authenticated_client_negotiates_the_supported_protocol() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().expect("owner context");
    let token = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "ChatGPT MCP test".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![
                    HarnessCapability::FeedRead,
                    HarnessCapability::CandidateSubmission,
                    HarnessCapability::DiscoveryTasks,
                ],
                pod_ids: None,
            },
        )
        .expect("register scoped test harness");
    let app = streamable_http_router(tools);

    let incomplete = app
        .clone()
        .oneshot(mcp_request(
            token.token.expose(),
            json!({
                "jsonrpc": "2.0",
                "id": "incomplete-init",
                "method": "initialize",
                "params": {"protocolVersion": "2025-06-18"}
            }),
        ))
        .await
        .expect("incomplete initialize response");
    let incomplete = response_json(incomplete).await;
    assert_eq!(incomplete["error"]["code"], -32602);

    let initialize = app
        .oneshot(mcp_request(
            token.token.expose(),
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "integration-test", "version": "1"}
                }
            }),
        ))
        .await
        .expect("initialize response");
    assert_eq!(initialize.status(), StatusCode::OK);
    let initialized = response_json(initialize).await;
    assert_eq!(initialized["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(initialized["result"]["serverInfo"]["name"], "stumble");
}

#[tokio::test]
async fn tool_catalog_is_annotated_and_scoped_to_the_harness_grant() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().expect("owner context");
    let token = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "ChatGPT feed-only catalog".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::FeedRead],
                pod_ids: None,
            },
        )
        .expect("register feed-only harness");
    let app = streamable_http_router(tools);

    let listed = app
        .oneshot(mcp_request(
            token.token.expose(),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
        ))
        .await
        .expect("tools/list response");
    assert_eq!(listed.status(), StatusCode::OK);
    let listed = response_json(listed).await;
    let tools = listed["result"]["tools"]
        .as_array()
        .expect("tool descriptor array");
    let feed = tools
        .iter()
        .find(|tool| tool["name"] == "get_feed_batch")
        .expect("get_feed_batch descriptor");
    assert_eq!(feed["annotations"]["readOnlyHint"], false);
    assert_eq!(feed["annotations"]["destructiveHint"], false);
    assert!(
        feed["inputSchema"]["properties"]["feed_mix"]["properties"]["exploration_percent"]
            .is_object()
    );
    assert!(tools.iter().any(|tool| tool["name"] == "list_pods"));
    assert!(!tools.iter().any(|tool| tool["name"] == "submit_candidate"));
    assert!(!tools
        .iter()
        .any(|tool| tool["name"] == "list_ready_discovery_tasks"));
    assert!(!tools
        .iter()
        .any(|tool| tool["name"] == "record_feed_feedback"));
}

#[tokio::test]
async fn origin_curation_tools_are_advertised_only_for_their_harness_capability() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().expect("owner context");
    let grants = [
        (
            HarnessCapability::PodCuration,
            vec![
                "create_pod",
                "route_candidate",
                "review_candidate_placement",
            ],
        ),
        (
            HarnessCapability::Approval,
            vec![
                "get_pending_proposal",
                "approve_pending_proposal",
                "reject_pending_proposal",
            ],
        ),
        (HarnessCapability::FeedRead, vec!["list_pod_content"]),
    ];

    for (capability, expected_names) in grants {
        let token = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: format!("{capability} catalog"),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![capability],
                    pod_ids: None,
                },
            )
            .expect("register scoped harness");
        let response = streamable_http_router(tools.clone())
            .oneshot(mcp_request(
                token.token.expose(),
                json!({"jsonrpc": "2.0", "id": 11, "method": "tools/list", "params": {}}),
            ))
            .await
            .expect("tools/list response");
        let response = response_json(response).await;
        let names = response["result"]["tools"]
            .as_array()
            .expect("tool descriptor array")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();

        for expected_name in &expected_names {
            assert!(
                names.contains(expected_name),
                "{capability} catalog should contain {expected_name}: {names:?}"
            );
        }
        for forbidden_name in [
            "create_pod",
            "route_candidate",
            "review_candidate_placement",
            "get_pending_proposal",
            "approve_pending_proposal",
            "reject_pending_proposal",
            "list_pod_content",
        ] {
            let belongs_to_capability = expected_names.contains(&forbidden_name);
            assert_eq!(
                names.contains(&forbidden_name),
                belongs_to_capability,
                "unexpected {forbidden_name} visibility for {capability}: {names:?}"
            );
        }
    }
}

#[tokio::test]
async fn tool_calls_return_structured_content() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().expect("owner context");
    let token = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "ChatGPT feed reader".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::FeedRead],
                pod_ids: None,
            },
        )
        .expect("register feed harness");
    let app = streamable_http_router(tools);

    let called = app
        .oneshot(mcp_request(
            token.token.expose(),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {"name": "list_pods", "arguments": {}}
            }),
        ))
        .await
        .expect("tools/call response");
    assert_eq!(called.status(), StatusCode::OK);
    let called = response_json(called).await;
    assert_eq!(called["result"]["structuredContent"], json!({"value": []}));
    assert_eq!(called["result"]["content"][0]["text"], r#"{"value":[]}"#);
    assert_eq!(called["result"]["isError"], false);
}

#[tokio::test]
async fn unknown_tools_and_invalid_arguments_are_jsonrpc_protocol_errors() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().expect("owner context");
    let token = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "ChatGPT protocol errors".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::FeedRead],
                pod_ids: None,
            },
        )
        .expect("register feed harness");
    let app = streamable_http_router(tools);

    let unknown = app
        .clone()
        .oneshot(mcp_request(
            token.token.expose(),
            json!({
                "jsonrpc": "2.0",
                "id": 8,
                "method": "tools/call",
                "params": {"name": "not_a_stumble_tool", "arguments": {}}
            }),
        ))
        .await
        .expect("unknown-tool response");
    let unknown = response_json(unknown).await;
    assert_eq!(unknown["error"]["code"], -32602);
    assert!(unknown.get("result").is_none());

    let invalid = app
        .clone()
        .oneshot(mcp_request(
            token.token.expose(),
            json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "tools/call",
                "params": {"name": "list_pods", "arguments": []}
            }),
        ))
        .await
        .expect("invalid-arguments response");
    let invalid = response_json(invalid).await;
    assert_eq!(invalid["error"]["code"], -32602);
    assert!(invalid.get("result").is_none());

    let invalid_constraint = app
        .oneshot(mcp_request(
            token.token.expose(),
            json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "tools/call",
                "params": {"name": "get_feed_batch", "arguments": {"size": 0}}
            }),
        ))
        .await
        .expect("schema-constraint response");
    let invalid_constraint = response_json(invalid_constraint).await;
    assert_eq!(invalid_constraint["error"]["code"], -32602);
    assert!(invalid_constraint.get("result").is_none());
}

#[tokio::test]
async fn invalid_jsonrpc_ids_and_params_are_rejected_at_the_http_boundary() {
    let app = streamable_http_router(AgentTools::new(seed_store()));
    let invalid_requests = [
        json!({"jsonrpc": "2.0", "id": null, "method": "ping", "params": {}}),
        json!({"jsonrpc": "2.0", "id": 10, "method": "ping", "params": []}),
    ];

    for body in invalid_requests {
        let response = app
            .clone()
            .oneshot(mcp_request("not-dispatched", body))
            .await
            .expect("invalid JSON-RPC boundary response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response = response_json(response).await;
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], Value::Null);
        assert_eq!(response["error"]["code"], -32600);
    }
}

#[tokio::test]
async fn malformed_json_returns_a_jsonrpc_parse_error() {
    let app = streamable_http_router(AgentTools::new(seed_store()));
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("authorization", "Bearer not-dispatched")
        .header("content-type", "application/json")
        .body(Body::from("{"))
        .expect("malformed JSON request");

    let response = app.oneshot(request).await.expect("JSON parse response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = response_json(response).await;
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], Value::Null);
    assert_eq!(response["error"]["code"], -32700);
}

#[tokio::test]
async fn invalid_harness_token_is_rejected_before_dispatch() {
    let app = streamable_http_router(AgentTools::new(seed_store()));

    let response = app
        .oneshot(mcp_request(
            "not-a-real-token",
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {}}),
        ))
        .await
        .expect("invalid-token response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn untrusted_origin_is_rejected_before_dispatch() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().expect("owner context");
    let token = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "Origin defense test".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::FeedRead],
                pod_ids: None,
            },
        )
        .expect("register test harness");
    let app = streamable_http_router(tools);
    let mut request = mcp_request(
        token.token.expose(),
        json!({"jsonrpc": "2.0", "id": 6, "method": "tools/list", "params": {}}),
    );
    request.headers_mut().insert(
        "origin",
        "https://attacker.example"
            .parse()
            .expect("valid Origin header"),
    );

    let response = app
        .oneshot(request)
        .await
        .expect("untrusted-origin response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn unsupported_protocol_version_is_rejected_before_dispatch() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().expect("owner context");
    let token = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "Protocol version test".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::FeedRead],
                pod_ids: None,
            },
        )
        .expect("register test harness");
    let app = streamable_http_router(tools);
    let mut request = mcp_request(
        token.token.expose(),
        json!({"jsonrpc": "2.0", "id": 7, "method": "tools/list", "params": {}}),
    );
    request.headers_mut().insert(
        "mcp-protocol-version",
        "2099-01-01".parse().expect("valid protocol header"),
    );

    let response = app
        .oneshot(request)
        .await
        .expect("unsupported-version response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn chatgpt_can_submit_a_provenance_bearing_link_to_an_authorized_pod() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().expect("owner context");
    let token = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "ChatGPT link intake".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![
                    HarnessCapability::PodCuration,
                    HarnessCapability::PackageManagement,
                    HarnessCapability::CandidateSubmission,
                ],
                pod_ids: None,
            },
        )
        .expect("register link harness");
    let harness = tools
        .authenticate_token(token.token.expose())
        .expect("authenticate link harness")
        .expect("current link harness token");
    let created = tools
        .create_private_pod_with_package(
            &harness,
            CreatePrivatePodWithPackageRequest {
                name: "Interesting systems".into(),
                slug: "interesting-systems".into(),
                description: "Links worth revisiting".into(),
                package: complete_package(),
            },
        )
        .expect("create private Pod");
    let app = streamable_http_router(tools);

    let response = app
        .oneshot(mcp_request(
            token.token.expose(),
            json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {
                    "name": "submit_candidate",
                    "arguments": {
                        "source_url": "https://example.com/field-note?utm_source=chatgpt",
                        "source_metadata": {
                            "title": "A useful field note"
                        },
                        "summary": "A concrete systems lesson.",
                        "content_type": "article",
                        "tags": ["systems"],
                        "provenance": {
                            "discovered_at": "2026-07-17T22:00:00Z",
                            "discovery_method": "chatgpt_conversation"
                        },
                        "proposed_placements": [{
                            "pod_id": created.pod.id,
                            "reason": "The user explicitly sent this link for the Pod.",
                            "confidence": 1.0
                        }],
                        "harness_idempotency_key": "chatgpt-link-1",
                        "client_idempotency_key": "conversation-message-1"
                    }
                }
            }),
        ))
        .await
        .expect("submit_candidate MCP response");

    assert_eq!(response.status(), StatusCode::OK);
    let response = response_json(response).await;
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(
        response["result"]["structuredContent"]["value"]["candidate"]["canonical_url"],
        "https://example.com/field-note"
    );
    assert_eq!(
        response["result"]["structuredContent"]["value"]["submission"]["provenance"]
            ["discovery_method"],
        "chatgpt_conversation"
    );
}

#[tokio::test]
async fn harnesses_can_curate_an_origin_pod_without_bypassing_scope_or_approval() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().expect("owner context");
    let curator = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "interactive Origin curator".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![
                    HarnessCapability::PodCuration,
                    HarnessCapability::CandidateSubmission,
                ],
                pod_ids: None,
            },
        )
        .expect("register curator");
    let grant_admin = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "interactive grant administrator".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Administration],
                pod_ids: None,
            },
        )
        .expect("register grant administrator");
    let approver = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "independent public exposure approver".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Approval],
                pod_ids: None,
            },
        )
        .expect("register independent approver");
    let now = chrono::Utc::now();
    let grant_admin_context = tools
        .authenticate_token(grant_admin.token.expose())
        .expect("authenticate grant administrator")
        .expect("grant administrator context");
    let approver_context = tools
        .authenticate_token(approver.token.expose())
        .expect("authenticate independent approver")
        .expect("independent approver context");
    let expansion = tools
        .request_harness_grant_expansion(
            &grant_admin_context,
            curator.harness.id,
            vec![
                HarnessCapability::PodCuration,
                HarnessCapability::CandidateSubmission,
                HarnessCapability::Approval,
            ],
            None,
            now,
        )
        .expect("request curator approval capability");
    tools
        .approve_pending_proposal(&approver_context, expansion.id, now)
        .expect("independently approve curator grant expansion");
    let app = streamable_http_router(tools.clone());

    let inbox = call_mcp_tool(
        app.clone(),
        curator.token.expose(),
        20,
        "create_pod",
        json!({
            "name": "Federation Inbox",
            "slug": "federation-inbox",
            "description": "Private discovery intake",
            "visibility": "private"
        }),
    )
    .await;
    assert_eq!(
        inbox["result"]["structuredContent"]["value"]["status"],
        "created"
    );
    let inbox_id = inbox["result"]["structuredContent"]["value"]["result"]["id"]
        .as_str()
        .expect("private Inbox Pod id");

    let proposed = call_mcp_tool(
        app.clone(),
        curator.token.expose(),
        21,
        "create_pod",
        json!({
            "name": "Federated Finds",
            "slug": "federated-finds",
            "description": "Accepted public discoveries",
            "visibility": "public"
        }),
    )
    .await;
    assert_eq!(
        proposed["result"]["structuredContent"]["value"]["status"],
        "pending_approval"
    );
    let proposal_id = proposed["result"]["structuredContent"]["value"]["result"]["id"]
        .as_str()
        .expect("Pending Proposal id");

    let self_approval = call_mcp_tool(
        app.clone(),
        curator.token.expose(),
        22,
        "approve_pending_proposal",
        json!({"proposal_id": proposal_id}),
    )
    .await;
    assert_eq!(self_approval["result"]["isError"], true);
    assert!(self_approval["result"]["content"][0]["text"]
        .as_str()
        .expect("core authorization error")
        .contains("cannot approve its own Pending Proposal"));

    let inspected = call_mcp_tool(
        app.clone(),
        approver.token.expose(),
        23,
        "get_pending_proposal",
        json!({"proposal_id": proposal_id}),
    )
    .await;
    assert_eq!(
        inspected["result"]["structuredContent"]["value"]["status"],
        "pending"
    );
    let approved = call_mcp_tool(
        app.clone(),
        approver.token.expose(),
        24,
        "approve_pending_proposal",
        json!({"proposal_id": proposal_id}),
    )
    .await;
    assert_eq!(
        approved["result"]["structuredContent"]["value"]["status"],
        "accepted"
    );

    let listed = call_mcp_tool(
        app.clone(),
        curator.token.expose(),
        25,
        "list_pods",
        json!({}),
    )
    .await;
    let public_pod_id = listed["result"]["structuredContent"]["value"]
        .as_array()
        .expect("visible Pods")
        .iter()
        .find(|pod| pod["slug"] == "federated-finds")
        .and_then(|pod| pod["id"].as_str())
        .expect("approved public Pod id");

    let submitted = call_mcp_tool(
        app.clone(),
        curator.token.expose(),
        26,
        "submit_candidate",
        json!({
            "source_url": "https://example.com/origin-discovery?utm_source=harness",
            "source_metadata": {"title": "Origin discovery"},
            "summary": "One private Candidate routed through authorized curation.",
            "content_type": "article",
            "tags": ["federation"],
            "provenance": {
                "discovered_at": "2026-07-18T12:00:00Z",
                "discovery_method": "interactive_browser"
            },
            "proposed_placements": [{
                "pod_id": inbox_id,
                "reason": "Initial private discovery intake.",
                "confidence": 0.9
            }],
            "harness_idempotency_key": "origin-curation-1",
            "client_idempotency_key": "origin-message-1"
        }),
    )
    .await;
    let candidate_id = submitted["result"]["structuredContent"]["value"]["candidate"]["id"]
        .as_str()
        .expect("Candidate id");

    let scoped_curator = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "Inbox-only curator".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::PodCuration],
                pod_ids: Some(vec![inbox_id.parse().expect("Inbox Pod id")]),
            },
        )
        .expect("register scoped curator");
    let denied_route = call_mcp_tool(
        app.clone(),
        scoped_curator.token.expose(),
        27,
        "route_candidate",
        json!({
            "candidate_id": candidate_id,
            "pod_id": public_pod_id,
            "reason": "This Harness lacks scope for the public Pod.",
            "confidence": 0.95
        }),
    )
    .await;
    assert_eq!(denied_route["result"]["isError"], true);

    let routed = call_mcp_tool(
        app.clone(),
        curator.token.expose(),
        28,
        "route_candidate",
        json!({
            "candidate_id": candidate_id,
            "pod_id": public_pod_id,
            "reason": "The discovery matches the public Pod's scope.",
            "confidence": 0.95
        }),
    )
    .await;
    assert_eq!(
        routed["result"]["structuredContent"]["value"]["status"],
        "pending"
    );
    let denied_review = call_mcp_tool(
        app.clone(),
        scoped_curator.token.expose(),
        29,
        "review_candidate_placement",
        json!({
            "candidate_id": candidate_id,
            "pod_id": public_pod_id,
            "decision": "accept"
        }),
    )
    .await;
    assert_eq!(denied_review["result"]["isError"], true);
    let reviewed = call_mcp_tool(
        app.clone(),
        curator.token.expose(),
        30,
        "review_candidate_placement",
        json!({
            "candidate_id": candidate_id,
            "pod_id": public_pod_id,
            "decision": "accept",
            "note": "Reviewed through the interactive curation workflow."
        }),
    )
    .await;
    assert_eq!(
        reviewed["result"]["structuredContent"]["value"]["status"],
        "accepted"
    );

    let reader = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "accepted content reader".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::FeedRead],
                pod_ids: Some(vec![public_pod_id.parse().expect("public Pod id")]),
            },
        )
        .expect("register scoped reader");
    let content = call_mcp_tool(
        app,
        reader.token.expose(),
        31,
        "list_pod_content",
        json!({"pod_id": public_pod_id}),
    )
    .await;
    let items = content["result"]["structuredContent"]["value"]
        .as_array()
        .expect("accepted Pod content");
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]["content_item"]["canonical_url"],
        "https://example.com/origin-discovery"
    );
    assert!(items[0].get("candidate").is_none());
    assert!(items[0].get("submissions").is_none());
}

fn complete_package() -> PodPackageContents {
    PodPackageContents {
        context_md: "# Interesting systems\n\nReliable systems material.\n".into(),
        skill_md: "# Curation\n\nPrefer concrete primary sources.\n".into(),
        sources_yaml: "source_rules:\n  - inspect:\n      kind: website\n      url: https://example.com\n    seek:\n      description: concrete systems lessons\n    schedule:\n      cadence: weekly\n".into(),
        filters_yaml: "blocked_topics: []\nblocked_domains: []\n".into(),
        examples_good_md: "# Good\n\n- Primary engineering notes.\n".into(),
        examples_bad_md: "# Bad\n\n- Unsourced summaries.\n".into(),
    }
}

fn mcp_request(token: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-06-18")
        .body(Body::from(body.to_string()))
        .expect("valid request")
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    serde_json::from_slice(&bytes).expect("JSON response")
}

async fn call_mcp_tool(
    app: axum::Router,
    token: &str,
    id: u64,
    name: &str,
    arguments: Value,
) -> Value {
    let response = app
        .oneshot(mcp_request(
            token,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments}
            }),
        ))
        .await
        .expect("MCP tool response");
    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}
