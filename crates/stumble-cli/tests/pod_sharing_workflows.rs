use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};
use stumble_api::router_with_base_url;
use stumble_core::AgentTools;

struct Environment {
    root: PathBuf,
    data_dir: PathBuf,
}

impl Environment {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "stumble-pod-sharing-{label}-{}",
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
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].clone()
    }
}

async fn http_get(url: &str) -> (String, Vec<u8>) {
    let (host, path) = url
        .strip_prefix("http://")
        .unwrap()
        .split_once('/')
        .unwrap();
    let mut stream = tokio::net::TcpStream::connect(host).await.unwrap();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream
        .write_all(format!("GET /{path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    (
        String::from_utf8_lossy(&raw[..split]).to_string(),
        raw[split + 4..].to_vec(),
    )
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publish_share_subscribe_and_origin_resync_deliver_the_package_and_content() {
    // ── Alice: create, fill, and publish a Pod entirely through the CLI ──────
    let alice = Environment::new("alice");
    alice.run(&[
        "pod",
        "create",
        "--name",
        "Rust Craft",
        "--slug",
        "rust-craft",
        "--visibility",
        "private",
    ]);
    let alice_cover = alice.root.join("generated-cover.png");
    fs::write(&alice_cover, b"alice-generated-cover").unwrap();
    let added = alice.run(&[
        "add",
        "https://example.com/rust-essay",
        "--pod",
        "rust-craft",
        "--title",
        "A Rust essay",
        "--summary",
        "Ownership explained",
        "--tag",
        "rust",
        "--cover",
        alice_cover.to_str().unwrap(),
    ]);
    let essay_item_id = added["content_item"]["id"].as_str().unwrap().to_string();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());

    let published = alice.run(&["pod", "publish", "rust-craft", "--base-url", &base_url]);
    assert_eq!(published["status"], "published");
    let share_url = published["share_url"].as_str().unwrap().to_string();
    assert_eq!(share_url, format!("{base_url}/federation/pods/rust-craft"));
    assert!(published["announcement"]["signature"].is_string());

    // Publishing is idempotent for the direct Owner.
    let republished = alice.run(&["pod", "publish", "rust-craft"]);
    assert_eq!(republished["status"], "published");

    // ── Serve Alice's node so the share URL is reachable ─────────────────────
    let origin = AgentTools::open_initialized_home_node(&alice.data_dir).unwrap();
    let router = router_with_base_url(origin, &base_url);
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    // ── Bob: subscribe by the shared URL ─────────────────────────────────────
    let bob = Environment::new("bob");
    let subscribed = bob.run(&["pod", "subscribe", &share_url]);
    assert_eq!(subscribed["slug"], "rust-craft");
    assert!(subscribed["imported_events"].as_u64().unwrap() >= 2);

    // Bob's Feed contains Alice's item without any further ceremony.
    let batch = bob.run(&["feed", "batch", "get"]);
    let titles: Vec<_> = batch["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["content_reference"]["title"].as_str().unwrap())
        .collect();
    assert!(titles.contains(&"A Rust essay"), "{titles:?}");
    bob.run(&["feed", "batch", "complete", batch["id"].as_str().unwrap()]);

    // Bob has the Pod Package (CONTEXT.md / SKILL.md) locally — the Pod's
    // portable context arrived with the subscription.
    let package = bob.run(&["pod", "package", "show", "rust-craft"]);
    assert_eq!(package["package"]["version"], 1);
    assert!(package["package"]["context_md"]
        .as_str()
        .unwrap()
        .contains("Rust Craft"));
    assert!(!package["package"]["skill_md"].as_str().unwrap().is_empty());

    // Bob installs the subscribed Pod as a harness skill.
    let skills_dir = bob.root.join("skills");
    let installed = bob.run(&[
        "pod",
        "skill",
        "install",
        "rust-craft",
        "--dir",
        skills_dir.to_str().unwrap(),
    ]);
    assert_eq!(installed["skill_name"], "stumble-rust-craft");
    let skill_md =
        fs::read_to_string(skills_dir.join("stumble-rust-craft/SKILL.md")).unwrap();
    assert!(skill_md.starts_with("---\nname: stumble-rust-craft\n"));
    assert!(skill_md.contains("Use when discovering, adding, curating"));
    assert!(skill_md.contains("stumble add <url> --pod rust-craft"));
    assert!(
        fs::read_to_string(skills_dir.join("stumble-rust-craft/references/CONTEXT.md"))
            .unwrap()
            .contains("Rust Craft")
    );
    // Re-running the install is an idempotent update.
    let reinstalled = bob.run(&[
        "pod",
        "skill",
        "install",
        "rust-craft",
        "--dir",
        skills_dir.to_str().unwrap(),
    ]);
    assert_eq!(reinstalled["package_version"], 1);

    // Alice's own generated cover is served over federation for the public
    // Pod; Bob's harness can fetch it when the source page has nothing usable.
    let cover_url = format!("{base_url}/federation/pods/rust-craft/content/{essay_item_id}/cover");
    let response = http_get(&cover_url).await;
    assert!(
        response.0.starts_with("HTTP/1.1 200"),
        "{}\nbody: {}",
        response.0,
        String::from_utf8_lossy(&response.1)
    );
    assert_eq!(response.1, b"alice-generated-cover");
    let missing = http_get(&format!(
        "{base_url}/federation/pods/rust-craft/content/{}/cover",
        uuid::Uuid::nil()
    ))
    .await;
    assert!(missing.0.starts_with("HTTP/1.1 404"), "{}", missing.0);

    // Bob's status output carries the origin URL his harness needs to build
    // cover URLs later.
    let status = bob.run(&["sync", "pod", "status", "rust-craft"]);
    assert_eq!(
        status["public_pod_url"].as_str().unwrap(),
        format!("{base_url}/federation/pods/rust-craft")
    );

    // If the source ever dies, Bob generates his own depiction locally,
    // keyed by his node's own id for the projected item.
    let bob_items = bob.run(&["pod", "content", "list", "rust-craft"]);
    let bob_item_id = bob_items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["content_item"]["title"] == "A Rust essay")
        .map(|item| {
            item["content_item"]["id"]
                .as_str()
                .unwrap_or_else(|| item["content_item"]["content_item_id"].as_str().unwrap())
                .to_string()
        })
        .unwrap_or_else(|| panic!("essay in {bob_items}"));
    let bob_cover = bob.root.join("bob-generated.png");
    fs::write(&bob_cover, b"bob-depiction").unwrap();
    let bob_asset = bob.run(&[
        "pod",
        "content",
        "cover",
        "rust-craft",
        &bob_item_id,
        "--file",
        bob_cover.to_str().unwrap(),
        "--alt",
        "generated from the summary",
    ]);
    assert_eq!(bob_asset["source"], "ai_generated");
    assert!(fs::read(bob_asset["local_path"].as_str().unwrap()).unwrap() == b"bob-depiction");

    // ── Alice adds more through the CLI while the server keeps running; the
    // long-lived server must observe the new store generation ────────────────
    alice.run(&[
        "add",
        "https://example.com/borrow-checker",
        "--pod",
        "rust-craft",
        "--title",
        "Borrow checker deep dive",
    ]);

    let resync = bob.run(&["sync", "pod", "run", "rust-craft"]);
    assert_eq!(resync["verification"], "verified");
    assert_eq!(resync["peer_id"], Value::Null);
    assert!(resync["imported_events"].as_u64().unwrap() >= 1);

    let batch = bob.run(&["feed", "batch", "get"]);
    let titles: Vec<_> = batch["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["content_reference"]["title"].as_str().unwrap())
        .collect();
    assert!(titles.contains(&"Borrow checker deep dive"), "{titles:?}");

    server.abort();
    let _ = server.await;
}

#[test]
fn harness_publish_returns_a_pending_proposal_instead_of_self_approving() {
    let owner = Environment::new("harness-owner");
    owner.run(&[
        "pod",
        "create",
        "--name",
        "Curated",
        "--slug",
        "curated",
        "--visibility",
        "private",
    ]);
    let issued = owner.run(&[
        "node",
        "harness",
        "register",
        "--label",
        "curator",
        "--kind",
        "interactive",
        "--capability",
        "pod_curation",
    ]);
    let credential = issued["credential"].as_str().unwrap();

    let output = owner
        .command()
        .args(["pod", "publish", "curated"])
        .env("STUMBLE_HARNESS_CREDENTIAL", credential)
        .output()
        .unwrap();
    assert!(output.status.success());
    let published: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(published["data"]["status"], "pending_approval");
    let proposal_id = published["data"]["proposal"]["id"].as_str().unwrap();

    // The bare Owner approves, and a plain re-publish now succeeds.
    owner.run(&["node", "proposal", "approve", proposal_id]);
    let republished = owner.run(&["pod", "publish", "curated"]);
    assert_eq!(republished["status"], "published");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subscribing_over_a_local_slug_collision_explains_the_conflict() {
    let alice = Environment::new("collision-origin");
    alice.run(&[
        "pod", "create", "--name", "Shared", "--slug", "shared", "--visibility", "private",
    ]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    alice.run(&["pod", "publish", "shared", "--base-url", &base_url]);
    let origin = AgentTools::open_initialized_home_node(&alice.data_dir).unwrap();
    let router = router_with_base_url(origin, &base_url);
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let bob = Environment::new("collision-home");
    bob.run(&[
        "pod", "create", "--name", "Mine", "--slug", "shared", "--visibility", "private",
    ]);
    let output = bob
        .command()
        .args(["pod", "subscribe", &format!("{base_url}/federation/pods/shared")])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("a local Pod already uses the slug shared"),
        "{stderr}"
    );

    server.abort();
    let _ = server.await;
}

#[test]
fn harnesses_cannot_install_pod_skills_for_themselves() {
    let owner = Environment::new("skill-gate");
    owner.run(&[
        "pod", "create", "--name", "Gated", "--slug", "gated", "--visibility", "private",
    ]);
    let issued = owner.run(&[
        "node", "harness", "register", "--label", "curator", "--kind", "interactive",
        "--capability", "pod_curation", "--capability", "feed_read",
    ]);
    let credential = issued["credential"].as_str().unwrap();
    let output = owner
        .command()
        .args(["pod", "skill", "install", "gated", "--dir", "/tmp/unused"])
        .env("STUMBLE_HARNESS_CREDENTIAL", credential)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "owner_required");
}
