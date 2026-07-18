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

fn podctl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_podctl"))
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("output should be one JSON document")
}

#[test]
fn exposes_only_the_five_workflow_families() {
    let output = stumble().arg("--help").output().expect("run stumble");

    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("help should be UTF-8");
    for family in ["node", "pod", "discover", "feed", "sync"] {
        assert!(
            help.contains(&format!("  {family}")),
            "missing {family}: {help}"
        );
    }
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
        .args(["pod", "list", "--limit", "25", "--cursor", "opaque-page-2"])
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
}

#[test]
fn structured_input_reads_a_file_or_stdin_and_reports_validation_errors() {
    let mut path = std::env::temp_dir();
    path.push(format!("stumble-shell-input-{}.json", std::process::id()));
    std::fs::write(&path, r#"{"url":"https://example.com"}"#).expect("write input");
    let from_file = stumble()
        .args(["discover", "candidate", "submit", "--input"])
        .arg(&path)
        .output()
        .expect("run file input");
    std::fs::remove_file(&path).expect("remove input");
    assert!(
        from_file.status.success(),
        "{}",
        String::from_utf8_lossy(&from_file.stderr)
    );
    assert_eq!(
        json(&from_file.stdout)["data"]["input"]["url"],
        "https://example.com"
    );

    let mut child = stumble()
        .args(["discover", "candidate", "submit", "--input", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stdin input");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"url":"https://example.org"}"#)
        .expect("write stdin");
    let from_stdin = child.wait_with_output().expect("finish stdin input");
    assert!(from_stdin.status.success());
    assert_eq!(
        json(&from_stdin.stdout)["data"]["input"]["url"],
        "https://example.org"
    );

    let invalid = stumble()
        .args(["discover", "candidate", "submit", "--input", "-"])
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
fn legacy_executable_remains_the_unchanged_expand_phase_bridge() {
    let output = podctl().arg("--help").output().expect("run podctl help");

    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("help should be UTF-8");
    assert!(help.starts_with("Stumble local/admin CLI"));
    assert!(help.contains("  init-node"));
    assert!(help.contains("  create-pod"));
}
