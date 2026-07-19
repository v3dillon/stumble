use axum::{body::Body, http::Request};
use serde_json::{json, Value};
use stumble_api::router_with_base_url;
use stumble_core::*;
use stumble_mcp::{streamable_http_router, McpToolCall, McpToolRouter};
use tower::ServiceExt;

struct TestNodeDir(std::path::PathBuf);

impl TestNodeDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-two-node-acceptance-{name}-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).expect("create persistent test node directory");
        Self(path)
    }
}

impl Drop for TestNodeDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn register_harness(
    tools: &AgentTools,
    label: &str,
    capabilities: Vec<HarnessCapability>,
    pod_ids: Option<Vec<PodId>>,
) -> RegisterAgentHarnessResponse {
    tools
        .register_agent_harness(
            &tools.default_auth_context().expect("node owner context"),
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind: AgentHarnessKind::Interactive,
                capabilities,
                pod_ids,
            },
        )
        .expect("register capability-scoped Agent Harness")
}

#[tokio::test]
async fn agent_harness_discoveries_federate_between_two_independent_nodes() {
    let origin_dir = TestNodeDir::new("origin");
    let home_dir = TestNodeDir::new("home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).expect("open Origin Node");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).expect("open Home Node");
    let origin_creator = register_harness(
        &origin,
        "Origin Pod creator",
        vec![HarnessCapability::PodCuration],
        None,
    );
    let origin_approver = register_harness(
        &origin,
        "Origin public exposure approver",
        vec![HarnessCapability::Approval],
        None,
    );
    let origin_mcp = streamable_http_router(origin.clone());

    let inbox = call_tool(
        origin_mcp.clone(),
        origin_creator.token.expose(),
        1,
        "create_pod",
        json!({
            "name": "Federation Acceptance Inbox",
            "slug": "federation-acceptance-inbox",
            "description": "Private intake that must never federate",
            "visibility": "private"
        }),
    )
    .await;
    let inbox = tool_value(&inbox);
    assert_eq!(inbox["status"], "created");
    let inbox_id: PodId = inbox["result"]["id"]
        .as_str()
        .expect("Inbox Pod identity")
        .parse()
        .expect("valid Inbox Pod identity");

    let proposed = call_tool(
        origin_mcp.clone(),
        origin_creator.token.expose(),
        2,
        "create_pod",
        json!({
            "name": "Federated Post Acceptance",
            "slug": "federated-post-acceptance",
            "description": "Isolated public Pod for two-node acceptance",
            "visibility": "public"
        }),
    )
    .await;
    let proposed = tool_value(&proposed);
    assert_eq!(proposed["status"], "pending_approval");
    let proposal_id = proposed["result"]["id"]
        .as_str()
        .expect("public exposure Pending Proposal identity");
    let approved = call_tool(
        origin_mcp.clone(),
        origin_approver.token.expose(),
        3,
        "approve_pending_proposal",
        json!({"proposal_id": proposal_id}),
    )
    .await;
    assert_eq!(tool_value(&approved)["status"], "accepted");

    let origin_pods = call_tool(
        origin_mcp.clone(),
        origin_creator.token.expose(),
        4,
        "list_pods",
        json!({}),
    )
    .await;
    let origin_pods = tool_value(&origin_pods).as_array().expect("Origin Pods");
    assert!(origin_pods.iter().any(|pod| {
        pod["slug"] == "federation-acceptance-inbox" && pod["visibility"] == "private"
    }));
    let public_pod_id: PodId = origin_pods
        .iter()
        .find(|pod| pod["slug"] == "federated-post-acceptance")
        .and_then(|pod| pod["id"].as_str())
        .expect("approved public Pod identity")
        .parse()
        .expect("valid public Pod identity");

    let origin_discovery = register_harness(
        &origin,
        "Origin discovery worker",
        vec![
            HarnessCapability::CandidateSubmission,
            HarnessCapability::DiscoveryTasks,
        ],
        Some(vec![inbox_id]),
    );
    let origin_curator = register_harness(
        &origin,
        "Origin public Pod curator",
        vec![HarnessCapability::PodCuration],
        Some(vec![public_pod_id]),
    );
    let origin_reader = register_harness(
        &origin,
        "Origin accepted content reader",
        vec![HarnessCapability::FeedRead],
        Some(vec![public_pod_id]),
    );

    let discovery_tools =
        list_tool_names(origin_mcp.clone(), origin_discovery.token.expose(), 5).await;
    assert!(has_tool(&discovery_tools, "submit_candidate"));
    assert!(has_tool(
        &discovery_tools,
        "create_immediate_discovery_task"
    ));
    assert!(!has_tool(&discovery_tools, "route_candidate"));
    assert!(!has_tool(&discovery_tools, "approve_pending_proposal"));
    assert!(!has_tool(&discovery_tools, "subscribe_public_pod"));
    let curator_tools = list_tool_names(origin_mcp.clone(), origin_curator.token.expose(), 6).await;
    assert!(has_tool(&curator_tools, "route_candidate"));
    assert!(has_tool(&curator_tools, "review_candidate_placement"));
    assert!(!has_tool(&curator_tools, "submit_candidate"));
    assert!(!has_tool(&curator_tools, "approve_pending_proposal"));

    let discovery_task = call_tool(
        origin_mcp.clone(),
        origin_discovery.token.expose(),
        7,
        "create_immediate_discovery_task",
        json!({
            "pod_id": inbox_id,
            "instructions": "Find exactly six relevant public posts.",
            "idempotency_key": "two-node-six-posts"
        }),
    )
    .await;
    let discovery_task = tool_value(&discovery_task);
    let task_id = discovery_task["id"]
        .as_str()
        .expect("Discovery Task identity");
    let package_version = discovery_task["package_version"]
        .as_u64()
        .expect("Discovery Task Package version");
    call_tool(
        origin_mcp.clone(),
        origin_discovery.token.expose(),
        8,
        "claim_discovery_task",
        json!({"task_id": task_id}),
    )
    .await;

    let mut candidate_ids = Vec::new();
    for index in 1..=6 {
        let media_references = if index == 1 {
            json!([{
                "media_type": "image",
                "url": "https://media.example/post-1/image.jpg"
            }])
        } else {
            json!([])
        };
        let submitted = call_tool(
            origin_mcp.clone(),
            origin_discovery.token.expose(),
            10 + index,
            "submit_candidate",
            json!({
                "source_url": format!("https://social.example/author/status/{index}"),
                "source_metadata": {
                    "title": format!("Federated post {index}"),
                    "author": "@author"
                },
                "summary": format!("Acceptance post {index}"),
                "content_type": "other",
                "media_references": media_references,
                "tags": ["federation", "acceptance"],
                "provenance": {
                    "discovered_at": "2026-07-18T16:00:00Z",
                    "discovery_method": "agent_harness_browser"
                },
                "proposed_placements": [{
                    "pod_id": inbox_id,
                    "reason": "Keep discovery intake private before explicit routing.",
                    "confidence": 0.95
                }],
                "task_context": {
                    "task_id": task_id,
                    "package_version": package_version
                },
                "harness_idempotency_key": format!("two-node-harness-{index}"),
                "client_idempotency_key": format!("two-node-client-{index}")
            }),
        )
        .await;
        assert_eq!(submitted["result"]["isError"], false);
        candidate_ids.push(
            tool_value(&submitted)["candidate"]["id"]
                .as_str()
                .expect("private Candidate identity")
                .to_owned(),
        );
    }
    call_tool(
        origin_mcp.clone(),
        origin_discovery.token.expose(),
        20,
        "complete_discovery_task",
        json!({"task_id": task_id}),
    )
    .await;

    let direct_discovery_router =
        McpToolRouter::authenticated(origin.clone(), origin_discovery.token.expose())
            .expect("authenticate direct discovery router");
    let denied_route = direct_discovery_router
        .call(McpToolCall {
            tool: "route_candidate".into(),
            arguments: json!({
                "candidate_id": candidate_ids[0],
                "pod_id": public_pod_id,
                "reason": "This discovery-only Harness must not curate.",
                "confidence": 1.0
            }),
        })
        .expect_err("core denies direct curation without Pod Curation");
    assert!(denied_route
        .to_string()
        .contains("harness grant lacks pod_curation"));
    let origin_completed_task = direct_discovery_router
        .call(McpToolCall {
            tool: "discovery_task_status".into(),
            arguments: json!({"task_id": task_id}),
        })
        .expect("Origin retains its completed Discovery Task");
    assert_eq!(origin_completed_task["state"]["status"], "completed");

    for (index, candidate_id) in candidate_ids.iter().enumerate() {
        let routed = call_tool(
            origin_mcp.clone(),
            origin_curator.token.expose(),
            30 + index as u64,
            "route_candidate",
            json!({
                "candidate_id": candidate_id,
                "pod_id": public_pod_id,
                "reason": "The post matches the isolated public acceptance Pod.",
                "confidence": 0.98
            }),
        )
        .await;
        assert_eq!(tool_value(&routed)["status"], "pending");
        let accepted = call_tool(
            origin_mcp.clone(),
            origin_curator.token.expose(),
            40 + index as u64,
            "review_candidate_placement",
            json!({
                "candidate_id": candidate_id,
                "pod_id": public_pod_id,
                "decision": "accept",
                "note": "Accepted through the scoped Origin curation adapter."
            }),
        )
        .await;
        assert_eq!(tool_value(&accepted)["status"], "accepted");
    }

    let origin_content = call_tool(
        origin_mcp.clone(),
        origin_reader.token.expose(),
        50,
        "list_pod_content",
        json!({"pod_id": public_pod_id}),
    )
    .await;
    assert_six_post_references_with_seed_image(tool_value(&origin_content));

    let private_sentinel_task = call_tool(
        origin_mcp.clone(),
        origin_discovery.token.expose(),
        51,
        "create_immediate_discovery_task",
        json!({
            "pod_id": inbox_id,
            "instructions": "Private task that must remain only on the Origin.",
            "idempotency_key": "two-node-private-sentinel"
        }),
    )
    .await;
    let private_sentinel_task_id = tool_value(&private_sentinel_task)["id"]
        .as_str()
        .expect("private sentinel Discovery Task identity");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral Origin listener");
    let origin_base_url = format!("http://{}", listener.local_addr().expect("Origin address"));
    let origin_app = router_with_base_url(origin.clone(), &origin_base_url);
    let origin_server = tokio::spawn(async move { axum::serve(listener, origin_app).await });

    let home_subscriber = register_harness(
        &home,
        "Home subscription manager",
        vec![HarnessCapability::SubscriptionManagement],
        None,
    );
    let home_reader = register_harness(
        &home,
        "Home accepted content reader",
        vec![HarnessCapability::FeedRead],
        None,
    );
    let home_private_state_reader = register_harness(
        &home,
        "Home private state verifier",
        vec![
            HarnessCapability::CandidateSubmission,
            HarnessCapability::DiscoveryTasks,
        ],
        None,
    );
    let home_mcp = streamable_http_router(home.clone());
    let subscriber_tools =
        list_tool_names(home_mcp.clone(), home_subscriber.token.expose(), 60).await;
    assert!(has_tool(&subscriber_tools, "subscribe_public_pod"));
    assert!(has_tool(&subscriber_tools, "synchronize_subscription"));
    assert!(!has_tool(&subscriber_tools, "submit_candidate"));
    assert!(!has_tool(&subscriber_tools, "list_pod_content"));
    assert!(!has_tool(&subscriber_tools, "list_ready_discovery_tasks"));
    let subscribed = call_tool(
        home_mcp.clone(),
        home_subscriber.token.expose(),
        61,
        "subscribe_public_pod",
        json!({
            "public_pod_url": format!(
                "{origin_base_url}/federation/pods/federated-post-acceptance"
            )
        }),
    )
    .await;
    assert_eq!(subscribed["result"]["isError"], false);
    let subscribed = tool_value(&subscribed);
    assert!(subscribed["imported_events"]
        .as_u64()
        .is_some_and(|count| count >= 6));
    assert!(subscribed["subscription"]["last_event_hash"]
        .as_str()
        .is_some_and(|cursor| !cursor.is_empty()));

    let home_pods = call_tool(
        home_mcp.clone(),
        home_reader.token.expose(),
        62,
        "list_pods",
        json!({}),
    )
    .await;
    let home_pods = tool_value(&home_pods).as_array().expect("Home Node Pods");
    assert!(!home_pods
        .iter()
        .any(|pod| pod["slug"] == "federation-acceptance-inbox"));
    let synchronized_pod_id = home_pods
        .iter()
        .find(|pod| pod["slug"] == "federated-post-acceptance")
        .and_then(|pod| pod["id"].as_str())
        .expect("synchronized public Pod identity");
    let synchronized_content = call_tool(
        home_mcp.clone(),
        home_reader.token.expose(),
        63,
        "list_pod_content",
        json!({"pod_id": synchronized_pod_id}),
    )
    .await;
    assert_six_post_references_with_seed_image(tool_value(&synchronized_content));

    for (index, candidate_id) in candidate_ids.iter().enumerate() {
        let missing_candidate = call_tool(
            home_mcp.clone(),
            home_private_state_reader.token.expose(),
            70 + index as u64,
            "inspect_candidate",
            json!({"candidate_id": candidate_id}),
        )
        .await;
        assert_eq!(missing_candidate["result"]["isError"], true);
        let candidate_privacy_error = tool_error_text(&missing_candidate);
        assert!(candidate_privacy_error.contains("Candidate"));
        assert!(candidate_privacy_error.contains("not found"));
    }
    let home_private_router =
        McpToolRouter::authenticated(home, home_private_state_reader.token.expose())
            .expect("authenticate direct Home private-state router");
    let missing_completed_task = home_private_router
        .call(McpToolCall {
            tool: "discovery_task_status".into(),
            arguments: json!({"task_id": task_id}),
        })
        .expect_err("Origin Discovery Task must not exist on Home Node");
    assert!(missing_completed_task
        .to_string()
        .contains("Discovery Task"));
    assert!(missing_completed_task.to_string().contains("not found"));
    let home_ready_tasks = call_tool(
        home_mcp,
        home_private_state_reader.token.expose(),
        80,
        "list_ready_discovery_tasks",
        json!({}),
    )
    .await;
    assert_eq!(tool_value(&home_ready_tasks), &json!([]));
    let origin_ready_tasks = call_tool(
        origin_mcp,
        origin_discovery.token.expose(),
        81,
        "list_ready_discovery_tasks",
        json!({}),
    )
    .await;
    assert!(tool_value(&origin_ready_tasks)
        .as_array()
        .expect("Origin ready Discovery Tasks")
        .iter()
        .any(|task| task["id"] == private_sentinel_task_id));

    origin_server.abort();
}

fn assert_six_post_references_with_seed_image(content: &Value) {
    let content = content.as_array().expect("accepted Pod content");
    assert_eq!(content.len(), 6);
    let mut urls = content
        .iter()
        .map(|placement| {
            placement["content_item"]["canonical_url"]
                .as_str()
                .expect("post Content Reference URL")
        })
        .collect::<Vec<_>>();
    urls.sort_unstable();
    assert_eq!(
        urls,
        (1..=6)
            .map(|index| format!("https://social.example/author/status/{index}"))
            .collect::<Vec<_>>()
    );
    let seed = content
        .iter()
        .find(|placement| {
            placement["content_item"]["canonical_url"] == "https://social.example/author/status/1"
        })
        .expect("seed post Content Reference");
    assert_eq!(
        seed["content_item"]["media_references"],
        json!([{
            "media_type": "image",
            "url": "https://media.example/post-1/image.jpg"
        }])
    );
    assert!(content.iter().all(|placement| {
        placement.get("candidate").is_none() && placement.get("submissions").is_none()
    }));
}

async fn call_tool(app: axum::Router, token: &str, id: u64, name: &str, arguments: Value) -> Value {
    call_mcp(
        app,
        token,
        id,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    )
    .await
}

async fn list_tool_names(app: axum::Router, token: &str, id: u64) -> Vec<String> {
    let response = call_mcp(app, token, id, "tools/list", json!({})).await;
    response["result"]["tools"]
        .as_array()
        .expect("capability-filtered MCP tool descriptors")
        .iter()
        .map(|tool| tool["name"].as_str().expect("MCP tool name").to_owned())
        .collect()
}

fn has_tool(tools: &[String], expected: &str) -> bool {
    tools.iter().any(|tool| tool == expected)
}

fn tool_error_text(response: &Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .expect("MCP tool error text")
}

fn tool_value(response: &Value) -> &Value {
    &response["result"]["structuredContent"]["value"]
}

async fn call_mcp(app: axum::Router, token: &str, id: u64, method: &str, params: Value) -> Value {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .header("mcp-protocol-version", "2025-06-18")
                .body(Body::from(
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "method": method,
                        "params": params
                    })
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
