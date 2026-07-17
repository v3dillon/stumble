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
    let podctl = fixture.executable(
        "podctl",
        "#!/usr/bin/env bash\nif [[ \"${*: -1}\" == \"list-ready-discovery-tasks\" ]]; then printf '[{\"id\":\"task-1\"}]\\n'; else printf '[]\\n'; fi\n",
    );
    let event_path = fixture.0.join("event.json");
    let harness = fixture.executable(
        "harness",
        "#!/usr/bin/env bash\nIFS= read -r event\nprintf '%s\\n' \"$event\" >\"$EVENT_PATH\"\n",
    );
    let wake =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/wake-discovery.sh");

    let emitted = Command::new(&wake)
        .env("STUMBLE_PODCTL", &podctl)
        .env("STUMBLE_DISCOVERY_TOKEN", "scoped-token")
        .env("STUMBLE_DATA_DIR", &fixture.0)
        .env("STUMBLE_DISCOVERY_EVENT_PATH", &event_path)
        .output()
        .unwrap();
    assert!(emitted.status.success());
    assert!(String::from_utf8_lossy(&emitted.stdout).contains("discovery_ready"));
    assert!(std::fs::read_to_string(&event_path)
        .unwrap()
        .contains("task-1"));

    let invoked = Command::new(&wake)
        .env("STUMBLE_PODCTL", podctl)
        .env("STUMBLE_DISCOVERY_TOKEN", "scoped-token")
        .env("STUMBLE_DISCOVERY_HARNESS_COMMAND", harness)
        .env("EVENT_PATH", &event_path)
        .output()
        .unwrap();
    assert!(invoked.status.success());
    let event = std::fs::read_to_string(event_path).unwrap();
    assert!(event.contains("discovery_ready"));
    assert!(event.contains("task-1"));
}

#[test]
fn wake_adapter_uses_running_home_node_api_when_configured() {
    let fixture = TestDir::new();
    fixture.executable(
        "curl",
        "#!/usr/bin/env bash\nif [[ \"$*\" == *\"/discovery-tasks/ready\"* ]]; then printf '[{\"id\":\"api-task\"}]\\n'; else printf '[]\\n'; fi\n",
    );
    let event_path = fixture.0.join("api-event.json");
    let wake =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/wake-discovery.sh");
    let path = format!(
        "{}:{}",
        fixture.0.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = Command::new(wake)
        .env("PATH", path)
        .env("STUMBLE_API_URL", "http://127.0.0.1:8787")
        .env("STUMBLE_PODCTL", "/bin/false")
        .env("STUMBLE_DISCOVERY_TOKEN", "scoped-token")
        .env("STUMBLE_DISCOVERY_EVENT_PATH", &event_path)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(std::fs::read_to_string(event_path)
        .unwrap()
        .contains("api-task"));
}
