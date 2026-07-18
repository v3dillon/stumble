use serde_json::Value;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::OnceLock,
};

fn stumble() -> Command {
    static ENVIRONMENT: OnceLock<PathBuf> = OnceLock::new();
    let root = ENVIRONMENT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!(
            "stumble-shell-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let output = Command::new(env!("CARGO_BIN_EXE_stumble"))
            .env("STUMBLE_DATA_DIR", root.join("home"))
            .env("STUMBLE_CREDENTIAL_STORE_DIR", root.join("credentials"))
            .args(["node", "init"])
            .output()
            .expect("initialize shell test Home Node");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        root
    });
    let mut command = Command::new(env!("CARGO_BIN_EXE_stumble"));
    command
        .env("STUMBLE_DATA_DIR", root.join("home"))
        .env("STUMBLE_CREDENTIAL_STORE_DIR", root.join("credentials"));
    command
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("output should be one JSON document")
}

#[test]
fn exposes_only_the_five_workflow_families() {
    let output = stumble().arg("--help").output().expect("run stumble");

    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("help should be UTF-8");
    let commands = help
        .split("Commands:\n")
        .nth(1)
        .expect("help should contain a Commands section")
        .split("\n\nOptions:")
        .next()
        .unwrap()
        .lines()
        .map(|line| line.split_whitespace().next().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(commands, ["node", "pod", "discover", "feed", "sync"]);
    for retired in [
        "serve",
        "mcp",
        "init-node",
        "create-pod",
        "submit-candidate",
    ] {
        assert!(
            !help.contains(&format!("  {retired}")),
            "retired command {retired} leaked: {help}"
        );
    }
}

#[test]
fn removed_public_operations_are_ordinary_usage_errors() {
    for arguments in [
        &["serve"][..],
        &["mcp"][..],
        &["--api", "http://127.0.0.1:8787"][..],
        &["create-tenant", "example", "example"][..],
        &["create-dev-token", "owner"][..],
        &["propose-change", "--input", "-"][..],
        &["materialize-discovery-tasks"][..],
        &["list-ready-discovery-tasks"][..],
        &["export-events", "example"][..],
        &["import-events", "events.jsonl"][..],
        &["verify-events", "example"][..],
        &["submit", "--pod", "example", "--url", "https://example.com"][..],
        &["crawl", "example"][..],
        &["discover", "example"][..],
        &["stumble"][..],
        &["brief"][..],
        &["block-source", "example.com"][..],
    ] {
        let output = stumble()
            .args(arguments)
            .output()
            .expect("run removed operation");
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        let error = json(&output.stderr);
        assert_eq!(error["version"], 1, "{arguments:?}");
        assert_eq!(error["error"]["code"], "usage_error", "{arguments:?}");
        assert_ne!(
            error["error"]["code"], "legacy_contract_retired",
            "{arguments:?}"
        );
    }
}

#[test]
fn rejects_long_running_and_remote_transport_modes() {
    for arguments in [
        ["serve", ""],
        ["mcp", ""],
        ["--api", "http://127.0.0.1:8787"],
    ] {
        let arguments = arguments
            .iter()
            .filter(|argument| !argument.is_empty())
            .copied()
            .collect::<Vec<_>>();
        let output = stumble()
            .args(&arguments)
            .output()
            .expect("run rejected transport mode");
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        assert_eq!(json(&output.stderr)["error"]["code"], "usage_error");
    }
}

#[test]
fn resource_first_paths_are_discoverable() {
    let cases = [
        (
            ["node", "harness", "--help"].as_slice(),
            ["list", "show", "register", "revoke"].as_slice(),
        ),
        (
            ["pod", "content", "--help"].as_slice(),
            ["list", "show", "add", "remove"].as_slice(),
        ),
        (
            ["discover", "candidate", "--help"].as_slice(),
            ["list", "submit", "show", "evaluate", "route", "review"].as_slice(),
        ),
        (
            ["feed", "batch", "--help"].as_slice(),
            ["get", "complete"].as_slice(),
        ),
        (
            ["sync", "pod", "--help"].as_slice(),
            ["run", "status"].as_slice(),
        ),
    ];

    for (path, words) in cases {
        let output = stumble().args(path).output().expect("run nested help");
        assert!(
            output.status.success(),
            "{path:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let help = String::from_utf8(output.stdout).expect("help should be UTF-8");
        for word in words {
            assert!(
                help.contains(&format!("  {word}")),
                "missing {word} from {path:?}: {help}"
            );
            assert!(!word.contains('-'));
        }
    }
}

#[test]
fn success_and_usage_failure_use_versioned_json_envelopes() {
    let success = stumble()
        .args(["node", "show"])
        .output()
        .expect("run node show");
    assert!(success.status.success());
    assert!(success.stderr.is_empty());
    let success_json = json(&success.stdout);
    assert_eq!(success_json["version"], 1);
    assert!(success_json["data"]["data_dir"].as_str().is_some());
    assert!(success_json["data"]["node"]["node_id"].as_str().is_some());

    let failure = stumble()
        .arg("retired-command")
        .output()
        .expect("run invalid command");
    assert_eq!(failure.status.code(), Some(2));
    assert!(failure.stdout.is_empty());
    let failure_json = json(&failure.stderr);
    assert_eq!(failure_json["version"], 1);
    assert_eq!(failure_json["error"]["code"], "usage_error");
    assert!(failure_json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("retired-command"));
}

#[test]
fn list_results_use_the_shared_cursor_page_shape() {
    let output = stumble()
        .args(["pod", "list", "--limit", "25"])
        .output()
        .expect("run pod list");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body = json(&output.stdout);
    assert_eq!(body["data"]["items"], serde_json::json!([]));
    assert_eq!(body["data"]["next_cursor"], Value::Null);

    let invalid = stumble()
        .args(["pod", "list", "--cursor", "opaque-page-2"])
        .output()
        .expect("reject invalid cursor");
    assert_eq!(invalid.status.code(), Some(4));
    assert_eq!(json(&invalid.stderr)["error"]["code"], "invalid_cursor");
}

#[test]
fn candidate_submission_requires_an_idempotency_key_and_reports_invalid_json() {
    let missing_key = stumble()
        .args(["discover", "candidate", "submit", "--input", "-"])
        .output()
        .expect("require idempotency key");
    assert_eq!(missing_key.status.code(), Some(2));
    assert_eq!(json(&missing_key.stderr)["error"]["code"], "usage_error");
    let invalid = stumble()
        .args([
            "discover",
            "candidate",
            "submit",
            "--input",
            "-",
            "--idempotency-key",
            "invalid-json",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn invalid input");
    invalid
        .stdin
        .as_ref()
        .unwrap()
        .write_all(b"not-json")
        .expect("write invalid input");
    let invalid = invalid.wait_with_output().expect("finish invalid input");
    assert_eq!(invalid.status.code(), Some(4));
    assert!(invalid.stdout.is_empty());
    assert_eq!(json(&invalid.stderr)["error"]["code"], "invalid_input");
}

#[test]
fn text_format_renders_the_same_result_data() {
    let output = stumble()
        .args(["--format", "text", "node", "show"])
        .output()
        .expect("run text output");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).expect("text should be UTF-8");
    assert!(text.contains("data_dir:"));
    assert!(text.contains("node_id:"));
}

#[test]
fn old_executable_is_absent_from_the_package_surface() {
    let package = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(package.join("Cargo.toml")).unwrap();

    assert!(!manifest.contains("name = \"podctl\""));
    assert!(!package.join("src/main.rs").exists());
}
