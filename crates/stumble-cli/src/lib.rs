use serde::Serialize;
use serde_json::Value;
use std::{
    ffi::OsString,
    fmt::Write as _,
    hash::{Hash, Hasher},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const ENVELOPE_VERSION: u8 = 2;
const CREDENTIAL_SERVICE: &str = "dev.stumble.home-node";
const OWNER_AUTHORITY_MARKER: &str = "present";

#[derive(Debug)]
pub enum CredentialStoreError {
    Io(std::io::Error),
    Backend(String),
}

impl std::fmt::Display for CredentialStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "credential store I/O failed: {error}"),
            Self::Backend(error) => write!(
                formatter,
                "operating-system credential store failed: {error}"
            ),
        }
    }
}

impl std::error::Error for CredentialStoreError {}

impl From<std::io::Error> for CredentialStoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Records whether the current operating-system User has Owner authority for a
/// Home Node. The entry's presence is the authority boundary; it contains no
/// credential that Stumble authenticates or exposes.
pub trait OwnerAuthorityStore {
    fn register(&self, data_dir: &Path) -> Result<(), CredentialStoreError>;
    fn is_registered(&self, data_dir: &Path) -> Result<bool, CredentialStoreError>;
    fn remove(&self, data_dir: &Path) -> Result<(), CredentialStoreError>;
}

pub struct SystemOwnerAuthorityStore;

impl OwnerAuthorityStore for SystemOwnerAuthorityStore {
    fn register(&self, data_dir: &Path) -> Result<(), CredentialStoreError> {
        system_register(data_dir)
    }

    fn is_registered(&self, data_dir: &Path) -> Result<bool, CredentialStoreError> {
        system_is_registered(data_dir)
    }

    fn remove(&self, data_dir: &Path) -> Result<(), CredentialStoreError> {
        system_remove(data_dir)
    }
}

/// Filesystem-backed boundary used by executable tests so they never access a
/// developer's operating-system keychain.
pub struct IsolatedOwnerAuthorityStore {
    root: PathBuf,
}

impl IsolatedOwnerAuthorityStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn authority_path(&self, data_dir: &Path) -> PathBuf {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        data_dir.hash(&mut hasher);
        self.root.join(format!("{:016x}", hasher.finish()))
    }
}

