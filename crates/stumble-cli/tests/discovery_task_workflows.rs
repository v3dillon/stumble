use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};

struct Environment {
    root: PathBuf,
    data_dir: PathBuf,
}

impl Environment {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "stumble-discovery-workflows-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
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
        self.output(None, arguments, true)
    }

    fn run_as(&self, credential: &str, arguments: &[&str]) -> Value {
        self.output(Some(credential), arguments, true)
    }

    fn fail_as(&self, credential: &str, arguments: &[&str]) -> (i32, Value) {
        let output = self
            .command()
            .env("STUMBLE_HARNESS_CREDENTIAL", credential)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "{arguments:?} unexpectedly passed"
        );
        (
            output.status.code().unwrap(),
            serde_json::from_slice(&output.stderr).unwrap(),
        )
    }

    fn output(&self, credential: Option<&str>, arguments: &[&str], success: bool) -> Value {
        let mut command = self.command();
        if let Some(credential) = credential {
            command.env("STUMBLE_HARNESS_CREDENTIAL", credential);
        }
        let output = command.args(arguments).output().unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn package(&self, name: &str) -> PathBuf {
        let directory = self.root.join(name);
        fs::create_dir_all(&directory).unwrap();
        for (file, contents) in [
            ("CONTEXT.md", "# Systems\n\nReliable systems engineering.\n"),
            ("SKILL.md", "# Discovery\n\nPrefer primary sources.\n"),
            (
                "sources.yaml",
                "source_rules:\n  - inspect:\n      kind: publication\n      name: engineering reports\n    seek:\n      description: incident analyses\n    schedule:\n      cadence: daily\n  - inspect:\n      kind: publication\n      name: research journals\n    seek:\n      description: reliability research\n    schedule:\n      cadence: daily\n",
            ),
            ("filters.yaml", "blocked_topics: []\nblocked_domains: []\n"),
            ("examples.good.md", "# Good\n\n- Primary evidence.\n"),
            ("examples.bad.md", "# Bad\n\n- Unsourced claims.\n"),
            ("events.jsonl", ""),
        ] {
            fs::write(directory.join(file), contents).unwrap();
        }
        directory
    }

    fn create_pod(&self, slug: &str) -> String {
        let package = self.package(&format!("{slug}-package"));
        self.run(&[
            "pod",
            "create",
            "--name",
            slug,
            "--slug",
            slug,
            "--visibility",
            "private",
            "--package",
            package.to_str().unwrap(),
        ])["data"]["result"]["pod_id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn harness(&self, label: &str, pod_id: &str) -> String {
        self.run(&[
            "node",
            "harness",
            "register",
            "--label",
            label,
            "--kind",
            "unattended",
            "--capability",
            "discovery_tasks",
            "--pod-id",
            pod_id,
        ])["data"]["credential"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn due_source_rules_automatically_materialize_once_and_list_with_filters_and_pagination() {
    let environment = Environment::new();
    let pod_id = environment.create_pod("systems");
    let credential = environment.harness("worker", &pod_id);

    let first = environment.run_as(
        &credential,
        &[
            "discover", "task", "list", "--state", "ready", "--limit", "1",
        ],
    );
    let first_items = first["data"]["items"].as_array().unwrap();
    assert_eq!(first_items.len(), 1);
    assert_eq!(first_items[0]["pod_id"], pod_id);
    assert_eq!(first_items[0]["package_version"], 1);
    assert_eq!(first_items[0]["target"]["kind"], "pod");
    assert_eq!(first_items[0]["target"]["pod_id"], pod_id);
    assert_eq!(first_items[0]["target"]["package_version"], 1);
    assert_eq!(first_items[0]["origin"]["kind"], "scheduled");
    let cursor = first["data"]["next_cursor"].as_str().unwrap();

    let second = environment.run_as(
        &credential,
        &[
            "discover", "task", "list", "--state", "ready", "--limit", "1", "--cursor", cursor,
        ],
    );
    assert_eq!(second["data"]["items"].as_array().unwrap().len(), 1);
    assert!(second["data"]["next_cursor"].is_null());

    let repeated = environment.run_as(
        &credential,
        &["discover", "task", "list", "--pod", "systems"],
    );
    assert_eq!(repeated["data"]["items"].as_array().unwrap().len(), 2);
    assert!(repeated["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item.get("allowed_actions").is_none()));
}

#[test]
fn scoped_harness_claims_renews_fails_and_completes_tasks_with_deterministic_ownership() {
    let environment = Environment::new();
    let pod_id = environment.create_pod("leases");
    let other_pod_id = environment.create_pod("other");
    let worker = environment.harness("worker", &pod_id);
    let competitor = environment.harness("competitor", &pod_id);
    let outsider = environment.harness("outsider", &other_pod_id);
    let listed = environment.run_as(&worker, &["discover", "task", "list", "--state", "ready"]);
    let task_id = listed["data"]["items"][0]["id"].as_str().unwrap();

    let shown = environment.run_as(&worker, &["discover", "task", "show", task_id]);
    assert_eq!(
        shown["data"]["allowed_actions"],
        serde_json::json!(["claim"])
    );

    let claimed = environment.run_as(
        &worker,
        &[
            "discover",
            "task",
            "claim",
            task_id,
            "--lease-seconds",
            "300",
        ],
    );
    assert_eq!(claimed["data"]["task"]["state"]["status"], "leased");
    assert_eq!(
        claimed["data"]["allowed_actions"],
        serde_json::json!(["renew", "complete", "fail"])
    );

    let (code, conflict) = environment.fail_as(
        &competitor,
        &[
            "discover",
            "task",
            "claim",
            task_id,
            "--lease-seconds",
            "300",
        ],
    );
    assert_eq!(code, 4);
    assert_eq!(conflict["error"]["code"], "task_lease_conflict");
    let (code, foreign_renewal) = environment.fail_as(
        &competitor,
        &[
            "discover",
            "task",
            "renew",
            task_id,
            "--lease-seconds",
            "600",
        ],
    );
    assert_eq!(code, 4);
    assert_eq!(foreign_renewal["error"]["code"], "task_lease_required");
    let renewed = environment.run_as(
        &worker,
        &[
            "discover",
            "task",
            "renew",
            task_id,
            "--lease-seconds",
            "600",
        ],
    );
    assert_eq!(renewed["data"]["task"]["state"]["status"], "leased");

    let failed = environment.run_as(
        &worker,
        &[
            "discover",
            "task",
            "fail",
            task_id,
            "--reason",
            "temporary outage",
        ],
    );
    assert_eq!(failed["data"]["task"]["state"]["status"], "pending");
    assert_eq!(
        failed["data"]["task"]["attempts"][0]["outcome"]["failed"]["reason"],
        "temporary outage"
    );
    environment.run_as(
        &worker,
        &[
            "discover",
            "task",
            "claim",
            task_id,
            "--lease-seconds",
            "300",
        ],
    );
    let completed = environment.run_as(&worker, &["discover", "task", "complete", task_id]);
    assert_eq!(completed["data"]["task"]["state"]["status"], "completed");
    assert_eq!(completed["data"]["allowed_actions"], serde_json::json!([]));

    let (code, terminal) = environment.fail_as(
        &worker,
        &[
            "discover",
            "task",
            "claim",
            task_id,
            "--lease-seconds",
            "300",
        ],
    );
    assert_eq!(code, 4);
    assert_eq!(terminal["error"]["code"], "task_terminal");
    let (code, forbidden) = environment.fail_as(&outsider, &["discover", "task", "show", task_id]);
    assert_eq!(code, 3);
    assert_eq!(forbidden["error"]["code"], "forbidden");
}

#[test]
fn manual_task_creation_and_materialization_are_not_public_commands() {
    let environment = Environment::new();
    for arguments in [
        vec!["discover", "task", "create"],
        vec!["discover", "task", "materialize"],
        vec!["materialize-discovery-tasks"],
        vec!["list-ready-discovery-tasks"],
    ] {
        let output = environment.command().args(&arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "usage_error", "{arguments:?}");
    }
}
