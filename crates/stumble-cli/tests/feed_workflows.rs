use chrono::Utc;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
use stumble_core::{
    AgentHarnessKind, AgentTools, CandidateConfidence, CandidateContentType, CandidateProvenance,
    CandidateSourceMetadata, CandidateSubmissionEvidence, CandidateSubmissionRequest,
    CurationPolicy, HarnessCapability, PlacementReviewDecision, ProposedCandidatePlacement,
    RegisterAgentHarnessRequest,
};

struct Environment {
    root: PathBuf,
    data_dir: PathBuf,
}

impl Environment {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("stumble-feed-workflows-{}", uuid::Uuid::now_v7()));
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
        self.output(arguments, None, true)
    }

    fn run_json(&self, arguments: &[&str], input: &Value) -> Value {
        self.output(arguments, Some(input), true)
    }

    fn output(&self, arguments: &[&str], input: Option<&Value>, success: bool) -> Value {
        let mut command = self.command();
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
        serde_json::from_slice(if success {
            &output.stdout
        } else {
            &output.stderr
        })
        .unwrap()
    }

    fn accept_feed_item(&self) -> (String, String) {
        let created = self.run(&[
            "pod",
            "create",
            "--name",
            "Systems",
            "--slug",
            "systems",
            "--visibility",
            "private",
        ]);
        let pod_id = created["data"]["result"]["pod_id"]
            .as_str()
            .unwrap()
            .to_owned();
        self.run(&["pod", "subscribe", "systems"]);

        let tools = AgentTools::open_initialized_home_node(&self.data_dir).unwrap();
        let owner = tools.local_owner_auth_context().unwrap();
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "Feed fixture curator".into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![
                        HarnessCapability::CandidateSubmission,
                        HarnessCapability::PodCuration,
                    ],
                    pod_ids: Some(vec![pod_id.parse().unwrap()]),
                },
            )
            .unwrap();
        let curator = tools
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap();
        tools
            .set_pod_curation_policy(
                &curator,
                pod_id.parse().unwrap(),
                CurationPolicy::Manual,
                Utc::now(),
            )
            .unwrap();
        let submitted = tools
            .submit_candidate(
                &curator,
                CandidateSubmissionRequest {
                    evidence: CandidateSubmissionEvidence {
                        source_url: "https://research.example/systems-report".into(),
                        source_metadata: CandidateSourceMetadata {
                            title: Some("Systems report".into()),
                            author: Some("Research group".into()),
                            published_at: None,
                        },
                        permitted_excerpt: Some("Permitted evidence".into()),
                        summary: Some("A report about reliable systems".into()),
                        content_type: CandidateContentType::Article,
                        tags: vec!["systems".into(), "reliability".into()],
                        provenance: CandidateProvenance {
                            discovered_at: Utc::now(),
                            discovery_method: "browser_search".into(),
                            referrer_url: Some("https://search.example/results".into()),
                        },
                        proposed_placements: vec![ProposedCandidatePlacement {
                            pod_id: pod_id.parse().unwrap(),
                            reason: "Primary evidence concerns reliable systems".into(),
                            confidence: CandidateConfidence::new(0.95).unwrap(),
                        }],
                        task_context: None,
                        harness_idempotency_key: "feed-fixture-worker".into(),
                        client_idempotency_key: "feed-fixture-client".into(),
                    },
                },
            )
            .unwrap();
        tools
            .curate_candidate(&curator, submitted.candidate.id, Utc::now())
            .unwrap();
        let content_item_id = tools
            .review_candidate_placement(
                &curator,
                submitted.candidate.id,
                pod_id.parse().unwrap(),
                PlacementReviewDecision::Accept,
                None,
                Utc::now(),
            )
            .unwrap()
            .content_item_id
            .unwrap()
            .to_string();
        (pod_id, content_item_id)
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn current_batch_is_stable_until_explicit_completion_and_exposes_evidence() {
    let environment = Environment::new();
    let (pod_id, _) = environment.accept_feed_item();
    let request = json!({
        "size": 3,
        "feed_mix": {
            "high_value_percent": 80,
            "exploration_percent": 10,
            "old_gem_percent": 10,
            "per_pod_cap": 3,
            "per_source_cap": 2
        },
        "batch_intent": {
            "focus_topics": ["systems"],
            "avoid_topics": ["politics"]
        }
    });
    let first = environment.run_json(&["feed", "batch", "get", "--input", "-"], &request);
    assert_eq!(first["version"], 1);
    assert_eq!(first["data"]["state"], "ready");
    assert_eq!(
        first["data"]["batch_intent"]["focus_topics"],
        json!(["systems"])
    );
    assert_eq!(first["data"]["feed_mix"]["per_source_cap"], 2);
    assert_eq!(first["data"]["allowed_actions"], json!(["complete"]));
    assert_eq!(first["data"]["items"][0]["placements"][0]["pod_id"], pod_id);
    assert_eq!(
        first["data"]["items"][0]["placements"][0]["slug"],
        "systems"
    );
    assert!(first["data"]["items"][0]["ranking_evidence"]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason.as_str().unwrap().contains("Batch Intent")));
    assert!(first["data"]["items"][0]["allowed_actions"].is_array());

    let repeated = environment.run_json(&["feed", "batch", "get", "--input", "-"], &request);
    assert_eq!(repeated["data"]["id"], first["data"]["id"]);
    assert_eq!(repeated["data"]["items"], first["data"]["items"]);

    let batch_id = first["data"]["id"].as_str().unwrap();
    let completed = environment.run(&["feed", "batch", "complete", batch_id]);
    assert_eq!(completed["data"]["id"], batch_id);
    assert_eq!(completed["data"]["state"], "caught_up");
    assert_eq!(completed["data"]["allowed_actions"], json!([]));

    let next = environment.run_json(&["feed", "batch", "get", "--input", "-"], &request);
    assert_ne!(next["data"]["id"], first["data"]["id"]);
}

