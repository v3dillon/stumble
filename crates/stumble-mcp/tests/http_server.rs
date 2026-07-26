mod support;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use stumble_core::{
    seed_store, AgentHarnessKind, AgentTools, CreatePodOutcome, CreatePrivatePodWithPackageRequest,
    HarnessCapability, MediaReference, MediaReferenceType, PodPackageContents, PodPlacementStatus,
    ProposalStatus, RegisterAgentHarnessRequest,
};
use stumble_mcp::streamable_http_router;
use support::{mcp_request, response_json, McpClient, ScopedHarness};
use tower::ServiceExt;

#[tokio::test]
async fn authenticated_client_negotiates_the_supported_protocol() {
    let tools = AgentTools::new(seed_store());
    let token = ScopedHarness::register(
        &tools,
        "ChatGPT MCP test",
        vec![
            HarnessCapability::FeedRead,
            HarnessCapability::CandidateSubmission,
            HarnessCapability::DiscoveryTasks,
        ],
        None,
    );
    let app = streamable_http_router(tools);

    let incomplete = app
        .clone()
        .oneshot(mcp_request(
            token.token(),
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
            token.token(),
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
    let token = ScopedHarness::register(
        &tools,
        "ChatGPT feed-only catalog",
        vec![HarnessCapability::FeedRead],
        None,
    );
    let app = streamable_http_router(tools);

    let tools = McpClient::new(app, token.token()).list_tools(2).await;
    let feed = tools.descriptor("get_feed_batch");
    assert_eq!(feed["annotations"]["readOnlyHint"], false);
    assert_eq!(feed["annotations"]["destructiveHint"], false);
    assert!(
        feed["inputSchema"]["properties"]["feed_mix"]["properties"]["exploration_percent"]
            .is_object()
    );
    let names = tools.names();
    assert!(names.iter().any(|name| name == "list_pods"));
    assert!(!names.iter().any(|name| name == "submit_candidate"));
    assert!(!names
        .iter()
        .any(|name| name == "list_ready_discovery_tasks"));
    assert!(!names.iter().any(|name| name == "record_feed_feedback"));
}

#[tokio::test]
async fn feedback_catalog_advertises_interest_seed_retraction() {
    let tools = AgentTools::new(seed_store());
    let token = ScopedHarness::register(
        &tools,
        "ChatGPT feedback catalog",
        vec![HarnessCapability::Feedback],
        None,
    );
    let tools = McpClient::new(streamable_http_router(tools), token.token())
        .list_tools(2)
        .await;

    assert!(tools
        .names()
        .iter()
        .any(|name| name == "retract_interest_seed"));
    assert!(tools.names().iter().any(|name| name == "get_taste_profile"));
}

#[tokio::test]
async fn unattended_feedback_catalog_omits_interactive_private_tools() {
    let tools = AgentTools::new(seed_store());
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "unattended feedback catalog".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
                pod_ids: None,
            },
        )
        .unwrap();
    let catalog = McpClient::new(streamable_http_router(tools), issued.token.expose())
        .list_tools(2)
        .await;
    let names = catalog.names();

    assert!(!names.iter().any(|name| name == "record_feed_feedback"));
    assert!(!names.iter().any(|name| name == "retract_interest_seed"));
    assert!(!names.iter().any(|name| name == "get_taste_profile"));
}

#[tokio::test]
async fn origin_curation_tools_are_advertised_only_for_their_harness_capability() {
    let tools = AgentTools::new(seed_store());
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
        let token = ScopedHarness::register(
            &tools,
            &format!("{capability} catalog"),
            vec![capability],
            None,
        );
        let names = McpClient::new(streamable_http_router(tools.clone()), token.token())
            .list_tool_names(11)
            .await;

        for expected_name in &expected_names {
            assert!(
                names.iter().any(|name| name == expected_name),
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
                names.iter().any(|name| name == forbidden_name),
                belongs_to_capability,
                "unexpected {forbidden_name} visibility for {capability}: {names:?}"
            );
        }
    }
}

