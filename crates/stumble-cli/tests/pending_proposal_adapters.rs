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
        let path = std::env::temp_dir().join(format!(
            "stumble-proposal-adapters-{}",
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

#[tokio::test]
async fn http_mcp_and_cli_share_pending_proposal_behavior() {
    // Arrange
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let owner = tools.default_auth_context().unwrap();
    let pod = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Adapter Approval".into(),
                slug: "adapter-approval".into(),
                description: String::new(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let proposer = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "proposal worker".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![HarnessCapability::PodCuration],
                pod_ids: Some(vec![pod.id]),
            },
        )
        .unwrap();
    let approver = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "proposal approver".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Approval],
                pod_ids: None,
            },
        )
        .unwrap();
    let proposer_token = proposer.token.expose().to_string();
    let approver_token = approver.token.expose().to_string();
    drop(tools);

    // Act: request the sensitive domain workflow through the canonical CLI.
    let cli = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.0.to_str().unwrap(),
            "pod",
            "visibility",
            "set",
            "adapter-approval",
            "--visibility",
            "public",
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &proposer_token)
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let created: Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(created["version"], 2);
    assert_eq!(created["data"]["outcome"]["status"], "pending_approval");
    let proposal_id = created["data"]["outcome"]["result"]["id"].as_str().unwrap();

    // Act: inspect through MCP.
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(tools.clone(), &proposer_token).unwrap();
    let inspected = mcp
        .call(McpToolCall {
            tool: "get_pending_proposal".into(),
            arguments: json!({"proposal_id": proposal_id}),
        })
        .unwrap();
    assert_eq!(inspected["id"], proposal_id);
    assert_eq!(inspected["status"], "pending");
    drop(mcp);
    drop(tools);

    let shown = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.0.to_str().unwrap(),
            "node",
            "proposal",
            "show",
            proposal_id,
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &approver_token)
        .output()
        .unwrap();
    assert!(shown.status.success());
    let shown: Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(shown["data"]["id"], proposal_id);
    assert_eq!(
        shown["data"]["allowed_actions"],
        json!(["approve", "reject"])
    );

    // Act: approve through HTTP.
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let response = router(tools.clone())
        .oneshot(
            Request::post(format!("/pending-proposals/{proposal_id}/approve"))
                .header("authorization", format!("Bearer {approver_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Assert
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
    let accepted: Value = serde_json::from_slice(&response_body).unwrap();
    assert_eq!(accepted["status"], "accepted");
    assert_eq!(
        tools
            .pod_by_slug("adapter-approval", None)
            .unwrap()
            .visibility,
        Visibility::Public
    );
}
