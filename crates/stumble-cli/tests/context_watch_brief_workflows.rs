use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};

struct Environment {
    root: PathBuf,
    data_dir: PathBuf,
}

impl Environment {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "stumble-context-watch-brief-cli-{}",
            uuid::Uuid::now_v7()
        ));
        let data_dir = root.join("home");
        fs::create_dir_all(&root).unwrap();
        let environment = Self { root, data_dir };
        environment.run(None, &["node", "init", "--demo"]);
        environment
    }

    fn run(&self, credential: Option<&str>, arguments: &[&str]) -> Value {
        let mut command = Command::new(env!("CARGO_BIN_EXE_stumble"));
        command
            .env("STUMBLE_DATA_DIR", &self.data_dir)
            .env(
                "STUMBLE_CREDENTIAL_STORE_DIR",
                self.root.join("credentials"),
            )
            .args(arguments);
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

    fn fail(&self, credential: Option<&str>, arguments: &[&str]) -> (i32, Value) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_stumble"));
        command
            .env("STUMBLE_DATA_DIR", &self.data_dir)
            .env(
                "STUMBLE_CREDENTIAL_STORE_DIR",
                self.root.join("credentials"),
            )
            .args(arguments);
        if let Some(credential) = credential {
            command.env("STUMBLE_HARNESS_CREDENTIAL", credential);
        }
        let output = command.output().unwrap();
        assert!(
            !output.status.success(),
            "{arguments:?} unexpectedly passed"
        );
        (
            output.status.code().unwrap(),
            serde_json::from_slice(&output.stderr).unwrap(),
        )
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn cli_shows_empty_context_then_sets_prose() {
    let environment = Environment::new();

    let shown = environment.run(None, &["context", "show"]);
    assert_eq!(shown["data"]["context_md"], "");
    assert_eq!(shown["data"]["watches"], serde_json::json!([]));
    assert!(shown["data"]["taste"].is_object());
    assert!(shown["data"]["readiness"]["ready"].is_boolean());
    assert!(shown["data"]["allowed_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action == "set"));

    let input = environment.root.join("context.json");
    fs::write(&input, r##"{"context_md":"# Me\nLoves systems papers."}"##).unwrap();
    let updated = environment.run(
        None,
        &["context", "set", "--input", input.to_str().unwrap()],
    );
    assert_eq!(updated["data"]["context_md"], "# Me\nLoves systems papers.");

    let reread = environment.run(None, &["context", "show"]);
    assert_eq!(reread["data"]["context_md"], "# Me\nLoves systems papers.");
}

#[test]
fn cli_adds_and_lists_watches_and_plans_carry_due_watches() {
    let environment = Environment::new();

    let added = environment.run(
        None,
        &[
            "discover",
            "watch",
            "add",
            "https://x.com/home",
            "--kind",
            "timeline",
        ],
    );
    assert_eq!(added["data"]["kind"], "timeline");
    assert_eq!(added["data"]["cadence"], "daily");
    assert_eq!(added["data"]["skill"], "watch-x");

    let listed = environment.run(None, &["discover", "watch", "list"]);
    let watches = listed["data"].as_array().unwrap();
    assert_eq!(watches.len(), 1);
    assert_eq!(watches[0]["url"], "https://x.com/home");

    let input = environment.root.join("request.json");
    fs::write(&input, r#"{"idempotency_key":"watch-plan"}"#).unwrap();
    let created = environment.run(
        None,
        &[
            "discover",
            "personal",
            "request",
            "--input",
            input.to_str().unwrap(),
        ],
    );
    let neighborhoods = created["data"]["plan"]["source_neighborhoods"]
        .as_array()
        .unwrap();
    let first = &neighborhoods[0];
    assert_eq!(first["watch"]["url"], "https://x.com/home");
    assert_eq!(first["watch"]["skill"], "watch-x");
    assert_eq!(first["rationale"], "due User watch");

    // The watch is stamped for this period and shows in the packet.
    let packet = environment.run(None, &["context", "show"]);
    assert!(packet["data"]["watches"][0]["last_planned_at"].is_string());

    let watch_id = added["data"]["id"].as_str().unwrap().to_owned();
    let removed = environment.run(None, &["discover", "watch", "remove", &watch_id]);
    assert_eq!(removed["data"]["id"], watch_id);
    let listed_after = environment.run(None, &["discover", "watch", "list"]);
    assert_eq!(listed_after["data"].as_array().unwrap().len(), 0);
}

#[test]
fn cli_brief_get_always_has_all_four_sections() {
    let environment = Environment::new();

    // Keep the test hermetic: disable Bootstrap endpoints so the best-effort
    // sync never leaves the machine.
    let endpoints = environment.run(None, &["sync", "bootstrap", "list"]);
    for endpoint in endpoints["data"].as_array().unwrap() {
        let id = endpoint["id"].as_str().unwrap().to_owned();
        environment.run(None, &["sync", "bootstrap", "disable", &id]);
    }

    let brief = environment.run(None, &["brief", "get"]);
    let data = &brief["data"];
    assert!(data["user"]["context_md"].is_string());
    assert!(data["user"]["taste_summary"].is_string());
    assert!(data["outside"].is_object());
    assert!(data["outside"]["items"].is_array());
    assert!(data["network"]["feed"].is_array());
    assert!(data["network"]["explore"].is_array());
    assert!(data["gaps"].is_array());
}

#[test]
fn worker_credential_cannot_read_the_user_context() {
    let environment = Environment::new();
    let worker = environment.run(
        None,
        &[
            "node",
            "harness",
            "register",
            "--label",
            "context-worker",
            "--kind",
            "unattended",
            "--capability",
            "personal_discovery_execution",
        ],
    )["data"]["credential"]
        .as_str()
        .unwrap()
        .to_owned();

    let (_, error) = environment.fail(Some(&worker), &["context", "show"]);
    let message = error["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("interactive") || message.contains("grant"),
        "unexpected error: {message}"
    );
}
