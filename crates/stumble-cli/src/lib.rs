use serde::Serialize;
use serde_json::Value;
use std::{fmt::Write as _, io::Read, path::Path};

pub const ENVELOPE_VERSION: u8 = 1;

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
