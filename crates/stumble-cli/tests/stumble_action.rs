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
        let root =
            std::env::temp_dir().join(format!("stumble-action-{label}-{}", uuid::Uuid::now_v7()));
        let data_dir = root.join("home");
        fs::create_dir_all(&root).unwrap();
        let environment = Self { root, data_dir };
        environment.run(&["node", "init"]);
        environment
    }

    fn raw(&self, arguments: &[&str]) -> std::process::Output {
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
        output
    }

    fn run(&self, arguments: &[&str]) -> Value {
        let output = self.raw(arguments);
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].clone()
    }

    /// One Stumble with the machine-readable envelope.
    fn stumble(&self) -> Value {
        self.run(&["--format", "json"])
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

#[test]
fn stumble_walks_the_feed_one_new_item_at_a_time() {
    let user = Environment::new("local");
    // Distinct source domains keep the constrained Feed Mix's per-source cap
    // from splitting these across batches.
    let saved = [
        (
            "https://example.com/bell-labs",
            "Why Bell Labs Worked",
            "Unstructured research time produced the transistor and Unix.",
        ),
        (
            "https://example.org/local-first",
            "Local-first software",
            "Seven ideals for software that keeps data on your device.",
        ),
        (
            "https://example.net/small-web",
            "Rediscovering the small web",
            "Personal sites as an antidote to platform feeds.",
        ),
    ];
    for (url, title, summary) in saved {
        user.run(&[
            "add",
            url,
            "--title",
            title,
            "--summary",
            summary,
            "--tag",
            "curated",
            "--image",
            "https://example.com/hero.png",
        ]);
    }

    // Three runs walk the whole batch without repeating an item.
    let mut shown_ids = Vec::new();
    for expected_position in 1..=3u64 {
        let item = user.stumble();
        assert_eq!(item["kind"], "feed_item", "{item}");
        assert_eq!(item["batch"]["position"], expected_position, "{item}");
        assert_eq!(item["batch"]["total"], 3, "{item}");
        let reference = &item["item"]["content_reference"];
        let title = reference["title"].as_str().unwrap();
        assert!(
            saved
                .iter()
                .any(|(_, saved_title, _)| *saved_title == title),
            "{item}"
        );
        assert_eq!(item["item"]["placements"][0]["slug"], "saved", "{item}");
        // The page image travels as a reference-first submission asset.
        assert_eq!(item["assets"][0]["source"], "page_image", "{item}");
        let content_item_id = reference["content_item_id"].as_str().unwrap().to_string();
        assert!(!shown_ids.contains(&content_item_id), "{item}");
        assert!(
            item["hints"][0]
                .as_str()
                .unwrap()
                .contains(&content_item_id),
            "{item}"
        );
        shown_ids.push(content_item_id);
    }

    // The deck is exhausted and no announcements are known: caught up.
    let item = user.stumble();
    assert_eq!(item["kind"], "caught_up", "{item}");
    assert!(item["hints"]
        .as_array()
        .is_some_and(|hints| !hints.is_empty()));

    // New content revives the action, and the bare command renders a text card.
    user.run(&[
        "add",
        "https://example.com/new-find",
        "--title",
        "A brand new find",
        "--summary",
        "Added after the feed went dry.",
    ]);
    let card = String::from_utf8(user.raw(&[]).stdout).unwrap();
    assert!(card.starts_with("── stumble "), "{card}");
    assert!(card.contains("A brand new find"), "{card}");
    assert!(card.contains("https://example.com/new-find"), "{card}");
    assert!(card.contains("1 of 1"), "{card}");

    let item = user.stumble();
    assert_eq!(item["kind"], "caught_up", "{item}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stumble_falls_back_to_network_samples_when_caught_up() {
    // ── A live Bootstrap node with open admission and a real Origin probe ────
    let bootstrap_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bootstrap_base = format!("http://{}", bootstrap_listener.local_addr().unwrap());
    let bootstrap_tools =
        AgentTools::new(seed_store()).with_bootstrap_capability(true, Arc::new(ReqwestOriginProbe));
    let bootstrap_router = router_with_base_url(bootstrap_tools, &bootstrap_base);
    let bootstrap_server = tokio::spawn(async move {
        axum::serve(bootstrap_listener, bootstrap_router)
            .await
            .unwrap()
    });

    // ── Alice curates before publishing so the announcement binds her latest
    // event pointer and her live Origin can serve signed samples ─────────────
    let alice = Environment::new("alice");
    alice.run(&[
        "pod",
        "create",
        "--name",
        "Distributed Craft",
        "--slug",
        "distributed-craft",
        "--description",
        "Distributed systems craft and reliability engineering.",
        "--visibility",
        "private",
    ]);
    let curated = [
        (
            "https://example.com/raft-explained",
            "Raft explained visually",
        ),
        ("https://example.com/paxos-made-live", "Paxos made live"),
    ];
    for (url, title) in curated {
        alice.run(&[
            "add",
            url,
            "--pod",
            "distributed-craft",
            "--title",
            title,
            "--tag",
            "distributed-systems",
        ]);
    }
    alice.use_bootstrap(&bootstrap_base);

    let alice_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let alice_base = format!("http://{}", alice_listener.local_addr().unwrap());
    let alice_origin = AgentTools::open_initialized_home_node(&alice.data_dir).unwrap();
    let alice_router = router_with_base_url(alice_origin, &alice_base);
    let alice_server =
        tokio::spawn(async move { axum::serve(alice_listener, alice_router).await.unwrap() });

    let published = alice.run(&[
        "pod",
        "publish",
        "distributed-craft",
        "--base-url",
        &alice_base,
    ]);
    assert_eq!(published["bootstrap_submissions"][0]["status"], "admitted");

    // ── Bob has an empty feed: the action reaches through to the network ─────
    let bob = Environment::new("bob");
    bob.use_bootstrap(&bootstrap_base);
    let report = bob.run(&["sync", "bootstrap", "run"]);
    assert!(report["retained_announcements"].as_u64().unwrap() >= 1);

    let mut sample_titles = Vec::new();
    for _ in 0..2 {
        let item = bob.stumble();
        assert_eq!(item["kind"], "network_sample", "{item}");
        assert_eq!(item["pod"]["slug"], "distributed-craft", "{item}");
        assert_eq!(
            item["pod"]["public_pod_url"].as_str().unwrap(),
            format!("{alice_base}/federation/pods/distributed-craft"),
            "{item}"
        );
        assert!(
            item["hints"][0]
                .as_str()
                .unwrap()
                .starts_with("stumble pod subscribe "),
            "{item}"
        );
        sample_titles.push(item["sample"]["title"].as_str().unwrap().to_string());
    }
    // Both curated items surfaced exactly once across the two runs.
    sample_titles.sort();
    assert_eq!(
        sample_titles,
        ["Paxos made live", "Raft explained visually"]
    );

    // Every signed sample has been shown, so the action reports caught up.
    let item = bob.stumble();
    assert_eq!(item["kind"], "caught_up", "{item}");

    alice_server.abort();
    bootstrap_server.abort();
    let _ = alice_server.await;
    let _ = bootstrap_server.await;
}
