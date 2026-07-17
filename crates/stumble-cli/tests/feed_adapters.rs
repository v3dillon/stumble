use axum::{body::Body, http::Request};
use serde_json::{json, Value};
use std::process::Command;
use stumble_api::router;
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

fn accepted_item(tools: &AgentTools, ctx: &AuthContext) -> ContentItemId {
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
    tools
        .set_pod_curation_policy(ctx, pod.id, CurationPolicy::Manual, chrono::Utc::now())
        .unwrap();
    let submitted = tools
        .submit_candidate(
            ctx,
            CandidateSubmissionRequest {
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
                    tags: vec!["adapters".into()],
                    provenance: CandidateProvenance {
                        discovered_at: chrono::Utc::now(),
                        discovery_method: "adapter_test".into(),
                        referrer_url: None,
                    },
                    proposed_placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Adapter evidence".into(),
                        confidence: CandidateConfidence::new(0.9).unwrap(),
                    }],
                    task_context: None,
                    harness_idempotency_key: "feed-adapter-harness".into(),
                    client_idempotency_key: "feed-adapter-client".into(),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(ctx, submitted.candidate.id, chrono::Utc::now())
        .unwrap();
    tools
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
        .unwrap()
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
                ],
                pod_ids: None,
            },
        )
        .unwrap();
    let token = issued.token.expose().to_string();
    let ctx = tools.authenticate_token(&token).unwrap().unwrap();
    let content_item_id = accepted_item(&tools, &ctx);
    drop(tools);

    let cli = Command::new(env!("CARGO_BIN_EXE_podctl"))
        .args([
            "--data-dir",
            data_dir.0.to_str().unwrap(),
            "--token",
            &token,
            "feed",
        ])
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let cli_batch: Value = serde_json::from_slice(&cli.stdout).unwrap();

    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(tools.clone(), &token).unwrap();
    let mcp_batch = mcp
        .call(McpToolCall {
            tool: "get_feed_batch".into(),
            arguments: json!({}),
        })
        .unwrap();
    assert_eq!(mcp_batch, cli_batch);

    let response = router(tools)
        .oneshot(
            Request::get("/feed")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let http_batch: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_batch, cli_batch);

    let content_item_id = content_item_id.to_string();
    let cli_feedback = Command::new(env!("CARGO_BIN_EXE_podctl"))
        .args([
            "--data-dir",
            data_dir.0.to_str().unwrap(),
            "--token",
            &token,
            "feed-feedback",
            &content_item_id,
            "--kind",
            "more-like-this",
        ])
        .output()
        .unwrap();
    assert!(cli_feedback.status.success());
    let cli_feedback: Value = serde_json::from_slice(&cli_feedback.stdout).unwrap();

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
    assert_eq!(mcp_feedback, cli_feedback);

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
    assert_eq!(http_feedback, cli_feedback);
}
