use axum::{body::Body, http::Request};
use serde_json::json;
use std::process::Command;
use stumble_api::router;
use stumble_core::*;
use stumble_mcp::{McpToolCall, McpToolRouter};
use tower::ServiceExt;
use uuid::Uuid;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("stumble-cli-harness-{}", Uuid::now_v7())))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn representative_adapters_return_equivalent_authorization_denials() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).unwrap();
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "submitter".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![HarnessCapability::CandidateSubmission],
                pod_ids: None,
            },
        )
        .unwrap();
    let token = issued.token.expose().to_string();
    drop(tools);

    let output = Command::new(env!("CARGO_BIN_EXE_podctl"))
        .args([
            "--data-dir",
            data_dir.path().to_str().unwrap(),
            "--token",
            &token,
            "block-source",
            "example.com",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let expected = "harness grant lacks feedback";
    assert!(String::from_utf8_lossy(&output.stderr).contains(expected));

    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(tools.clone(), &token).unwrap();
    let mcp_error = mcp
        .call(McpToolCall {
            tool: "save_link".into(),
            arguments: json!({"submission_id": Uuid::nil()}),
        })
        .unwrap_err();
    assert!(mcp_error.to_string().contains(expected));

    let response = router(tools)
        .oneshot(
            Request::post(format!("/links/{}/save", Uuid::nil()))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains(expected));
}
