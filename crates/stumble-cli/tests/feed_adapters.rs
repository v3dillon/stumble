use axum::{body::Body, http::Request};
use serde_json::{json, Value};
use std::process::Command;
use stumble_api::{router, router_with_options, RouterOptions};
use stumble_core::*;
use stumble_mcp::{McpToolCall, McpToolRouter};
use tower::ServiceExt;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("stumble-feed-adapters-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn accepted_item(tools: &AgentTools, ctx: &AuthContext) -> (PodId, ContentItemId) {
    let pod = tools
        .create_pod(
            ctx,
            CreatePodRequest {
                name: "Feed adapters".into(),
                slug: "feed-adapters".into(),
                description: "Adapter parity".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    tools.join_pod(ctx, &pod.slug).unwrap();
    tools
        .set_pod_curation_policy(ctx, pod.id, CurationPolicy::Manual, chrono::Utc::now())
        .unwrap();
    let submitted = tools
        .submit_candidate(
            ctx,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Adapter evidence".into(),
                        confidence: CandidateConfidence::new(0.9).unwrap(),
                    }],
                    task_context: None,
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: "https://feed-adapter.example/report".into(),
                    source_metadata: CandidateSourceMetadata {
                        title: Some("Feed adapter report".into()),
                        author: None,
                        published_at: None,
                    },
                    permitted_excerpt: None,
                    summary: Some("Adapter parity".into()),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec!["adapters".into()],
                    provenance: CandidateProvenance {
                        discovered_at: chrono::Utc::now(),
                        discovery_method: "adapter_test".into(),
                        referrer_url: None,
                    },
                    harness_idempotency_key: "feed-adapter-harness".into(),
                    client_idempotency_key: "feed-adapter-client".into(),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(ctx, submitted.candidate.id, chrono::Utc::now())
        .unwrap();
    let content_item_id = tools
        .review_candidate_placement(
            ctx,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            chrono::Utc::now(),
        )
        .unwrap()
        .content_item_id
        .unwrap();
    (pod.id, content_item_id)
}

#[tokio::test]
async fn http_mcp_and_cli_return_the_same_stable_feed_batch() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "feed adapter".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![
                    HarnessCapability::FeedRead,
                    HarnessCapability::Feedback,
                    HarnessCapability::CandidateSubmission,
                    HarnessCapability::PodCuration,
                    HarnessCapability::SubscriptionManagement,
                ],
                pod_ids: None,
            },
        )
        .unwrap();
    let token = issued.token.expose().to_string();
    let ctx = tools.authenticate_token(&token).unwrap().unwrap();
    let (pod_id, content_item_id) = accepted_item(&tools, &ctx);
    drop(tools);

    let feed_request = data_dir.0.join("feed-request.json");
    std::fs::write(
        &feed_request,
        serde_json::to_vec(&json!({
            "feed_mix": {
                "high_value_percent": 70,
                "exploration_percent": 20,
                "old_gem_percent": 10,
                "per_pod_cap": 4,
                "per_source_cap": 3
            },
            "batch_intent": {
                "focus_topics": ["adapters"],
                "avoid_topics": ["politics"]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let cli = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.0.to_str().unwrap(),
            "feed",
            "batch",
            "get",
            "--input",
            feed_request.to_str().unwrap(),
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &token)
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let cli_envelope: Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(cli_envelope["version"], 1);
    let cli_batch = cli_envelope["data"].clone();

    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(tools.clone(), &token).unwrap();
    let mcp_batch = mcp
        .call(McpToolCall {
            tool: "get_feed_batch".into(),
            arguments: json!({
                "feed_mix": {
                    "high_value_percent": 70,
                    "exploration_percent": 20,
                    "old_gem_percent": 10,
                    "per_pod_cap": 4,
                    "per_source_cap": 3
                },
                "batch_intent": {
                    "focus_topics": ["adapters"],
                    "avoid_topics": ["politics"]
                }
            }),
        })
        .unwrap();
    assert_eq!(mcp_batch["id"], cli_batch["id"]);
    assert_eq!(mcp_batch["state"], cli_batch["state"]);
    assert_eq!(
        mcp_batch["items"][0]["content_reference"]["content_item_id"],
        cli_batch["items"][0]["content_reference"]["content_item_id"]
    );
    assert_eq!(cli_batch["allowed_actions"], json!(["complete"]));
    assert_eq!(
        cli_batch["items"][0]["allowed_actions"],
        mcp_batch["items"][0]["allowed_actions"]
    );

    let response = router(tools)
        .oneshot(
            Request::get("/feed?high_value_percent=70&exploration_percent=20&old_gem_percent=10&per_pod_cap=4&per_source_cap=3&focus=adapters&avoid=politics")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&response_body)
    );
    let http_batch: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(http_batch["id"], cli_batch["id"]);
    assert_eq!(http_batch["state"], cli_batch["state"]);
    assert_eq!(
        http_batch["items"][0]["content_reference"]["content_item_id"],
        cli_batch["items"][0]["content_reference"]["content_item_id"]
    );
    assert_eq!(cli_batch["feed_mix"]["high_value_percent"], 70);
    assert_eq!(
        cli_batch["batch_intent"]["focus_topics"],
        json!(["adapters"])
    );

    let pod_id_string = pod_id.to_string();
    let cli_priority = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.0.to_str().unwrap(),
            "pod",
            "subscription",
            "set",
            &pod_id_string,
            "--priority",
            "true",
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &token)
        .output()
        .unwrap();
    assert!(
        cli_priority.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_priority.stderr)
    );

    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(tools.clone(), &token).unwrap();
    assert_eq!(
        mcp.call(McpToolCall {
            tool: "set_priority_subscription".into(),
            arguments: json!({"pod_id": pod_id, "is_priority": false}),
        })
        .unwrap(),
        json!({"status": "updated"})
    );
    let response = router(tools)
        .oneshot(
            Request::post(format!("/subscriptions/{pod_id}/priority"))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"is_priority":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);

    let content_item_id = content_item_id.to_string();
    let cli_feedback = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.0.to_str().unwrap(),
            "feed",
            "feedback",
            "record",
            &content_item_id,
            "--kind",
            "more-like-this",
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &token)
        .output()
        .unwrap();
    assert!(cli_feedback.status.success());
    let cli_feedback: Value = serde_json::from_slice(&cli_feedback.stdout).unwrap();
    assert_eq!(cli_feedback["version"], 1);
    let cli_feedback = cli_feedback["data"].clone();

    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(tools.clone(), &token).unwrap();
    let mcp_feedback = mcp
        .call(McpToolCall {
            tool: "record_feed_feedback".into(),
            arguments: json!({
                "content_item_id": content_item_id,
                "kind": "more_like_this"
            }),
        })
        .unwrap();
    assert_eq!(mcp_feedback, cli_feedback["feedback_state"]);
    assert_eq!(cli_feedback["content_item_id"], content_item_id);
    assert!(cli_feedback["allowed_actions"].is_array());

    let response = router(tools)
        .oneshot(
            Request::post(format!("/feed/items/{content_item_id}/feedback"))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind":"more_like_this"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let http_feedback: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_feedback, cli_feedback["feedback_state"]);
}

