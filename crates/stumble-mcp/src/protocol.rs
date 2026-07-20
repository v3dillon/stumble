//! Shared MCP protocol dispatch and authenticated Streamable HTTP transport.

use axum::{
    extract::{rejection::JsonRejection, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use stumble_core::{AgentTools, AgentToolsError, AuthContext};
use tracing::error;
use url::Url;

use crate::{McpToolCall, McpToolCallError, McpToolRouter};

const PROTOCOL_VERSION: &str = "2025-06-18";
const COMPATIBILITY_PROTOCOL_VERSION: &str = "2025-03-26";

#[derive(Clone)]
struct McpHttpState {
    tools: AgentTools,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: RequestId,
    method: String,
    #[serde(default = "Map::new")]
    params: Map<String, Value>,
}

#[derive(Debug, Default)]
enum RequestId {
    #[default]
    Missing,
    Present(JsonRpcId),
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let id = match value {
            Value::String(value) => JsonRpcId::String(value),
            Value::Number(value) => {
                if let Some(value) = value.as_i64() {
                    JsonRpcId::Signed(value)
                } else if let Some(value) = value.as_u64() {
                    JsonRpcId::Unsigned(value)
                } else {
                    return Err(serde::de::Error::custom("request id must be an integer"));
                }
            }
            _ => {
                return Err(serde::de::Error::custom(
                    "request id must be a string or integer",
                ));
            }
        };
        Ok(Self::Present(id))
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum JsonRpcId {
    String(String),
    Signed(i64),
    Unsigned(u64),
}

impl RequestId {
    fn to_json(&self) -> Option<Value> {
        match self {
            Self::Missing => None,
            Self::Present(id) => Some(json!(id)),
        }
    }

    fn into_json(self) -> Option<Value> {
        match self {
            Self::Missing => None,
            Self::Present(id) => Some(json!(id)),
        }
    }
}

impl JsonRpcRequest {
    pub(crate) fn has_valid_version(&self) -> bool {
        self.jsonrpc == "2.0"
    }

    pub(crate) fn id_json(&self) -> Option<Value> {
        self.id.to_json()
    }
}

/// Builds the authenticated, stateless Streamable HTTP MCP router.
pub fn streamable_http_router(tools: AgentTools) -> Router {
    Router::new()
        .route("/mcp", post(handle_mcp))
        .with_state(McpHttpState { tools })
}

async fn handle_mcp(
    State(state): State<McpHttpState>,
    headers: HeaderMap,
    payload: Result<Json<JsonRpcRequest>, JsonRejection>,
) -> Response {
    if !origin_is_allowed(&headers) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "untrusted Origin header"})),
        )
            .into_response();
    }
    let Json(request) = match payload {
        Ok(request) => request,
        Err(error) => {
            let code = if matches!(error, JsonRejection::JsonSyntaxError(_)) {
                -32700
            } else {
                -32600
            };
            return (
                StatusCode::BAD_REQUEST,
                Json(rpc_error_value(Value::Null, code, &error.to_string())),
            )
                .into_response();
        }
    };
    if request.jsonrpc != "2.0" {
        return rpc_error(
            request.id.to_json().unwrap_or(Value::Null),
            -32600,
            "jsonrpc must be 2.0",
        );
    }
    if request.method != "initialize" && !protocol_version_is_supported(&headers) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "unsupported MCP-Protocol-Version"})),
        )
            .into_response();
    }
    let Some(token) = bearer_token(&headers).map(str::to_owned) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "bearer token required"})),
        )
            .into_response();
    };
    let tools = state.tools;
    match dispatch(tools, &token, request).await {
        Ok(Some(response)) => Json(response).into_response(),
        Ok(None) => StatusCode::ACCEPTED.into_response(),
        Err(DispatchError::Unauthorized) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "invalid or revoked bearer token"})),
        )
            .into_response(),
        Err(DispatchError::Internal(error)) => {
            error!(error = %error, "MCP authentication failed internally");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "MCP authentication failed internally"})),
            )
                .into_response()
        }
        Err(DispatchError::Task(error)) => {
            error!(error = %error, "MCP authentication task failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "MCP authentication failed internally"})),
            )
                .into_response()
        }
    }
}

