use registry::{McpTool, ToolDefinition, ToolHandlerKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use stumble_core::*;

mod protocol;
mod registry;
mod stdio;

pub use protocol::streamable_http_router;
pub use stdio::serve_stdio;

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

fn tool_definition(name: &str) -> Option<&'static ToolDefinition> {
    registry::definition(name)
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
        registry::names()
    }

    pub fn call(&self, call: McpToolCall) -> anyhow::Result<Value> {
        if let Some(error) = legacy_tool_error(&call.tool) {
            return Err(error);
        }
        let definition = tool_definition(&call.tool)
            .ok_or_else(|| anyhow::anyhow!("unknown MCP tool {}", call.tool))?;
        if definition.handler == ToolHandlerKind::Async {
            return Err(anyhow::anyhow!(
                "{} requires the asynchronous MCP dispatcher",
                call.tool
            ));
        }
        use McpTool::*;
        match definition.tool {
            GetFeedBatch => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.get_feed_batch(
                    &self.ctx,
                    request,
                    chrono::Utc::now(),
                )?))
            }
            CompleteFeedBatch => {
                let id = arg_string(&call.arguments, "batch_id")?.parse()?;
                Ok(json!(self.tools.complete_feed_batch(
                    &self.ctx,
                    id,
                    chrono::Utc::now(),
                )?))
            }
            RecordFeedFeedback => {
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
            SetPrioritySubscription => {
                let request: SetPrioritySubscriptionRequest =
                    serde_json::from_value(call.arguments)?;
                self.tools.set_priority_subscription(
                    &self.ctx,
                    request.pod_id,
                    request.is_priority,
                )?;
                Ok(json!({"status": "updated"}))
            }
            GetTasteProfile => Ok(json!(self.tools.taste_profile(&self.ctx)?)),
            UpdateTasteProfile => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self
                    .tools
                    .update_taste_profile(&self.ctx, request)?))
            }
            ResetLearnedTaste => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.reset_learned_taste(&self.ctx, request)?))
            }
            RegisterAgentHarness => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self
                    .tools
                    .register_agent_harness(&self.ctx, request)?))
            }
            RevokeAgentHarness => {
                let id = arg_string(&call.arguments, "harness_id")?.parse()?;
                self.tools.revoke_agent_harness(&self.ctx, id)?;
                Ok(json!({"revoked": id}))
            }
            CreatePendingProposal => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.create_pending_proposal_from_request(
                    &self.ctx,
                    request,
                    chrono::Utc::now(),
                )?))
            }
            GetPendingProposal => {
                let id = arg_string(&call.arguments, "proposal_id")?.parse()?;
                Ok(json!(self.tools.pending_proposal(
                    &self.ctx,
                    id,
                    chrono::Utc::now(),
                )?))
            }
            ApprovePendingProposal => {
                let id = arg_string(&call.arguments, "proposal_id")?.parse()?;
                Ok(json!(self.tools.approve_pending_proposal(
                    &self.ctx,
                    id,
                    chrono::Utc::now(),
                )?))
            }
            RejectPendingProposal => {
                let id = arg_string(&call.arguments, "proposal_id")?.parse()?;
                let reason = arg_string(&call.arguments, "reason")?;
                Ok(json!(self.tools.reject_pending_proposal(
                    &self.ctx,
                    id,
                    chrono::Utc::now(),
                    reason,
                )?))
            }
            ListPods => Ok(json!(self.tools.list_pods_for_harness(&self.ctx)?)),
            CreatePod => {
                let request: CreatePodRequest = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.request_create_pod(
                    &self.ctx,
                    request,
                    chrono::Utc::now(),
                )?))
            }
            RouteCandidate => {
                let request: RouteCandidateArguments = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.route_candidate_placement(
                    &self.ctx,
                    request.candidate_id,
                    RouteCandidatePlacementRequest::new(
                        request.pod_id,
                        request.reason,
                        request.confidence,
                    )?,
                    chrono::Utc::now(),
                )?))
            }
            ReviewCandidatePlacement => {
                let request: ReviewCandidatePlacementArguments =
                    serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.review_candidate_placement(
                    &self.ctx,
                    request.candidate_id,
                    request.pod_id,
                    request.decision,
                    request.note,
                    chrono::Utc::now(),
                )?))
            }
            ListPodContent => {
                let pod_id = arg_string(&call.arguments, "pod_id")?.parse()?;
                Ok(json!(self.tools.pod_content_stream(&self.ctx, pod_id)?))
            }
            CreatePrivatePodWithPackage => {
                let request: CreatePrivatePodWithPackageRequest =
                    serde_json::from_value(call.arguments)?;
                Ok(json!(self
                    .tools
                    .create_private_pod_with_package(&self.ctx, request)?))
            }
            JoinPod => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                self.tools.join_pod(&self.ctx, &pod_slug)?;
                Ok(json!({"joined": pod_slug}))
            }
            SubmitCandidate => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.submit_candidate(&self.ctx, request)?))
            }
            InspectCandidate => {
                let candidate_id = arg_string(&call.arguments, "candidate_id")?.parse()?;
                Ok(json!(self
                    .tools
                    .inspect_candidate(&self.ctx, candidate_id)?))
            }
            MaterializeDiscoveryTasks => Ok(json!(self
                .tools
                .materialize_due_discovery_tasks(&self.ctx, chrono::Utc::now(),)?)),
            ListDiscoveryTasks => Ok(json!(self
                .tools
                .list_discovery_tasks(&self.ctx, chrono::Utc::now())?)),
            ListReadyDiscoveryTasks => Ok(json!(self
                .tools
                .list_ready_discovery_tasks(&self.ctx, chrono::Utc::now())?)),
            CreateImmediateDiscoveryTask => {
                let request = serde_json::from_value(call.arguments)?;
                Ok(json!(self.tools.create_immediate_discovery_task(
                    &self.ctx,
                    request,
                    chrono::Utc::now(),
                )?))
            }
            DiscoveryTaskStatus => {
                let task_id = arg_string(&call.arguments, "task_id")?.parse()?;
                Ok(json!(self.tools.discovery_task_status(
                    &self.ctx,
                    task_id,
                    chrono::Utc::now(),
                )?))
            }
            ClaimDiscoveryTask | RenewDiscoveryTask => {
                let task_id = arg_string(&call.arguments, "task_id")?.parse()?;
                let lease_seconds = call
                    .arguments
                    .get("lease_seconds")
                    .and_then(Value::as_u64)
                    .unwrap_or(300);
                let lease_seconds = DiscoveryLeaseSeconds::new(lease_seconds)?;
                let now = chrono::Utc::now();
                let task = if definition.tool == ClaimDiscoveryTask {
                    self.tools
                        .claim_discovery_task(&self.ctx, task_id, now, lease_seconds)?
                } else {
                    self.tools
                        .renew_discovery_task_lease(&self.ctx, task_id, now, lease_seconds)?
                };
                Ok(json!(task))
            }
            CompleteDiscoveryTask => {
                let task_id = arg_string(&call.arguments, "task_id")?.parse()?;
                Ok(json!(self.tools.complete_discovery_task(
                    &self.ctx,
                    task_id,
                    chrono::Utc::now()
                )?))
            }
            FailDiscoveryTask => {
                let task_id = arg_string(&call.arguments, "task_id")?.parse()?;
                let reason = arg_string(&call.arguments, "reason")?;
                Ok(json!(self.tools.fail_discovery_task(
                    &self.ctx,
                    task_id,
                    chrono::Utc::now(),
                    reason
                )?))
            }
            GetPodPackage => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                Ok(json!(self.tools.get_skill_pack(&self.ctx, &pod_slug)?))
            }
            ExportPodPackage => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                Ok(json!(self.tools.export_skill_pack(&self.ctx, &pod_slug)?))
            }
            ImportPodPackage => {
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
            ForkPodPackage => {
                let source = arg_string(&call.arguments, "source_pod_slug")?;
                let target: CreatePodRequest =
                    serde_json::from_value(call.arguments["target"].clone())?;
                Ok(json!(self
                    .tools
                    .fork_skill_pack(&self.ctx, &source, target)?))
            }
            ValidatePodPackage => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                Ok(json!(self
                    .tools
                    .validate_pod_skill_pack(&self.ctx, &pod_slug)?))
            }
            GetNodeInfo => Ok(json!(self.tools.node_info(&self.ctx)?)),
            ListTrustedPeers => Ok(json!(self.tools.trusted_peers(&self.ctx)?)),
            AddTrustedPeer => {
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
            ExportPodEvents => {
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                Ok(json!(self.tools.export_pod_events(&self.ctx, &pod_slug)?))
            }
            ImportPodEvents => {
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
            SubscribePublicPod | SynchronizeSubscription | SyncPodWithPeer => unreachable!(),
        }
    }

    /// Calls a tool while preserving the MCP distinction between malformed
    /// arguments and failures produced by a valid tool execution.
    pub fn call_checked(&self, call: McpToolCall) -> Result<Value, McpToolCallError> {
        validate_call_arguments(&call)?;
        self.call(call).map_err(classify_call_error)
    }

    /// Dispatches tools that may perform outbound synchronization.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid arguments, authorization failures,
    /// incompatible peers, network failures, or underlying tool failures.
    pub async fn call_async(&self, call: McpToolCall) -> anyhow::Result<Value> {
        if let Some(error) = legacy_tool_error(&call.tool) {
            return Err(error);
        }
        let definition = tool_definition(&call.tool)
            .ok_or_else(|| anyhow::anyhow!("unknown MCP tool {}", call.tool))?;
        if definition.handler == ToolHandlerKind::Async {
            if let Some(capability) = definition.capability {
                self.require_capability_on_blocking(capability).await?;
            }
        }
        match definition.handler {
            ToolHandlerKind::Blocking => {
                let router = self.clone();
                tokio::task::spawn_blocking(move || router.call(call)).await?
            }
            ToolHandlerKind::Async if definition.tool == McpTool::SubscribePublicPod => {
                let public_pod_url = arg_string(&call.arguments, "public_pod_url")?;
                Ok(json!(
                    stumble_sync::subscribe_pod_from_url(&self.tools, &self.ctx, &public_pod_url,)
                        .await?
                ))
            }
            ToolHandlerKind::Async if definition.tool == McpTool::SynchronizeSubscription => {
                let subscription_id = arg_string(&call.arguments, "subscription_id")?.parse()?;
                Ok(json!(
                    stumble_sync::synchronize_subscription_from_origin(
                        &self.tools,
                        &self.ctx,
                        subscription_id,
                    )
                    .await?
                ))
            }
            ToolHandlerKind::Async if definition.tool == McpTool::SyncPodWithPeer => {
                let peer_id = arg_string(&call.arguments, "peer_id")?.parse()?;
                let pod_slug = arg_string(&call.arguments, "pod_slug")?;
                let tools = self.tools.clone();
                let ctx = self.ctx.clone();
                let peer = tokio::task::spawn_blocking(move || tools.trusted_peer(&ctx, peer_id))
                    .await??;
                Ok(json!(
                    stumble_sync::sync_pod_from_peer(&self.tools, &self.ctx, &peer, &pod_slug,)
                        .await?
                ))
            }
            ToolHandlerKind::Async => unreachable!("registry async handler has no dispatcher"),
        }
    }

    /// Asynchronously calls a tool while preserving malformed-argument errors.
    pub async fn call_async_checked(&self, call: McpToolCall) -> Result<Value, McpToolCallError> {
        validate_call_arguments(&call)?;
        self.call_async(call).await.map_err(classify_call_error)
    }

    async fn require_capability_on_blocking(
        &self,
        capability: HarnessCapability,
    ) -> anyhow::Result<()> {
        let tools = self.tools.clone();
        let ctx = self.ctx.clone();
        tokio::task::spawn_blocking(move || tools.require_harness_capability(&ctx, capability))
            .await??;
        Ok(())
    }
}

