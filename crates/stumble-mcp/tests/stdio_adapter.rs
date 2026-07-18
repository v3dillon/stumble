use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
};
use stumble_core::{
    seed_store, AgentHarnessKind, AgentTools, HarnessCapability, RegisterAgentHarnessRequest,
};
use uuid::Uuid;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("stumble-cli-mcp-{}", Uuid::now_v7())))
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

#[test]
fn dedicated_mcp_serves_scoped_jsonrpc_over_stdio() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).expect("open Home Node");
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().expect("owner context"),
            RegisterAgentHarnessRequest {
                label: "stdio Candidate submitter".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::CandidateSubmission],
                pod_ids: None,
            },
        )
        .expect("register scoped harness");
    let token = issued.token.expose().to_owned();
    drop(tools);

    let mut child = Command::new(env!("CARGO_BIN_EXE_stumble-mcp"))
        .args([
            "--data-dir",
            data_dir.path().to_str().expect("UTF-8 test path"),
            "--transport",
            "stdio",
        ])
        .env("STUMBLE_MCP_TOKEN", &token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start dedicated MCP adapter");
    let mut stdin = child.stdin.take().expect("child stdin");
    writeln!(
        stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "process-test", "version": "1"}
            }
        })
    )
    .expect("write initialize request");
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})
    )
    .expect("write initialized notification");
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": "tools", "method": "tools/list", "params": {}})
    )
    .expect("write tools/list request");
    writeln!(
        stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": "pods",
            "method": "tools/call",
            "params": {"name": "list_pods", "arguments": {}}
        })
    )
    .expect("write list_pods tool call");
    drop(stdin);

    let output = child.wait_with_output().expect("read dedicated MCP output");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("\"jsonrpc\""));
    let responses = String::from_utf8(output.stdout)
        .expect("UTF-8 MCP output")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("one JSON-RPC response per line"))
        .collect::<Vec<_>>();
    assert_eq!(
        responses.len(),
        3,
        "notifications must not produce responses"
    );
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-06-18");
    let names = responses[1]["result"]["tools"]
        .as_array()
        .expect("tool descriptors")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<Vec<_>>();
    assert!(names.contains(&"submit_candidate"));
    assert!(!names.contains(&"get_feed_batch"));
    assert!(!names.contains(&"record_feed_feedback"));
    assert_eq!(
        responses[2]["result"]["structuredContent"],
        json!({"value": []})
    );
}

#[test]
fn dedicated_mcp_requires_a_current_harness_token() {
    let data_dir = TestDataDir::new();
    AgentTools::open_home_node(data_dir.path(), seed_store).expect("initialize Home Node");

    let output = Command::new(env!("CARGO_BIN_EXE_stumble-mcp"))
        .args([
            "--data-dir",
            data_dir.path().to_str().expect("UTF-8 test path"),
            "--transport",
            "stdio",
        ])
        .env_remove("STUMBLE_MCP_TOKEN")
        .output()
        .expect("run dedicated MCP adapter without a token");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires a Harness token"));
}

#[test]
fn dedicated_mcp_observes_harness_revocation_without_a_restart() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).expect("open Home Node");
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().expect("owner context"),
            RegisterAgentHarnessRequest {
                label: "revocable stdio harness".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::CandidateSubmission],
                pod_ids: None,
            },
        )
        .expect("register scoped harness");
    let harness_id = issued.harness.id;
    let token = issued.token.expose().to_owned();
    drop(tools);

    let mut child = Command::new(env!("CARGO_BIN_EXE_stumble-mcp"))
        .args([
            "--data-dir",
            data_dir.path().to_str().expect("UTF-8 test path"),
            "--transport",
            "stdio",
        ])
        .env("STUMBLE_MCP_TOKEN", &token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start dedicated MCP adapter");
    let mut stdin = child.stdin.take().expect("child stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("child stdout"));
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}})
    )
    .expect("write initial tools/list request");
    let mut initial_response = String::new();
    stdout
        .read_line(&mut initial_response)
        .expect("read initial tools/list response");
    assert!(initial_response.contains("submit_candidate"));

    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).expect("reopen Home Node");
    tools
        .revoke_agent_harness(
            &tools.default_auth_context().expect("owner context"),
            harness_id,
        )
        .expect("revoke harness through the domain boundary");
    drop(tools);

    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    )
    .expect("write tools/list request after revocation");
    drop(stdin);
    let status = child.wait().expect("wait for revoked MCP process");
    let mut remaining_stdout = String::new();
    stdout
        .read_to_string(&mut remaining_stdout)
        .expect("read remaining MCP stdout");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("child stderr")
        .read_to_string(&mut stderr)
        .expect("read MCP stderr");

    assert!(!status.success());
    assert!(remaining_stdout.is_empty());
    assert!(stderr.contains("invalid or revoked Harness token"));
}