#[tokio::test]
async fn http_mcp_and_cli_inspect_the_same_private_taste_profile() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "taste adapter".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Feedback],
                pod_ids: None,
            },
        )
        .unwrap();
    let token = issued.token.expose().to_string();
    drop(tools);

    let taste_request = data_dir.0.join("taste-request.json");
    std::fs::write(
        &taste_request,
        serde_json::to_vec(&json!({
            "interests": ["systems"],
            "blocked_source_affinities": [
                {"kind": "publisher", "value": "Systems Weekly"}
            ],
            "recurrence_penalty_days": 21
        }))
        .unwrap(),
    )
    .unwrap();

    let cli_update = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.0.to_str().unwrap(),
            "feed",
            "taste",
            "set",
            "--input",
            taste_request.to_str().unwrap(),
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &token)
        .output()
        .unwrap();
    assert!(cli_update.status.success());
    let cli_update: Value = serde_json::from_slice(&cli_update.stdout).unwrap();
    assert_eq!(cli_update["version"], 1);
    let cli_update = cli_update["data"].clone();

    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(tools.clone(), &token).unwrap();
    let mcp_update = mcp
        .call(McpToolCall {
            tool: "update_taste_profile".into(),
            arguments: json!({
                "interests": ["systems"],
                "blocked_source_affinities": [
                    {"kind": "publisher", "value": "Systems Weekly"}
                ],
                "recurrence_penalty_days": 21
            }),
        })
        .unwrap();
    assert_eq!(mcp_update["user_id"], cli_update["user_id"]);
    assert_eq!(mcp_update["explicit"], cli_update["explicit"]);
    assert_eq!(cli_update["allowed_actions"], json!(["set", "reset"]));
    let response = router(tools.clone())
        .oneshot(
            Request::patch("/taste-profile")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"interests":["systems"],"blocked_source_affinities":[{"kind":"publisher","value":"Systems Weekly"}],"recurrence_penalty_days":21}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let http_update: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_update["user_id"], cli_update["user_id"]);
    assert_eq!(http_update["explicit"], cli_update["explicit"]);
    drop(tools);

    let cli = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.0.to_str().unwrap(),
            "feed",
            "taste",
            "show",
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &token)
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let cli_profile: Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(cli_profile["version"], 1);
    let cli_profile = cli_profile["data"].clone();

    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(tools.clone(), &token).unwrap();
    let mcp_profile = mcp
        .call(McpToolCall {
            tool: "get_taste_profile".into(),
            arguments: json!({}),
        })
        .unwrap();
    assert_eq!(mcp_profile["user_id"], cli_profile["user_id"]);
    assert_eq!(mcp_profile["explicit"], cli_profile["explicit"]);
    assert_eq!(mcp_profile["learned"], cli_profile["learned"]);
    assert_eq!(cli_profile["allowed_actions"], json!(["set", "reset"]));

    let response = router(tools)
        .oneshot(
            Request::get("/taste-profile")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let http_profile: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_profile["user_id"], cli_profile["user_id"]);
    assert_eq!(http_profile["explicit"], cli_profile["explicit"]);
    assert_eq!(http_profile["learned"], cli_profile["learned"]);

    let cli_reset = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.0.to_str().unwrap(),
            "feed",
            "taste",
            "reset",
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &token)
        .output()
        .unwrap();
    assert!(cli_reset.status.success());
    let cli_reset: Value = serde_json::from_slice(&cli_reset.stdout).unwrap();
    assert_eq!(cli_reset["version"], 1);
    let cli_reset = cli_reset["data"].clone();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(tools.clone(), &token).unwrap();
    let mcp_reset = mcp
        .call(McpToolCall {
            tool: "reset_learned_taste".into(),
            arguments: json!({}),
        })
        .unwrap();
    assert_eq!(mcp_reset["user_id"], cli_reset["user_id"]);
    assert_eq!(mcp_reset["learned"], cli_reset["learned"]);
    assert_eq!(cli_reset["allowed_actions"], json!(["set", "reset"]));
    let response = router(tools)
        .oneshot(
            Request::post("/taste-profile/learned/reset")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    let http_reset: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_reset["user_id"], cli_reset["user_id"]);
    assert_eq!(http_reset["learned"], cli_reset["learned"]);
}

