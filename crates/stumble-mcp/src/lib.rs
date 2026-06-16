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

    pub fn tool_names() -> &'static [&'static str] {
        &[
            "list_pods",
            "create_pod",
            "join_pod",
            "submit_link_to_pod",
            "add_source_to_pod",
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
            "list_pods" => Ok(json!(self.tools.list_pods(self.ctx.tenant_id)?)),
            "create_pod" => {
                let request: CreatePodRequest = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.create_pod(&self.ctx, request)?))
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
            "list_pod_skills" => Ok(json!(self.tools.list_pods(self.ctx.tenant_id)?)),
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
            | "list_trusted_peers"
            | "add_trusted_peer" => Ok(
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
