use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    time::Duration,
};
use stumble_core::{
    seed_store, AgentHarnessKind, AgentTools, HarnessCapability, RegisterAgentHarnessRequest,
};
use uuid::Uuid;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("stumble-mcp-process-{}", Uuid::now_v7())))
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

fn stdio_process(data_dir: &TestDataDir, token: &str) -> Child {
    Command::new(env!("CARGO_BIN_EXE_stumble-mcp"))
        .args([
            "--data-dir",
            data_dir.path().to_str().expect("UTF-8 test path"),
            "--transport",
            "stdio",
        ])
        .env("STUMBLE_MCP_TOKEN", token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start stumble-mcp stdio")
}

fn http_request(address: std::net::SocketAddr, token: &str, body: &Value) -> String {
    // Generous deadline: under full-workspace test load the child process can
    // take seconds to spawn and bind.
    let mut stream = (0..200)
        .find_map(|_| match TcpStream::connect(address) {
            Ok(stream) => Some(stream),
            Err(_) => {
                std::thread::sleep(Duration::from_millis(50));
                None
            }
        })
        .expect("connect to stumble-mcp HTTP process");
    let body = body.to_string();
    write!(
        stream,
        "POST /mcp HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: 2025-06-18\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("write HTTP request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read HTTP response");
    response
}

#[test]
fn streamable_http_process_authenticates_and_scopes_tool_discovery() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).expect("open Home Node");
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().expect("owner context"),
            RegisterAgentHarnessRequest {
                label: "HTTP Feed reader".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::FeedRead],
                pod_ids: None,
            },
        )
        .expect("register scoped harness");
    let token = issued.token.expose().to_owned();
    drop(tools);

    let probe = TcpListener::bind("127.0.0.1:0").expect("reserve test port");
    let address = probe.local_addr().expect("test address");
    drop(probe);
    let mut child = Command::new(env!("CARGO_BIN_EXE_stumble-mcp"))
        .args([
            "--data-dir",
            data_dir.path().to_str().expect("UTF-8 test path"),
            "--bind",
            &address.to_string(),
            "--transport",
            "http",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start stumble-mcp HTTP");

    let unauthorized = http_request(
        address,
        "not-a-current-token",
        &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}),
    );
    assert!(unauthorized.starts_with("HTTP/1.1 401 Unauthorized"));
    let authorized = http_request(
        address,
        &token,
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    );
    assert!(authorized.starts_with("HTTP/1.1 200 OK"));
    let body = authorized
        .split_once("\r\n\r\n")
        .expect("HTTP response body")
        .1;
    let response: Value = serde_json::from_str(body).expect("JSON-RPC body");
    let names = response["result"]["tools"]
        .as_array()
        .expect("tool descriptors")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<Vec<_>>();
    assert!(names.contains(&"get_feed_batch"));
    assert!(!names.contains(&"submit_candidate"));

    child.kill().expect("stop stumble-mcp HTTP");
    let output = child
        .wait_with_output()
        .expect("collect HTTP process output");
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("HTTP server listening"));
}

#[test]
fn stdio_process_authenticates_scopes_calls_and_keeps_stdout_protocol_clean() {
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

    let mut child = stdio_process(&data_dir, &token);
    let mut stdin = child.stdin.take().expect("child stdin");
    for message in [
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "process-test", "version": "1"}
            }
        }),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}}),
        json!({"jsonrpc": "2.0", "id": "tools", "method": "tools/list", "params": {}}),
        json!({
            "jsonrpc": "2.0",
            "id": "pods",
            "method": "tools/call",
            "params": {"name": "list_pods", "arguments": {}}
        }),
    ] {
        writeln!(stdin, "{message}").expect("write MCP request");
    }
    drop(stdin);

    let output = child.wait_with_output().expect("read stumble-mcp output");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses = String::from_utf8(output.stdout)
        .expect("UTF-8 MCP output")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("JSON-RPC response line"))
        .collect::<Vec<_>>();
    assert_eq!(
        responses.len(),
        3,
        "notifications produce no protocol output"
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
    assert!(String::from_utf8_lossy(&output.stderr).contains("stdio"));
}

#[test]
fn stdio_process_reauthenticates_and_stops_cleanly_after_revocation() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).expect("open Home Node");
    let owner = tools.default_auth_context().expect("owner context");
    let issued = tools
        .register_agent_harness(
            &owner,
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

    let mut child = stdio_process(&data_dir, &token);
    let mut stdin = child.stdin.take().expect("child stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("child stdout"));
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}})
    )
    .expect("write initial request");
    let mut first_response = String::new();
    stdout
        .read_line(&mut first_response)
        .expect("read initial response");
    assert!(first_response.contains("submit_candidate"));

    let tools = AgentTools::open_initialized_home_node(data_dir.path()).expect("reopen Home Node");
    tools
        .revoke_agent_harness(
            &tools.default_auth_context().expect("owner context"),
            harness_id,
        )
        .expect("revoke harness");
    drop(tools);
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    )
    .expect("write request after revocation");
    drop(stdin);

    let status = child.wait().expect("wait for stumble-mcp");
    let mut remaining_stdout = String::new();
    stdout
        .read_to_string(&mut remaining_stdout)
        .expect("read remaining stdout");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("child stderr")
        .read_to_string(&mut stderr)
        .expect("read stderr");
    assert!(!status.success());
    assert!(remaining_stdout.is_empty());
    assert!(stderr.contains("invalid or revoked Harness token"));
}

#[test]
fn stdio_process_requires_a_harness_token_without_writing_stdout() {
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
        .expect("run stumble-mcp stdio without a token");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires a Harness token"));
}

#[test]
fn http_process_refuses_to_initialize_an_empty_home_node() {
    let data_dir = TestDataDir::new();
    let output = Command::new(env!("CARGO_BIN_EXE_stumble-mcp"))
        .args([
            "--data-dir",
            data_dir.path().to_str().expect("UTF-8 test path"),
            "--transport",
            "http",
        ])
        .output()
        .expect("run stumble-mcp HTTP");

    assert!(!output.status.success());
    assert!(!data_dir.path().join("stumble.sqlite3").exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Home Node is not initialized"));
}
