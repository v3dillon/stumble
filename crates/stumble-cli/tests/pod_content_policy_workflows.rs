use chrono::Utc;
use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};
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
        let root = std::env::temp_dir().join(format!(
            "stumble-pod-content-policy-{}",
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
        let output = self.command().args(arguments).output().unwrap();
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn tools(&self) -> AgentTools {
        AgentTools::open_initialized_home_node(&self.data_dir).unwrap()
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn accept_item(tools: &AgentTools, pod_id: uuid::Uuid, suffix: &str) -> uuid::Uuid {
    let owner = tools.local_owner_auth_context().unwrap();
    let issued = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: format!("Content curator {suffix}"),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![
                    HarnessCapability::CandidateSubmission,
                    HarnessCapability::PodCuration,
                ],
                pod_ids: Some(vec![pod_id]),
            },
        )
        .unwrap();
    let actor = tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap();
    tools
        .set_pod_curation_policy(&actor, pod_id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let submitted = tools
        .submit_candidate(
            &actor,
            CandidateSubmissionRequest {
                evidence: CandidateSubmissionEvidence {
                    source_url: format!("https://reference.example/{suffix}"),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(format!("Reference {suffix}")),
                        author: Some("Reference author".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("Permitted evidence".into()),
                    summary: Some("Canonical Content Item".into()),
                    content_type: CandidateContentType::Article,
                    tags: vec!["systems".into()],
                    provenance: CandidateProvenance {
                        discovered_at: Utc::now(),
                        discovery_method: "browser_search".into(),
                        referrer_url: Some("https://search.example/results".into()),
                    },
                    proposed_placements: vec![ProposedCandidatePlacement {
                        pod_id,
                        reason: "Directly concerns the Pod subject".into(),
                        confidence: CandidateConfidence::new(0.95).unwrap(),
                    }],
                    task_context: None,
                    harness_idempotency_key: format!("worker-{suffix}"),
                    client_idempotency_key: format!("client-{suffix}"),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&actor, submitted.candidate.id, Utc::now())
        .unwrap();
    tools
        .review_candidate_placement(
            &actor,
            submitted.candidate.id,
            pod_id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap()
        .content_item_id
        .unwrap()
        .into()
}

#[test]
fn content_stream_is_paginated_outside_feed_and_details_expose_placement_actions() {
    let environment = Environment::new();
    let created = environment.run(&[
        "pod",
        "create",
        "--name",
        "Research",
        "--slug",
        "research",
        "--visibility",
        "private",
    ]);
    let pod_id = created["data"]["result"]["pod_id"].as_str().unwrap();
    let tools = environment.tools();
    let first_id = accept_item(&tools, pod_id.parse().unwrap(), "first");
    let second_id = accept_item(&tools, pod_id.parse().unwrap(), "second");

    let first_page = environment.run(&["pod", "content", "list", "research", "--limit", "1"]);
    assert_eq!(first_page["data"]["pod_id"], pod_id);
    assert_eq!(first_page["data"]["slug"], "research");
    assert_eq!(first_page["data"]["items"].as_array().unwrap().len(), 1);
    assert!(first_page["data"]["items"][0]
        .get("allowed_actions")
        .is_none());
    let cursor = first_page["data"]["next_cursor"].as_str().unwrap();
    let second_page = environment.run(&[
        "pod", "content", "list", pod_id, "--limit", "1", "--cursor", cursor,
    ]);
    let listed_ids = [
        first_page["data"]["items"][0]["content_item"]["id"]
            .as_str()
            .unwrap(),
        second_page["data"]["items"][0]["content_item"]["id"]
            .as_str()
            .unwrap(),
    ];
    assert!(listed_ids.contains(&first_id.to_string().as_str()));
    assert!(listed_ids.contains(&second_id.to_string().as_str()));

    let shown = environment.run(&["pod", "content", "show", "research", &first_id.to_string()]);
    assert_eq!(shown["data"]["pod_id"], pod_id);
    assert_eq!(shown["data"]["content_item"]["id"], first_id.to_string());
    assert_eq!(
        shown["data"]["accepted_placement"]["content_item_id"],
        first_id.to_string()
    );
    assert_eq!(
        shown["data"]["allowed_actions"],
        serde_json::json!(["remove"])
    );
}

#[test]
fn add_preserves_evidence_and_private_remove_only_reverses_the_selected_placement() {
    let environment = Environment::new();
    for (name, slug) in [("Source", "source"), ("Target", "target")] {
        environment.run(&[
            "pod",
            "create",
            "--name",
            name,
            "--slug",
            slug,
            "--visibility",
            "private",
        ]);
    }
    let tools = environment.tools();
    let source = tools.pod_by_slug("source", None).unwrap();
    let content_item_id = accept_item(&tools, source.id, "shared");

    let added = environment.run(&[
        "pod",
        "content",
        "add",
        "target",
        &content_item_id.to_string(),
        "--note",
        "Useful supporting evidence",
    ]);
    assert_eq!(added["data"]["placement"]["status"], "accepted");
    assert_eq!(added["data"]["placement"]["curation_path"], "add_to_pod");
    assert_eq!(
        added["data"]["placement"]["origin_placements"][0]["pod_id"],
        source.id.to_string()
    );

    let removed = environment.run(&[
        "pod",
        "content",
        "remove",
        "target",
        &content_item_id.to_string(),
        "--reason",
        "Outside the target boundary",
    ]);
    assert_eq!(removed["data"]["status"], "removed");
    assert!(
        environment.run(&["pod", "content", "list", "target"])["data"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        environment.run(&["pod", "content", "list", "source"])["data"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn public_remove_and_autonomous_policy_wait_for_approval() {
    let environment = Environment::new();
    environment.run(&[
        "pod",
        "create",
        "--name",
        "Public",
        "--slug",
        "public",
        "--visibility",
        "private",
    ]);
    let tools = environment.tools();
    let owner = tools.local_owner_auth_context().unwrap();
    let pod = tools.pod_by_slug("public", None).unwrap();
    let item_id = accept_item(&tools, pod.id, "public-item");
    let issued = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "Curation proposer".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::PodCuration],
                pod_ids: Some(vec![pod.id]),
            },
        )
        .unwrap();
    let proposer = tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap();
    let publish = tools
        .create_pending_proposal(
            &proposer,
            stumble_core::SensitiveChange::PublishPod { pod_id: pod.id },
            Utc::now(),
            Utc::now() + chrono::Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&owner, publish.id, Utc::now())
        .unwrap();
    drop(tools);

    let credential = issued.token.expose();
    let removal_output = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", credential)
        .args([
            "pod",
            "content",
            "remove",
            "public",
            &item_id.to_string(),
            "--reason",
            "No longer within scope",
        ])
        .output()
        .unwrap();
    assert!(removal_output.status.success());
    let removal: Value = serde_json::from_slice(&removal_output.stdout).unwrap();
    assert_eq!(removal["data"]["status"], "pending_approval");
    assert_eq!(
        environment.run(&["pod", "content", "list", "public"])["data"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let removal_id = removal["data"]["proposal"]["id"].as_str().unwrap();
    environment.run(&["node", "proposal", "approve", removal_id]);
    assert!(
        environment.run(&["pod", "content", "list", "public"])["data"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    assert_eq!(
        environment.run(&["pod", "policy", "show", "public"])["data"]["policy"]["mode"],
        "manual"
    );
    let assisted = environment.run(&[
        "pod",
        "policy",
        "set",
        "public",
        "--mode",
        "assisted",
        "--confidence-threshold",
        "0.72",
    ]);
    assert_eq!(assisted["data"]["status"], "updated");
    assert_eq!(assisted["data"]["policy"]["mode"], "assisted");

    let autonomous_output = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", credential)
        .args([
            "pod",
            "policy",
            "set",
            "public",
            "--mode",
            "autonomous",
            "--confidence-threshold",
            "0.91",
        ])
        .output()
        .unwrap();
    assert!(autonomous_output.status.success());
    let autonomous: Value = serde_json::from_slice(&autonomous_output.stdout).unwrap();
    assert_eq!(autonomous["data"]["status"], "pending_approval");
    assert_eq!(
        environment.run(&["pod", "policy", "show", "public"])["data"]["policy"]["mode"],
        "assisted"
    );
    environment.run(&[
        "node",
        "proposal",
        "approve",
        autonomous["data"]["proposal"]["id"].as_str().unwrap(),
    ]);
    assert_eq!(
        environment.run(&["pod", "policy", "show", "public"])["data"]["policy"]["mode"],
        "autonomous"
    );
}
