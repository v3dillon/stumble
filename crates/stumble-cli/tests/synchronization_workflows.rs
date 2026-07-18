use serde_json::{json, Value};
use std::{fs, path::PathBuf, process::Command};
use stumble_api::router_with_base_url;
use stumble_core::{seed_store, AgentTools, CreatePodRequest, Visibility};

struct Environment {
    root: PathBuf,
    data_dir: PathBuf,
}

impl Environment {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "stumble-synchronization-workflows-{}",
            uuid::Uuid::now_v7()
        ));
        let data_dir = root.join("home");
        fs::create_dir_all(&root).unwrap();
        let environment = Self { root, data_dir };
        environment.run(&["node", "init"]);
        environment
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_stumble"));
        command.env("STUMBLE_DATA_DIR", &self.data_dir).env(
            "STUMBLE_CREDENTIAL_STORE_DIR",
            self.root.join("credentials"),
        );
        command
    }

    fn run(&self, arguments: &[&str]) -> Value {
        self.run_with_credential(arguments, None)
    }

    fn run_with_credential(&self, arguments: &[&str], credential: Option<&str>) -> Value {
        let mut command = self.command();
        command.args(arguments);
        if let Some(credential) = credential {
            command.env("STUMBLE_HARNESS_CREDENTIAL", credential);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trusted_peer_and_selected_subscription_use_high_level_sync_workflows() {
    let environment = Environment::new();
    let origin = AgentTools::new(seed_store());
    let origin_actor = origin.default_auth_context().unwrap();
    let origin_node = origin.node_info(&origin_actor).unwrap();
    let mut origin_pod = origin
        .create_pod(
            &origin_actor,
            CreatePodRequest {
                name: "Origin systems".into(),
                slug: "origin-systems".into(),
                description: "Signed systems research".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    origin_pod.visibility = Visibility::Public;
    {
        let store = origin.store();
        let mut store = store.write().unwrap();
        store.pods.insert(origin_pod.id, origin_pod.clone());
        store
            .pod_rules
            .get_mut(&origin_pod.id)
            .unwrap()
            .federate_sources = true;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let router = router_with_base_url(origin, &base_url);
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let subscribed = environment.run(&[
        "pod",
        "subscribe",
        &format!("{base_url}/federation/pods/{}", origin_pod.slug),
    ]);
    let local_pod_id = subscribed["data"]["pod_id"].as_str().unwrap();

    let harness = environment.run(&[
        "node",
        "harness",
        "register",
        "--label",
        "trust operator",
        "--kind",
        "interactive",
        "--capability",
        "administration",
    ]);
    let credential = harness["data"]["credential"].as_str().unwrap();

    let proposed = environment.run_with_credential(
        &[
            "sync",
            "peer",
            "add",
            "--node-id",
            &origin_node.node_id.to_string(),
            "--display-name",
            &origin_node.display_name,
            "--base-url",
            &base_url,
            "--public-key",
            &origin_node.public_key,
        ],
        Some(credential),
    );
    assert_eq!(proposed["data"]["status"], "pending_approval");
    assert_eq!(proposed["data"]["node_id"], origin_node.node_id.to_string());
    let proposal_id = proposed["data"]["proposal"]["id"].as_str().unwrap();
    environment.run(&["node", "proposal", "approve", proposal_id]);

    let peers = environment.run(&["sync", "peer", "list", "--limit", "1"]);
    let peer = &peers["data"]["items"][0];
    assert_eq!(peer["node_id"], origin_node.node_id.to_string());
    assert!(peer.get("allowed_actions").is_none());
    let peer_id = peer["id"].as_str().unwrap();

    let run = environment.run(&["sync", "pod", "run", local_pod_id, "--peer", peer_id]);
    assert_eq!(run["data"]["pod_id"], local_pod_id);
    assert_eq!(run["data"]["slug"], origin_pod.slug);
    assert_eq!(run["data"]["peer_id"], peer_id);
    assert_eq!(run["data"]["verification"], "verified");

    let status = environment.run(&["sync", "pod", "status", local_pod_id]);
    assert_eq!(status["data"]["pod_id"], local_pod_id);
    assert_eq!(status["data"]["slug"], origin_pod.slug);
    assert_eq!(status["data"]["verification"], "verified");
    assert!(status["data"]["cursor"].is_string());
    assert!(status["data"]["latest_event"].is_string());
    assert!(status["data"]["last_success"].is_string());
    assert_eq!(status["data"]["failure"], Value::Null);
    assert_eq!(status["data"]["allowed_actions"], json!(["run"]));

    server.abort();
    let _ = server.await;
    let failed = environment
        .command()
        .args(["sync", "pod", "run", local_pod_id, "--peer", peer_id])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let failed_status = environment.run(&["sync", "pod", "status", local_pod_id]);
    assert_eq!(
        failed_status["data"]["failure"]["code"],
        "synchronization_failed"
    );
    assert_eq!(failed_status["data"]["failure"]["retryable"], true);
    assert_eq!(failed_status["data"]["failure"]["action"], "run");

    let removal =
        environment.run_with_credential(&["sync", "peer", "remove", peer_id], Some(credential));
    assert_eq!(removal["data"]["status"], "pending_approval");
    assert_eq!(removal["data"]["node_id"], origin_node.node_id.to_string());
    let removal_id = removal["data"]["proposal"]["id"].as_str().unwrap();
    environment.run(&["node", "proposal", "approve", removal_id]);
    assert!(environment.run(&["sync", "peer", "list"])["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|peer| peer["id"] != peer_id));
    assert_eq!(
        environment.run(&["sync", "pod", "status", local_pod_id])["data"]["allowed_actions"],
        json!([])
    );
}

#[test]
fn event_file_operations_are_not_public_sync_commands() {
    let environment = Environment::new();
    for operation in ["export", "import", "verify"] {
        let output = environment
            .command()
            .args(["sync", "pod", operation])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "usage_error");
    }
}
