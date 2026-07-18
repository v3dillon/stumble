use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};
use stumble_core::{seed_store, AgentTools};
use uuid::Uuid;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("stumble-api-process-{}", Uuid::now_v7())))
    }

    fn initialize(&self) {
        AgentTools::initialize_home_node(&self.0, seed_store).expect("initialize Home Node");
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn api_process_serves_http_and_keeps_diagnostics_off_stdout() {
    let data_dir = TestDataDir::new();
    data_dir.initialize();
    let mut child = Command::new(env!("CARGO_BIN_EXE_stumble-api"))
        .args([
            "--data-dir",
            data_dir.0.to_str().expect("UTF-8 test path"),
            "--bind",
            "127.0.0.1:0",
            "--disable-hub-refresh",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start stumble-api");
    let mut stderr = BufReader::new(child.stderr.take().expect("child stderr"));
    let mut first_diagnostic = String::new();
    stderr
        .read_line(&mut first_diagnostic)
        .expect("read listener diagnostic");
    let address = first_diagnostic
        .split("http://")
        .nth(1)
        .expect("diagnostic includes listener URL")
        .trim();

    let response = reqwest::get(format!("http://{address}/health"))
        .await
        .expect("request running stumble-api");
    assert!(response.status().is_success());
    let body: Value = response.json().await.expect("health JSON");
    assert_eq!(body["status"], "ok");

    child.kill().expect("stop stumble-api");
    let output = child.wait_with_output().expect("collect process output");
    assert!(output.stdout.is_empty());
    let mut remaining_stderr = String::new();
    stderr
        .read_to_string(&mut remaining_stderr)
        .expect("read diagnostics");
    assert!(first_diagnostic.contains("stumble-api running"));
}

#[test]
fn api_process_refuses_to_initialize_an_empty_home_node() {
    let data_dir = TestDataDir::new();
    let output = Command::new(env!("CARGO_BIN_EXE_stumble-api"))
        .args([
            "--data-dir",
            data_dir.0.to_str().expect("UTF-8 test path"),
            "--disable-hub-refresh",
        ])
        .output()
        .expect("run stumble-api");

    assert!(!output.status.success());
    assert!(!data_dir.0.join("stumble.sqlite3").exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Home Node is not initialized"));
}
