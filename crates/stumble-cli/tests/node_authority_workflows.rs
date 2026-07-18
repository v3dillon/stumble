use serde_json::{json, Value};
use std::{path::PathBuf, process::Command};
use stumble_core::{AgentHarnessId, AgentTools, HarnessCapability, RegisterAgentHarnessRequest};
use uuid::Uuid;

struct Environment {
    root: PathBuf,
}

impl Environment {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("stumble-node-authority-{}", Uuid::now_v7()));
        let environment = Self { root };
        let output = environment
            .command()
            .args(["node", "init"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        environment
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_stumble"));
        command.env("STUMBLE_DATA_DIR", self.root.join("home")).env(
            "STUMBLE_CREDENTIAL_STORE_DIR",
            self.root.join("credentials"),
        );
        command
    }

    fn tools(&self) -> AgentTools {
        AgentTools::open_initialized_home_node(self.root.join("home")).unwrap()
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn body(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn owner_registers_reads_and_revokes_a_harness_without_revealing_its_credential_again() {
    let environment = Environment::new();
    let registered = environment
        .command()
        .args([
            "node",
            "harness",
            "register",
            "--label",
            "Feed companion",
            "--kind",
            "interactive",
            "--capability",
            "feed_read",
            "--capability",
            "feedback",
        ])
        .output()
        .unwrap();
    assert!(
        registered.status.success(),
        "{}",
        String::from_utf8_lossy(&registered.stderr)
    );
    let registered = body(&registered);
    let harness_id = registered["data"]["harness"]["id"].as_str().unwrap();
    let credential = registered["data"]["credential"].as_str().unwrap();
    assert!(credential.starts_with("st_"));

    let listed = environment
        .command()
        .args(["node", "harness", "list"])
        .output()
        .unwrap();
    assert!(listed.status.success());
    assert!(!String::from_utf8_lossy(&listed.stdout).contains(credential));
    let listed = body(&listed);
    assert_eq!(listed["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(listed["data"]["items"][0]["id"], harness_id);
    assert!(listed["data"]["items"][0]["credential_fingerprint"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));

    let shown = environment
        .command()
        .args(["node", "harness", "show", harness_id])
        .output()
        .unwrap();
    assert!(shown.status.success());
    let shown = body(&shown);
    assert_eq!(shown["data"]["id"], harness_id);
    assert_eq!(shown["data"]["status"], "active");
    assert_eq!(
        shown["data"]["grant"]["capabilities"],
        json!(["feed_read", "feedback"])
    );
    assert!(shown["data"].get("credential").is_none());
    assert!(shown["data"].get("token").is_none());
    assert_eq!(shown["data"]["allowed_actions"], json!(["revoke"]));

    let revoked = environment
        .command()
        .args(["node", "harness", "revoke", harness_id])
        .output()
        .unwrap();
    assert!(revoked.status.success());
    assert_eq!(body(&revoked)["data"]["status"], "revoked");
    assert!(environment
        .tools()
        .authenticate_token(credential)
        .unwrap()
        .is_none());
}

#[test]
fn harness_credentials_cannot_use_owner_only_bootstrap_commands() {
    let environment = Environment::new();
    let tools = environment.tools();
    let owner = tools.local_owner_auth_context().unwrap();
    let issued = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "administrator".into(),
                kind: stumble_core::AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Administration],
                pod_ids: None,
            },
        )
        .unwrap();

    for arguments in [
        vec![
            "node",
            "harness",
            "register",
            "--label",
            "child",
            "--kind",
            "unattended",
            "--capability",
            "feed_read",
        ],
        vec!["node", "harness", "revoke", &issued.harness.id.to_string()],
    ] {
        let output = environment
            .command()
            .env("STUMBLE_HARNESS_CREDENTIAL", issued.token.expose())
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3));
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "forbidden");
    }
}

