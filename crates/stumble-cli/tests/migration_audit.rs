use std::path::{Path, PathBuf};

const RETIRED_MARKERS: &[&str] = &[
    "podctl",
    "CARGO_BIN_EXE_podctl",
    "propose-change",
    "approve-proposal",
    "submit-candidate",
    "inspect-candidate",
    "list-discovery-tasks",
    "feed-feedback",
    "--api",
];

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name != "target" && name != ".git" {
                collect_files(&path, files);
            }
        } else {
            files.push(path);
        }
    }
}

fn is_intentional_history_or_contraction_surface(path: &str) -> bool {
    path.starts_with(".scratch/")
        || path.starts_with("docs/adr/")
        || matches!(
            path,
            "crates/stumble-cli/Cargo.toml"
                | "crates/stumble-cli/src/main.rs"
                | "crates/stumble-cli/tests/legacy_contracts.rs"
                | "crates/stumble-cli/tests/migration_audit.rs"
                | "crates/stumble-cli/tests/stumble_shell.rs"
        )
}

#[test]
fn maintained_repository_callers_have_no_retired_cli_dependencies() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    collect_files(&workspace, &mut files);
    let mut unexpected = Vec::new();

    for file in files {
        let relative = file.strip_prefix(&workspace).unwrap().to_string_lossy();
        if is_intentional_history_or_contraction_surface(&relative) {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (line_index, line) in contents.lines().enumerate() {
            for marker in RETIRED_MARKERS {
                if line.contains(marker) {
                    unexpected.push(format!("{relative}:{}: {marker}", line_index + 1));
                }
            }
        }
    }

    assert!(
        unexpected.is_empty(),
        "unmigrated maintained CLI references:\n{}",
        unexpected.join("\n")
    );
}

#[test]
fn cli_integration_tests_do_not_construct_generic_pending_proposals() {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut files = Vec::new();
    collect_files(&tests, &mut files);
    let offenders = files
        .into_iter()
        .filter(|path| {
            path.file_name().and_then(|name| name.to_str()) != Some("migration_audit.rs")
        })
        .filter_map(|path| {
            let contents = std::fs::read_to_string(&path).ok()?;
            contents
                .contains(".create_pending_proposal(")
                .then(|| path.file_name().unwrap().to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    assert!(
        offenders.is_empty(),
        "CLI integration tests still construct generic proposals: {}",
        offenders.join(", ")
    );
}
