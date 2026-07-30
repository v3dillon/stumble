use serde_json::Value;
use std::{fs, path::PathBuf, process::Command, sync::Arc};
use stumble_api::{router_with_base_url, ReqwestOriginProbe};
use stumble_core::{seed_store, AgentTools};

struct Environment {
    root: PathBuf,
    data_dir: PathBuf,
}

impl Environment {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "stumble-network-discovery-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        let data_dir = root.join("home");
        fs::create_dir_all(&root).unwrap();
        let environment = Self { root, data_dir };
        environment.run(&["node", "init"]);
        environment
    }

    fn run(&self, arguments: &[&str]) -> Value {
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
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].clone()
    }

    /// Disables every configured Bootstrap endpoint (e.g. the sponsored default).
    fn disable_all_bootstraps(&self) {
        let endpoints = self.run(&["sync", "bootstrap", "list"]);
        for endpoint in endpoints.as_array().unwrap() {
            self.run(&[
                "sync",
                "bootstrap",
                "disable",
                endpoint["id"].as_str().unwrap(),
            ]);
        }
    }

    /// Points this node at exactly one Bootstrap: the test's live one.
    fn use_bootstrap(&self, base_url: &str) {
        self.disable_all_bootstraps();
        self.run(&[
            "sync",
            "bootstrap",
            "add",
            "--label",
            "test-bootstrap",
            "--base-url",
            base_url,
        ]);
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn published_pods_travel_bootstrap_to_explore_to_subscription() {
    // ── A live Bootstrap node with open admission and a real Origin probe ────
    let bootstrap_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bootstrap_base = format!("http://{}", bootstrap_listener.local_addr().unwrap());
    let bootstrap_tools = AgentTools::new(seed_store())
        .with_bootstrap_capability(true, Arc::new(ReqwestOriginProbe));
    let bootstrap_router = router_with_base_url(bootstrap_tools, &bootstrap_base);
    let bootstrap_server = tokio::spawn(async move {
        axum::serve(bootstrap_listener, bootstrap_router).await.unwrap()
    });

    // ── Alice: origin node serving before she publishes, so the Bootstrap can
    // probe her live manifest during announcement admission ──────────────────
    let alice = Environment::new("alice");
    alice.run(&[
        "pod", "create", "--name", "Distributed Craft", "--slug", "distributed-craft",
        "--description", "Distributed systems craft and reliability engineering.",
        "--visibility", "private",
    ]);
    alice.run(&[
        "add",
        "https://example.com/raft-explained",
        "--pod",
        "distributed-craft",
        "--title",
        "Raft explained visually",
        "--tag",
        "distributed-systems",
    ]);
    alice.use_bootstrap(&bootstrap_base);

    let alice_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let alice_base = format!("http://{}", alice_listener.local_addr().unwrap());
    let alice_origin = AgentTools::open_initialized_home_node(&alice.data_dir).unwrap();
    let alice_router = router_with_base_url(alice_origin, &alice_base);
    let alice_server =
        tokio::spawn(async move { axum::serve(alice_listener, alice_router).await.unwrap() });

    // Publish announces to the configured Bootstrap in the same command.
    let published = alice.run(&[
        "pod",
        "publish",
        "distributed-craft",
        "--base-url",
        &alice_base,
    ]);
    assert_eq!(published["status"], "published");
    let submissions = published["bootstrap_submissions"].as_array().unwrap();
    assert_eq!(submissions.len(), 1, "{submissions:?}");
    assert_eq!(submissions[0]["status"], "admitted", "{submissions:?}");

    // Alice keeps curating after publishing: the Bootstrap's announcement is
    // now stale (it binds the old event pointer), which would silently break
    // sample fetches. `pod announce` re-signs and re-pushes current state.
    alice.run(&[
        "add",
        "https://example.com/paxos-made-live",
        "--pod",
        "distributed-craft",
        "--title",
        "Paxos made live",
        "--tag",
        "distributed-systems",
    ]);
    let announced = alice.run(&["pod", "announce", "distributed-craft"]);
    assert_eq!(
        announced["refreshed"][0]["pod_slug"], "distributed-craft",
        "{announced}"
    );
    assert_eq!(
        announced["bootstrap_submissions"][0]["status"], "admitted",
        "{announced}"
    );

    // ── Carol: publishes her own Pod, then endorses Alice's from it ──────────
    let carol = Environment::new("carol");
    carol.use_bootstrap(&bootstrap_base);
    carol.run(&[
        "pod", "create", "--name", "Chaos Engineering", "--slug", "chaos-eng",
        "--description", "Failure injection and resilience.", "--visibility", "private",
    ]);
    let carol_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let carol_base = format!("http://{}", carol_listener.local_addr().unwrap());
    let carol_origin = AgentTools::open_initialized_home_node(&carol.data_dir).unwrap();
    let carol_router = router_with_base_url(carol_origin, &carol_base);
    let carol_server =
        tokio::spawn(async move { axum::serve(carol_listener, carol_router).await.unwrap() });
    carol.run(&["pod", "publish", "chaos-eng", "--base-url", &carol_base]);
    carol.run(&["sync", "bootstrap", "run"]);
    let endorsed = carol.run(&[
        "pod",
        "endorse",
        "distributed-craft",
        "--from",
        "chaos-eng",
        "--reason",
        "Best practical distributed systems collection I know.",
    ]);
    assert_eq!(endorsed["endorsed_pod"], "distributed-craft");
    assert_eq!(
        endorsed["bootstrap_submissions"][0]["status"], "admitted",
        "{endorsed}"
    );

    // ── Bob: pulls the Announcement Stream and discovers Alice's Pod locally ─
    let bob = Environment::new("bob");
    bob.use_bootstrap(&bootstrap_base);
    let report = bob.run(&["sync", "bootstrap", "run"]);
    assert!(
        report["retained_announcements"].as_u64().unwrap() >= 1,
        "{report}"
    );

    let explored = bob.run(&["pod", "explore", "--query", "distributed systems"]);
    let items = explored["items"].as_array().unwrap();
    let found = items
        .iter()
        .find(|item| item["announcement"]["pod_slug"] == "distributed-craft")
        .unwrap_or_else(|| panic!("expected discovered pod in {explored}"));
    assert_eq!(found["is_subscribed"], false);
    // Explore fetched verified Origin samples, so Bob previews real content
    // from a Pod he has never subscribed to.
    let samples = found["sample_content_references"].as_array().unwrap();
    assert!(
        samples
            .iter()
            .any(|sample| sample["title"] == "Raft explained visually"),
        "{samples:?}"
    );
    // The re-announced pointer is current, so post-publication content
    // appears in the signed previews too.
    assert!(
        samples.iter().any(|sample| sample["title"] == "Paxos made live"),
        "{samples:?}"
    );
    // Carol's endorsement traveled through the Bootstrap and is inspectable
    // evidence in Bob's local ranking.
    let endorsements = found["endorsements"].as_array().unwrap();
    assert!(
        endorsements
            .iter()
            .any(|endorsement| endorsement["endorsing_pod_slug"] == "chaos-eng"),
        "{endorsements:?}"
    );
    let public_pod_url = found["announcement"]["public_pod_url"].as_str().unwrap();

    // Explore hands Bob everything needed to subscribe in one command.
    let subscribed = bob.run(&["pod", "subscribe", public_pod_url]);
    assert_eq!(subscribed["slug"], "distributed-craft");

    let batch = bob.run(&["feed", "batch", "get"]);
    let titles: Vec<_> = batch["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["content_reference"]["title"].as_str().unwrap())
        .collect();
    assert!(titles.contains(&"Raft explained visually"), "{titles:?}");

    alice_server.abort();
    carol_server.abort();
    bootstrap_server.abort();
    let _ = alice_server.await;
    let _ = carol_server.await;
    let _ = bootstrap_server.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn index_search_discovers_pods_without_announcement_sync() {
    // One node serving both network roles: Bootstrap admission + Index search.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let index_base = format!("http://{}", listener.local_addr().unwrap());
    let index_tools = AgentTools::new(seed_store())
        .with_bootstrap_capability(true, Arc::new(ReqwestOriginProbe))
        .with_index_capability(true);
    let index_router = router_with_base_url(index_tools, &index_base);
    let index_server =
        tokio::spawn(async move { axum::serve(listener, index_router).await.unwrap() });

    let alice = Environment::new("index-alice");
    alice.run(&[
        "pod", "create", "--name", "Type Systems", "--slug", "type-systems",
        "--description", "Type theory in practice.", "--visibility", "private",
    ]);
    alice.use_bootstrap(&index_base);
    let alice_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let alice_base = format!("http://{}", alice_listener.local_addr().unwrap());
    let alice_origin = AgentTools::open_initialized_home_node(&alice.data_dir).unwrap();
    let alice_router = router_with_base_url(alice_origin, &alice_base);
    let alice_server =
        tokio::spawn(async move { axum::serve(alice_listener, alice_router).await.unwrap() });
    let published = alice.run(&["pod", "publish", "type-systems", "--base-url", &alice_base]);
    assert_eq!(published["bootstrap_submissions"][0]["status"], "admitted");

    // Bob never syncs the Announcement Stream; the Index alone surfaces the Pod.
    let bob = Environment::new("index-bob");
    bob.disable_all_bootstraps();
    let added = bob.run(&[
        "sync",
        "discovery",
        "index",
        "add",
        "--label",
        "test-index",
        "--base-url",
        &index_base,
    ]);
    assert_eq!(added["status"], "applied", "{added}");
    let listed = bob.run(&["sync", "discovery", "index", "list"]);
    assert_eq!(listed.as_array().unwrap().len(), 1);

    let explored = bob.run(&["pod", "explore", "--query", "type theory"]);
    let found = explored["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["announcement"]["pod_slug"] == "type-systems")
        .unwrap_or_else(|| panic!("expected index-discovered pod in {explored}"))
        .clone();
    assert_eq!(found["is_subscribed"], false);

    let subscribed = bob.run(&[
        "pod",
        "subscribe",
        found["announcement"]["public_pod_url"].as_str().unwrap(),
    ]);
    assert_eq!(subscribed["slug"], "type-systems");

    let removed = bob.run(&["sync", "discovery", "index", "remove", &index_base]);
    assert_eq!(removed["status"], "applied", "{removed}");
    assert!(bob.run(&["sync", "discovery", "index", "list"])
        .as_array()
        .unwrap()
        .is_empty());

    alice_server.abort();
    index_server.abort();
    let _ = alice_server.await;
    let _ = index_server.await;
}