#[derive(Debug)]
enum DispatchError {
    Unauthorized,
    Internal(AgentToolsError),
    Task(tokio::task::JoinError),
}

#[derive(Debug, Deserialize)]
struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    protocol_version: String,
    #[serde(rename = "capabilities")]
    _capabilities: Map<String, Value>,
    #[serde(rename = "clientInfo")]
    _client_info: ClientInfo,
}

#[derive(Debug, Deserialize)]
struct ClientInfo {
    #[serde(rename = "name")]
    _name: String,
    #[serde(rename = "version")]
    _version: String,
}

async fn dispatch(
    tools: AgentTools,
    token: &str,
    request: JsonRpcRequest,
) -> Result<Option<Value>, DispatchError> {
    let authentication_tools = tools.clone();
    let token = token.to_owned();
    let context =
        tokio::task::spawn_blocking(move || authentication_tools.authenticate_token(&token))
            .await
            .map_err(DispatchError::Task)?
            .map_err(DispatchError::Internal)?
            .ok_or(DispatchError::Unauthorized)?;
    Ok(dispatch_authenticated(tools, context, request).await)
}

pub(crate) async fn dispatch_authenticated(
    tools: AgentTools,
    context: AuthContext,
    request: JsonRpcRequest,
) -> Option<Value> {
    let id = request.id.into_json()?;
    let result = match request.method.as_str() {
        "initialize" => {
            let Ok(params) =
                serde_json::from_value::<InitializeParams>(Value::Object(request.params.clone()))
            else {
                return Some(rpc_error_value(
                    id,
                    -32602,
                    "initialize requires protocolVersion, capabilities, and clientInfo",
                ));
            };
            json!({
                "protocolVersion": negotiated_version(&params.protocol_version),
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "stumble", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Use Stumble to save provenance-bearing links, claim discovery work, and retrieve finite personal Feed Batches. Confirm write intent and preserve source provenance."
            })
        }
        "tools/list" => {
            let Ok(descriptors) = tool_descriptors_on_blocking(&tools, &context).await else {
                return Some(rpc_error_value(id, -32603, "tool discovery task failed"));
            };
            json!({"tools": descriptors})
        }
        "tools/call" => {
            let Some(name) = request.params.get("name").and_then(Value::as_str) else {
                return Some(rpc_error_value(id, -32602, "missing tool name"));
            };
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let Ok(available_tools) = tool_descriptors_on_blocking(&tools, &context).await else {
                return Some(rpc_error_value(id, -32603, "tool discovery task failed"));
            };
            let Some(descriptor) = available_tools.iter().find(|tool| tool["name"] == name) else {
                return Some(rpc_error_value(id, -32602, "unknown tool"));
            };
            if let Err(message) = validate_schema(&descriptor["inputSchema"], &arguments, "$args") {
                return Some(rpc_error_value(id, -32602, &message));
            }
            let router = McpToolRouter::new(tools, context);
            let call = McpToolCall {
                tool: name.to_string(),
                arguments,
            };
            let called = router.call_async_checked(call).await;
            return Some(match called {
                Ok(value) => {
                    let structured = json!({"value": value});
                    let text = structured.to_string();
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{"type": "text", "text": text}],
                            "structuredContent": structured,
                            "isError": false
                        }
                    })
                }
                Err(McpToolCallError::InvalidArguments(error)) => {
                    rpc_error_value(id, -32602, &error.to_string())
                }
                Err(McpToolCallError::Execution(error)) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{"type": "text", "text": error.to_string()}],
                        "isError": true
                    }
                }),
            });
        }
        "ping" => json!({}),
        _ => return Some(rpc_error_value(id, -32601, "method not found")),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn rpc_error(id: Value, code: i32, message: &str) -> Response {
    Json(rpc_error_value(id, code, message)).into_response()
}

pub(crate) fn rpc_error_value(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message}
    })
}

fn origin_is_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get("origin") else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Ok(origin) = Url::parse(origin) else {
        return false;
    };
    origin.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
}

fn protocol_version_is_supported(headers: &HeaderMap) -> bool {
    let version = match headers.get("mcp-protocol-version") {
        Some(value) => match value.to_str() {
            Ok(value) => value,
            Err(_) => return false,
        },
        None => COMPATIBILITY_PROTOCOL_VERSION,
    };
    matches!(version, PROTOCOL_VERSION | COMPATIBILITY_PROTOCOL_VERSION)
}