impl OwnerAuthorityStore for IsolatedOwnerAuthorityStore {
    fn register(&self, data_dir: &Path) -> Result<(), CredentialStoreError> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.authority_path(data_dir);
        if path.try_exists()? {
            return if path.is_file() {
                Ok(())
            } else {
                Err(CredentialStoreError::Backend(format!(
                    "Owner authority entry {} is not a file",
                    path.display()
                )))
            };
        }
        let temporary = self.root.join(format!(
            ".owner-authority-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700))?;
            std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temporary)?;
        }
        #[cfg(not(unix))]
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        match std::fs::rename(&temporary, path) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = std::fs::remove_file(temporary);
                Err(error.into())
            }
        }
    }

    fn is_registered(&self, data_dir: &Path) -> Result<bool, CredentialStoreError> {
        match std::fs::metadata(self.authority_path(data_dir)) {
            Ok(metadata) if metadata.is_file() => Ok(true),
            Ok(_) => Err(CredentialStoreError::Backend(
                "Owner authority entry is not a file".into(),
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    fn remove(&self, data_dir: &Path) -> Result<(), CredentialStoreError> {
        match std::fs::remove_file(self.authority_path(data_dir)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

pub fn owner_authority_store() -> Box<dyn OwnerAuthorityStore> {
    if let Some(root) = std::env::var_os("STUMBLE_CREDENTIAL_STORE_DIR") {
        Box::new(IsolatedOwnerAuthorityStore::new(root))
    } else {
        Box::new(SystemOwnerAuthorityStore)
    }
}

pub fn selected_data_dir(explicit: Option<&Path>) -> Result<PathBuf, ErrorBody> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    if let Some(path) = std::env::var_os("STUMBLE_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| ErrorBody::new("home_directory_unavailable", "HOME is not set"))?;
    Ok(PathBuf::from(home).join(".stumble/nodes/home"))
}

pub fn resolve_existing_data_dir(path: &Path) -> Result<PathBuf, ErrorBody> {
    path.canonicalize().map_err(|_| {
        ErrorBody::new(
            "node_not_initialized",
            format!("Home Node is not initialized at {}", path.display()),
        )
    })
}

pub fn resolve_initialized_data_dir(path: &Path) -> Result<PathBuf, ErrorBody> {
    std::fs::create_dir_all(path).map_err(|error| {
        ErrorBody::new(
            "node_initialization_failed",
            format!("could not create {}: {error}", path.display()),
        )
    })?;
    path.canonicalize().map_err(|error| {
        ErrorBody::new(
            "node_initialization_failed",
            format!("could not resolve {}: {error}", path.display()),
        )
    })
}

#[allow(dead_code)] // Both variants are exercised by platform-independent contract tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SystemCredentialPlatform {
    MacOs,
    Linux,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SystemCredentialOperation {
    Register,
    Check,
    Remove,
}

struct SystemCredentialCommand {
    program: &'static str,
    args: Vec<OsString>,
    stdin: Option<&'static [u8]>,
    stdout: SystemCommandStdout,
    missing_status: MissingStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SystemCommandStdout {
    Capture,
    Discard,
}

enum MissingStatus {
    ExitCode(i32),
    ExitCodeWithEmptyStderr(i32),
}

impl SystemCredentialCommand {
    fn is_missing(&self, output: &std::process::Output) -> bool {
        match self.missing_status {
            MissingStatus::ExitCode(code) => output.status.code() == Some(code),
            MissingStatus::ExitCodeWithEmptyStderr(code) => {
                output.status.code() == Some(code) && output.stderr.is_empty()
            }
        }
    }
}

fn system_command(
    platform: SystemCredentialPlatform,
    operation: SystemCredentialOperation,
    data_dir: &Path,
) -> SystemCredentialCommand {
    let data_dir = OsString::from(data_dir);
    match (platform, operation) {
        (SystemCredentialPlatform::MacOs, SystemCredentialOperation::Register) => {
            SystemCredentialCommand {
                program: "security",
                args: ["add-generic-password", "-U", "-s", CREDENTIAL_SERVICE, "-a"]
                    .into_iter()
                    .map(OsString::from)
                    .chain([data_dir, "-w".into(), OWNER_AUTHORITY_MARKER.into()])
                    .collect(),
                stdin: None,
                stdout: SystemCommandStdout::Capture,
                missing_status: MissingStatus::ExitCode(44),
            }
        }
        (SystemCredentialPlatform::MacOs, SystemCredentialOperation::Check) => {
            SystemCredentialCommand {
                program: "security",
                args: ["find-generic-password", "-s", CREDENTIAL_SERVICE, "-a"]
                    .into_iter()
                    .map(OsString::from)
                    .chain([data_dir])
                    .collect(),
                stdin: None,
                stdout: SystemCommandStdout::Capture,
                missing_status: MissingStatus::ExitCode(44),
            }
        }
        (SystemCredentialPlatform::MacOs, SystemCredentialOperation::Remove) => {
            SystemCredentialCommand {
                program: "security",
                args: ["delete-generic-password", "-s", CREDENTIAL_SERVICE, "-a"]
                    .into_iter()
                    .map(OsString::from)
                    .chain([data_dir])
                    .collect(),
                stdin: None,
                stdout: SystemCommandStdout::Capture,
                missing_status: MissingStatus::ExitCode(44),
            }
        }
        (SystemCredentialPlatform::Linux, operation) => SystemCredentialCommand {
            program: "secret-tool",
            args: match operation {
                SystemCredentialOperation::Register => vec![
                    "store".into(),
                    "--label=Stumble Home Node Owner".into(),
                    "service".into(),
                    CREDENTIAL_SERVICE.into(),
                    "data-dir".into(),
                    data_dir,
                ],
                SystemCredentialOperation::Remove => vec![
                    "clear".into(),
                    "service".into(),
                    CREDENTIAL_SERVICE.into(),
                    "data-dir".into(),
                    data_dir,
                ],
                SystemCredentialOperation::Check => vec![
                    "lookup".into(),
                    "service".into(),
                    CREDENTIAL_SERVICE.into(),
                    "data-dir".into(),
                    data_dir,
                ],
            },
            stdin: (operation == SystemCredentialOperation::Register)
                .then_some(OWNER_AUTHORITY_MARKER.as_bytes()),
            stdout: if operation == SystemCredentialOperation::Check {
                SystemCommandStdout::Discard
            } else {
                SystemCommandStdout::Capture
            },
            missing_status: MissingStatus::ExitCodeWithEmptyStderr(1),
        },
    }
}

fn execute_system_command(
    specification: &SystemCredentialCommand,
) -> Result<std::process::Output, CredentialStoreError> {
    let mut command = Command::new(specification.program);
    command.args(&specification.args);
    match specification.stdout {
        SystemCommandStdout::Capture => command.stdout(Stdio::piped()),
        SystemCommandStdout::Discard => command.stdout(Stdio::null()),
    };
    if let Some(input) = specification.stdin {
        let mut child = command
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child.stdin.take().expect("piped stdin").write_all(input)?;
        return Ok(child.wait_with_output()?);
    }
    Ok(command.output()?)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn current_system_credential_platform() -> SystemCredentialPlatform {
    #[cfg(target_os = "macos")]
    return SystemCredentialPlatform::MacOs;
    #[cfg(target_os = "linux")]
    return SystemCredentialPlatform::Linux;
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn system_register(data_dir: &Path) -> Result<(), CredentialStoreError> {
    let specification = system_command(
        current_system_credential_platform(),
        SystemCredentialOperation::Register,
        data_dir,
    );
    command_succeeded(
        execute_system_command(&specification)?,
        "register Home Node Owner Credential",
    )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn system_is_registered(data_dir: &Path) -> Result<bool, CredentialStoreError> {
    let specification = system_command(
        current_system_credential_platform(),
        SystemCredentialOperation::Check,
        data_dir,
    );
    let output = execute_system_command(&specification)?;
    if specification.is_missing(&output) {
        return Ok(false);
    }
    command_succeeded(output, "check Home Node Owner Credential").map(|()| true)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn system_remove(data_dir: &Path) -> Result<(), CredentialStoreError> {
    let specification = system_command(
        current_system_credential_platform(),
        SystemCredentialOperation::Remove,
        data_dir,
    );
    let output = execute_system_command(&specification)?;
    if specification.is_missing(&output) {
        return Ok(());
    }
    command_succeeded(output, "remove Home Node Owner Credential")
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn system_register(_data_dir: &Path) -> Result<(), CredentialStoreError> {
    Err(CredentialStoreError::Backend(
        "unsupported operating system".into(),
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn system_is_registered(_data_dir: &Path) -> Result<bool, CredentialStoreError> {
    Err(CredentialStoreError::Backend(
        "unsupported operating system".into(),
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn system_remove(_data_dir: &Path) -> Result<(), CredentialStoreError> {
    Err(CredentialStoreError::Backend(
        "unsupported operating system".into(),
    ))
}

fn command_succeeded(
    output: std::process::Output,
    operation: &str,
) -> Result<(), CredentialStoreError> {
    if !output.status.success() {
        return Err(CredentialStoreError::Backend(format!(
            "{operation}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct SuccessEnvelope<T> {
    pub version: u8,
    pub data: T,
}

impl<T> SuccessEnvelope<T> {
    pub fn new(data: T) -> Self {
        Self {
            version: ENVELOPE_VERSION,
            data,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub version: u8,
    pub error: ErrorBody,
}

impl ErrorEnvelope {
    pub fn new(error: ErrorBody) -> Self {
        Self {
            version: ENVELOPE_VERSION,
            error,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl ErrorBody {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ExitStatusCategory {
    Internal = 1,
    Usage = 2,
    Authorization = 3,
    ValidationOrConflict = 4,
}

#[derive(Debug, Serialize)]
pub struct CursorPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

impl<T> CursorPage<T> {
    pub fn empty() -> Self {
        Self {
            items: Vec::new(),
            next_cursor: None,
        }
    }
}

pub fn paginate<T>(
    items: Vec<T>,
    limit: u16,
    cursor: Option<&str>,
) -> Result<CursorPage<T>, ErrorBody> {
    let offset = match cursor {
        None => 0,
        Some(cursor) => cursor
            .strip_prefix("v1.")
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| {
                ErrorBody::new("invalid_cursor", "cursor is not valid for this collection")
            })?,
    };
    if offset > items.len() {
        return Err(ErrorBody::new(
            "invalid_cursor",
            "cursor is outside this collection",
        ));
    }
    let limit = usize::from(limit);
    let has_more = offset.saturating_add(limit) < items.len();
    let items = items.into_iter().skip(offset).take(limit).collect();
    Ok(CursorPage {
        items,
        next_cursor: has_more.then(|| format!("v1.{}", offset + limit)),
    })
}

#[derive(Debug, Serialize)]
pub struct ResourceDetail<T, A> {
    #[serde(flatten)]
    pub resource: T,
    pub allowed_actions: Vec<A>,
}

pub fn read_json_input(path: &Path) -> Result<Value, ErrorBody> {
    let mut contents = String::new();
    if path == Path::new("-") {
        std::io::stdin()
            .read_to_string(&mut contents)
            .map_err(|error| ErrorBody::new("invalid_input", error.to_string()))?;
    } else {
        contents = std::fs::read_to_string(path)
            .map_err(|error| ErrorBody::new("invalid_input", error.to_string()))?;
    }
    serde_json::from_str(&contents).map_err(|error| {
        ErrorBody::new("invalid_input", format!("input is not valid JSON: {error}"))
    })
}

pub fn render_text(value: &Value) -> String {
    let mut output = String::new();
    render_value(&mut output, None, value, 0);
    output
}

fn render_value(output: &mut String, key: Option<&str>, value: &Value, depth: usize) {
    let indentation = "  ".repeat(depth);
    match value {
        Value::Object(fields) => {
            if let Some(key) = key {
                let _ = writeln!(output, "{indentation}{key}:");
            }
            let child_depth = depth + usize::from(key.is_some());
            for (child_key, child_value) in fields {
                render_value(output, Some(child_key), child_value, child_depth);
            }
        }
        Value::Array(items) if items.is_empty() => {
            let _ = writeln!(output, "{indentation}{}: []", key.unwrap_or("value"));
        }
        Value::Array(items) => {
            let _ = writeln!(output, "{indentation}{}:", key.unwrap_or("value"));
            for item in items {
                render_value(output, Some("-"), item, depth + 1);
            }
        }
        Value::String(text) => {
            let _ = writeln!(output, "{indentation}{}: {text}", key.unwrap_or("value"));
        }
        scalar => {
            let _ = writeln!(output, "{indentation}{}: {scalar}", key.unwrap_or("value"));
        }
    }
}

#[cfg(test)]
mod owner_authority_store_tests {
    use super::{
        IsolatedOwnerAuthorityStore, OwnerAuthorityStore, SystemCommandStdout,
        SystemCredentialOperation, SystemCredentialPlatform,
    };

    #[test]
    fn credential_store_contract_registers_checks_and_removes_owner_authority() {
        let root = std::env::temp_dir().join(format!(
            "stumble-credential-contract-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let data_dir = root.join("home");
        let store = IsolatedOwnerAuthorityStore::new(root.join("credentials"));

        assert!(!store.is_registered(&data_dir).unwrap());
        store.register(&data_dir).unwrap();
        assert!(store.is_registered(&data_dir).unwrap());
        assert_eq!(
            std::fs::metadata(store.authority_path(&data_dir))
                .unwrap()
                .len(),
            0
        );
        store.register(&data_dir).unwrap();
        assert_eq!(
            std::fs::read_dir(root.join("credentials")).unwrap().count(),
            1
        );
        store.remove(&data_dir).unwrap();
        assert!(!store.is_registered(&data_dir).unwrap());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_isolated_authority_entry_is_an_error() {
        let root = std::env::temp_dir().join(format!(
            "stumble-corrupt-authority-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let data_dir = root.join("home");
        let store = IsolatedOwnerAuthorityStore::new(root.join("credentials"));
        std::fs::create_dir_all(store.authority_path(&data_dir)).unwrap();

        assert!(store.is_registered(&data_dir).is_err());
        assert!(store.register(&data_dir).is_err());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn production_backend_commands_cover_register_check_and_remove_without_secret_reads() {
        let data_dir = std::path::Path::new("/nodes/home");
        let arguments = |platform, operation| {
            super::system_command(platform, operation, data_dir)
                .args
                .into_iter()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };

        let macos_register = arguments(
            SystemCredentialPlatform::MacOs,
            SystemCredentialOperation::Register,
        );
        let macos_check = arguments(
            SystemCredentialPlatform::MacOs,
            SystemCredentialOperation::Check,
        );
        let macos_remove = arguments(
            SystemCredentialPlatform::MacOs,
            SystemCredentialOperation::Remove,
        );
        assert_eq!(macos_register.last().map(String::as_str), Some("present"));
        assert_eq!(
            macos_check.first().map(String::as_str),
            Some("find-generic-password")
        );
        assert!(!macos_check.iter().any(|argument| argument == "-w"));
        assert_eq!(
            macos_remove.first().map(String::as_str),
            Some("delete-generic-password")
        );

        let linux_register = super::system_command(
            SystemCredentialPlatform::Linux,
            SystemCredentialOperation::Register,
            data_dir,
        );
        let linux_register_args = linux_register
            .args
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            linux_register_args.first().map(String::as_str),
            Some("store")
        );
        assert_eq!(linux_register.stdin, Some(b"present".as_slice()));
        let linux_check = super::system_command(
            SystemCredentialPlatform::Linux,
            SystemCredentialOperation::Check,
            data_dir,
        );
        let linux_check_args = linux_check
            .args
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(linux_check.program, "secret-tool");
        assert_eq!(linux_check_args.first().map(String::as_str), Some("lookup"));
        assert_eq!(linux_check.stdout, SystemCommandStdout::Discard);
        assert_eq!(
            arguments(
                SystemCredentialPlatform::Linux,
                SystemCredentialOperation::Remove
            )
            .first()
            .map(String::as_str),
            Some("clear")
        );
    }

    #[cfg(unix)]
    #[test]
    fn production_backend_status_mapping_distinguishes_missing_entries_from_failures() {
        use std::os::unix::process::ExitStatusExt;

        let output = |code, stdout: &[u8], stderr: &[u8]| std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
        };
        let data_dir = std::path::Path::new("/nodes/home");
        let macos = super::system_command(
            SystemCredentialPlatform::MacOs,
            SystemCredentialOperation::Check,
            data_dir,
        );
        let linux = super::system_command(
            SystemCredentialPlatform::Linux,
            SystemCredentialOperation::Check,
            data_dir,
        );

        assert!(macos.is_missing(&output(44, b"", b"keychain item not found")));
        assert!(linux.is_missing(&output(1, b"", b"")));
        assert!(!linux.is_missing(&output(0, b"", b"")));
        assert!(!linux.is_missing(&output(1, b"", b"credential service unavailable")));
        assert!(super::command_succeeded(output(0, b"", b""), "check").is_ok());
        assert!(super::command_succeeded(
            output(1, b"", b"credential service unavailable"),
            "check"
        )
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn discarded_backend_stdout_never_enters_stumble_memory() {
        for (script, stdin) in [
            ("printf retrieved-secret", None),
            (
                "read input; printf retrieved-secret",
                Some(b"input\n".as_slice()),
            ),
        ] {
            let specification = super::SystemCredentialCommand {
                program: "sh",
                args: vec!["-c".into(), script.into()],
                stdin,
                stdout: SystemCommandStdout::Discard,
                missing_status: super::MissingStatus::ExitCode(1),
            };

            let output = super::execute_system_command(&specification).unwrap();

            assert!(output.status.success());
            assert!(output.stdout.is_empty());
        }
    }
}
