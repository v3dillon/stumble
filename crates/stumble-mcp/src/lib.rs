use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use stumble_core::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCall {
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
}

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
            "register_agent_harness",
            "revoke_agent_harness",
            "create_pending_proposal",
            "get_pending_proposal",
            "approve_pending_proposal",
            "reject_pending_proposal",
            "create_pod",
            "create_private_pod_with_package",
            "join_pod",
            "submit_link_to_pod",
            "submit_candidate",
            "inspect_candidate",
            "add_source_to_pod",
            "materialize_discovery_tasks",
            "list_discovery_tasks",
            "list_ready_discovery_tasks",
            "create_immediate_discovery_task",
            "discovery_task_status",
            "claim_discovery_task",
            "renew_discovery_task",
            "complete_discovery_task",
            "fail_discovery_task",
            "crawl_pod_sources",
            "discover_in_pod",
            "stumble_pod",
            "get_pod_brief",
            "save_link",
            "rate_link",
            "block_source",
            "block_topic",
            "explain_recommendation",
            "get_pod_skill",
            "list_pod_skills",
            "export_pod_skill_pack",
            "import_pod_skill_pack",
            "fork_pod_skill_pack",
            "validate_pod_skill_pack",
            "suggest_pod_skill_update",
            "get_node_info",
            "list_trusted_peers",
            "add_trusted_peer",
            "sync_peer",
            "sync_pod_with_peer",
            "export_pod_events",
            "import_pod_events",
            "verify_pod_events",
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
                    .parse()
                    .map_err(anyhow::Error::msg)?;
                Ok(json!(self.tools.record_feed_feedback(
                    &self.ctx,
                    id,
                    kind,
                    opt_string(&call.arguments, "topic"),
                    opt_string(&call.arguments, "reason"),
                    chrono::Utc::now(),
                )?))
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
            "submit_link_to_pod" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                let skill_context = self.tools.pod_agent_context(&self.ctx, &pod_slug)?;
                let request = SubmitLinkRequest {
                    url: arg_string(&call.arguments, "url")?,
                    title: opt_string(&call.arguments, "title"),
                    description: opt_string(&call.arguments, "description"),
                    note: opt_string(&call.arguments, "note"),
                    tags: string_list(&call.arguments, "tags"),
                    discovered_by_crawler: false,
                };
                let submission = self
                    .tools
                    .submit_link_to_pod(&self.ctx, &pod_slug, request)?;
                Ok(json!({
                    "pod_skill_read": skill_read_receipt(&skill_context),
                    "submission": submission
                }))
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
            "add_source_to_pod" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                let url = arg_string(&call.arguments, "url")?;
                Ok(json!(self.tools.add_source_to_pod(
                    &self.ctx,
                    &pod_slug,
                    CrawlerSourceType::Rss,
                    url
                )?))
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
            "discover_in_pod" | "stumble_pod" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                let skill_context = self.tools.pod_agent_context(&self.ctx, &pod_slug)?;
                let mode = if call.tool == "stumble_pod" {
                    DiscoveryMode::Stumble
                } else {
                    DiscoveryMode::DeepMatch
                };
                let request = DiscoverRequest {
                    query: arg_string(&call.arguments, "query")?,
                    avoid: string_list(&call.arguments, "avoid"),
                    limit: call
                        .arguments
                        .get("limit")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(7) as usize,
                    mode,
                    user_id: self.ctx.user_id,
                };
                let items = self.tools.discover_in_pod(&self.ctx, &pod_slug, request)?;
                Ok(json!({
                    "pod_skill_read": skill_read_receipt(&skill_context),
                    "items": items
                }))
            }
            "get_pod_brief" => {
                let pod_slugs = string_list(&call.arguments, "pod_slugs");
                let skill_contexts = pod_slugs
                    .iter()
                    .map(|slug| self.tools.pod_agent_context(&self.ctx, slug))
                    .collect::<Result<Vec<_>, _>>()?;
                let request = GenerateBriefRequest {
                    pod_slugs,
                    query: opt_string(&call.arguments, "query"),
                    user_id: self.ctx.user_id,
                };
                let brief = self.tools.generate_brief(&self.ctx, request)?;
                Ok(json!({
                    "pod_skills_read": skill_contexts.iter().map(skill_read_receipt).collect::<Vec<_>>(),
                    "brief": brief
                }))
            }
            "save_link" => {
                let id = arg_string(&call.arguments, "submission_id")?.parse()?;
                self.tools.save_link(&self.ctx, id)?;
                Ok(json!({"saved": id}))
            }
            "block_source" => {
                let source = arg_string(&call.arguments, "source")?;
                self.tools.block_source(&self.ctx, source.clone())?;
                Ok(json!({"blocked_source": source}))
            }
            "block_topic" => {
                let topic = arg_string(&call.arguments, "topic")?;
                self.tools.block_topic(&self.ctx, topic.clone())?;
                Ok(json!({"blocked_topic": topic}))
            }
            "get_pod_skill" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                Ok(json!(self.tools.get_skill_pack(&self.ctx, &pod_slug)?))
            }
            "list_pod_skills" => Ok(json!(self.tools.list_pods_for_harness(&self.ctx)?)),
            "export_pod_skill_pack" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                Ok(json!(self.tools.export_skill_pack(&self.ctx, &pod_slug)?))
            }
            "import_pod_skill_pack" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                let files = call
                    .arguments
                    .get("files")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("missing files"))?;
                Ok(json!(self.tools.import_skill_pack(
                    &self.ctx,
                    &pod_slug,
                    serde_json::from_value(files)?
                )?))
            }
            "fork_pod_skill_pack" => {
                let source = arg_string(&call.arguments, "source_pod_slug")?;
                let target: CreatePodRequest =
                    serde_json::from_value(call.arguments["target"].clone())?;
                Ok(json!(self
                    .tools
                    .fork_skill_pack(&self.ctx, &source, target)?))
            }
            "validate_pod_skill_pack" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                Ok(json!(self
                    .tools
                    .validate_pod_skill_pack(&self.ctx, &pod_slug)?))
            }
            "get_node_info" => Ok(json!(self.tools.node_info(&self.ctx)?)),
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
            "export_pod_events" | "verify_pod_events" => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                Ok(json!(self.tools.export_pod_events(&self.ctx, &pod_slug)?))
            }
            "explain_recommendation"
            | "crawl_pod_sources"
            | "rate_link"
            | "suggest_pod_skill_update"
            | "sync_peer"
            | "sync_pod_with_peer"
            | "import_pod_events"
            | "list_trusted_peers" => Ok(
                json!({"status":"adapter_boundary","tool": call.tool, "note":"Tool name is reserved and routed through AgentTools-compatible boundaries in the MVP."}),
            ),
            unknown => Err(anyhow::anyhow!("unknown MCP tool {unknown}")),
        }
    }
}

fn skill_read_receipt(context: &PodAgentContext) -> serde_json::Value {
    json!({
        "pod_slug": context.pod_slug,
        "pod_name": context.pod_name,
        "skill_pack_version": context.skill_pack_version,
        "skill_md_bytes": context.skill_md.len(),
        "valid": context.validation.valid,
    })
}

fn arg_string(args: &Value, key: &str) -> anyhow::Result<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| anyhow::anyhow!("missing argument {key}"))
}

fn opt_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

fn string_list(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default()
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
                tool: "save_link".into(),
                arguments: json!({"submission_id": Uuid::nil()}),
            })
            .unwrap_err();
        assert!(error.to_string().contains("harness grant lacks feedback"));
    }
}
