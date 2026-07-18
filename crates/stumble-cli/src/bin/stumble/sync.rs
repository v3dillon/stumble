use super::{
    agent_tools_error, internal_error, page, parse_id, peer_sync_error, peer_sync_failure_code,
    peer_sync_failure_is_retryable, resolve_pod, CliResult,
};
use crate::parser::{PeerWorkflow, SyncPodWorkflow, SyncWorkflow};
use serde_json::json;
use stumble_core::{
    AgentTools, AuthContext, HarnessCapability, NodeInfo, PeerId, CURRENT_PROTOCOL_VERSION,
};

pub(super) fn execute(command: SyncWorkflow, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    match command {
        SyncWorkflow::Peer { command } => execute_peer(command, tools, actor),
        SyncWorkflow::Pod { command } => execute_pod(command, tools, actor),
    }
}

fn execute_peer(command: PeerWorkflow, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    match command {
        PeerWorkflow::List(args) => {
            let peers = tools.trusted_peers(actor).map_err(agent_tools_error)?;
            serde_json::to_value(page(peers, &args)?).map_err(internal_error)
        }
        PeerWorkflow::Add(args) => {
            let node = NodeInfo {
                node_id: parse_id(&args.node_id)?,
                display_name: args.display_name,
                public_key: args.public_key,
                supported_protocol_version: CURRENT_PROTOCOL_VERSION.to_string(),
            };
            let node_id = node.node_id;
            let proposal = tools
                .request_add_trusted_node(actor, node, args.base_url, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            Ok(json!({
                "status": "pending_approval",
                "node_id": node_id,
                "proposal": proposal,
            }))
        }
        PeerWorkflow::Remove(args) => {
            let peer_id = parse_id::<PeerId>(&args.peer_id)?;
            let peer = tools
                .trusted_peer(actor, peer_id)
                .map_err(agent_tools_error)?;
            let proposal = tools
                .request_remove_trusted_peer(actor, peer_id, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            Ok(json!({
                "status": "pending_approval",
                "peer_id": peer.id,
                "node_id": peer.node_id,
                "proposal": proposal,
            }))
        }
    }
}

fn execute_pod(command: SyncPodWorkflow, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    match command {
        SyncPodWorkflow::Run(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let subscription = tools
                .subscription_for_pod(actor, pod.id)
                .map_err(agent_tools_error)?;
            let peer_id = parse_id::<PeerId>(&args.peer)?;
            let peer = tools
                .trusted_peer(actor, peer_id)
                .map_err(agent_tools_error)?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(internal_error)?;
            let result = match runtime.block_on(stumble_sync::synchronize_subscription_from_peer(
                tools,
                actor,
                &peer,
                subscription.id,
            )) {
                Ok(result) => result,
                Err(error) => {
                    let message = error.to_string();
                    let retryable = peer_sync_failure_is_retryable(&error);
                    let code = peer_sync_failure_code(&error);
                    tools
                        .record_subscription_sync_failure(
                            actor,
                            subscription.id,
                            code,
                            message,
                            retryable,
                            chrono::Utc::now(),
                        )
                        .map_err(agent_tools_error)?;
                    return Err(peer_sync_error(error));
                }
            };
            Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "peer_id": peer.id,
                "subscription_id": result.subscription.id,
                "cursor": result.subscription.last_event_hash,
                "verification": "verified",
                "latest_event": result.subscription.last_event_hash,
                "last_success": result.subscription.synchronized_at,
                "failure": null,
                "imported_events": result.imported_events,
            }))
        }
        SyncPodWorkflow::Status(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let subscription = tools
                .subscription_for_pod(actor, pod.id)
                .map_err(agent_tools_error)?;
            let can_run = tools
                .require_harness_capability(actor, HarnessCapability::Administration)
                .is_ok()
                && tools.trusted_peers(actor).is_ok_and(|peers| {
                    peers.into_iter().any(|peer| {
                        peer.node_id == subscription.origin_node_id
                            && peer.public_key == subscription.origin_public_key
                    })
                });
            let verification = if subscription
                .last_sync_failure
                .as_ref()
                .is_some_and(|failure| !failure.retryable)
            {
                "failed"
            } else {
                "verified"
            };
            let failure = subscription.last_sync_failure.as_ref().map(|failure| {
                json!({
                    "code": failure.code,
                    "message": failure.message,
                    "retryable": failure.retryable,
                    "occurred_at": failure.occurred_at,
                    "action": if failure.retryable { "run" } else { "review_peer" },
                })
            });
            Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "subscription_id": subscription.id,
                "origin_node_id": subscription.origin_node_id,
                "cursor": subscription.last_event_hash,
                "verification": verification,
                "latest_event": subscription.last_event_hash,
                "last_success": subscription.synchronized_at,
                "failure": failure,
                "allowed_actions": if can_run { vec!["run"] } else { Vec::new() },
            }))
        }
    }
}