#[test]
fn proposal_workflow_lists_shows_and_allows_only_an_independent_scoped_approver_to_decide() {
    let environment = Environment::new();
    let tools = environment.tools();
    let owner = tools.local_owner_auth_context().unwrap();
    let target = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "target".into(),
                kind: stumble_core::AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::FeedRead],
                pod_ids: None,
            },
        )
        .unwrap();
    let proposer = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "proposer".into(),
                kind: stumble_core::AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Administration],
                pod_ids: None,
            },
        )
        .unwrap();
    let approver = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "approver".into(),
                kind: stumble_core::AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Approval],
                pod_ids: None,
            },
        )
        .unwrap();
    let proposer_context = tools
        .authenticate_token(proposer.token.expose())
        .unwrap()
        .unwrap();
    let proposal = tools
        .request_harness_grant_expansion(
            &proposer_context,
            target.harness.id,
            vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
            None,
            chrono::Utc::now(),
        )
        .unwrap();
    assert_eq!(
        tools
            .agent_harness(&owner, target.harness.id)
            .unwrap()
            .harness
            .grant
            .capabilities,
        vec![HarnessCapability::FeedRead]
    );

    let listed = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", approver.token.expose())
        .args(["node", "proposal", "list"])
        .output()
        .unwrap();
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    assert_eq!(
        body(&listed)["data"]["items"][0]["id"],
        proposal.id.to_string()
    );

    let shown = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", approver.token.expose())
        .args(["node", "proposal", "show", &proposal.id.to_string()])
        .output()
        .unwrap();
    assert!(shown.status.success());
    assert_eq!(body(&shown)["data"]["status"], "pending");
    assert_eq!(
        body(&shown)["data"]["allowed_actions"],
        json!(["approve", "reject"])
    );

    let self_approval = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", proposer.token.expose())
        .args(["node", "proposal", "approve", &proposal.id.to_string()])
        .output()
        .unwrap();
    assert_eq!(self_approval.status.code(), Some(3));

    let approved = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", approver.token.expose())
        .args(["node", "proposal", "approve", &proposal.id.to_string()])
        .output()
        .unwrap();
    assert!(
        approved.status.success(),
        "{}",
        String::from_utf8_lossy(&approved.stderr)
    );
    assert_eq!(body(&approved)["data"]["status"], "accepted");
    assert!(environment
        .tools()
        .agent_harness(&owner, target.harness.id)
        .unwrap()
        .harness
        .grant
        .capabilities
        .contains(&HarnessCapability::Feedback));

    let second = tools
        .request_harness_grant_expansion(
            &proposer_context,
            target.harness.id,
            vec![
                HarnessCapability::FeedRead,
                HarnessCapability::Feedback,
                HarnessCapability::SubscriptionManagement,
            ],
            None,
            chrono::Utc::now(),
        )
        .unwrap();
    let rejected = environment
        .command()
        .args([
            "node",
            "proposal",
            "reject",
            &second.id.to_string(),
            "--reason",
            "too broad",
        ])
        .output()
        .unwrap();
    assert!(
        rejected.status.success(),
        "{}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert_eq!(body(&rejected)["data"]["status"], "rejected");
    assert_eq!(body(&rejected)["data"]["rejection_reason"], "too broad");
    assert!(!environment
        .tools()
        .agent_harness(&owner, target.harness.id)
        .unwrap()
        .harness
        .grant
        .capabilities
        .contains(&HarnessCapability::SubscriptionManagement));
}

#[test]
fn node_lists_honor_bounded_opaque_cursor_pagination() {
    let environment = Environment::new();
    for label in ["one", "two", "three"] {
        let output = environment
            .command()
            .args([
                "node",
                "harness",
                "register",
                "--label",
                label,
                "--kind",
                "interactive",
                "--capability",
                "feed_read",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    let first = environment
        .command()
        .args(["node", "harness", "list", "--limit", "2"])
        .output()
        .unwrap();
    let first = body(&first);
    assert_eq!(first["data"]["items"].as_array().unwrap().len(), 2);
    let cursor = first["data"]["next_cursor"].as_str().unwrap();

    let second = environment
        .command()
        .args([
            "node", "harness", "list", "--limit", "2", "--cursor", cursor,
        ])
        .output()
        .unwrap();
    let second = body(&second);
    assert_eq!(second["data"]["items"].as_array().unwrap().len(), 1);
    assert!(second["data"]["next_cursor"].is_null());
}

#[test]
fn generic_proposal_token_and_tenant_commands_are_not_public() {
    let environment = Environment::new();
    for arguments in [
        ["node", "proposal", "create"].as_slice(),
        ["node", "token", "list"].as_slice(),
        ["node", "tenant", "list"].as_slice(),
    ] {
        let output = environment.command().args(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}

#[allow(dead_code)]
fn _id_is_publicly_parseable(id: &str) -> AgentHarnessId {
    id.parse().unwrap()
}
