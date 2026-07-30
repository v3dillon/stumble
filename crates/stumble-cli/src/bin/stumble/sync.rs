use super::{
    agent_tools_error, direct_subscription_error, internal_error, page, parse_id, peer_sync_error,
    peer_sync_failure_code, peer_sync_failure_is_retryable, resolve_pod, CliResult,
};
use crate::parser::{
    BootstrapWorkflow, DiscoveryServeWorkflow, DiscoveryWorkflow, IndexNodeWorkflow, PeerWorkflow,
    SyncPodWorkflow, SyncWorkflow,
};
use serde_json::json;
use stumble_core::{
    AgentTools, AuthContext, BootstrapEndpointId, HarnessCapability, NodeInfo, PeerId,
    CURRENT_PROTOCOL_VERSION,
};

pub(super) fn execute(command: SyncWorkflow, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    match command {
        SyncWorkflow::Peer { command } => execute_peer(command, tools, actor),
        SyncWorkflow::Pod { command } => execute_pod(command, tools, actor),
        SyncWorkflow::Bootstrap { command } => execute_bootstrap(command, tools, actor),
        SyncWorkflow::Discovery { command } => execute_discovery(command, tools, actor),
    }
}

fn execute_discovery(
    command: DiscoveryWorkflow,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    match command {
        DiscoveryWorkflow::Status => {
            serde_json::to_value(tools.discovery_status(actor).map_err(agent_tools_error)?)
                .map_err(internal_error)
        }
        DiscoveryWorkflow::Serve { command } => match command {
            DiscoveryServeWorkflow::Show => serde_json::to_value(
                tools
                    .discovery_peer_service_status(actor)
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error),
            DiscoveryServeWorkflow::Enable(args) => serde_json::to_value(
                tools
                    .enable_discovery_peer_service(
                        actor,
                        &args.public_endpoint,
                        chrono::Utc::now(),
                    )
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error),
            DiscoveryServeWorkflow::Disable => serde_json::to_value(
                tools
                    .disable_discovery_peer_service(actor, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error),
        },
        DiscoveryWorkflow::Peers => serde_json::to_value(
            tools
                .outbound_discovery_peers(actor)
                .map_err(agent_tools_error)?,
        )
        .map_err(internal_error),
        DiscoveryWorkflow::Gossip(args) => serde_json::to_value(
            tools
                .set_automatic_peer_gossip_enabled(actor, args.enabled, chrono::Utc::now())
                .map_err(agent_tools_error)?,
        )
        .map_err(internal_error),
        DiscoveryWorkflow::Index { command } => {
            let outcome = |change| {
                tools
                    .change_trust_policy(actor, change, chrono::Utc::now())
                    .map_err(agent_tools_error)
                    .and_then(|outcome| serde_json::to_value(outcome).map_err(internal_error))
            };
            match command {
                IndexNodeWorkflow::List => serde_json::to_value(
                    tools
                        .trust_policy(actor)
                        .map_err(agent_tools_error)?
                        .index_nodes,
                )
                .map_err(internal_error),
                IndexNodeWorkflow::Add(args) => {
                    outcome(stumble_core::TrustPolicyChange::AddIndexNode {
                        label: args.label,
                        base_url: args.base_url,
                    })
                }
                IndexNodeWorkflow::Remove(args) => {
                    outcome(stumble_core::TrustPolicyChange::RemoveIndexNode {
                        base_url: args.base_url,
                    })
                }
            }
        }
        DiscoveryWorkflow::Run(args) => {
            // Multi-thread runtime so the HTTP clients can block_on off the
            // store write path without nesting on a current_thread runtime.
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .map_err(internal_error)?;
            let now = chrono::Utc::now();
            let mut report = serde_json::Map::new();
            if args.learn {
                let sample_client = stumble_api::ReqwestPeerAdvertisementSampleClient::new(
                    runtime.handle().clone(),
                );
                let selection_seed = {
                    use rand_core::{OsRng, RngCore};
                    OsRng.next_u64()
                };
                let selected = tools
                    .learn_and_select_discovery_peers(actor, &sample_client, now, selection_seed)
                    .map_err(agent_tools_error)?;
                report.insert(
                    "selected".into(),
                    serde_json::to_value(selected).map_err(internal_error)?,
                );
            }
            if !args.no_sync {
                let stream_client =
                    stumble_api::ReqwestDiscoveryPeerStreamClient::new(runtime.handle().clone());
                let sync = tools
                    .sync_outbound_discovery_peers(actor, &stream_client, now)
                    .map_err(agent_tools_error)?;
                report.insert(
                    "sync".into(),
                    serde_json::to_value(sync).map_err(internal_error)?,
                );
            }
            drop(runtime);
            Ok(serde_json::Value::Object(report))
        }
    }
}

fn execute_bootstrap(
    command: BootstrapWorkflow,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    match command {
        BootstrapWorkflow::List => serde_json::to_value(
            tools
                .list_bootstrap_endpoints(actor)
                .map_err(agent_tools_error)?,
        )
        .map_err(internal_error),
        BootstrapWorkflow::Status => {
            serde_json::to_value(tools.bootstrap_status(actor).map_err(agent_tools_error)?)
                .map_err(internal_error)
        }
        BootstrapWorkflow::Run => {
            // Multi-thread runtime so the sync client can block_on HTTP off the
            // store write path without nesting on a current_thread runtime.
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .map_err(internal_error)?;
            let client =
                stumble_api::ReqwestAnnouncementStreamClient::new(runtime.handle().clone());
            let report = tools
                .sync_bootstrap_endpoints(actor, &client, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            // Keep runtime alive until all block_on calls finish.
            drop(runtime);
            serde_json::to_value(report).map_err(internal_error)
        }
        BootstrapWorkflow::Add(args) => serde_json::to_value(
            tools
                .add_bootstrap_endpoint(actor, &args.label, &args.base_url, chrono::Utc::now())
                .map_err(agent_tools_error)?,
        )
        .map_err(internal_error),
        BootstrapWorkflow::Disable(args) => {
            let id = parse_id::<BootstrapEndpointId>(&args.id)?;
            serde_json::to_value(
                tools
                    .set_bootstrap_endpoint_enabled(actor, id, false)
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        BootstrapWorkflow::Enable(args) => {
            let id = parse_id::<BootstrapEndpointId>(&args.id)?;
            serde_json::to_value(
                tools
                    .set_bootstrap_endpoint_enabled(actor, id, true)
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        BootstrapWorkflow::Remove(args) => {
            let id = parse_id::<BootstrapEndpointId>(&args.id)?;
            serde_json::to_value(
                tools
                    .remove_bootstrap_endpoint(actor, id)
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
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

fn run_origin_sync(
    tools: &AgentTools,
    actor: &AuthContext,
    pod: &stumble_core::Pod,
    subscription_id: stumble_core::SubscriptionId,
) -> CliResult {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(internal_error)?;
    let result = match runtime.block_on(stumble_sync::synchronize_subscription_from_origin(
        tools,
        actor,
        subscription_id,
    )) {
        Ok(result) => result,
        Err(error) => {
            let retryable = !matches!(
                error,
                stumble_sync::DirectSubscriptionError::InvalidUrl { .. }
                    | stumble_sync::DirectSubscriptionError::InvalidAddress(_)
            );
            let code = match &error {
                stumble_sync::DirectSubscriptionError::UnknownCursor => "unknown_cursor",
                stumble_sync::DirectSubscriptionError::Request { .. } => "origin_unreachable",
                _ => "synchronization_failed",
            };
            tools
                .record_subscription_sync_failure(
                    actor,
                    subscription_id,
                    code,
                    error.to_string(),
                    retryable,
                    chrono::Utc::now(),
                )
                .map_err(agent_tools_error)?;
            return Err(direct_subscription_error(error));
        }
    };
    Ok(json!({
        "pod_id": pod.id,
        "slug": pod.slug,
        "peer_id": null,
        "subscription_id": result.subscription.id,
        "cursor": result.subscription.last_event_hash,
        "verification": "verified",
        "latest_event": result.subscription.last_event_hash,
        "last_success": result.subscription.synchronized_at,
        "failure": null,
        "imported_events": result.imported_events,
    }))
}

fn execute_pod(command: SyncPodWorkflow, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    match command {
        SyncPodWorkflow::Run(args) => {
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let subscription = tools
                .subscription_for_pod(actor, pod.id)
                .map_err(agent_tools_error)?;
            let Some(peer) = args.peer.as_deref() else {
                return run_origin_sync(tools, actor, &pod, subscription.id);
            };
            let peer_id = parse_id::<PeerId>(peer)?;
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
                "public_pod_url": subscription.public_pod_url,
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
