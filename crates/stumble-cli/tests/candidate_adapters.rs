use axum::{body::Body, http::Request};
use chrono::Utc;
use serde_json::{json, Value};
use std::process::Command;
use stumble_api::router;
use stumble_core::*;
use stumble_mcp::{McpToolCall, McpToolRouter};
use tower::ServiceExt;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-candidate-adapters-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn request(pod_id: PodId) -> CandidateSubmissionRequest {
    CandidateSubmissionRequest {
        evidence: CandidateSubmissionEvidence {
            source_url: "https://example.com/adapter-report?utm_campaign=test".into(),
            source_metadata: CandidateSourceMetadata {
                title: Some("Adapter report".into()),
                author: Some("Example".into()),
                published_at: None,
            },
            permitted_excerpt: Some("Permitted excerpt".into()),
            summary: Some("Adapter parity evidence".into()),
            content_type: CandidateContentType::Article,
            tags: vec!["adapters".into()],
            provenance: CandidateProvenance {
                discovered_at: Utc::now(),
                discovery_method: "interactive_browser".into(),
                referrer_url: None,
            },
            proposed_placements: vec![ProposedCandidatePlacement {
                pod_id,
                reason: "Relevant to adapter design".into(),
                confidence: CandidateConfidence::new(0.75).unwrap(),
            }],
            task_context: None,
            harness_idempotency_key: "adapter-client-key".into(),
            client_idempotency_key: "adapter-client-key".into(),
        },
    }
}

#[tokio::test]
async fn http_mcp_and_cli_submit_and_inspect_equivalent_candidates() {
    let data_dir = TestDataDir::new();
    let request_path = data_dir.0.join("candidate.json");
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let pod = tools
        .create_pod(
            &tools.default_auth_context().unwrap(),
            CreatePodRequest {
                name: "Adapters".into(),
                slug: "candidate-adapters".into(),
                description: "Candidate adapter acceptance".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "adapter candidate worker".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::CandidateSubmission],
                pod_ids: Some(vec![pod.id]),
            },
        )
        .unwrap();
    let token = issued.token.expose().to_string();
    let candidate_request = request(pod.id);
    std::fs::write(
        &request_path,
        serde_json::to_vec_pretty(&candidate_request.evidence).unwrap(),
    )
    .unwrap();
    drop(tools);

    let cli = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.0.to_str().unwrap(),
            "discover",
            "candidate",
            "submit",
            "--input",
            request_path.to_str().unwrap(),
            "--idempotency-key",
            "adapter-client-key",
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &token)
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let cli_submission: Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(cli_submission["version"], 1);
    let candidate_id = cli_submission["data"]["candidate"]["id"].as_str().unwrap();

    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(tools.clone(), &token).unwrap();
    let mcp_submission = mcp
        .call(McpToolCall {
            tool: "submit_candidate".into(),
            arguments: serde_json::to_value(&candidate_request).unwrap(),
        })
        .unwrap();
    assert_eq!(mcp_submission["candidate"]["id"], candidate_id);
    let mcp_inspection = mcp
        .call(McpToolCall {
            tool: "inspect_candidate".into(),
            arguments: json!({"candidate_id": candidate_id}),
        })
        .unwrap();

    let http_submission = router(tools.clone())
        .oneshot(
            Request::post("/candidates")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&candidate_request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(http_submission.status(), axum::http::StatusCode::OK);
    let http_submission: Value = serde_json::from_slice(
        &axum::body::to_bytes(http_submission.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_submission["candidate"]["id"], candidate_id);

    let http_inspection = router(tools)
        .oneshot(
            Request::get(format!("/candidates/{candidate_id}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(http_inspection.status(), axum::http::StatusCode::OK);
    let http_inspection: Value = serde_json::from_slice(
        &axum::body::to_bytes(http_inspection.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_inspection, mcp_inspection);

    let cli_inspection = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.0.to_str().unwrap(),
            "discover",
            "candidate",
            "show",
            candidate_id,
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &token)
        .output()
        .unwrap();
    assert!(cli_inspection.status.success());
    let cli_inspection: Value = serde_json::from_slice(&cli_inspection.stdout).unwrap();
    assert_eq!(cli_inspection["version"], 1);
    assert_eq!(cli_inspection["data"]["candidate"]["id"], candidate_id);
    assert_eq!(
        cli_inspection["data"]["candidate"]["id"],
        mcp_inspection["candidate"]["id"]
    );
    assert_eq!(
        cli_inspection["data"]["allowed_actions"],
        mcp_inspection["allowed_actions"]
    );
    assert_eq!(http_inspection["candidate"]["id"], candidate_id);
}