#[test]
fn feedback_and_taste_commands_manage_only_the_private_profile() {
    let environment = Environment::new();
    let (_, content_item_id) = environment.accept_feed_item();
    let request = json!({"size": 3});
    environment.run_json(&["feed", "batch", "get", "--input", "-"], &request);

    let source = environment.run(&[
        "feed",
        "feedback",
        "record",
        &content_item_id,
        "--kind",
        "block-source",
        "--reason",
        "Not useful",
    ]);
    assert_eq!(source["data"]["content_item_id"], content_item_id);
    assert_eq!(source["data"]["feedback_state"]["source_blocked"], true);

    let topic = environment.run(&[
        "feed",
        "feedback",
        "record",
        &content_item_id,
        "--kind",
        "block-topic",
        "--topic",
        "systems",
    ]);
    assert_eq!(topic["data"]["feedback_state"]["topic_blocked"], true);

    let unrelated = environment.output(
        &[
            "feed",
            "feedback",
            "record",
            &content_item_id,
            "--kind",
            "block-topic",
            "--topic",
            "unrelated-topic",
        ],
        None,
        false,
    );
    assert_eq!(unrelated["error"]["code"], "validation_error");

    environment.run(&[
        "feed",
        "feedback",
        "record",
        &content_item_id,
        "--kind",
        "save",
    ]);
    let shown = environment.run(&["feed", "taste", "show"]);
    assert!(shown["data"]["explicit"]["blocked_sources"]
        .as_array()
        .unwrap()
        .contains(&json!("research.example")));
    assert!(shown["data"]["explicit"]["blocked_topics"]
        .as_array()
        .unwrap()
        .contains(&json!("systems")));
    assert!(!shown["data"]["learned"].as_array().unwrap().is_empty());
    assert_eq!(shown["data"]["allowed_actions"], json!(["set", "reset"]));

    let set = environment.run_json(
        &["feed", "taste", "set", "--input", "-"],
        &json!({
            "interests": ["distributed systems"],
            "blocked_topics": ["politics"],
            "blocked_sources": ["noise.example"],
            "recurrence_penalty_days": 14
        }),
    );
    assert_eq!(
        set["data"]["explicit"]["interests"],
        json!(["distributed systems"])
    );
    assert_eq!(set["data"]["explicit"]["recurrence_penalty_days"], 14);
    assert!(!set["data"]["learned"].as_array().unwrap().is_empty());

    let reset = environment.run(&["feed", "taste", "reset"]);
    assert!(reset["data"]["learned"].as_array().unwrap().is_empty());
    assert_eq!(reset["data"]["explicit"], set["data"]["explicit"]);
}

#[test]
fn standalone_blocks_and_drip_are_not_parser_aliases() {
    let environment = Environment::new();
    for arguments in [
        &["drip"][..],
        &["feed", "drip"][..],
        &["block-source", "example.com"][..],
        &["block-topic", "systems"][..],
    ] {
        let failure = environment.output(arguments, None, false);
        assert_eq!(failure["error"]["code"], "usage_error", "{arguments:?}");
    }
}
