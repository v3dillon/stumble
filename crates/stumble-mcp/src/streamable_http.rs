//! Stateless Streamable HTTP transport for authenticated Stumble MCP tools.

use axum::{
    extract::{rejection::JsonRejection, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use stumble_core::{AgentTools, AgentToolsError, AuthContext, HarnessCapability};
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
struct JsonRpcRequest {
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
    match tokio::task::spawn_blocking(move || dispatch(tools, &token, request)).await {
        Ok(Ok(Some(response))) => Json(response).into_response(),
        Ok(Ok(None)) => StatusCode::ACCEPTED.into_response(),
        Ok(Err(DispatchError::Unauthorized)) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "invalid or revoked bearer token"})),
        )
            .into_response(),
        Ok(Err(DispatchError::Internal(error))) => {
            error!(error = %error, "MCP authentication failed internally");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "MCP authentication failed internally"})),
            )
                .into_response()
        }
        Err(error) => {
            error!(error = %error, "MCP dispatch task failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "MCP dispatch task failed"})),
            )
                .into_response()
        }
    }
}

#[derive(Debug)]
enum DispatchError {
    Unauthorized,
    Internal(AgentToolsError),
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

fn dispatch(
    tools: AgentTools,
    token: &str,
    request: JsonRpcRequest,
) -> Result<Option<Value>, DispatchError> {
    let context = tools
        .authenticate_token(token)
        .map_err(DispatchError::Internal)?
        .ok_or(DispatchError::Unauthorized)?;
    let Some(id) = request.id.into_json() else {
        return Ok(None);
    };
    let result = match request.method.as_str() {
        "initialize" => {
            let Ok(params) =
                serde_json::from_value::<InitializeParams>(Value::Object(request.params.clone()))
            else {
                return Ok(Some(rpc_error_value(
                    id,
                    -32602,
                    "initialize requires protocolVersion, capabilities, and clientInfo",
                )));
            };
            json!({
                "protocolVersion": negotiated_version(&params.protocol_version),
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "stumble", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Use Stumble to save provenance-bearing links, claim discovery work, and retrieve finite personal Feed Batches. Confirm write intent and preserve source provenance."
            })
        }
        "tools/list" => json!({"tools": tool_descriptors(&tools, &context)}),
        "tools/call" => {
            let Some(name) = request.params.get("name").and_then(Value::as_str) else {
                return Ok(Some(rpc_error_value(id, -32602, "missing tool name")));
            };
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let available_tools = tool_descriptors(&tools, &context);
            let Some(descriptor) = available_tools.iter().find(|tool| tool["name"] == name) else {
                return Ok(Some(rpc_error_value(id, -32602, "unknown tool")));
            };
            if let Err(message) = validate_schema(&descriptor["inputSchema"], &arguments, "$args") {
                return Ok(Some(rpc_error_value(id, -32602, &message)));
            }
            let router = McpToolRouter::new(tools, context);
            return Ok(Some(
                match router.call_checked(McpToolCall {
                    tool: name.to_string(),
                    arguments,
                }) {
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
                },
            ));
        }
        "ping" => json!({}),
        _ => return Ok(Some(rpc_error_value(id, -32601, "method not found"))),
    };
    Ok(Some(json!({"jsonrpc": "2.0", "id": id, "result": result})))
}

fn rpc_error(id: Value, code: i32, message: &str) -> Response {
    Json(rpc_error_value(id, code, message)).into_response()
}

fn rpc_error_value(id: Value, code: i32, message: &str) -> Value {
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
    vec![
        descriptor(
            "list_pods",
            "List Pods",
            "List the Pods visible to this Agent Harness.",
            object_schema(json!({}), &[]),
            true,
            false,
            None,
        ),
        descriptor(
            "get_pod_package",
            "Read Pod Package",
            "Read the versioned context, curation instructions, and Source Rules for one Pod.",
            object_schema(json!({"pod_slug": {"type": "string"}}), &["pod_slug"]),
            true,
            false,
            None,
        ),
        descriptor(
            "submit_candidate",
            "Save Discovered Link",
            "Submit one externally discovered link with source metadata, provenance, and proposed Pod placements. This creates a private Candidate, not an accepted placement.",
            candidate_schema(),
            false,
            false,
            Some(HarnessCapability::CandidateSubmission),
        ),
        descriptor(
            "inspect_candidate",
            "Inspect Candidate",
            "Inspect a private Candidate and its independent provenance records.",
            object_schema(
                json!({"candidate_id": {"type": "string", "format": "uuid"}}),
                &["candidate_id"],
            ),
            true,
            false,
            Some(HarnessCapability::CandidateSubmission),
        ),
        descriptor(
            "list_ready_discovery_tasks",
            "List Ready Discovery Tasks",
            "List due discovery work that this Agent Harness is authorized to claim.",
            object_schema(json!({}), &[]),
            true,
            false,
            Some(HarnessCapability::DiscoveryTasks),
        ),
        descriptor(
            "create_immediate_discovery_task",
            "Request Discovery",
            "Create retry-safe discovery work for a Pod from the user's current instructions.",
            object_schema(
                json!({
                    "pod_id": {"type": "string", "format": "uuid"},
                    "instructions": {"type": "string"},
                    "idempotency_key": {"type": "string"}
                }),
                &["pod_id", "instructions", "idempotency_key"],
            ),
            false,
            false,
            Some(HarnessCapability::DiscoveryTasks),
        ),
        descriptor(
            "claim_discovery_task",
            "Claim Discovery Task",
            "Claim a ready Discovery Task with an exclusive, expiring lease.",
            task_lease_schema(),
            false,
            false,
            Some(HarnessCapability::DiscoveryTasks),
        ),
        descriptor(
            "complete_discovery_task",
            "Complete Discovery Task",
            "Mark a claimed Discovery Task complete after its Candidates have been submitted.",
            task_id_schema(),
            false,
            false,
            Some(HarnessCapability::DiscoveryTasks),
        ),
        descriptor(
            "fail_discovery_task",
            "Fail Discovery Task",
            "Record a failed Discovery Task attempt with an inspectable reason.",
            object_schema(
                json!({
                    "task_id": {"type": "string", "format": "uuid"},
                    "reason": {"type": "string"}
                }),
                &["task_id", "reason"],
            ),
            false,
            true,
            Some(HarnessCapability::DiscoveryTasks),
        ),
        descriptor(
            "get_feed_batch",
            "Get Personal Feed",
            "Return a stable finite Feed Batch with provenance, explanations, and allowed actions.",
            feed_batch_schema(),
            false,
            false,
            Some(HarnessCapability::FeedRead),
        ),
        descriptor(
            "complete_feed_batch",
            "Complete Feed Batch",
            "Mark a finite Feed Batch complete after presentation.",
            object_schema(
                json!({"batch_id": {"type": "string", "format": "uuid"}}),
                &["batch_id"],
            ),
            false,
            false,
            Some(HarnessCapability::FeedRead),
        ),
        descriptor(
            "record_feed_feedback",
            "Record Feed Feedback",
            "Record an explicit private Feedback Signal for a delivered Content Item.",
            object_schema(
                json!({
                    "content_item_id": {"type": "string", "format": "uuid"},
                    "kind": {
                        "type": "string",
                        "enum": ["interesting", "not_for_me", "dismissed", "saved", "block_source", "block_topic"]
                    },
                    "topic": {"type": "string"},
                    "reason": {"type": "string"}
                }),
                &["content_item_id", "kind"],
            ),
            false,
            false,
            Some(HarnessCapability::Feedback),
        ),
    ]
    .into_iter()
    .filter_map(|(capability, descriptor)| {
        capability
            .is_none_or(|capability| {
                tools
                    .require_harness_capability(context, capability)
                    .is_ok()
            })
            .then_some(descriptor)
    })
    .collect()
}

fn descriptor(
    name: &str,
    title: &str,
    description: &str,
    input_schema: Value,
    read_only: bool,
    destructive: bool,
    capability: Option<HarnessCapability>,
) -> (Option<HarnessCapability>, Value) {
    (
        capability,
        json!({
            "name": name,
            "title": title,
            "description": description,
            "inputSchema": input_schema,
            "annotations": {
                "readOnlyHint": read_only,
                "openWorldHint": false,
                "destructiveHint": destructive
            }
        }),
    )
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

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn task_id_schema() -> Value {
    object_schema(
        json!({"task_id": {"type": "string", "format": "uuid"}}),
        &["task_id"],
    )
}

fn task_lease_schema() -> Value {
    object_schema(
        json!({
            "task_id": {"type": "string", "format": "uuid"},
            "lease_seconds": {"type": "integer", "minimum": 1, "maximum": 604800}
        }),
        &["task_id"],
    )
}

fn feed_batch_schema() -> Value {
    object_schema(
        json!({
            "size": {"type": "integer", "minimum": 1, "maximum": 100},
            "recurrence_penalty_days": {
                "type": "integer",
                "minimum": 0,
                "maximum": 36500
            },
            "feed_mix": {
                "type": "object",
                "properties": {
                    "high_value_percent": {"type": "integer", "minimum": 0, "maximum": 100},
                    "exploration_percent": {"type": "integer", "minimum": 0, "maximum": 100},
                    "old_gem_percent": {"type": "integer", "minimum": 0, "maximum": 100},
                    "per_pod_cap": {"type": "integer", "minimum": 1},
                    "per_source_cap": {"type": "integer", "minimum": 1}
                },
                "additionalProperties": false
            },
            "batch_intent": {
                "type": "object",
                "properties": {
                    "focus_topics": {"type": "array", "items": {"type": "string"}},
                    "avoid_topics": {"type": "array", "items": {"type": "string"}}
                },
                "additionalProperties": false
            }
        }),
        &[],
    )
}

fn candidate_schema() -> Value {
    object_schema(
        json!({
            "source_url": {"type": "string", "format": "uri"},
            "source_metadata": {
                "type": "object",
                "properties": {
                    "title": {"type": ["string", "null"]},
                    "author": {"type": ["string", "null"]},
                    "published_at": {"type": ["string", "null"], "format": "date-time"}
                },
                "additionalProperties": false
            },
            "permitted_excerpt": {"type": ["string", "null"]},
            "summary": {"type": ["string", "null"]},
            "content_type": {
                "type": "string",
                "enum": ["article", "video", "audio", "image", "podcast", "repository", "dataset", "other"]
            },
            "tags": {"type": "array", "items": {"type": "string"}},
            "provenance": {
                "type": "object",
                "properties": {
                    "discovered_at": {"type": "string", "format": "date-time"},
                    "discovery_method": {"type": "string"},
                    "referrer_url": {"type": ["string", "null"]}
                },
                "required": ["discovered_at", "discovery_method"],
                "additionalProperties": false
            },
            "proposed_placements": {
                "type": "array",
                "minItems": 1,
                "items": {
                    "type": "object",
                    "properties": {
                        "pod_id": {"type": "string", "format": "uuid"},
                        "reason": {"type": "string"},
                        "confidence": {"type": "number", "minimum": 0, "maximum": 1}
                    },
                    "required": ["pod_id", "reason", "confidence"],
                    "additionalProperties": false
                }
            },
            "task_context": {
                "type": ["object", "null"],
                "properties": {
                    "task_id": {"type": "string", "format": "uuid"},
                    "package_version": {"type": "integer", "minimum": 1}
                },
                "required": ["task_id", "package_version"],
                "additionalProperties": false
            },
            "harness_idempotency_key": {"type": "string"},
            "client_idempotency_key": {"type": "string"}
        }),
        &[
            "source_url",
            "source_metadata",
            "content_type",
            "tags",
            "provenance",
            "proposed_placements",
            "harness_idempotency_key",
            "client_idempotency_key",
        ],
    )
}
