use serde::Serialize;
use serde_json::Value;
#[cfg(target_os = "linux")]
use std::process::Stdio;
use std::{
    fmt::Write as _,
    hash::{Hash, Hasher},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

pub const ENVELOPE_VERSION: u8 = 1;
const CREDENTIAL_SERVICE: &str = "dev.stumble.home-node";

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

pub trait OwnerCredentialStore {
    fn store(&self, data_dir: &Path, credential: &str) -> Result<(), CredentialStoreError>;
    fn load(&self, data_dir: &Path) -> Result<Option<String>, CredentialStoreError>;
    fn remove(&self, data_dir: &Path) -> Result<(), CredentialStoreError>;
}

pub struct SystemCredentialStore;

impl OwnerCredentialStore for SystemCredentialStore {
    fn store(&self, data_dir: &Path, credential: &str) -> Result<(), CredentialStoreError> {
        system_store(data_dir, credential)
    }

    fn load(&self, data_dir: &Path) -> Result<Option<String>, CredentialStoreError> {
        system_load(data_dir)
    }

    fn remove(&self, data_dir: &Path) -> Result<(), CredentialStoreError> {
        system_remove(data_dir)
    }
}

/// Filesystem-backed boundary used by executable tests so they never access a
/// developer's operating-system keychain.
pub struct IsolatedCredentialStore {
    root: PathBuf,
}

impl IsolatedCredentialStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn credential_path(&self, data_dir: &Path) -> PathBuf {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        data_dir.hash(&mut hasher);
        self.root.join(format!("{:016x}", hasher.finish()))
    }
}

impl OwnerCredentialStore for IsolatedCredentialStore {
    fn store(&self, data_dir: &Path, credential: &str) -> Result<(), CredentialStoreError> {
        std::fs::create_dir_all(&self.root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700))?;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .mode(0o600)
                .open(self.credential_path(data_dir))?;
            file.write_all(credential.as_bytes())?;
        }
        #[cfg(not(unix))]
        std::fs::write(self.credential_path(data_dir), credential)?;
        Ok(())
    }

    fn load(&self, data_dir: &Path) -> Result<Option<String>, CredentialStoreError> {
        match std::fs::read_to_string(self.credential_path(data_dir)) {
            Ok(credential) => Ok(Some(credential)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn remove(&self, data_dir: &Path) -> Result<(), CredentialStoreError> {
        match std::fs::remove_file(self.credential_path(data_dir)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

pub fn owner_credential_store() -> Box<dyn OwnerCredentialStore> {
    if let Some(root) = std::env::var_os("STUMBLE_CREDENTIAL_STORE_DIR") {
        Box::new(IsolatedCredentialStore::new(root))
    } else {
        Box::new(SystemCredentialStore)
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

#[cfg(target_os = "macos")]
fn system_store(data_dir: &Path, credential: &str) -> Result<(), CredentialStoreError> {
    let output = Command::new("security")
        .args(["add-generic-password", "-U", "-s", CREDENTIAL_SERVICE, "-a"])
        .arg(data_dir)
        .args(["-w", credential])
        .output()?;
    command_succeeded(output, "store Home Node Owner credential").map(|_| ())
}

#[cfg(target_os = "macos")]
fn system_load(data_dir: &Path) -> Result<Option<String>, CredentialStoreError> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-w",
            "-s",
            CREDENTIAL_SERVICE,
            "-a",
        ])
        .arg(data_dir)
        .output()?;
    if output.status.code() == Some(44) {
        return Ok(None);
    }
    command_succeeded(output, "load Home Node Owner credential").map(Some)
}

#[cfg(target_os = "macos")]
fn system_remove(data_dir: &Path) -> Result<(), CredentialStoreError> {
    let output = Command::new("security")
        .args(["delete-generic-password", "-s", CREDENTIAL_SERVICE, "-a"])
        .arg(data_dir)
        .output()?;
    if output.status.code() == Some(44) {
        return Ok(());
    }
    command_succeeded(output, "remove Home Node Owner credential").map(|_| ())
}

#[cfg(target_os = "linux")]
fn system_store(data_dir: &Path, credential: &str) -> Result<(), CredentialStoreError> {
    let mut child = Command::new("secret-tool")
        .args([
            "store",
            "--label=Stumble Home Node Owner",
            "service",
            CREDENTIAL_SERVICE,
            "data-dir",
        ])
        .arg(data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(credential.as_bytes())?;
    command_succeeded(
        child.wait_with_output()?,
        "store Home Node Owner credential",
    )
    .map(|_| ())
}

#[cfg(target_os = "linux")]
fn system_load(data_dir: &Path) -> Result<Option<String>, CredentialStoreError> {
    let output = Command::new("secret-tool")
        .args(["lookup", "service", CREDENTIAL_SERVICE, "data-dir"])
        .arg(data_dir)
        .output()?;
    if output.status.success() && output.stdout.is_empty() {
        return Ok(None);
    }
    command_succeeded(output, "load Home Node Owner credential").map(Some)
}

#[cfg(target_os = "linux")]
fn system_remove(data_dir: &Path) -> Result<(), CredentialStoreError> {
    let output = Command::new("secret-tool")
        .args(["clear", "service", CREDENTIAL_SERVICE, "data-dir"])
        .arg(data_dir)
        .output()?;
    command_succeeded(output, "remove Home Node Owner credential").map(|_| ())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn system_store(_data_dir: &Path, _credential: &str) -> Result<(), CredentialStoreError> {
    Err(CredentialStoreError::Backend(
        "unsupported operating system".into(),
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn system_load(_data_dir: &Path) -> Result<Option<String>, CredentialStoreError> {
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
) -> Result<String, CredentialStoreError> {
    if !output.status.success() {
        return Err(CredentialStoreError::Backend(format!(
            "{operation}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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
mod credential_store_tests {
    use super::{IsolatedCredentialStore, OwnerCredentialStore};

    #[test]
    fn credential_store_contract_stores_loads_and_removes_owner_secret() {
        let root = std::env::temp_dir().join(format!(
            "stumble-credential-contract-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let data_dir = root.join("home");
        let store = IsolatedCredentialStore::new(root.join("credentials"));

        assert_eq!(store.load(&data_dir).unwrap(), None);
        store.store(&data_dir, "owner-secret").unwrap();
        assert_eq!(
            store.load(&data_dir).unwrap().as_deref(),
            Some("owner-secret")
        );
        store.remove(&data_dir).unwrap();
        assert_eq!(store.load(&data_dir).unwrap(), None);

        let _ = std::fs::remove_dir_all(root);
    }
}
