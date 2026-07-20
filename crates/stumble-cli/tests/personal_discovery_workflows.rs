use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};

struct Environment {
    root: PathBuf,
    data_dir: PathBuf,
}

impl Environment {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "stumble-personal-discovery-cli-{}",
            uuid::Uuid::now_v7()
        ));
        let data_dir = root.join("home");
        fs::create_dir_all(&root).unwrap();
        let environment = Self { root, data_dir };
        environment.run(None, &["node", "init"]);
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

    fn fail(&self, arguments: &[&str]) -> (i32, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_stumble"))
            .env("STUMBLE_DATA_DIR", &self.data_dir)
            .env(
                "STUMBLE_CREDENTIAL_STORE_DIR",
                self.root.join("credentials"),
            )
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
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn cli_requests_inspects_and_executes_personal_discovery_without_a_pod() {
    let environment = Environment::new();
    let input = environment.root.join("request.json");
    fs::write(&input, r#"{"idempotency_key":"cli-personal"}"#).unwrap();

    let readiness = environment.run(None, &["discover", "personal", "readiness"]);
    assert_eq!(readiness["data"]["ready"], true);
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
    assert_eq!(created["data"]["task"]["target"]["kind"], "personal");
    assert_eq!(created["data"]["plan"]["result_count"], 10);
    let plan_id = created["data"]["plan"]["id"].as_str().unwrap();
    let plan = environment.run(None, &["discover", "personal", "plan", plan_id]);
    assert_eq!(plan["data"]["id"], plan_id);

    let worker = environment.run(
        None,
        &[
            "node",
            "harness",
            "register",
            "--label",
            "personal-worker",
            "--kind",
            "unattended",
            "--capability",
            "personal_discovery_execution",
        ],
    )["data"]["credential"]
        .as_str()
        .unwrap()
        .to_owned();
    let tasks = environment.run(
        Some(&worker),
        &["discover", "task", "list", "--state", "ready"],
    );
    assert_eq!(tasks["data"]["items"].as_array().unwrap().len(), 1);
}

#[test]
fn cli_classifies_personal_readiness_and_retry_conflicts_as_domain_validation() {
    let environment = Environment::new();
    let first = environment.root.join("first.json");
    let changed = environment.root.join("changed.json");
    fs::write(&first, r#"{"idempotency_key":"same-key"}"#).unwrap();
    fs::write(
        &changed,
        r#"{"idempotency_key":"same-key","result_count":4}"#,
    )
    .unwrap();
    environment.run(
        None,
        &[
            "discover",
            "personal",
            "request",
            "--input",
            first.to_str().unwrap(),
        ],
    );
    let (code, conflict) = environment.fail(&[
        "discover",
        "personal",
        "request",
        "--input",
        changed.to_str().unwrap(),
    ]);
    assert_eq!(code, 4);
    assert_eq!(conflict["error"]["code"], "idempotency_conflict");

    let taste = environment.root.join("taste.json");
    fs::write(&taste, r#"{"interests":[]}"#).unwrap();
    environment.run(
        None,
        &["feed", "taste", "set", "--input", taste.to_str().unwrap()],
    );
    environment.run(None, &["feed", "taste", "reset"]);
    let cold = environment.root.join("cold.json");
    fs::write(&cold, r#"{"idempotency_key":"cold"}"#).unwrap();
    let (code, not_ready) = environment.fail(&[
        "discover",
        "personal",
        "request",
        "--input",
        cold.to_str().unwrap(),
    ]);
    assert_eq!(code, 4);
    assert_eq!(not_ready["error"]["code"], "personal_discovery_not_ready");
}

#[test]
fn cli_completes_lists_and_dismisses_result_batches() {
    let environment = Environment::new();
    let input = environment.root.join("request.json");
    fs::write(
        &input,
        r#"{"idempotency_key":"cli-batch","result_count":4}"#,
    )
    .unwrap();
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
    let task_id = created["data"]["task"]["id"].as_str().unwrap().to_owned();
    let worker = environment.run(
        None,
        &[
            "node",
            "harness",
            "register",
            "--label",
            "batch-worker",
            "--kind",
            "unattended",
            "--capability",
            "personal_discovery_execution",
        ],
    )["data"]["credential"]
        .as_str()
        .unwrap()
        .to_owned();
    environment.run(
        Some(&worker),
        &[
            "discover",
            "task",
            "claim",
            &task_id,
            "--lease-seconds",
            "300",
        ],
    );
    let candidate = environment.root.join("candidate.json");
    fs::write(
        &candidate,
        format!(
            r#"{{
              "source_url":"https://cli.example/result",
              "target":{{"kind":"personal_discovery","task_id":"{task_id}","allocation_role":"proven"}},
              "source_metadata":{{}},
              "content_type":"article",
              "tags":["systems"],
              "provenance":{{"discovered_at":"2026-07-20T12:00:00Z","discovery_method":"browser_search"}}
            }}"#
        ),
    )
    .unwrap();
    let submitted = environment.run(
        Some(&worker),
        &[
            "discover",
            "candidate",
            "submit",
            "--input",
            candidate.to_str().unwrap(),
            "--idempotency-key",
            "cli-result-1",
        ],
    );
    let submission_id = submitted["data"]["submission"]["id"].as_str().unwrap();
    let complete = environment.root.join("complete.json");
    fs::write(
        &complete,
        format!(r#"{{"task_id":"{task_id}","submission_ids":["{submission_id}"]}}"#),
    )
    .unwrap();
    let batch = environment.run(
        Some(&worker),
        &[
            "discover",
            "personal",
            "complete-batch",
            "--input",
            complete.to_str().unwrap(),
        ],
    );
    assert_eq!(batch["data"]["state"], "ready");
    let batch_id = batch["data"]["id"].as_str().unwrap();
    let listed = environment.run(None, &["discover", "personal", "batches"]);
    assert_eq!(listed["data"].as_array().unwrap().len(), 1);
    let dismissed = environment.run(None, &["discover", "personal", "dismiss-batch", batch_id]);
    assert_eq!(dismissed["data"]["state"], "dismissed");
}

#[test]
fn cli_reviews_result_item_and_exposes_allowed_actions() {
    let environment = Environment::new();
    let input = environment.root.join("request.json");
    fs::write(
        &input,
        r#"{"idempotency_key":"cli-review","result_count":4}"#,
    )
    .unwrap();
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
    let task_id = created["data"]["task"]["id"].as_str().unwrap().to_owned();
    let worker = environment.run(
        None,
        &[
            "node",
            "harness",
            "register",
            "--label",
            "review-worker",
            "--kind",
            "unattended",
            "--capability",
            "personal_discovery_execution",
        ],
    )["data"]["credential"]
        .as_str()
        .unwrap()
        .to_owned();
    environment.run(
        Some(&worker),
        &[
            "discover",
            "task",
            "claim",
            &task_id,
            "--lease-seconds",
            "300",
        ],
    );
    let candidate = environment.root.join("candidate.json");
    fs::write(
        &candidate,
        format!(
            r#"{{
              "source_url":"https://cli-review.example/result",
              "target":{{"kind":"personal_discovery","task_id":"{task_id}","allocation_role":"proven"}},
              "source_metadata":{{}},
              "content_type":"article",
              "tags":["systems"],
              "provenance":{{"discovered_at":"2026-07-20T12:00:00Z","discovery_method":"browser_search"}}
            }}"#
        ),
    )
    .unwrap();
    let submitted = environment.run(
        Some(&worker),
        &[
            "discover",
            "candidate",
            "submit",
            "--input",
            candidate.to_str().unwrap(),
            "--idempotency-key",
            "cli-review-1",
        ],
    );
    let submission_id = submitted["data"]["submission"]["id"].as_str().unwrap();
    let complete = environment.root.join("complete.json");
    fs::write(
        &complete,
        format!(r#"{{"task_id":"{task_id}","submission_ids":["{submission_id}"]}}"#),
    )
    .unwrap();
    let batch = environment.run(
        Some(&worker),
        &[
            "discover",
            "personal",
            "complete-batch",
            "--input",
            complete.to_str().unwrap(),
        ],
    );
    let batch_id = batch["data"]["id"].as_str().unwrap();
    let candidate_id = batch["data"]["items"][0]["candidate_id"].as_str().unwrap();
    let review = environment.root.join("review.json");
    fs::write(
        &review,
        format!(
            r#"{{"batch_id":"{batch_id}","candidate_id":"{candidate_id}","action":{{"action":"not_for_me"}}}}"#
        ),
    )
    .unwrap();
    let outcome = environment.run(
        None,
        &[
            "discover",
            "personal",
            "review-item",
            "--input",
            review.to_str().unwrap(),
        ],
    );
    assert_eq!(outcome["data"]["item"]["review"]["action"], "not_for_me");
    assert_eq!(outcome["data"]["batch"]["state"], "ready");
    assert!(outcome["data"]["taste_profile"]["source_affinities"].is_array());
    assert!(outcome["data"]["allowed_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action == "not_for_me"));
}