fn legacy_tool_error(name: &str) -> Option<anyhow::Error> {
    let contract = match name {
        "submit_link_to_pod" => LegacyContract::LegacySubmission,
        "add_source_to_pod" | "crawl_pod_sources" => LegacyContract::CrawlerSourceConnector,
        "discover_in_pod" | "stumble_pod" | "get_pod_brief" => {
            LegacyContract::LegacyFeedPresentation
        }
        "save_link" | "rate_link" | "block_source" | "block_topic" => {
            LegacyContract::LegacyFeedback
        }
        "get_pod_skill"
        | "list_pod_skills"
        | "export_pod_skill_pack"
        | "import_pod_skill_pack"
        | "fork_pod_skill_pack"
        | "validate_pod_skill_pack" => LegacyContract::LegacySkillPack,
        _ => return None,
    };
    Some(contract.error().into())
}

fn validate_call_arguments(call: &McpToolCall) -> Result<(), McpToolCallError> {
    call.arguments.is_object().then_some(()).ok_or_else(|| {
        McpToolCallError::InvalidArguments(invalid_arguments("arguments must be an object"))
    })
}

fn classify_call_error(error: anyhow::Error) -> McpToolCallError {
    if error.downcast_ref::<InvalidToolArguments>().is_some()
        || error.downcast_ref::<serde_json::Error>().is_some()
        || error.downcast_ref::<uuid::Error>().is_some()
        || error.downcast_ref::<DiscoveryLeaseSecondsError>().is_some()
    {
        McpToolCallError::InvalidArguments(error)
    } else {
        McpToolCallError::Execution(error)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteCandidateArguments {
    candidate_id: CandidateId,
    pod_id: PodId,
    reason: String,
    confidence: CandidateConfidence,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewCandidatePlacementArguments {
    candidate_id: CandidateId,
    pod_id: PodId,
    decision: PlacementReviewDecision,
    #[serde(default)]
    note: Option<CurationRationale>,
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
    fn every_supported_tool_has_one_complete_definition() {
        let definitions = registry::definitions();
        assert_eq!(definitions.len(), McpTool::VARIANT_COUNT);
        assert_eq!(
            definitions
                .iter()
                .map(|definition| definition.tool)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            definitions.len()
        );
        assert_eq!(
            definitions
                .iter()
                .map(|definition| definition.name)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            definitions.len()
        );
        assert_eq!(
            definitions
                .iter()
                .filter(|definition| definition.handler == ToolHandlerKind::Async)
                .map(|definition| definition.tool)
                .collect::<std::collections::HashSet<_>>(),
            std::collections::HashSet::from([
                McpTool::SubscribePublicPod,
                McpTool::SynchronizeSubscription,
                McpTool::SyncPodWithPeer,
            ])
        );
        let mut discovery_order = definitions
            .iter()
            .filter_map(|definition| definition.discovery_order)
            .collect::<Vec<_>>();
        discovery_order.sort_unstable();
        assert_eq!(
            discovery_order,
            (0..discovery_order.len()).collect::<Vec<_>>()
        );
        for name in McpToolRouter::tool_names() {
            let definition = tool_definition(name).expect("supported tool definition");
            assert_eq!(definition.name, *name);
            assert!(definition.input_schema.is_object());
        }
    }

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

    #[tokio::test]
    async fn asynchronous_entrypoint_preserves_retired_contract_errors() {
        let tools = AgentTools::new(seed_store());
        let router = McpToolRouter::new(tools.clone(), tools.default_auth_context().unwrap());

        let error = router
            .call_async(McpToolCall {
                tool: "submit_link_to_pod".into(),
                arguments: json!({}),
            })
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            LegacyContract::LegacySubmission.error().to_string()
        );
    }
}
