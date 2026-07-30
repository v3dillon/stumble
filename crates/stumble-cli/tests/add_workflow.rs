use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};

struct Environment {
    root: PathBuf,
    data_dir: PathBuf,
}

impl Environment {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("stumble-add-workflow-{}", uuid::Uuid::now_v7()));
        let data_dir = root.join("home");
        fs::create_dir_all(&root).unwrap();
        let environment = Self { root, data_dir };
        environment.run(&["node", "init"]);
        environment
    }

    fn run(&self, arguments: &[&str]) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_stumble"))
            .env("STUMBLE_DATA_DIR", &self.data_dir)
            .env(
                "STUMBLE_CREDENTIAL_STORE_DIR",
                self.root.join("credentials"),
            )
            .args(arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "command {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
        envelope["data"].clone()
    }

    fn run_failure(&self, arguments: &[&str]) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_stumble"))
            .env("STUMBLE_DATA_DIR", &self.data_dir)
            .env(
                "STUMBLE_CREDENTIAL_STORE_DIR",
                self.root.join("credentials"),
            )
            .args(arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert!(!output.status.success(), "command {arguments:?} succeeded");
        let envelope: Value = serde_json::from_slice(&output.stderr).unwrap();
        envelope["error"].clone()
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn add_creates_the_saved_pod_and_a_feed_eligible_item_in_one_step() {
    let environment = Environment::new();

    let added = environment.run(&[
        "add",
        "https://example.com/essays/attention",
        "--title",
        "On Attention",
        "--summary",
        "Why finite feeds beat infinite scroll",
        "--tag",
        "attention",
        "--tag",
        "design",
    ]);
    assert_eq!(added["pod_slug"], "saved");
    assert_eq!(added["pod_created"], true);
    assert_eq!(added["subscribed"], true);
    assert_eq!(added["placement"]["status"], "accepted");
    assert_eq!(added["placement"]["curation_path"], "add_to_pod");
    assert_eq!(added["content_item"]["title"], "On Attention");

    let batch = environment.run(&["feed", "batch", "get"]);
    let items = batch["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]["content_reference"]["canonical_url"],
        "https://example.com/essays/attention"
    );

    let again = environment.run(&["add", "https://example.com/essays/attention"]);
    assert_eq!(again["pod_created"], false);
    assert_eq!(
        again["content_item"]["id"], added["content_item"]["id"],
        "re-adding the same URL dedupes on canonical URL"
    );
}

#[test]
fn add_requires_an_explicit_pod_slug_to_already_exist() {
    let environment = Environment::new();
    let error = environment.run_failure(&[
        "add",
        "https://example.com/essays/attention",
        "--pod",
        "does-not-exist",
    ]);
    assert_eq!(error["code"], "not_found");
}
