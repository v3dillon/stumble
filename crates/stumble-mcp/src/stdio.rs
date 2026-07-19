//! Newline-delimited MCP transport over standard input and output.

use anyhow::Context;
use serde_json::Value;
use std::io::{BufRead, Write};
use std::sync::Arc;
use stumble_core::{AgentTools, AuthContext};

use crate::protocol::{dispatch_authenticated, rpc_error_value, JsonRpcRequest};

/// Serves MCP JSON-RPC messages until standard input reaches EOF.
///
/// Each input line must contain exactly one JSON-RPC message. Responses are
/// emitted as one compact JSON object per line; notifications produce no line.
///
/// # Errors
///
/// Returns an error when a session cannot be authenticated, input cannot be
/// read, or a response cannot be written.
pub async fn serve_stdio(
    authenticate: impl Fn() -> anyhow::Result<(AgentTools, AuthContext)> + Send + Sync + 'static,
    input: impl BufRead,
    mut output: impl Write,
) -> anyhow::Result<()> {
    let authenticate = Arc::new(authenticate);
    for line in input.lines() {
        let line = line.context("read MCP message from stdin")?;
        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(request) if request.has_valid_version() => {
                let authenticate = authenticate.clone();
                let (tools, context) = tokio::task::spawn_blocking(move || authenticate())
                    .await
                    .context("join MCP authentication task")?
                    .context("authenticate MCP session")?;
                dispatch_authenticated(tools, context, request).await
            }
            Ok(request) => Some(rpc_error_value(
                request.id_json().unwrap_or(Value::Null),
                -32600,
                "jsonrpc must be 2.0",
            )),
            Err(error) => {
                let code = if error.is_syntax() || error.is_eof() {
                    -32700
                } else {
                    -32600
                };
                Some(rpc_error_value(Value::Null, code, &error.to_string()))
            }
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)
                .context("write MCP response to stdout")?;
            output
                .write_all(b"\n")
                .context("terminate MCP response line")?;
            output.flush().context("flush MCP response to stdout")?;
        }
    }
    Ok(())
}
