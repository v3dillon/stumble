use serde_json::{json, Value};
use std::{path::PathBuf, process::Command};
use stumble_core::{
    AgentHarnessKind, AgentTools, AuthContext, CreatePodRequest, HarnessCapability, PodRole,
    PodRoleAssignment, RegisterAgentHarnessRequest, Visibility,
};
use uuid::Uuid;

struct Environment {
    root: PathBuf,
}

impl Environment {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("stumble-pod-relationships-{}", Uuid::now_v7()));
        let environment = Self { root };
        let output = environment
            .command()
            .args(["node", "init", "--demo"])
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
fn owner_subscribes_sets_priority_and_unsubscribes_without_losing_authority() {
    let environment = Environment::new();
    let tools = environment.tools();
    let owner = tools.local_owner_auth_context().unwrap();
    let pod = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Rust craft".into(),
                slug: "rust-craft".into(),
                description: "Deep Rust practice".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();

    let shown = environment
        .command()
        .args(["pod", "show", "rust-craft"])
        .output()
        .unwrap();
    assert!(
        shown.status.success(),
        "{}",
        String::from_utf8_lossy(&shown.stderr)
    );
    assert_eq!(body(&shown)["data"]["id"], pod.id.to_string());
    assert_eq!(body(&shown)["data"]["pod_id"], pod.id.to_string());
    assert_eq!(body(&shown)["data"]["slug"], "rust-craft");
    assert_eq!(
        body(&shown)["data"]["allowed_actions"],
        json!([
            "subscribe",
            "role_list",
            "visibility_set",
            "role_grant",
            "role_revoke"
        ])
    );

    let subscribed = environment
        .command()
        .args(["pod", "subscribe", &pod.id.to_string()])
        .output()
        .unwrap();
    assert!(
        subscribed.status.success(),
        "{}",
        String::from_utf8_lossy(&subscribed.stderr)
    );
    assert_eq!(body(&subscribed)["data"]["pod_id"], pod.id.to_string());
    assert_eq!(body(&subscribed)["data"]["slug"], "rust-craft");

    let prioritized = environment
        .command()
        .args([
            "pod",
            "subscription",
            "set",
            "rust-craft",
            "--priority",
            "true",
        ])
        .output()
        .unwrap();
    assert!(
        prioritized.status.success(),
        "{}",
        String::from_utf8_lossy(&prioritized.stderr)
    );
    assert_eq!(body(&prioritized)["data"]["is_priority"], true);

    let unsubscribed = environment
        .command()
        .args(["pod", "unsubscribe", "rust-craft"])
        .output()
        .unwrap();
    assert!(
        unsubscribed.status.success(),
        "{}",
        String::from_utf8_lossy(&unsubscribed.stderr)
    );
    let roles = environment
        .command()
        .args(["pod", "role", "list", "rust-craft"])
        .output()
        .unwrap();
    assert!(
        roles.status.success(),
        "{}",
        String::from_utf8_lossy(&roles.stderr)
    );
    assert_eq!(body(&roles)["data"]["items"][0]["role"], "owner");
}

#[test]
fn every_pod_command_uses_the_same_exact_slug_or_id_resolver() {
    let environment = Environment::new();
    let tools = environment.tools();
    let owner = tools.local_owner_auth_context().unwrap();
    let pod = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Resolver".into(),
                slug: "resolver".into(),
                description: "Uniform references".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    for reference in [pod.slug.as_str(), &pod.id.to_string()] {
        let output = environment
            .command()
            .args(["pod", "show", reference])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(body(&output)["data"]["id"], pod.id.to_string());
        assert_eq!(body(&output)["data"]["pod_id"], pod.id.to_string());
    }

    let listed = environment
        .command()
        .args(["pod", "list", "--limit", "1"])
        .output()
        .unwrap();
    assert_eq!(
        body(&listed)["data"]["items"][0]["pod_id"],
        pod.id.to_string()
    );
    assert_eq!(body(&listed)["data"]["items"][0]["slug"], "resolver");
}