#[tokio::test]
async fn unauthenticated_public_http_responses_never_expose_taste_profile_data() {
    let tools = AgentTools::new(seed_store());
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "private profile".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![
                    HarnessCapability::Feedback,
                    HarnessCapability::CandidateSubmission,
                ],
                pod_ids: None,
            },
        )
        .unwrap();
    let user = tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap();
    let mut update = UpdateTasteProfileRequest::default();
    update.interests = Some(vec!["http-private-needle".into()]);
    tools.update_taste_profile(&user, update).unwrap();
    let private_candidate = tools
        .submit_candidate(
            &user,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::User {
                    learn: true,
                    interest_seed_metadata: Default::default(),
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: "https://seed-private-needle.example/reference".into(),
                    source_metadata: CandidateSourceMetadata {
                        title: None,
                        author: None,
                        published_at: None,
                    },
                    permitted_excerpt: None,
                    summary: None,
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec!["seed-private-topic-needle".into()],
                    provenance: CandidateProvenance {
                        discovered_at: chrono::Utc::now(),
                        discovery_method: "user_submission".into(),
                        referrer_url: None,
                    },
                    harness_idempotency_key: "private-seed-harness".into(),
                    client_idempotency_key: "private-seed-client".into(),
                },
            },
        )
        .unwrap();

    let federation = tools.default_auth_context().unwrap();
    let mut paths = vec![
        "/.well-known/stumble-node".to_string(),
        "/federation/node".to_string(),
        "/federation/pods".to_string(),
    ];
    for pod in tools.list_public_pods(&federation).unwrap() {
        paths.push(format!("/federation/pods/{}/manifest", pod.slug));
        paths.push(format!("/federation/pods/{}/events", pod.slug));
    }
    let public_router = || {
        router_with_options(
            tools.clone(),
            "https://public.example",
            RouterOptions {
                dev_tokens_allowed: false,
                owner_access_allowed: false,
            },
        )
    };
    for path in paths {
        let response = public_router()
            .oneshot(Request::get(&path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK, "{path}");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body.contains("http-private-needle"), "{path}: {body}");
        assert!(!body.contains("taste_profile"), "{path}: {body}");
        assert!(!body.contains("evidence_summary"), "{path}: {body}");
        assert!(!body.contains("seed-private-needle"), "{path}: {body}");
        assert!(!body.contains("interest_seed"), "{path}: {body}");
        assert!(!body.contains("source_affinit"), "{path}: {body}");
    }
    let retraction_path = format!(
        "/taste-profile/interest-seeds/{}/retract",
        private_candidate.candidate.id
    );
    for (method, path) in [
        ("GET", "/taste-profile"),
        ("PATCH", "/taste-profile"),
        ("POST", "/taste-profile/learned/reset"),
        ("POST", retraction_path.as_str()),
        ("GET", "/home/discover-public-pods?topics=design"),
        ("GET", "/hub/search-pods?q=design"),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let response = public_router().oneshot(request).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    }
}