#[tokio::test]
async fn tool_calls_return_structured_content() {
    let tools = AgentTools::new(seed_store());
    let token = ScopedHarness::register(
        &tools,
        "ChatGPT feed reader",
        vec![HarnessCapability::FeedRead],
        None,
    );
    let app = streamable_http_router(tools);

    let called = McpClient::new(app, token.token())
        .call_tool(4, "list_pods", json!({}))
        .await;
    assert_eq!(called.structured_content(), &json!({"value": []}));
    assert_eq!(called.content_text(), r#"{"value":[]}"#);
    assert!(!called.is_error());
}

#[tokio::test]
async fn unknown_tools_and_invalid_arguments_are_jsonrpc_protocol_errors() {
    let tools = AgentTools::new(seed_store());
    let token = ScopedHarness::register(
        &tools,
        "ChatGPT protocol errors",
        vec![HarnessCapability::FeedRead],
        None,
    );
    let app = streamable_http_router(tools);

    let unknown = app
        .clone()
        .oneshot(mcp_request(
            token.token(),
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
            token.token(),
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
            token.token(),
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
    let token = ScopedHarness::register(
        &tools,
        "Origin defense test",
        vec![HarnessCapability::FeedRead],
        None,
    );
    let app = streamable_http_router(tools);
    let mut request = mcp_request(
        token.token(),
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
    let token = ScopedHarness::register(
        &tools,
        "Protocol version test",
        vec![HarnessCapability::FeedRead],
        None,
    );
    let app = streamable_http_router(tools);
    let mut request = mcp_request(
        token.token(),
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
    let token = ScopedHarness::register(
        &tools,
        "ChatGPT link intake",
        vec![
            HarnessCapability::PodCuration,
            HarnessCapability::PackageManagement,
            HarnessCapability::CandidateSubmission,
        ],
        None,
    );
    let harness = tools
        .authenticate_token(token.token())
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

    let mcp = McpClient::new(app, token.token());
    let listed = mcp.list_tools(4).await;
    let submit_schema = listed.descriptor("submit_candidate");
    assert_eq!(
        submit_schema["inputSchema"]["properties"]["media_references"]["items"]["properties"]
            ["media_type"]["enum"],
        json!(["image", "video"])
    );

    let response = mcp
        .call_tool(
            5,
            "submit_candidate",
            json!({
                        "source_url": "https://example.com/field-note?utm_source=chatgpt",
                        "source_metadata": {
                            "title": "A useful field note"
                        },
                        "summary": "A concrete systems lesson.",
                        "content_type": "article",
                        "media_references": [{
                            "media_type": "image",
                            "url": "https://media.example.com/field-note.png"
                        }],
                        "tags": ["systems"],
                        "provenance": {
                            "discovered_at": "2026-07-17T22:00:00Z",
                            "discovery_method": "chatgpt_conversation"
                        },
                        "target": {
                            "kind": "pod_placements",
                            "placements": [{
                                "pod_id": created.pod.id,
                                "reason": "The user explicitly sent this link for the Pod.",
                                "confidence": 1.0
                            }]
                        },
                        "harness_idempotency_key": "chatgpt-link-1",
                        "client_idempotency_key": "conversation-message-1"
            }),
        )
        .await;

    assert!(!response.is_error());
    let submitted = response.submitted_candidate();
    assert_eq!(
        submitted.candidate.canonical_url,
        "https://example.com/field-note"
    );
    assert_eq!(
        submitted.submission.evidence.provenance.discovery_method,
        "chatgpt_conversation"
    );
    assert_eq!(
        submitted.submission.evidence.media_references,
        vec![MediaReference::new(
            MediaReferenceType::Image,
            "https://media.example.com/field-note.png",
        )
        .expect("valid expected media reference")]
    );
}

#[tokio::test]
async fn harnesses_can_curate_an_origin_pod_without_bypassing_scope_or_approval() {
    let tools = AgentTools::new(seed_store());
    let curator = ScopedHarness::register(
        &tools,
        "interactive Origin curator",
        vec![
            HarnessCapability::PodCuration,
            HarnessCapability::CandidateSubmission,
        ],
        None,
    );
    let grant_admin = ScopedHarness::register(
        &tools,
        "interactive grant administrator",
        vec![HarnessCapability::Administration],
        None,
    );
    let approver = ScopedHarness::register(
        &tools,
        "independent public exposure approver",
        vec![HarnessCapability::Approval],
        None,
    );
    let now = chrono::Utc::now();
    let grant_admin_context = tools
        .authenticate_token(grant_admin.token())
        .expect("authenticate grant administrator")
        .expect("grant administrator context");
    let approver_context = tools
        .authenticate_token(approver.token())
        .expect("authenticate independent approver")
        .expect("independent approver context");
    let expansion = tools
        .request_harness_grant_expansion(
            &grant_admin_context,
            curator.id(),
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
    let curator_mcp = McpClient::new(app.clone(), curator.token());
    let approver_mcp = McpClient::new(app.clone(), approver.token());

    let inbox = curator_mcp
        .call_tool(
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
    let inbox_id = match inbox.create_pod_outcome() {
        CreatePodOutcome::Created(pod) => pod.id,
        CreatePodOutcome::PendingApproval(_) => panic!("private Pod must be created immediately"),
    };

    let proposed = curator_mcp
        .call_tool(
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
    let proposal_id = match proposed.create_pod_outcome() {
        CreatePodOutcome::PendingApproval(proposal) => proposal.id,
        CreatePodOutcome::Created(_) => panic!("public Pod creation requires approval"),
    };

    let self_approval = curator_mcp
        .call_tool(
            22,
            "approve_pending_proposal",
            json!({"proposal_id": proposal_id}),
        )
        .await;
    assert!(self_approval.is_error());
    assert!(self_approval
        .error_text()
        .contains("cannot approve its own Pending Proposal"));

    let inspected = approver_mcp
        .call_tool(
            23,
            "get_pending_proposal",
            json!({"proposal_id": proposal_id}),
        )
        .await;
    assert_eq!(inspected.pending_proposal().status, ProposalStatus::Pending);
    let approved = approver_mcp
        .call_tool(
            24,
            "approve_pending_proposal",
            json!({"proposal_id": proposal_id}),
        )
        .await;
    assert_eq!(approved.pending_proposal().status, ProposalStatus::Accepted);

    let listed = curator_mcp.call_tool(25, "list_pods", json!({})).await;
    let public_pod_id = listed
        .pods()
        .into_iter()
        .find(|pod| pod.slug == "federated-finds")
        .map(|pod| pod.id)
        .expect("approved public Pod id");

    let submitted = curator_mcp
        .call_tool(
            26,
            "submit_candidate",
            json!({
                "source_url": "https://example.com/origin-discovery?utm_source=harness",
                "source_metadata": {
                    "title": "Origin discovery",
                    "author": "Primary researcher",
                    "published_at": "2026-07-17T09:30:00Z"
                },
                "permitted_excerpt": "A retained source excerpt.",
                "summary": "One private Candidate routed through authorized curation.",
                "content_type": "article",
                "tags": ["federation"],
                "provenance": {
                    "discovered_at": "2026-07-18T12:00:00Z",
                    "discovery_method": "interactive_browser",
                    "referrer_url": "https://search.example/origin-discovery"
                },
                "target": {
                    "kind": "pod_placements",
                    "placements": [{
                        "pod_id": inbox_id,
                        "reason": "Initial private discovery intake.",
                        "confidence": 0.9
                    }]
                },
                "harness_idempotency_key": "origin-curation-1",
                "client_idempotency_key": "origin-message-1"
            }),
        )
        .await;
    let candidate_id = submitted.submitted_candidate().candidate.id;

    let inspected_candidate = curator_mcp
        .call_tool(
            261,
            "inspect_candidate",
            json!({"candidate_id": candidate_id}),
        )
        .await;
    let inspected_reference = &inspected_candidate.structured_content()["value"]["reference"];
    assert_eq!(
        inspected_reference["summary"],
        "One private Candidate routed through authorized curation."
    );
    assert_eq!(
        inspected_reference["source_metadata"]["author"],
        "Primary researcher"
    );
    assert_eq!(
        inspected_reference["provenance"]["discovery_method"],
        "interactive_browser"
    );

    let scoped_curator = ScopedHarness::register(
        &tools,
        "Inbox-only curator",
        vec![HarnessCapability::PodCuration],
        Some(vec![inbox_id]),
    );
    let scoped_curator_mcp = McpClient::new(app.clone(), scoped_curator.token());
    let denied_route = scoped_curator_mcp
        .call_tool(
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
    assert!(denied_route.is_error());

    let routed = curator_mcp
        .call_tool(
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
    assert_eq!(routed.pod_placement().status, PodPlacementStatus::Pending);
    let denied_review = scoped_curator_mcp
        .call_tool(
            29,
            "review_candidate_placement",
            json!({
                "candidate_id": candidate_id,
                "pod_id": public_pod_id,
                "decision": "accept"
            }),
        )
        .await;
    assert!(denied_review.is_error());
    let reviewed = curator_mcp
        .call_tool(
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
        reviewed.pod_placement().status,
        PodPlacementStatus::Accepted
    );

    let reader = ScopedHarness::register(
        &tools,
        "accepted content reader",
        vec![HarnessCapability::FeedRead],
        Some(vec![public_pod_id]),
    );
    let content = McpClient::new(app, reader.token())
        .call_tool(31, "list_pod_content", json!({"pod_id": public_pod_id}))
        .await;
    let items = content.pod_content();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].content_item.canonical_url(),
        "https://example.com/origin-discovery"
    );
    let content_reference = &content.structured_content()["value"][0]["content_item"];
    assert_eq!(
        content_reference["summary"],
        "One private Candidate routed through authorized curation."
    );
    assert_eq!(
        content_reference["source_metadata"]["author"],
        "Primary researcher"
    );
    assert_eq!(
        content_reference["source_metadata"]["published_at"],
        "2026-07-17T09:30:00Z"
    );
    assert_eq!(
        content_reference["provenance"][0]["discovery_method"],
        "interactive_browser"
    );
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
