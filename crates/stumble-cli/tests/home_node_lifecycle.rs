use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct TestEnvironment {
    root: PathBuf,
}

impl TestEnvironment {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "stumble-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("create test environment");
        Self { root }
    }

    fn data_dir(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_stumble"));
        command
            .env(
                "STUMBLE_CREDENTIAL_STORE_DIR",
                self.root.join("credentials"),
            )
            .env_remove("STUMBLE_DATA_DIR");
        command
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("output should be one JSON document")
}

fn resolved(path: &Path) -> String {
    path.canonicalize()
        .expect("path should exist")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn node_init_is_the_only_creation_path_and_rejects_reinitialization() {
    let environment = TestEnvironment::new("explicit-init");
    let data_dir = environment.data_dir("home");

    let before_init = environment
        .command()
        .args(["--data-dir", data_dir.to_str().unwrap(), "node", "show"])
        .output()
        .expect("show uninitialized node");
    assert_eq!(before_init.status.code(), Some(4));
    assert!(!data_dir.join("stumble.sqlite3").exists());
    assert_eq!(
        json(&before_init.stderr)["error"]["code"],
        "node_not_initialized"
    );

    fs::create_dir_all(&data_dir).expect("create uninitialized path");
    fs::write(data_dir.join("stumble.sqlite3"), []).expect("create empty database file");
    let empty_database = environment
        .command()
        .args(["--data-dir", data_dir.to_str().unwrap(), "node", "show"])
        .output()
        .expect("show path with empty database");
    assert_eq!(empty_database.status.code(), Some(4));
    assert_eq!(
        fs::metadata(data_dir.join("stumble.sqlite3"))
            .unwrap()
            .len(),
        0
    );

    let unrelated = environment
        .command()
        .args(["--data-dir", data_dir.to_str().unwrap(), "pod", "list"])
        .output()
        .expect("list Pods on uninitialized node");
    assert_eq!(unrelated.status.code(), Some(4));
    assert_eq!(
        fs::metadata(data_dir.join("stumble.sqlite3"))
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        json(&unrelated.stderr)["error"]["code"],
        "node_not_initialized"
    );

    let initialized = environment
        .command()
        .args(["--data-dir", data_dir.to_str().unwrap(), "node", "init"])
        .output()
        .expect("initialize node");
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let body = json(&initialized.stdout);
    assert_eq!(body["data"]["data_dir"], resolved(&data_dir));
    assert!(body["data"]["node"]["node_id"].as_str().is_some());
    assert!(data_dir.join("stumble.sqlite3").is_file());

    let duplicate = environment
        .command()
        .args(["--data-dir", data_dir.to_str().unwrap(), "node", "init"])
        .output()
        .expect("reject duplicate initialization");
    assert_eq!(duplicate.status.code(), Some(4));
    assert_eq!(
        json(&duplicate.stderr)["error"]["code"],
        "node_already_initialized"
    );
}

#[test]
fn node_show_resolves_flag_environment_and_default_paths() {
    let environment = TestEnvironment::new("path-resolution");
    let env_dir = environment.data_dir("from-env");
    let flag_dir = environment.data_dir("from-flag");
    let home_dir = environment.data_dir("os-home");

    for data_dir in [&env_dir, &flag_dir, &home_dir.join(".stumble/nodes/home")] {
        let initialized = environment
            .command()
            .args(["--data-dir", data_dir.to_str().unwrap(), "node", "init"])
            .output()
            .expect("initialize path case");
        assert!(
            initialized.status.success(),
            "{}",
            String::from_utf8_lossy(&initialized.stderr)
        );
    }

    let from_env = environment
        .command()
        .env("STUMBLE_DATA_DIR", &env_dir)
        .args(["node", "show"])
        .output()
        .expect("show environment node");
    assert_eq!(
        json(&from_env.stdout)["data"]["data_dir"],
        resolved(&env_dir)
    );

    let from_flag = environment
        .command()
        .env("STUMBLE_DATA_DIR", &env_dir)
        .args(["--data-dir", flag_dir.to_str().unwrap(), "node", "show"])
        .output()
        .expect("show flag node");
    assert_eq!(
        json(&from_flag.stdout)["data"]["data_dir"],
        resolved(&flag_dir)
    );

    let from_default = environment
        .command()
        .env("HOME", &home_dir)
        .args(["node", "show"])
        .output()
        .expect("show default node");
    assert_eq!(
        json(&from_default.stdout)["data"]["data_dir"],
        resolved(&home_dir.join(".stumble/nodes/home"))
    );
}

#[test]
fn owner_authority_is_external_to_the_node_and_detected_automatically() {
    let environment = TestEnvironment::new("owner-authority");
    let data_dir = environment.data_dir("home");

    let initialized = environment
        .command()
        .args(["--data-dir", data_dir.to_str().unwrap(), "node", "init"])
        .output()
        .expect("initialize node");
    assert!(initialized.status.success());

    let authority_entries = fs::read_dir(environment.root.join("credentials"))
        .expect("isolated Owner authority store exists")
        .collect::<Result<Vec<_>, _>>()
        .expect("read isolated Owner authority entries");
    assert_eq!(authority_entries.len(), 1);
    let authority_marker = authority_entries[0].path();
    assert_eq!(fs::metadata(&authority_marker).unwrap().len(), 0);
    let database = fs::read(data_dir.join("stumble.sqlite3")).expect("read database");
    assert!(!database.is_empty());

    let shown = environment
        .command()
        .args(["--data-dir", data_dir.to_str().unwrap(), "node", "show"])
        .output()
        .expect("show with automatic credential");
    assert!(
        shown.status.success(),
        "{}",
        String::from_utf8_lossy(&shown.stderr)
    );
    assert_eq!(json(&shown.stdout)["data"]["data_dir"], resolved(&data_dir));

    let wrong_harness_credential = environment
        .command()
        .env("STUMBLE_HARNESS_CREDENTIAL", "wrong-credential")
        .args(["--data-dir", data_dir.to_str().unwrap(), "node", "show"])
        .output()
        .expect("reject wrong Harness credential");
    assert_eq!(wrong_harness_credential.status.code(), Some(3));
    assert_eq!(
        json(&wrong_harness_credential.stderr)["error"]["code"],
        "invalid_harness_credential"
    );

    // Presence is the authority boundary. Contents are not credential material and
    // cannot be "wrong"; even a legacy value is treated only as an existing entry.
    fs::write(&authority_marker, "not-a-secret").expect("write legacy marker contents");
    let with_nonempty_marker = environment
        .command()
        .args(["--data-dir", data_dir.to_str().unwrap(), "node", "show"])
        .output()
        .expect("show with nonempty authority marker");
    assert!(with_nonempty_marker.status.success());

    fs::remove_file(authority_marker).expect("remove isolated Owner authority");
    let without_credential = environment
        .command()
        .args(["--data-dir", data_dir.to_str().unwrap(), "node", "show"])
        .output()
        .expect("show without Owner credential");
    assert_eq!(without_credential.status.code(), Some(3));
    assert_eq!(
        json(&without_credential.stderr)["error"]["code"],
        "owner_credential_not_found"
    );
}

#[cfg(unix)]
#[test]
fn failed_node_initialization_removes_owner_authority_registration() {
    use std::os::unix::fs::PermissionsExt;

    let environment = TestEnvironment::new("failed-init-cleanup");
    let data_dir = environment.data_dir("read-only-home");
    fs::create_dir_all(&data_dir).expect("create Home Node directory");
    fs::set_permissions(&data_dir, fs::Permissions::from_mode(0o500))
        .expect("make Home Node directory read-only");

    let initialized = environment
        .command()
        .args(["--data-dir", data_dir.to_str().unwrap(), "node", "init"])
        .output()
        .expect("attempt initialization");

    fs::set_permissions(&data_dir, fs::Permissions::from_mode(0o700))
        .expect("restore Home Node directory permissions");
    assert!(!initialized.status.success());
    let authority_entries = fs::read_dir(environment.root.join("credentials"))
        .expect("authority store was created")
        .collect::<Result<Vec<_>, _>>()
        .expect("read authority store");
    assert!(authority_entries.is_empty());
    assert!(!data_dir.join("stumble.sqlite3").exists());
}
