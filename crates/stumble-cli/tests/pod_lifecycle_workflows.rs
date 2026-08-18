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
        let actions = shown["data"]["allowed_actions"].as_array().unwrap();
        assert!(actions.contains(&Value::String("visibility_set".into())));
        assert!(actions.contains(&Value::String("delete".into())));
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

#[test]
fn owner_deletes_a_private_pod_and_keeps_content_that_still_has_another_placement() {
    let environment = Environment::new();
    environment.run(&[
        "pod",
        "create",
        "--name",
        "Rust",
        "--slug",
        "rust",
        "--visibility",
        "private",
    ]);
    environment.run(&[
        "pod",
        "create",
        "--name",
        "Keep",
        "--slug",
        "keep",
        "--visibility",
        "private",
    ]);
    let only_here = environment.run(&[
        "add",
        "https://example.com/only-rust",
        "--pod",
        "rust",
        "--title",
        "Only Rust",
        "--summary",
        "Lives in one Pod",
    ]);
    let shared = environment.run(&[
        "add",
        "https://example.com/shared",
        "--pod",
        "rust",
        "--title",
        "Shared",
        "--summary",
        "Lives in two Pods",
    ]);
    let shared_id = shared["data"]["content_item"]["id"].as_str().unwrap();
    environment.run(&["pod", "content", "add", "keep", shared_id]);

    let deleted = environment.run(&["pod", "delete", "rust"]);
    assert_eq!(deleted["data"]["status"], "deleted");
    assert_eq!(deleted["data"]["result"]["slug"], "rust");
    assert_eq!(deleted["data"]["result"]["withdrawn"], false);

    let listed = environment.run(&["pod", "list"]);
    let slugs = listed["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["slug"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert!(slugs.contains(&"keep".to_string()));
    assert!(!slugs.contains(&"rust".to_string()));

    let missing = environment
        .command()
        .args(["pod", "show", "rust"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(4));

    let kept = environment.run(&["pod", "content", "list", "keep"]);
    assert_eq!(kept["data"]["items"][0]["content_item"]["id"], shared_id);
    let only_id = only_here["data"]["content_item"]["id"].as_str().unwrap();
    let orphaned = environment
        .command()
        .args(["pod", "content", "show", "keep", only_id])
        .output()
        .unwrap();
    assert!(!orphaned.status.success());
}

#[test]
fn owner_cannot_delete_the_private_inbox() {
    let environment = Environment::new();
    let tools = AgentTools::open_initialized_home_node(&environment.data_dir).unwrap();
    let owner = tools.local_owner_auth_context().unwrap();
    let user_id = owner.user_id.unwrap();
    let inbox_slug = format!("inbox-{user_id}");
    environment.run(&[
        "pod",
        "create",
        "--name",
        "Inbox",
        "--slug",
        &inbox_slug,
        "--visibility",
        "private",
    ]);
    let failed = environment
        .command()
        .args(["pod", "delete", &inbox_slug])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(4));
    let error: Value = serde_json::from_slice(&failed.stderr).unwrap();
    assert_eq!(error["error"]["code"], "validation_error");
    assert_eq!(
        environment.run(&["pod", "show", &inbox_slug])["data"]["slug"],
        inbox_slug
    );
}

#[test]
fn owner_deletes_a_public_pod_immediately_and_a_harness_gets_a_proposal() {
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
    environment.run(&[
        "pod",
        "visibility",
        "set",
        "public",
        "--visibility",
        "public",
    ]);
    let deleted = environment.run(&["pod", "delete", "public"]);
    assert_eq!(deleted["data"]["status"], "deleted");
    assert_eq!(deleted["data"]["result"]["withdrawn"], true);

    environment.run(&[
        "pod",
        "create",
        "--name",
        "Later",
        "--slug",
        "later",
        "--visibility",
        "private",
    ]);
    environment.run(&[
        "pod",
        "visibility",
        "set",
        "later",
        "--visibility",
        "public",
    ]);
    let registered = environment.run(&[
        "node",
        "harness",
        "register",
        "--label",
        "delete proposer",
        "--kind",
        "interactive",
        "--capability",
        "pod_curation",
    ]);
    let credential = registered["data"]["credential"].as_str().unwrap();
    let proposed = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", credential)
        .args(["pod", "delete", "later"])
        .output()
        .unwrap();
    assert!(proposed.status.success());
    let proposed: Value = serde_json::from_slice(&proposed.stdout).unwrap();
    assert_eq!(proposed["data"]["status"], "pending_approval");
    assert_eq!(
        environment.run(&["pod", "show", "later"])["data"]["slug"],
        "later"
    );
}
