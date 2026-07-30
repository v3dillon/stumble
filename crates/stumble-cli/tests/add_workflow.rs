use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};

struct Environment {
    root: PathBuf,
    data_dir: PathBuf,
}

impl Environment {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("stumble-add-workflow-{}", uuid::Uuid::now_v7()));
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
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "command {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
        envelope["data"].clone()
    }

    fn run_failure(&self, arguments: &[&str]) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_stumble"))
            .env("STUMBLE_DATA_DIR", &self.data_dir)
            .env(
                "STUMBLE_CREDENTIAL_STORE_DIR",
                self.root.join("credentials"),
            )
            .args(arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert!(!output.status.success(), "command {arguments:?} succeeded");
        let envelope: Value = serde_json::from_slice(&output.stderr).unwrap();
        envelope["error"].clone()
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn add_creates_the_saved_pod_and_a_feed_eligible_item_in_one_step() {
    let environment = Environment::new();

    let added = environment.run(&[
        "add",
        "https://example.com/essays/attention",
        "--title",
        "On Attention",
        "--summary",
        "Why finite feeds beat infinite scroll",
        "--tag",
        "attention",
        "--tag",
        "design",
    ]);
    assert_eq!(added["pod_slug"], "saved");
    assert_eq!(added["pod_created"], true);
    assert_eq!(added["subscribed"], true);
    assert_eq!(added["placement"]["status"], "accepted");
    assert_eq!(added["placement"]["curation_path"], "add_to_pod");
    assert_eq!(added["content_item"]["title"], "On Attention");

    let batch = environment.run(&["feed", "batch", "get"]);
    let items = batch["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]["content_reference"]["canonical_url"],
        "https://example.com/essays/attention"
    );

    let again = environment.run(&["add", "https://example.com/essays/attention"]);
    assert_eq!(again["pod_created"], false);
    assert_eq!(
        again["content_item"]["id"], added["content_item"]["id"],
        "re-adding the same URL dedupes on canonical URL"
    );
}

#[test]
fn add_requires_an_explicit_pod_slug_to_already_exist() {
    let environment = Environment::new();
    let error = environment.run_failure(&[
        "add",
        "https://example.com/essays/attention",
        "--pod",
        "does-not-exist",
    ]);
    assert_eq!(error["code"], "not_found");
}

#[test]
fn add_stores_page_images_and_a_generated_cover() {
    let environment = Environment::new();
    let cover = environment.root.join("cover.png");
    fs::write(&cover, b"png-bytes").unwrap();

    let added = environment.run(&[
        "add",
        "https://example.com/visual-essay",
        "--title",
        "A visual essay",
        "--image",
        "https://example.com/hero.png",
        "--image",
        "https://example.com/figure-2.png",
        "--cover",
        cover.to_str().unwrap(),
    ]);
    let media: Vec<_> = added["content_item"]["media_references"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reference| reference["url"].as_str().unwrap())
        .collect();
    assert_eq!(
        media,
        ["https://example.com/hero.png", "https://example.com/figure-2.png"]
    );
    let assets = added["assets"].as_array().unwrap();
    assert_eq!(assets.len(), 2);
    assert_eq!(assets[0]["source"], "page_image");
    assert_eq!(assets[0]["url"], "https://example.com/hero.png");
    assert_eq!(assets[1]["source"], "ai_generated");
    let stored = assets[1]["local_path"].as_str().unwrap();
    assert!(stored.contains("/media/"), "{stored}");
    assert_eq!(fs::read(stored).unwrap(), b"png-bytes");

    // The cover survives in reads: pod content show lists the assets.
    let shown = environment.run(&[
        "pod",
        "content",
        "show",
        "saved",
        added["content_item"]["id"].as_str().unwrap(),
    ]);
    assert_eq!(shown["assets"].as_array().unwrap().len(), 2);
}

#[test]
fn add_archives_a_readable_snapshot_and_replaces_it_in_place() {
    let environment = Environment::new();
    let snapshot = environment.root.join("attention.md");
    fs::write(&snapshot, "# On Attention\n\nThe full readable text.").unwrap();

    let added = environment.run(&[
        "add",
        "https://example.com/essays/attention",
        "--title",
        "On Attention",
        "--snapshot",
        snapshot.to_str().unwrap(),
    ]);
    let assets = added["assets"].as_array().unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0]["asset_type"], "readable_snapshot");
    assert_eq!(assets[0]["source"], "page_text");
    assert_eq!(assets[0]["mime_type"], "text/markdown");
    let stored = assets[0]["local_path"].as_str().unwrap();
    assert!(stored.contains("/media/"), "{stored}");
    assert!(stored.ends_with("snapshot.md"), "{stored}");
    assert_eq!(
        fs::read_to_string(stored).unwrap(),
        "# On Attention\n\nThe full readable text."
    );

    // Re-archiving replaces the one snapshot instead of stacking assets,
    // even when the replacement arrives under a different extension.
    let revised = environment.root.join("attention.txt");
    fs::write(&revised, "The revised readable text.").unwrap();
    let item_id = added["content_item"]["id"].as_str().unwrap();
    let replaced = environment.run(&[
        "pod",
        "content",
        "snapshot",
        "saved",
        item_id,
        "--file",
        revised.to_str().unwrap(),
        "--source",
        "user-provided",
    ]);
    assert_eq!(replaced["asset_type"], "readable_snapshot");
    assert_eq!(replaced["source"], "user_provided");
    assert_eq!(replaced["mime_type"], "text/plain");
    let replaced_path = replaced["local_path"].as_str().unwrap();
    assert!(replaced_path.ends_with("snapshot.txt"), "{replaced_path}");
    assert_eq!(
        fs::read_to_string(replaced_path).unwrap(),
        "The revised readable text."
    );
    assert!(!PathBuf::from(stored).exists(), "stale {stored} lingers");

    let shown = environment.run(&["pod", "content", "show", "saved", item_id]);
    let shown_assets = shown["assets"].as_array().unwrap();
    assert_eq!(shown_assets.len(), 1, "{shown_assets:?}");
}