#[test]
fn role_grant_and_revoke_return_proposals_that_need_an_independent_approver() {
    let environment = Environment::new();
    let tools = environment.tools();
    let owner = tools.local_owner_auth_context().unwrap();
    let pod = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Role workflow".into(),
                slug: "role-workflow".into(),
                description: "Approved governance".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let target_user_id = *tools
        .store()
        .read()
        .unwrap()
        .users
        .keys()
        .find(|user_id| Some(**user_id) != owner.user_id)
        .unwrap();
    let proposer = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "Role proposer".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::PodCuration],
                pod_ids: Some(vec![pod.id]),
            },
        )
        .unwrap();
    let approver = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "Role approver".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Approval],
                pod_ids: Some(vec![pod.id]),
            },
        )
        .unwrap();

    let proposed = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", proposer.token.expose())
        .args([
            "pod",
            "role",
            "grant",
            "role-workflow",
            "--user-id",
            &target_user_id.to_string(),
            "--role",
            "curator",
        ])
        .output()
        .unwrap();
    assert!(
        proposed.status.success(),
        "{}",
        String::from_utf8_lossy(&proposed.stderr)
    );
    let proposal_id = body(&proposed)["data"]["proposal"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(body(&proposed)["data"]["pod_id"], pod.id.to_string());
    assert_eq!(body(&proposed)["data"]["slug"], "role-workflow");

    let self_approval = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", proposer.token.expose())
        .args(["node", "proposal", "approve", &proposal_id])
        .output()
        .unwrap();
    assert_eq!(self_approval.status.code(), Some(3));

    let approved = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", approver.token.expose())
        .args(["node", "proposal", "approve", &proposal_id])
        .output()
        .unwrap();
    assert!(
        approved.status.success(),
        "{}",
        String::from_utf8_lossy(&approved.stderr)
    );
    let roles = environment
        .command()
        .args(["pod", "role", "list", &pod.id.to_string()])
        .output()
        .unwrap();
    assert_eq!(body(&roles)["data"]["items"].as_array().unwrap().len(), 2);

    let revoke = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", proposer.token.expose())
        .args([
            "pod",
            "role",
            "revoke",
            &pod.id.to_string(),
            "--user-id",
            &target_user_id.to_string(),
            "--role",
            "curator",
        ])
        .output()
        .unwrap();
    assert!(
        revoke.status.success(),
        "{}",
        String::from_utf8_lossy(&revoke.stderr)
    );
    let revoke_id = body(&revoke)["data"]["proposal"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let approved = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", approver.token.expose())
        .args(["node", "proposal", "approve", &revoke_id])
        .output()
        .unwrap();
    assert!(
        approved.status.success(),
        "{}",
        String::from_utf8_lossy(&approved.stderr)
    );
    let roles = environment
        .command()
        .args(["pod", "role", "list", "role-workflow"])
        .output()
        .unwrap();
    assert_eq!(body(&roles)["data"]["items"].as_array().unwrap().len(), 1);
}

#[test]
fn subscribe_accepts_a_public_url_before_applying_canonical_address_validation() {
    let environment = Environment::new();
    let output = environment
        .command()
        .args(["pod", "subscribe", "https://origin.example/not-a-pod"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "validation_error");
}

#[test]
fn detailed_allowed_actions_respect_relationship_capability_and_pod_scope() {
    let environment = Environment::new();
    let tools = environment.tools();
    let owner = tools.local_owner_auth_context().unwrap();
    let pod = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Action scope".into(),
                slug: "action-scope".into(),
                description: "Actor-specific next actions".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let other = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Outside scope".into(),
                slug: "outside-scope".into(),
                description: "Not granted".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let users = tools
        .store()
        .read()
        .unwrap()
        .users
        .keys()
        .copied()
        .filter(|user_id| Some(*user_id) != owner.user_id)
        .collect::<Vec<_>>();
    let curator_id = users[0];
    let subscriber_id = users[1];
    tools
        .store()
        .write()
        .unwrap()
        .pod_roles
        .push(PodRoleAssignment {
            user_id: curator_id,
            pod_id: pod.id,
            role: PodRole::Curator,
            created_at: chrono::Utc::now(),
        });
    let context = |user_id| AuthContext {
        user_id: Some(user_id),
        tenant_id: owner.tenant_id,
        node_id: owner.node_id,
        harness_id: None,
    };
    tools
        .subscribe_local_pod(&context(subscriber_id), pod.id)
        .unwrap();
    let curator = tools
        .register_agent_harness(
            &context(curator_id),
            RegisterAgentHarnessRequest {
                label: "Scoped Curator".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::PodCuration],
                pod_ids: Some(vec![pod.id]),
            },
        )
        .unwrap();
    let subscriber = tools
        .register_agent_harness(
            &context(subscriber_id),
            RegisterAgentHarnessRequest {
                label: "Scoped Subscriber".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::SubscriptionManagement],
                pod_ids: Some(vec![pod.id]),
            },
        )
        .unwrap();

    let curator_show = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", curator.token.expose())
        .args(["pod", "show", "action-scope"])
        .output()
        .unwrap();
    assert_eq!(
        body(&curator_show)["data"]["allowed_actions"],
        json!(["role_list"])
    );
    let subscriber_show = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", subscriber.token.expose())
        .args(["pod", "show", &pod.id.to_string()])
        .output()
        .unwrap();
    assert_eq!(
        body(&subscriber_show)["data"]["allowed_actions"],
        json!(["unsubscribe", "subscription_set"])
    );
    let outside_scope = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", subscriber.token.expose())
        .args(["pod", "show", &other.id.to_string()])
        .output()
        .unwrap();
    assert_eq!(outside_scope.status.code(), Some(4));
    let error: Value = serde_json::from_slice(&outside_scope.stderr).unwrap();
    assert_eq!(error["error"]["code"], "not_found");
}
