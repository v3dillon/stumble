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
            tool: "get_pod_skill".into(),
            arguments: json!({"pod_slug": "mcp-package"}),
        })
        .unwrap();
    assert_eq!(mcp_read["version"], 1);
    let mcp_validation = mcp
        .call(McpToolCall {
            tool: "validate_pod_skill_pack".into(),
            arguments: json!({"pod_slug": "mcp-package"}),
        })
        .unwrap();
    assert_eq!(mcp_validation["valid"], true);
    let mcp_export = mcp
        .call(McpToolCall {
            tool: "export_pod_skill_pack".into(),
            arguments: json!({"pod_slug": "mcp-package"}),
        })
        .unwrap();
    let mcp_import = mcp
        .call(McpToolCall {
            tool: "import_pod_skill_pack".into(),
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
            Request::get("/pods/http-package/skill-pack")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(http_read.status(), axum::http::StatusCode::OK);
    let http_validation = router(tools.clone())
        .oneshot(
            Request::post("/pods/http-package/skill-pack/validate")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(http_validation.status(), axum::http::StatusCode::OK);
    let http_export = router(tools.clone())
        .oneshot(
            Request::post("/pods/http-package/skill-pack/export")
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
            Request::post("/pods/http-package/skill-pack/import")
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
    let rejected = Command::new(env!("CARGO_BIN_EXE_podctl"))
        .args([
            "--data-dir",
            data_dir.path().to_str().unwrap(),
            "--token",
            &token,
            "create-pod-package",
            "--name",
            "Rejected package",
            "--slug",
            "rejected-package",
            "--from",
            package_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("unsupported"));
    std::fs::remove_file(unsupported).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_podctl"))
        .args([
            "--data-dir",
            data_dir.path().to_str().unwrap(),
            "--token",
            &token,
            "create-pod-package",
            "--name",
            "CLI package",
            "--slug",
            "cli-package",
            "--from",
            package_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli_created: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(cli_created["pod"]["visibility"], "private");
    assert_eq!(cli_created["package"]["version"], 1);

    let run_cli = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_podctl"))
            .args([
                "--data-dir",
                data_dir.path().to_str().unwrap(),
                "--token",
                &token,
            ])
            .args(arguments)
            .output()
            .unwrap()
    };
    let read = run_cli(&["get-skill-pack", "cli-package"]);
    assert!(read.status.success());
    let read: serde_json::Value = serde_json::from_slice(&read.stdout).unwrap();
    assert_eq!(read["version"], 1);
    let validation = run_cli(&["validate-skill-pack", "cli-package"]);
    assert!(validation.status.success());
    let validation: serde_json::Value = serde_json::from_slice(&validation.stdout).unwrap();
    assert_eq!(validation["valid"], true);
    let export_dir = data_dir.path().join("cli-export");
    let export = run_cli(&[
        "export-skill-pack",
        "cli-package",
        export_dir.to_str().unwrap(),
    ]);
    assert!(export.status.success());
    let import = run_cli(&[
        "import-skill-pack",
        "cli-package",
        export_dir.to_str().unwrap(),
    ]);
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );
    let import: serde_json::Value = serde_json::from_slice(&import.stdout).unwrap();
    assert_eq!(import["version"], 2);
}
