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

    let output = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.path().to_str().unwrap(),
            "feed",
            "feedback",
            "record",
            &Uuid::nil().to_string(),
            "--kind",
            "save",
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &token)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let expected = "harness grant lacks feedback";
    assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["version"], 1);
    assert_eq!(error["error"]["code"], "forbidden");

    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(tools.clone(), &token).unwrap();
    let mcp_error = mcp
        .call(McpToolCall {
            tool: "record_feed_feedback".into(),
            arguments: json!({"content_item_id": Uuid::nil(), "kind": "save"}),
        })
        .unwrap_err();
    assert!(mcp_error.to_string().contains(expected));

    let response = router(tools)
        .oneshot(
            Request::post(format!("/feed/items/{}/feedback", Uuid::nil()))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"kind":"save"}"#))
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

#[tokio::test]
async fn discovery_task_adapters_return_equivalent_authorization_denials() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).unwrap();
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "feed reader".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::FeedRead],
                pod_ids: None,
            },
        )
        .unwrap();
    let token = issued.token.expose().to_string();
    drop(tools);
    let expected = "harness grant lacks discovery_tasks";

    let cli = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.path().to_str().unwrap(),
            "discover",
            "task",
            "list",
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &token)
        .output()
        .unwrap();
    assert!(!cli.status.success());
    assert!(String::from_utf8_lossy(&cli.stderr).contains(expected));
    let cli_error: serde_json::Value = serde_json::from_slice(&cli.stderr).unwrap();
    assert_eq!(cli_error["version"], 1);
    assert_eq!(cli_error["error"]["code"], "forbidden");

    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(tools.clone(), &token).unwrap();
    assert!(mcp
        .call(McpToolCall {
            tool: "list_discovery_tasks".into(),
            arguments: json!({}),
        })
        .unwrap_err()
        .to_string()
        .contains(expected));

    let response = router(tools)
        .oneshot(
            Request::get("/discovery-tasks")
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

fn package_contents() -> PodPackageContents {
    PodPackageContents {
        context_md: "# Systems\n\n## Scope\n\nReliable systems.\n".into(),
        skill_md: "# Instructions\n\nPrefer primary sources.\n".into(),
        sources_yaml: "source_rules:\n  - inspect:\n      kind: publication\n      name: official engineering blogs\n    seek:\n      description: reliability case studies\n    schedule:\n      cadence: daily\n".into(),
        filters_yaml: "blocked_topics: []\nblocked_domains: []\n".into(),
        examples_good_md: "# Good\n\n- An incident review.\n".into(),
        examples_bad_md: "# Bad\n\n- An unsourced listicle.\n".into(),
    }
}

fn create_request(slug: &str) -> CreatePrivatePodWithPackageRequest {
    CreatePrivatePodWithPackageRequest {
        name: slug.to_string(),
        slug: slug.to_string(),
        description: "Adapter acceptance Pod".to_string(),
        package: package_contents(),
    }
}

#[tokio::test]
async fn discovery_task_adapters_return_the_same_typed_target() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).unwrap();
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "discovery worker".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![
                    HarnessCapability::PodCuration,
                    HarnessCapability::PackageManagement,
                    HarnessCapability::DiscoveryTasks,
                ],
                pod_ids: None,
            },
        )
        .unwrap();
    let token = issued.token.expose().to_string();
    let worker = tools.authenticate_token(&token).unwrap().unwrap();
    let mut request = create_request("typed-target");
    request.package.sources_yaml = request.package.sources_yaml.replace("daily", "on_demand");
    let pod = tools
        .create_private_pod_with_package(&worker, request)
        .unwrap()
        .pod;
    tools
        .create_immediate_discovery_task(
            &worker,
            CreateImmediateDiscoveryTaskRequest {
                pod_id: pod.id,
                instructions: "find a primary incident report".into(),
                idempotency_key: "adapter-target".into(),
            },
            chrono::Utc::now(),
        )
        .unwrap();
    drop(tools);

    let cli = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.path().to_str().unwrap(),
            "discover",
            "task",
            "list",
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &token)
        .output()
        .unwrap();
    assert!(cli.status.success());
    let cli: serde_json::Value = serde_json::from_slice(&cli.stdout).unwrap();
    let cli_task = cli["data"]["items"][0].clone();

    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(tools.clone(), &token).unwrap();
    let mcp_task = mcp
        .call(McpToolCall {
            tool: "list_discovery_tasks".into(),
            arguments: json!({}),
        })
        .unwrap()[0]
        .clone();

    let response = router(tools)
        .oneshot(
            Request::get("/discovery-tasks")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let http: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let http_task = http[0].clone();

    assert_eq!(cli_task, mcp_task);
    assert_eq!(mcp_task, http_task);
    assert_eq!(http_task["target"]["kind"], "pod");
    assert_eq!(http_task["target"]["pod_id"], pod.id.to_string());
    assert_eq!(http_task["target"]["package_version"], 1);
    assert_eq!(http_task["pod_id"], pod.id.to_string());
    assert_eq!(http_task["package_version"], 1);
}

fn package_fixture() -> (TestDataDir, AgentTools, String) {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).unwrap();
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "package editor".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![
                    HarnessCapability::PodCuration,
                    HarnessCapability::PackageManagement,
                ],
                pod_ids: None,
            },
        )
        .unwrap();
    let token = issued.token.expose().to_string();
    (data_dir, tools, token)
}

