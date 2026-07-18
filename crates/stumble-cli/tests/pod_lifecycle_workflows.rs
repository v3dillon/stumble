use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};
use stumble_core::{AgentTools, PackageVersion};

struct Environment {
    root: PathBuf,
    data_dir: PathBuf,
}

impl Environment {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "stumble-pod-lifecycle-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let data_dir = root.join("home");
        fs::create_dir_all(&root).unwrap();
        let environment = Self { root, data_dir };
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
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn pod_create_list_and_show_use_explicit_visibility_canonical_references_and_actions() {
    let environment = Environment::new();
    let missing = environment
        .command()
        .args(["pod", "create", "--name", "Rust", "--slug", "rust"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));

    let created = environment.run(&[
        "pod",
        "create",
        "--name",
        "Rust",
        "--slug",
        "rust",
        "--visibility",
        "private",
    ]);
    assert_eq!(created["data"]["status"], "created");
    let pod_id = created["data"]["result"]["id"].as_str().unwrap();
    assert_eq!(created["data"]["result"]["pod_id"], pod_id);
    assert_eq!(created["data"]["result"]["slug"], "rust");

    for reference in ["rust", pod_id] {
        let shown = environment.run(&["pod", "show", reference]);
        assert_eq!(shown["data"]["pod_id"], pod_id);
        assert_eq!(shown["data"]["slug"], "rust");
        assert!(shown["data"]["allowed_actions"]
            .as_array()
            .unwrap()
            .contains(&Value::String("visibility_set".into())));
    }

    let listed = environment.run(&["pod", "list", "--limit", "1"]);
    assert_eq!(listed["data"]["items"][0]["pod_id"], pod_id);
    assert_eq!(listed["data"]["items"][0]["slug"], "rust");
    assert!(listed["data"]["items"][0].get("allowed_actions").is_none());
}

#[test]
fn package_directory_and_from_pod_are_mutually_exclusive_and_derivation_keeps_provenance() {
    let environment = Environment::new();
    let package_dir = environment.root.join("package");
    fs::create_dir_all(&package_dir).unwrap();
    for (name, contents) in [
        ("CONTEXT.md", "# Systems\n\nReliable systems scope.\n"),
        ("SKILL.md", "# Discovery\n\nPrefer primary sources.\n"),
        (
            "sources.yaml",
            "source_rules:\n  - inspect:\n      kind: publication\n      name: systems journals\n    seek:\n      description: reliability research\n    schedule:\n      cadence: weekly\n",
        ),
        ("filters.yaml", "blocked_topics: []\nblocked_domains: []\n"),
        ("examples.good.md", "# Good\n\n- Primary research.\n"),
        ("examples.bad.md", "# Bad\n\n- Unsourced claims.\n"),
        ("events.jsonl", ""),
    ] {
        fs::write(package_dir.join(name), contents).unwrap();
    }
    let source = environment.run(&[
        "pod",
        "create",
        "--name",
        "Source",
        "--slug",
        "source",
        "--visibility",
        "private",
        "--package",
        package_dir.to_str().unwrap(),
    ]);
    let source_id = source["data"]["result"]["id"].as_str().unwrap();
    let conflict = environment
        .command()
        .args([
            "pod",
            "create",
            "--name",
            "Conflict",
            "--slug",
            "conflict",
            "--visibility",
            "private",
            "--package",
            environment.root.to_str().unwrap(),
            "--from-pod",
            source_id,
        ])
        .output()
        .unwrap();
    assert_eq!(conflict.status.code(), Some(2));

    environment.run(&[
        "pod",
        "create",
        "--name",
        "Derived",
        "--slug",
        "derived",
        "--visibility",
        "invite-only",
        "--from-pod",
        source_id,
    ]);
    let tools = AgentTools::open_initialized_home_node(&environment.data_dir).unwrap();
    let owner = tools.local_owner_auth_context().unwrap();
    let source = tools
        .get_pod_package_version(&owner, "source", PackageVersion::new(1).unwrap())
        .unwrap();
    let derived = tools
        .get_pod_package_version(&owner, "derived", PackageVersion::new(1).unwrap())
        .unwrap();
    assert!(derived
        .pod_yaml
        .contains(&format!("forked_from_skill_pack: {}", source.id)));
    assert_eq!(derived.context_md, source.context_md);
}

#[test]
fn public_creation_and_visibility_expansion_wait_for_approval_without_partial_pods() {
    let environment = Environment::new();
    let registered = environment.run(&[
        "node",
        "harness",
        "register",
        "--label",
        "lifecycle proposer",
        "--kind",
        "interactive",
        "--capability",
        "pod_curation",
        "--capability",
        "package_management",
    ]);
    let credential = registered["data"]["credential"].as_str().unwrap();

    let proposed = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", credential)
        .args([
            "pod",
            "create",
            "--name",
            "Public",
            "--slug",
            "public",
            "--visibility",
            "public",
        ])
        .output()
        .unwrap();
    assert!(
        proposed.status.success(),
        "{}",
        String::from_utf8_lossy(&proposed.stderr)
    );
    let proposed: Value = serde_json::from_slice(&proposed.stdout).unwrap();
    assert_eq!(proposed["data"]["status"], "pending_approval");
    let proposal_id = proposed["data"]["result"]["id"].as_str().unwrap();
    assert!(environment.run(&["pod", "list"])["data"]["items"]
        .as_array()
        .unwrap()
        .is_empty());

    environment.run(&["node", "proposal", "approve", proposal_id]);
    let shown = environment.run(&["pod", "show", "public"]);
    assert_eq!(shown["data"]["visibility"], "public");

    let restricted = environment.run(&[
        "pod",
        "visibility",
        "set",
        "public",
        "--visibility",
        "private",
    ]);
    assert_eq!(restricted["data"]["outcome"]["status"], "updated");
    assert_eq!(
        environment.run(&["pod", "show", "public"])["data"]["visibility"],
        "private"
    );

    let expanded = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", credential)
        .args([
            "pod",
            "visibility",
            "set",
            "public",
            "--visibility",
            "invite-only",
        ])
        .output()
        .unwrap();
    assert!(expanded.status.success());
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    assert_eq!(expanded["data"]["outcome"]["status"], "pending_approval");
    assert_eq!(
        environment.run(&["pod", "show", "public"])["data"]["visibility"],
        "private"
    );
}

#[test]
fn explore_returns_a_paginated_read_only_collection() {
    let environment = Environment::new();
    let explored = environment.run(&[
        "pod",
        "explore",
        "--query",
        "systems",
        "--sample-size",
        "2",
        "--limit",
        "10",
    ]);
    assert_eq!(explored["data"]["query"], "systems");
    assert!(explored["data"]["items"].is_array());
    assert!(explored["data"]["next_cursor"].is_null());
    assert!(environment.run(&["pod", "list"])["data"]["items"]
        .as_array()
        .unwrap()
        .is_empty());
}
