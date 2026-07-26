use chrono::Utc;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

struct Environment {
    root: PathBuf,
    data_dir: PathBuf,
}

impl Environment {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "stumble-candidate-workflows-{}",
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
        self.output(None, arguments, None, true)
    }

    fn run_as(&self, credential: &str, arguments: &[&str]) -> Value {
        self.output(Some(credential), arguments, None, true)
    }

    fn run_json_as(&self, credential: &str, arguments: &[&str], input: &Value) -> Value {
        self.output(Some(credential), arguments, Some(input), true)
    }

    fn fail_json_as(&self, credential: &str, arguments: &[&str], input: &Value) -> (i32, Value) {
        let mut command = self.command();
        command
            .env("STUMBLE_HARNESS_CREDENTIAL", credential)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(input).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        (
            output.status.code().unwrap(),
            serde_json::from_slice(&output.stderr).unwrap(),
        )
    }

    fn output(
        &self,
        credential: Option<&str>,
        arguments: &[&str],
        input: Option<&Value>,
        success: bool,
    ) -> Value {
        let mut command = self.command();
        if let Some(credential) = credential {
            command.env("STUMBLE_HARNESS_CREDENTIAL", credential);
        }
        command.args(arguments);
        if input.is_some() {
            command.stdin(Stdio::piped());
        }
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        if let Some(input) = input {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(&serde_json::to_vec(input).unwrap())
                .unwrap();
        }
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn pod(&self, slug: &str) -> String {
        self.run(&[
            "pod",
            "create",
            "--name",
            slug,
            "--slug",
            slug,
            "--visibility",
            "private",
        ])["data"]["result"]["pod_id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn harness(&self, pod_ids: &[&str]) -> String {
        self.harness_with(&["candidate_submission", "pod_curation"], pod_ids)
    }

    fn harness_with(&self, capabilities: &[&str], pod_ids: &[&str]) -> String {
        let mut arguments = vec![
            "node",
            "harness",
            "register",
            "--label",
            "Candidate curator",
            "--kind",
            "interactive",
        ];
        for capability in capabilities {
            arguments.extend(["--capability", capability]);
        }
        for pod_id in pod_ids {
            arguments.extend(["--pod-id", pod_id]);
        }
        self.run(&arguments)["data"]["credential"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn submission(&self, source_url: &str, placements: &[(&str, &str, f64)]) -> Value {
        json!({
            "source_url": source_url,
            "source_metadata": {
                "title": "Candidate evidence",
                "author": "Primary source",
                "published_at": null
            },
            "permitted_excerpt": "A permitted excerpt",
            "summary": "An evidence-backed candidate",
            "content_type": "article",
            "tags": ["systems"],
            "provenance": {
                "discovered_at": Utc::now(),
                "discovery_method": "interactive_browser",
                "referrer_url": "https://search.example/results"
            },
            "target": {
                "kind": "pod_placements",
                "placements": placements.iter().map(|(pod_id, reason, confidence)| json!({
                    "pod_id": pod_id,
                    "reason": reason,
                    "confidence": confidence
                })).collect::<Vec<_>>(),
                "task_context": null
            },
        })
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn structured_submission_is_retry_safe_and_candidate_list_is_filtered_and_paginated() {
    let environment = Environment::new();
    let pod_id = environment.pod("systems");
    let credential = environment.harness(&[&pod_id]);
    let request = environment.submission(
        "https://example.com/candidate-one?utm_source=test",
        &[(&pod_id, "Directly concerns systems", 0.91)],
    );
    let arguments = [
        "discover",
        "candidate",
        "submit",
        "--input",
        "-",
        "--idempotency-key",
        "request-1",
    ];
    let request_path = environment.root.join("candidate.json");
    fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
    let original = environment.run_as(
        &credential,
        &[
            "discover",
            "candidate",
            "submit",
            "--input",
            request_path.to_str().unwrap(),
            "--idempotency-key",
            "request-1",
        ],
    );
    let retry = environment.run_json_as(&credential, &arguments, &request);
    assert_eq!(retry["data"], original["data"]);

    let mut changed = request.clone();
    changed["summary"] = json!("Changed evidence");
    let (code, conflict) = environment.fail_json_as(&credential, &arguments, &changed);
    assert_eq!(code, 4);
    assert_eq!(conflict["error"]["code"], "idempotency_conflict");

    let mut sparse_duplicate = request.clone();
    sparse_duplicate["source_metadata"] = json!({
        "title": null,
        "author": null,
        "published_at": null
    });
    sparse_duplicate["permitted_excerpt"] = Value::Null;
    sparse_duplicate["summary"] = Value::Null;
    sparse_duplicate["tags"] = json!([]);
    environment.run_json_as(
        &credential,
        &[
            "discover",
            "candidate",
            "submit",
            "--input",
            "-",
            "--idempotency-key",
            "request-1-sparse",
        ],
        &sparse_duplicate,
    );

    let second = environment.submission(
        "https://example.com/candidate-two",
        &[(&pod_id, "Also concerns systems", 0.7)],
    );
    environment.run_json_as(
        &credential,
        &[
            "discover",
            "candidate",
            "submit",
            "--input",
            "-",
            "--idempotency-key",
            "request-2",
        ],
        &second,
    );
    let first_page = environment.run_as(
        &credential,
        &[
            "discover",
            "candidate",
            "list",
            "--status",
            "pending",
            "--limit",
            "1",
        ],
    );
    assert_eq!(first_page["data"]["items"].as_array().unwrap().len(), 1);
    assert!(first_page["data"]["items"][0]
        .get("allowed_actions")
        .is_none());
    assert_eq!(
        first_page["data"]["items"][0]["id"],
        original["data"]["candidate"]["id"]
    );
    assert_eq!(
        first_page["data"]["items"][0]["reference"]["summary"],
        "An evidence-backed candidate"
    );
    assert_eq!(
        first_page["data"]["items"][0]["reference"]["source_metadata"]["author"],
        "Primary source"
    );
    assert_eq!(
        first_page["data"]["items"][0]["reference"]["permitted_excerpt"],
        "A permitted excerpt"
    );
    assert_eq!(
        first_page["data"]["items"][0]["reference"]["provenance"]["discovery_method"],
        "interactive_browser"
    );
    let cursor = first_page["data"]["next_cursor"].as_str().unwrap();
    let second_page = environment.run_as(
        &credential,
        &[
            "discover",
            "candidate",
            "list",
            "--status",
            "pending",
            "--limit",
            "1",
            "--cursor",
            cursor,
        ],
    );
    assert_eq!(second_page["data"]["items"].as_array().unwrap().len(), 1);
    assert!(second_page["data"]["next_cursor"].is_null());
}

#[test]
fn evaluation_routing_and_review_keep_placements_independent_and_content_identity_canonical() {
    let environment = Environment::new();
    let manual_id = environment.pod("manual");
    let autonomous_id = environment.pod("autonomous");
    let routed_id = environment.pod("routed");
    let submitter =
        environment.harness_with(&["candidate_submission"], &[&manual_id, &autonomous_id]);
    let curator =
        environment.harness_with(&["pod_curation"], &[&manual_id, &autonomous_id, &routed_id]);
    environment.run_as(
        &curator,
        &["pod", "policy", "set", "manual", "--mode", "manual"],
    );
    let proposal = environment.run_as(
        &curator,
        &[
            "pod",
            "policy",
            "set",
            "autonomous",
            "--mode",
            "autonomous",
            "--confidence-threshold",
            "0.8",
        ],
    );
    let proposal_id = proposal["data"]["proposal"]["id"].as_str().unwrap();
    environment.run(&["node", "proposal", "approve", proposal_id]);
    environment.run_as(
        &curator,
        &["pod", "policy", "set", "routed", "--mode", "manual"],
    );
    let request = environment.submission(
        "https://example.com/shared-item",
        &[
            (&manual_id, "Manual Pod evidence", 0.95),
            (&autonomous_id, "Autonomous policy evidence", 0.95),
        ],
    );
    let submitted = environment.run_json_as(
        &submitter,
        &[
            "discover",
            "candidate",
            "submit",
            "--input",
            "-",
            "--idempotency-key",
            "shared-item",
        ],
        &request,
    );
    let candidate_id = submitted["data"]["candidate"]["id"].as_str().unwrap();
    let evaluated = environment.run_as(
        &curator,
        &["discover", "candidate", "evaluate", candidate_id],
    );
    let placements = evaluated["data"]["placements"].as_array().unwrap();
    assert_eq!(
        placements
            .iter()
            .find(|p| p["pod_id"] == manual_id)
            .unwrap()["status"],
        "pending"
    );
    assert_eq!(
        placements
            .iter()
            .find(|p| p["pod_id"] == autonomous_id)
            .unwrap()["status"],
        "accepted"
    );

    let routed = environment.run_as(
        &curator,
        &[
            "discover",
            "candidate",
            "route",
            candidate_id,
            "routed",
            "--reason",
            "Separate routing evidence",
            "--confidence",
            "0.88",
        ],
    );
    assert_eq!(routed["data"]["pod_id"], routed_id);
    assert_eq!(routed["data"]["slug"], "routed");
    assert_eq!(routed["data"]["placement"]["status"], "pending");

    let rejected = environment.run_as(
        &curator,
        &[
            "discover",
            "candidate",
            "review",
            candidate_id,
            "routed",
            "--decision",
            "reject",
            "--note",
            "Outside this Pod's boundary",
        ],
    );
    assert_eq!(rejected["data"]["placement"]["status"], "rejected");
    let shown = environment.run_as(&curator, &["discover", "candidate", "show", candidate_id]);
    assert_eq!(shown["data"]["candidate"]["review_state"], "accepted");
    assert_eq!(
        shown["data"]["submissions"][0]["provenance"]["discovery_method"],
        "interactive_browser"
    );
    assert_eq!(
        shown["data"]["submissions"][0]["target"]["placements"][0]["reason"],
        "Manual Pod evidence"
    );
    assert_eq!(shown["data"]["placements"].as_array().unwrap().len(), 3);
    assert_eq!(
        shown["data"]["allowed_actions"],
        json!([
            "evaluate_candidate",
            "route_candidate_placement",
            "review_candidate_placement"
        ])
    );
    let accepted_list = environment.run_as(
        &curator,
        &["discover", "candidate", "list", "--status", "accepted"],
    );
    assert_eq!(accepted_list["data"]["items"].as_array().unwrap().len(), 1);

    let accepted = environment.run_as(
        &curator,
        &[
            "discover",
            "candidate",
            "review",
            candidate_id,
            "manual",
            "--decision",
            "accept",
            "--note",
            "Verified against primary evidence",
        ],
    );
    let manual_content_id = accepted["data"]["placement"]["content_item_id"]
        .as_str()
        .unwrap();
    let autonomous_content_id = shown["data"]["placements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["pod_id"] == autonomous_id)
        .unwrap()["content_item_id"]
        .as_str()
        .unwrap();
    assert_eq!(manual_content_id, autonomous_content_id);
}

#[test]
fn routing_rejects_pods_outside_the_harness_grant() {
    let environment = Environment::new();
    let scoped_id = environment.pod("scoped");
    let foreign_id = environment.pod("foreign");
    let credential = environment.harness(&[&scoped_id]);
    let request = environment.submission(
        "https://example.com/scoped-item",
        &[(&scoped_id, "In-scope evidence", 0.6)],
    );
    let submitted = environment.run_json_as(
        &credential,
        &[
            "discover",
            "candidate",
            "submit",
            "--input",
            "-",
            "--idempotency-key",
            "scoped-item",
        ],
        &request,
    );
    let candidate_id = submitted["data"]["candidate"]["id"].as_str().unwrap();
    let output = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", &credential)
        .args([
            "discover",
            "candidate",
            "route",
            candidate_id,
            &foreign_id,
            "--reason",
            "Out-of-scope attempt",
            "--confidence",
            "0.9",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "not_found");
}
