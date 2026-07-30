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
            std::env::temp_dir().join(format!("stumble-search-workflow-{}", uuid::Uuid::now_v7()));
        let data_dir = root.join("home");
        fs::create_dir_all(&root).unwrap();
        let environment = Self { root, data_dir };
        environment.run(&["node", "init"]);
        environment
    }

    fn run(&self, arguments: &[&str]) -> Value {
        let output = self.command(arguments);
        assert!(
            output.status.success(),
            "command {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
        envelope["data"].clone()
    }

    fn run_failure(&self, arguments: &[&str]) -> Value {
        let output = self.command(arguments);
        assert!(!output.status.success(), "command {arguments:?} succeeded");
        let envelope: Value = serde_json::from_slice(&output.stderr).unwrap();
        envelope["error"].clone()
    }

    fn command(&self, arguments: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_stumble"))
            .env("STUMBLE_DATA_DIR", &self.data_dir)
            .env(
                "STUMBLE_CREDENTIAL_STORE_DIR",
                self.root.join("credentials"),
            )
            .args(arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap()
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn hit_titles(results: &Value) -> Vec<&str> {
    results["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| hit["title"].as_str().unwrap())
        .collect()
}

#[test]
fn search_finds_saved_links_by_title_summary_and_tag() {
    let environment = Environment::new();
    environment.run(&[
        "add",
        "https://example.com/attention",
        "--title",
        "The attention economy",
        "--summary",
        "How platforms monetize focus",
        "--tag",
        "economics",
    ]);
    environment.run(&[
        "add",
        "https://example.com/rust",
        "--title",
        "Fearless concurrency",
        "--summary",
        "Ownership makes data races impossible",
        "--tag",
        "rust",
    ]);

    let by_title = environment.run(&["search", "attention"]);
    assert_eq!(hit_titles(&by_title), vec!["The attention economy"]);
    let hit = &by_title["hits"][0];
    assert_eq!(hit["url"], "https://example.com/attention");
    assert_eq!(hit["pods"], serde_json::json!(["saved"]));
    assert!(hit["score"].as_f64().unwrap() > 0.0);
    assert!(hit["snippet"].as_str().unwrap().contains("[attention]"));

    let by_summary = environment.run(&["search", "data races"]);
    assert_eq!(hit_titles(&by_summary), vec!["Fearless concurrency"]);

    let by_tag = environment.run(&["search", "economics"]);
    assert_eq!(hit_titles(&by_tag), vec!["The attention economy"]);
}

#[test]
fn search_sees_writes_from_other_processes_and_respects_limit() {
    let environment = Environment::new();
    environment.run(&[
        "add",
        "https://example.com/first",
        "--title",
        "Distributed systems reading",
        "--summary",
        "Consensus from first principles",
    ]);
    let first = environment.run(&["search", "consensus"]);
    assert_eq!(first["hits"].as_array().unwrap().len(), 1);

    // A later add from a separate process must be visible to the next search:
    // the derived index rebuilds when the store generation moves.
    environment.run(&[
        "add",
        "https://example.com/second",
        "--title",
        "Consensus in practice",
        "--summary",
        "Running Raft in production",
    ]);
    let second = environment.run(&["search", "consensus"]);
    assert_eq!(second["hits"].as_array().unwrap().len(), 2);

    let limited = environment.run(&["search", "consensus", "--limit", "1"]);
    assert_eq!(limited["hits"].as_array().unwrap().len(), 1);
}

#[test]
fn search_reaches_archived_snapshot_text() {
    let environment = Environment::new();
    let snapshot = environment.root.join("snapshot.md");
    fs::write(
        &snapshot,
        "# Field notes\n\nThe xylophone metaphor explains backpressure well.",
    )
    .unwrap();
    environment.run(&[
        "add",
        "https://example.com/notes",
        "--title",
        "Systems field notes",
        "--summary",
        "Assorted operational lessons",
        "--snapshot",
        snapshot.to_str().unwrap(),
    ]);

    let results = environment.run(&["search", "xylophone"]);
    assert_eq!(hit_titles(&results), vec!["Systems field notes"]);
    assert!(results["hits"][0]["snippet"]
        .as_str()
        .unwrap()
        .contains("[xylophone]"));
}

#[test]
fn search_rejects_empty_queries_and_bad_limits() {
    let environment = Environment::new();
    let empty = environment.run_failure(&["search", "   "]);
    assert_eq!(empty["code"], "validation_error");
    let zero = environment.run_failure(&["search", "fine", "--limit", "0"]);
    assert_eq!(zero["code"], "validation_error");
}

#[test]
fn hostile_query_syntax_is_treated_as_literal_terms() {
    let environment = Environment::new();
    environment.run(&[
        "add",
        "https://example.com/plain",
        "--title",
        "Plain title",
        "--summary",
        "Plain summary",
    ]);
    let results = environment.run(&["search", "title NOT missing\" OR ("]);
    assert!(results["hits"].as_array().unwrap().is_empty());
}
