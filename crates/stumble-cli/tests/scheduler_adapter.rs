use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use uuid::Uuid;

struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("stumble-scheduler-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn executable(&self, name: &str, contents: &str) -> std::path::PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, contents).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions).unwrap();
        path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn wake_adapter_emits_or_invokes_discovery_ready_without_browser_control() {
    let fixture = TestDir::new();
    let stumble = fixture.executable(
        "stumble",
        "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >\"$ARGS_PATH\"\nprintf '%s\\n' \"$STUMBLE_HARNESS_CREDENTIAL\" >\"$CREDENTIAL_PATH\"\nprintf '{\"version\":1,\"data\":{\"items\":[{\"id\":\"task-1\"}],\"next_cursor\":null}}\\n'\n",
    );
    let event_path = fixture.0.join("event.json");
    let args_path = fixture.0.join("args.txt");
    let credential_path = fixture.0.join("credential.txt");
    let harness = fixture.executable(
        "harness",
        "#!/usr/bin/env bash\nIFS= read -r event\nprintf '%s\\n' \"$event\" >\"$EVENT_PATH\"\n",
    );
    let wake =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/wake-discovery.sh");

    let emitted = Command::new(&wake)
        .env("STUMBLE_CLI", &stumble)
        .env("STUMBLE_DISCOVERY_TOKEN", "scoped-token")
        .env("STUMBLE_DATA_DIR", &fixture.0)
        .env("STUMBLE_DISCOVERY_EVENT_PATH", &event_path)
        .env("ARGS_PATH", &args_path)
        .env("CREDENTIAL_PATH", &credential_path)
        .output()
        .unwrap();
    assert!(emitted.status.success());
    assert!(String::from_utf8_lossy(&emitted.stdout).contains("discovery_ready"));
    assert!(std::fs::read_to_string(&event_path)
        .unwrap()
        .contains("task-1"));
    assert_eq!(
        std::fs::read_to_string(&args_path).unwrap().trim(),
        "--data-dir ".to_owned()
            + fixture.0.to_str().unwrap()
            + " discover task list --state ready --limit 100"
    );
    assert_eq!(
        std::fs::read_to_string(&credential_path).unwrap().trim(),
        "scoped-token"
    );

    let invoked = Command::new(&wake)
        .env("STUMBLE_CLI", stumble)
        .env("STUMBLE_DISCOVERY_TOKEN", "scoped-token")
        .env("STUMBLE_DISCOVERY_HARNESS_COMMAND", harness)
        .env("EVENT_PATH", &event_path)
        .env("ARGS_PATH", &args_path)
        .env("CREDENTIAL_PATH", &credential_path)
        .output()
        .unwrap();
    assert!(invoked.status.success());
    let event = std::fs::read_to_string(event_path).unwrap();
    assert!(event.contains("discovery_ready"));
    assert!(event.contains("task-1"));
}

#[test]
fn wake_adapter_emits_idle_for_an_empty_canonical_task_page() {
    let fixture = TestDir::new();
    let stumble = fixture.executable(
        "stumble",
        "#!/usr/bin/env bash\nprintf '{\"version\":1,\"data\":{\"items\":[],\"next_cursor\":null}}\\n'\n",
    );
    let event_path = fixture.0.join("idle-event.json");
    let wake =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/wake-discovery.sh");

    let output = Command::new(wake)
        .env("STUMBLE_CLI", stumble)
        .env("STUMBLE_DISCOVERY_TOKEN", "scoped-token")
        .env("STUMBLE_DISCOVERY_EVENT_PATH", &event_path)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        std::fs::read_to_string(event_path).unwrap(),
        "{\"type\":\"discovery_idle\",\"tasks\":[]}\n"
    );
}
