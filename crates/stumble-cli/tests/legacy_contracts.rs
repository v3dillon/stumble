use axum::{body::Body, http::Request};
use serde_json::{json, Value};
use std::process::Command;
use stumble_api::router;
use stumble_core::*;
use stumble_mcp::{McpToolCall, McpToolRouter};
use tower::ServiceExt;

#[tokio::test]
async fn first_release_catalogs_do_not_advertise_retired_or_placeholder_operations() {
    let tools = AgentTools::new(seed_store());

    let response = router(tools)
        .oneshot(Request::get("/openapi-lite").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let routes: Value = serde_json::from_slice(&body).unwrap();
    let serialized_routes = routes.to_string();

    for retired in [
        "crawl",
        "sources",
        "submissions",
        "briefs",
        "intake-link",
        "route-link",
    ] {
        assert!(
            !serialized_routes.contains(retired),
            "{retired} was advertised"
        );
    }
    for canonical in [
        "/candidates",
        "/discovery-tasks/:id/claim",
        "/pods/:slug/package/export",
        "/federation/pods/:slug/events",
    ] {
        assert!(
            serialized_routes.contains(canonical),
            "{canonical} was missing"
        );
    }
    for retired in [
        "crawl_pod_sources",
        "add_source_to_pod",
        "submit_link_to_pod",
        "get_pod_brief",
    ] {
        assert!(!McpToolRouter::tool_names().contains(&retired));
    }

    let cli_help = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(cli_help.status.success());
    let cli_help = String::from_utf8(cli_help.stdout).unwrap();
    for retired in ["crawl", "add-source", "submit ", "brief", "skill-pack"] {
        assert!(!cli_help.contains(retired), "{retired} was advertised");
    }
    for family in ["node", "pod", "discover", "feed", "sync"] {
        assert!(cli_help.contains(&format!("  {family}")));
    }
}

#[tokio::test]
async fn retired_crawler_contract_remains_versioned_only_on_compatible_transports() {
    let tools = AgentTools::new(seed_store());
    let ctx = tools.default_auth_context().unwrap();
    let mcp = McpToolRouter::new(tools.clone(), ctx);
    let mcp_error = mcp
        .call(McpToolCall {
            tool: "crawl_pod_sources".into(),
            arguments: json!({"pod_slug": "beautiful-interfaces"}),
        })
        .unwrap_err();
    let mcp_error: Value = serde_json::from_str(&mcp_error.to_string()).unwrap();

    let response = router(tools)
        .oneshot(
            Request::post("/pods/beautiful-interfaces/crawl")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::GONE);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let http_error: Value = serde_json::from_slice(&body).unwrap();

    let cli = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args(["crawl", "beautiful-interfaces"])
        .output()
        .unwrap();
    assert!(!cli.status.success());
    let cli_error: Value = serde_json::from_slice(&cli.stderr).unwrap();

    assert_eq!(http_error, mcp_error);
    assert_eq!(cli_error["version"], 1);
    assert_eq!(cli_error["error"]["code"], "usage_error");
    assert_eq!(http_error["code"], "legacy_contract_retired");
    assert_eq!(http_error["protocol_version"], CURRENT_PROTOCOL_VERSION);
    assert_eq!(
        http_error["replacement"],
        "discovery_tasks+submit_candidate"
    );
}

#[tokio::test]
async fn retired_submission_and_feedback_errors_are_transport_equivalent() {
    let tools = AgentTools::new(seed_store());
    let ctx = tools.default_auth_context().unwrap();
    let mcp = McpToolRouter::new(tools.clone(), ctx);

    let mcp_submission: Value = serde_json::from_str(
        &mcp.call(McpToolCall {
            tool: "submit_link_to_pod".into(),
            arguments: json!({}),
        })
        .unwrap_err()
        .to_string(),
    )
    .unwrap();
    let http_submission = router(tools.clone())
        .oneshot(
            Request::post("/pods/example/submit")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let http_submission: Value = serde_json::from_slice(
        &axum::body::to_bytes(http_submission.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let cli_submission = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args(["submit", "--pod", "example", "--url", "https://example.com"])
        .output()
        .unwrap();
    let cli_submission: Value = serde_json::from_slice(&cli_submission.stderr).unwrap();
    assert_eq!(http_submission, mcp_submission);
    assert_eq!(cli_submission["version"], 1);
    assert_eq!(cli_submission["error"]["code"], "usage_error");

    let mcp_feedback: Value = serde_json::from_str(
        &mcp.call(McpToolCall {
            tool: "save_link".into(),
            arguments: json!({}),
        })
        .unwrap_err()
        .to_string(),
    )
    .unwrap();
    let http_feedback = router(tools)
        .oneshot(
            Request::post(format!("/links/{}/save", uuid::Uuid::nil()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let http_feedback: Value = serde_json::from_slice(
        &axum::body::to_bytes(http_feedback.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let cli_feedback = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args(["block-source", "example.com"])
        .output()
        .unwrap();
    let cli_feedback: Value = serde_json::from_slice(&cli_feedback.stderr).unwrap();
    assert_eq!(http_feedback, mcp_feedback);
    assert_eq!(cli_feedback["version"], 1);
    assert_eq!(cli_feedback["error"]["code"], "usage_error");
}

#[test]
fn current_node_protocol_is_not_the_legacy_event_contract() {
    let tools = AgentTools::new(seed_store());
    let info = tools
        .node_info(&tools.default_auth_context().unwrap())
        .unwrap();

    assert_eq!(info.supported_protocol_version, CURRENT_PROTOCOL_VERSION);
    assert_ne!(CURRENT_PROTOCOL_VERSION, "stumble/0.1");
}
