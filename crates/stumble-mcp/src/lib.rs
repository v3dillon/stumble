use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use stumble_core::*;

mod streamable_http;

pub use streamable_http::streamable_http_router;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCall {
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
}

/// Classifies failures at the MCP tool-call protocol boundary.
#[derive(Debug, thiserror::Error)]
pub enum McpToolCallError {
    #[error("invalid tool arguments: {0}")]
    InvalidArguments(#[source] anyhow::Error),
    #[error(transparent)]
    Execution(#[from] anyhow::Error),
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct InvalidToolArguments(String);

#[derive(Clone)]
pub struct McpToolRouter {
    tools: AgentTools,
    ctx: AuthContext,
}

impl McpToolRouter {
    pub fn new(tools: AgentTools, ctx: AuthContext) -> Self {
        Self { tools, ctx }
    }

    /// Builds a router for a current, non-revoked Harness token.
    ///
    /// # Errors
    ///
    /// Returns an error when authentication or persistence fails, or when the
    /// token is invalid or revoked.
    pub fn authenticated(tools: AgentTools, token: &str) -> anyhow::Result<Self> {
        let ctx = tools
            .authenticate_token(token)?
            .ok_or_else(|| anyhow::anyhow!("invalid or revoked Harness token"))?;
        Ok(Self::new(tools, ctx))
    }

    pub fn tool_names() -> &'static [&'static str] {
        &[
            "list_pods",
            "get_feed_batch",
            "complete_feed_batch",
            "record_feed_feedback",
            "set_priority_subscription",
            "get_taste_profile",
            "update_taste_profile",
            "reset_learned_taste",
            "register_agent_harness",
            "revoke_agent_harness",
            "create_pending_proposal",
            "get_pending_proposal",
            "approve_pending_proposal",
            "reject_pending_proposal",
            "create_pod",
            "create_private_pod_with_package",
            "join_pod",
            "submit_candidate",
            "inspect_candidate",
            "materialize_discovery_tasks",
            "list_discovery_tasks",
            "list_ready_discovery_tasks",
            "create_immediate_discovery_task",
            "discovery_task_status",
            "claim_discovery_task",
            "renew_discovery_task",
            "complete_discovery_task",
            "fail_discovery_task",
            "get_pod_package",
            "export_pod_package",
            "import_pod_package",
            "fork_pod_package",
            "validate_pod_package",
            "get_node_info",
            "list_trusted_peers",
            "add_trusted_peer",
            "sync_pod_with_peer",
            "export_pod_events",
            "import_pod_events",
        ]
    }

    pub fn call(&self, call: McpToolCall) -> anyhow::Result<Value> {
        match call.tool.as_str() {
            "get_feed_batch" => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.get_feed_batch(
                    &self.ctx,
                    request,
                    chrono::Utc::now(),
                )?))
            }
            "complete_feed_batch" => {
                let id = arg_string(&call.arguments, "batch_id")?.parse()?;
                Ok(json!(self.tools.complete_feed_batch(
                    &self.ctx,
                    id,
                    chrono::Utc::now(),
                )?))
            }
            "record_feed_feedback" => {
                let id = arg_string(&call.arguments, "content_item_id")?.parse()?;
                let kind = arg_string(&call.arguments, "kind")?
                    .parse::<FeedbackKind>()
                    .map_err(|error| invalid_arguments(error.to_string()))?;
                Ok(json!(self.tools.record_feed_feedback(
                    &self.ctx,
                    id,
                    kind,
                    opt_string(&call.arguments, "topic"),
                    opt_string(&call.arguments, "reason"),
                    chrono::Utc::now(),
                )?))
            }
            "set_priority_subscription" => {
                let request: SetPrioritySubscriptionRequest =
                    serde_json::from_value(call.arguments)?;
                self.tools.set_priority_subscription(
                    &self.ctx,
                    request.pod_id,
                    request.is_priority,
                )?;
                Ok(json!({"status": "updated"}))
            }
            "get_taste_profile" => Ok(json!(self.tools.taste_profile(&self.ctx)?)),
            "update_taste_profile" => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self
                    .tools
                    .update_taste_profile(&self.ctx, request)?))
            }
            "reset_learned_taste" => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.reset_learned_taste(&self.ctx, request)?))
            }
            "register_agent_harness" => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self
                    .tools
                    .register_agent_harness(&self.ctx, request)?))
            }
            "revoke_agent_harness" => {
                let id = arg_string(&call.arguments, "harness_id")?.parse()?;
                self.tools.revoke_agent_harness(&self.ctx, id)?;
                Ok(json!({"revoked": id}))
            }
            "create_pending_proposal" => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.create_pending_proposal_from_request(
                    &self.ctx,
                    request,
                    chrono::Utc::now(),
                )?))
            }
            "get_pending_proposal" => {
                let id = arg_string(&call.arguments, "proposal_id")?.parse()?;
                Ok(json!(self.tools.pending_proposal(
                    &self.ctx,
                    id,
                    chrono::Utc::now(),
                )?))
            }
            "approve_pending_proposal" => {
                let id = arg_string(&call.arguments, "proposal_id")?.parse()?;
                Ok(json!(self.tools.approve_pending_proposal(
                    &self.ctx,
                    id,
                    chrono::Utc::now(),
                )?))
            }
            "reject_pending_proposal" => {
                let id = arg_string(&call.arguments, "proposal_id")?.parse()?;
                let reason = arg_string(&call.arguments, "reason")?;
                Ok(json!(self.tools.reject_pending_proposal(
                    &self.ctx,
                    id,
                    chrono::Utc::now(),
                    reason,
                )?))
            }
            "list_pods" => Ok(json!(self.tools.list_pods_for_harness(&self.ctx)?)),
            "create_pod" => {
                let request: CreatePodRequest = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.request_create_pod(
                    &self.ctx,
                    request,
                    chrono::Utc::now(),
                )?))
            }
            "create_private_pod_with_package" => {
                let request: CreatePrivatePodWithPackageRequest =
                    serde_json::from_value(call.arguments)?;
                Ok(json!(self
                    .tools
                    .create_private_pod_with_package(&self.ctx, request)?))
            }
            "join_pod" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                self.tools.join_pod(&self.ctx, &pod_slug)?;
                Ok(json!({"joined": pod_slug}))
            }
            "submit_candidate" => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.submit_candidate(&self.ctx, request)?))
            }
            "inspect_candidate" => {
                let candidate_id = arg_string(&call.arguments, "candidate_id")?.parse()?;
                Ok(json!(self
                    .tools
                    .inspect_candidate(&self.ctx, candidate_id)?))
            }
            "materialize_discovery_tasks" => Ok(json!(self
                .tools
                .materialize_due_discovery_tasks(&self.ctx, chrono::Utc::now(),)?)),
            "list_discovery_tasks" => Ok(json!(self
                .tools
                .list_discovery_tasks(&self.ctx, chrono::Utc::now())?)),
            "list_ready_discovery_tasks" => Ok(json!(self
                .tools
                .list_ready_discovery_tasks(&self.ctx, chrono::Utc::now())?)),
            "create_immediate_discovery_task" => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.create_immediate_discovery_task(
                    &self.ctx,
                    request,
                    chrono::Utc::now(),
                )?))
            }
            "discovery_task_status" => {
                let task_id = arg_string(&call.arguments, "task_id")?.parse()?;
                Ok(json!(self.tools.discovery_task_status(
                    &self.ctx,
                    task_id,
                    chrono::Utc::now(),
                )?))
            }
            "claim_discovery_task" | "renew_discovery_task" => {
                let task_id = arg_string(&call.arguments, "task_id")?.parse()?;
                let lease_seconds = call
                    .arguments
                    .get("lease_seconds")
                    .and_then(Value::as_u64)
                    .unwrap_or(300);
                let lease_seconds = DiscoveryLeaseSeconds::new(lease_seconds)?;
                let now = chrono::Utc::now();
                let task = if call.tool == "claim_discovery_task" {
                    self.tools
                        .claim_discovery_task(&self.ctx, task_id, now, lease_seconds)?
                } else {
                    self.tools
                        .renew_discovery_task_lease(&self.ctx, task_id, now, lease_seconds)?
                };
                Ok(json!(task))
            }
            "complete_discovery_task" => {
                let task_id = arg_string(&call.arguments, "task_id")?.parse()?;
                Ok(json!(self.tools.complete_discovery_task(
                    &self.ctx,
                    task_id,
                    chrono::Utc::now()
                )?))
            }
            "fail_discovery_task" => {
                let task_id = arg_string(&call.arguments, "task_id")?.parse()?;
                let reason = arg_string(&call.arguments, "reason")?;
                Ok(json!(self.tools.fail_discovery_task(
                    &self.ctx,
                    task_id,
                    chrono::Utc::now(),
                    reason
                )?))
            }
            "get_pod_package" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                Ok(json!(self.tools.get_skill_pack(&self.ctx, &pod_slug)?))
            }
            "export_pod_package" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                Ok(json!(self.tools.export_skill_pack(&self.ctx, &pod_slug)?))
            }
            "import_pod_package" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                let files = call
                    .arguments
                    .get("files")
                    .cloned()
                    .ok_or_else(|| invalid_arguments("missing argument files"))?;
                Ok(json!(self.tools.import_skill_pack(
                    &self.ctx,
                    &pod_slug,
                    serde_json::from_value(files)?
                )?))
            }
            "fork_pod_package" => {
                let source = arg_string(&call.arguments, "source_pod_slug")?;
                let target: CreatePodRequest =
                    serde_json::from_value(call.arguments["target"].clone())?;
                Ok(json!(self
                    .tools
                    .fork_skill_pack(&self.ctx, &source, target)?))
            }
            "validate_pod_package" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                Ok(json!(self
                    .tools
                    .validate_pod_skill_pack(&self.ctx, &pod_slug)?))
            }
            "get_node_info" => Ok(json!(self.tools.node_info(&self.ctx)?)),
            "list_trusted_peers" => Ok(json!(self.tools.trusted_peers(&self.ctx)?)),
            "add_trusted_peer" => {
                let display_name = arg_string(&call.arguments, "display_name")?;
                let base_url = arg_string(&call.arguments, "base_url")?;
                let public_key = arg_string(&call.arguments, "public_key")?;
                Ok(json!(self.tools.request_add_trusted_peer(
                    &self.ctx,
                    display_name,
                    base_url,
                    public_key,
                    chrono::Utc::now(),
                )?))
            }
            "export_pod_events" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                Ok(json!(self.tools.export_pod_events(&self.ctx, &pod_slug)?))
            }
            "import_pod_events" => {
                let peer_id = arg_string(&call.arguments, "peer_id")?.parse()?;
                let events = call
                    .arguments
                    .get("events")
                    .cloned()
                    .ok_or_else(|| invalid_arguments("missing argument events"))?;
                Ok(json!({"imported_events": self.tools.import_pod_events(
                    &self.ctx,
                    peer_id,
                    serde_json::from_value(events)?,
                )?}))
            }
            "sync_pod_with_peer" => Err(anyhow::anyhow!(
                "sync_pod_with_peer requires the asynchronous MCP dispatcher"
            )),
            "submit_link_to_pod" => Err(LegacyContract::LegacySubmission.error().into()),
            "add_source_to_pod" | "crawl_pod_sources" => {
                Err(LegacyContract::CrawlerSourceConnector.error().into())
            }
            "discover_in_pod" | "stumble_pod" | "get_pod_brief" => {
                Err(LegacyContract::LegacyFeedPresentation.error().into())
            }
            "save_link" | "rate_link" | "block_source" | "block_topic" => {
                Err(LegacyContract::LegacyFeedback.error().into())
            }
            "get_pod_skill"
            | "list_pod_skills"
            | "export_pod_skill_pack"
            | "import_pod_skill_pack"
            | "fork_pod_skill_pack"
            | "validate_pod_skill_pack" => Err(LegacyContract::LegacySkillPack.error().into()),
            unknown => Err(anyhow::anyhow!("unknown MCP tool {unknown}")),
        }
    }

    /// Calls a tool while preserving the MCP distinction between malformed
    /// arguments and failures produced by a valid tool execution.
    pub fn call_checked(&self, call: McpToolCall) -> Result<Value, McpToolCallError> {
        if !call.arguments.is_object() {
            return Err(McpToolCallError::InvalidArguments(invalid_arguments(
                "arguments must be an object",
            )));
        }
        self.call(call).map_err(|error| {
            if error.downcast_ref::<InvalidToolArguments>().is_some()
                || error.downcast_ref::<serde_json::Error>().is_some()
                || error.downcast_ref::<uuid::Error>().is_some()
                || error.downcast_ref::<DiscoveryLeaseSecondsError>().is_some()
            {
                McpToolCallError::InvalidArguments(error)
            } else {
                McpToolCallError::Execution(error)
            }
        })
    }

    /// Dispatches tools that may perform outbound synchronization.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid arguments, authorization failures,
    /// incompatible peers, network failures, or underlying tool failures.
    pub async fn call_async(&self, call: McpToolCall) -> anyhow::Result<Value> {
        if call.tool != "sync_pod_with_peer" {
            return self.call(call);
        }
        let peer_id = arg_string(&call.arguments, "peer_id")?.parse()?;
        let pod_slug = arg_string(&call.arguments, "pod_slug")?;
        let peer = self.tools.trusted_peer(&self.ctx, peer_id)?;
        Ok(json!(
            stumble_sync::sync_pod_from_peer(&self.tools, &self.ctx, &peer, &pod_slug,).await?
        ))
    }
}

fn arg_string(args: &Value, key: &str) -> anyhow::Result<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| invalid_arguments(format!("missing or non-string argument {key}")))
}

fn invalid_arguments(message: impl Into<String>) -> anyhow::Error {
    InvalidToolArguments(message.into()).into()
}

fn opt_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn harness_capability_denial_is_returned_by_mcp() {
        let tools = AgentTools::new(seed_store());
        let owner = tools.default_auth_context().unwrap();
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "submitter".into(),
                    kind: AgentHarnessKind::Unattended,
                    capabilities: vec![HarnessCapability::CandidateSubmission],
                    pod_ids: None,
                },
            )
            .unwrap();
        let router = McpToolRouter::authenticated(tools, issued.token.expose()).unwrap();
        let error = router
            .call(McpToolCall {
                tool: "record_feed_feedback".into(),
                arguments: json!({"content_item_id": Uuid::nil(), "kind": "save"}),
            })
            .unwrap_err();
        assert!(error.to_string().contains("harness grant lacks feedback"));
    }
}