#[test]
fn mcp_creates_reads_validates_exports_and_imports_pod_packages() {
    let (_data_dir, tools, token) = package_fixture();
    let mcp = McpToolRouter::authenticated(tools.clone(), &token).unwrap();
    let mcp_created = mcp
        .call(McpToolCall {
            tool: "create_private_pod_with_package".into(),
            arguments: serde_json::to_value(create_request("mcp-package")).unwrap(),
        })
        .unwrap();
    assert_eq!(mcp_created["pod"]["visibility"], "private");
    assert_eq!(mcp_created["package"]["version"], 1);
    let mcp_read = mcp
        .call(McpToolCall {
            tool: "get_pod_package".into(),
            arguments: json!({"pod_slug": "mcp-package"}),
        })
        .unwrap();
    assert_eq!(mcp_read["version"], 1);
    let mcp_validation = mcp
        .call(McpToolCall {
            tool: "validate_pod_package".into(),
            arguments: json!({"pod_slug": "mcp-package"}),
        })
        .unwrap();
    assert_eq!(mcp_validation["valid"], true);
    let mcp_export = mcp
        .call(McpToolCall {
            tool: "export_pod_package".into(),
            arguments: json!({"pod_slug": "mcp-package"}),
        })
        .unwrap();
    let mcp_import = mcp
        .call(McpToolCall {
            tool: "import_pod_package".into(),
            arguments: json!({"pod_slug": "mcp-package", "files": mcp_export["files"]}),
        })
        .unwrap();
    assert_eq!(mcp_import["version"], 2);
}

#[tokio::test]
async fn http_creates_reads_validates_exports_and_imports_pod_packages() {
    let (_data_dir, tools, token) = package_fixture();
    let response = router(tools.clone())
        .oneshot(
            Request::post("/pod-packages")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&create_request("http-package")).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let http_created: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_created["pod"]["visibility"], "private");
    assert_eq!(http_created["package"]["version"], 1);
    let http_read = router(tools.clone())
        .oneshot(
            Request::get("/pods/http-package/package")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(http_read.status(), axum::http::StatusCode::OK);
    let http_validation = router(tools.clone())
        .oneshot(
            Request::post("/pods/http-package/package/validate")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(http_validation.status(), axum::http::StatusCode::OK);
    let http_export = router(tools.clone())
        .oneshot(
            Request::post("/pods/http-package/package/export")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let http_export: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(http_export.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let http_import = router(tools.clone())
        .oneshot(
            Request::post("/pods/http-package/package/import")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&http_export["files"]).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(http_import.status(), axum::http::StatusCode::OK);
    let http_import: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(http_import.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(http_import["version"], 2);
}

#[test]
fn cli_rejects_extra_files_then_round_trips_pod_package() {
    let (data_dir, tools, token) = package_fixture();
    let package_dir = data_dir.path().join("portable-package");
    std::fs::create_dir_all(&package_dir).unwrap();
    let files = export_skill_pack(
        &PodSkillPack {
            id: Uuid::nil(),
            pod_id: Uuid::nil(),
            version: 1,
            context_md: package_contents().context_md,
            pod_yaml: String::new(),
            skill_md: package_contents().skill_md,
            sources_yaml: package_contents().sources_yaml,
            filters_yaml: package_contents().filters_yaml,
            examples_good_md: package_contents().examples_good_md,
            examples_bad_md: package_contents().examples_bad_md,
            owner_id: None,
            proposer_harness_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        },
        String::new(),
    );
    for (name, contents) in files.files {
        std::fs::write(package_dir.join(name), contents).unwrap();
    }
    drop(tools);
    let unsupported = package_dir.join("harness-grants.json");
    std::fs::write(&unsupported, "[]").unwrap();
    let rejected = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.path().to_str().unwrap(),
            "pod",
            "create",
            "--name",
            "Rejected package",
            "--slug",
            "rejected-package",
            "--visibility",
            "private",
            "--package",
            package_dir.to_str().unwrap(),
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &token)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("unsupported"));
    std::fs::remove_file(unsupported).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            data_dir.path().to_str().unwrap(),
            "pod",
            "create",
            "--name",
            "CLI package",
            "--slug",
            "cli-package",
            "--visibility",
            "private",
            "--package",
            package_dir.to_str().unwrap(),
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", &token)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli_created: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(cli_created["version"], 1);
    assert_eq!(cli_created["data"]["status"], "created");
    assert_eq!(cli_created["data"]["result"]["visibility"], "private");

    let run_cli = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_stumble"))
            .args(["--data-dir", data_dir.path().to_str().unwrap()])
            .env("STUMBLE_HARNESS_CREDENTIAL", &token)
            .args(arguments)
            .output()
            .unwrap()
    };
    let read = run_cli(&["pod", "package", "show", "cli-package"]);
    assert!(read.status.success());
    let read: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(read["version"], 1);
    let validation = run_cli(&[
        "pod",
        "package",
        "validate",
        "--package",
        package_dir.to_str().unwrap(),
    ]);
    assert!(validation.status.success());
    let validation: serde_json::Value = serde_json::from_slice(&validation.stdout).unwrap();
    assert_eq!(validation["version"], 1);
    assert_eq!(validation["data"]["valid"], true);
    let export_dir = data_dir.path().join("cli-export");
    let export = run_cli(&[
        "pod",
        "package",
        "export",
        "cli-package",
        "--output",
        export_dir.to_str().unwrap(),
    ]);
    assert!(export.status.success());
    let import = run_cli(&[
        "pod",
        "package",
        "revise",
        "cli-package",
        "--base-version",
        "1",
        "--package",
        export_dir.to_str().unwrap(),
    ]);
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );
    let import: serde_json::Value = serde_json::from_slice(&import.stdout).unwrap();
    assert_eq!(import["version"], 1);
    assert_eq!(import["data"]["status"], "revised");
    assert_eq!(import["data"]["package"]["version"], 2);
}
