use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use stumble_core::{
    seed_store, AgentHarnessKind, AgentTools, HarnessCapability, RegisterAgentHarnessRequest,
};

#[test]
fn runner_exposes_one_harness_neutral_command_surface() {
    let output = Command::new(env!("CARGO_BIN_EXE_stumble-runner"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("mcp"));
    assert!(help.contains("serve"));
    assert!(help.contains("discovery"));
    assert!(help.contains("curate"));
    assert!(help.contains("--config"));
}

#[test]
fn runner_validates_generic_agent_templates_before_touching_the_node() {
    let root = std::env::temp_dir().join(format!("stumble-runner-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).unwrap();
    let config = root.join("runner.yaml");
    std::fs::write(
        &config,
        format!(
            "version: 1\ndata_dir: {}\ncredentials:\n  worker:\n    command:\n      program: /usr/bin/printf\n      args: [token]\nagents:\n  portable:\n    program: /usr/bin/true\n    args: []\nworkers:\n  pod:\n    credential: worker\n    agent: portable\n    prompt: Find relevant material.\n",
            root.join("node").display()
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_stumble-runner"))
        .args(["--config", config.to_str().unwrap(), "discovery", "pod"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("{prompt}"), "{stderr}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unified_http_daemon_requires_the_callers_harness_token() {
    let root = std::env::temp_dir().join(format!("stumble-runner-http-{}", uuid::Uuid::now_v7()));
    let node = root.join("node");
    std::fs::create_dir_all(&node).unwrap();
    let tools = AgentTools::open_home_node(&node, seed_store).unwrap();
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "HTTP runner test".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::FeedRead],
                pod_ids: None,
            },
        )
        .unwrap();
    let token = issued.token.expose().to_owned();
    drop(tools);

    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = probe.local_addr().unwrap();
    drop(probe);
    let config = root.join("runner.yaml");
    std::fs::write(
        &config,
        format!(
            "version: 1\ndata_dir: {}\nbind: {}\ncredentials: {{}}\n",
            node.display(),
            address
        ),
    )
    .unwrap();
    let _child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_stumble-runner"))
            .args(["--config", config.to_str().unwrap(), "serve"])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    wait_for_listener(address);

    let unauthorized = initialize_request(address, None);
    assert!(unauthorized.starts_with("HTTP/1.1 401 Unauthorized"));
    let node_ops = get_request(address, "/home/bootstrap/status", None);
    assert!(node_ops.starts_with("HTTP/1.1 401 Unauthorized"));
    let authorized = initialize_request(address, Some(&token));
    assert!(authorized.starts_with("HTTP/1.1 200 OK"));
    let unauthorized_node_scope = get_request(address, "/home/bootstrap/status", Some(&token));
    assert!(unauthorized_node_scope.starts_with("HTTP/1.1 403 Forbidden"));

    let _ = std::fs::remove_dir_all(root);
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_listener(address: SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("runner did not listen on {address}");
}

fn initialize_request(address: SocketAddr, token: Option<&str>) -> String {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;
    let authorization = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    let mut stream = TcpStream::connect(address).unwrap();
    write!(
        stream,
        "POST /mcp HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nConnection: close\r\n{authorization}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn get_request(address: SocketAddr, path: &str, token: Option<&str>) -> String {
    let authorization = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    let mut stream = TcpStream::connect(address).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n{authorization}\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}