fn negotiated_version(requested: &str) -> &str {
    if requested == PROTOCOL_VERSION {
        requested
    } else {
        PROTOCOL_VERSION
    }
}

fn tool_descriptors(tools: &AgentTools, context: &AuthContext) -> Vec<Value> {
    let mut definitions = crate::registry::definitions()
        .iter()
        .filter_map(|definition| {
            definition.discovery_order.and_then(|order| {
                tool_is_available(tools, context, definition).then_some((order, definition))
            })
        })
        .collect::<Vec<_>>();
    definitions.sort_by_key(|(order, _)| *order);
    definitions
        .into_iter()
        .map(|(_, definition)| {
            json!({
                "name": definition.name,
                "title": definition.title,
                "description": definition.description,
                "inputSchema": definition.input_schema,
                "annotations": {
                    "readOnlyHint": definition.read_only,
                    "openWorldHint": false,
                    "destructiveHint": definition.destructive
                }
            })
        })
        .collect()
}

fn tool_is_available(
    tools: &AgentTools,
    context: &AuthContext,
    definition: &crate::registry::ToolDefinition,
) -> bool {
    use crate::registry::McpTool;

    match definition.tool {
        McpTool::RecordFeedFeedback => tools.require_interactive_feedback(context, false).is_ok(),
        McpTool::GetTasteProfile
        | McpTool::UpdateTasteProfile
        | McpTool::ResetLearnedTaste
        | McpTool::RetractInterestSeed => tools.require_interactive_feedback(context, true).is_ok(),
        _ => definition.capability.is_none_or(|capability| {
            tools
                .require_harness_capability(context, capability)
                .is_ok()
        }),
    }
}

async fn tool_descriptors_on_blocking(
    tools: &AgentTools,
    context: &AuthContext,
) -> Result<Vec<Value>, tokio::task::JoinError> {
    let tools = tools.clone();
    let context = context.clone();
    tokio::task::spawn_blocking(move || tool_descriptors(&tools, &context)).await
}

fn validate_schema(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    if let Some(expected) = schema.get("type") {
        let valid_type = match expected {
            Value::String(expected) => value_matches_type(value, expected),
            Value::Array(expected) => expected
                .iter()
                .filter_map(Value::as_str)
                .any(|expected| value_matches_type(value, expected)),
            _ => false,
        };
        if !valid_type {
            return Err(format!("{path} has the wrong type"));
        }
    }

    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            return Err(format!("{path} is not an allowed value"));
        }
    }

    if let Some(object) = value.as_object() {
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    return Err(format!("{path}.{key} is required"));
                }
            }
        }
        if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
            if let Some(key) = object.keys().find(|key| !properties.contains_key(*key)) {
                return Err(format!("{path}.{key} is not allowed"));
            }
        }
        for (key, item) in object {
            if let Some(property_schema) = properties.get(key) {
                validate_schema(property_schema, item, &format!("{path}.{key}"))?;
            }
        }
    }

    if let Some(array) = value.as_array() {
        if let Some(minimum) = schema.get("minItems").and_then(Value::as_u64) {
            if array.len() < minimum as usize {
                return Err(format!("{path} requires at least {minimum} items"));
            }
        }
        if let Some(item_schema) = schema.get("items") {
            for (index, item) in array.iter().enumerate() {
                validate_schema(item_schema, item, &format!("{path}[{index}]"))?;
            }
        }
    }

    if let Some(number) = value.as_f64() {
        if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64) {
            if number < minimum {
                return Err(format!("{path} must be at least {minimum}"));
            }
        }
        if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64) {
            if number > maximum {
                return Err(format!("{path} must be at most {maximum}"));
            }
        }
    }

    if let (Some(format), Some(text)) =
        (schema.get("format").and_then(Value::as_str), value.as_str())
    {
        let valid = match format {
            "uri" => Url::parse(text).is_ok(),
            "uuid" => uuid::Uuid::parse_str(text).is_ok(),
            "date-time" => chrono::DateTime::parse_from_rfc3339(text).is_ok(),
            _ => true,
        };
        if !valid {
            return Err(format!("{path} is not a valid {format}"));
        }
    }

    Ok(())
}

fn value_matches_type(value: &Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    }
}
