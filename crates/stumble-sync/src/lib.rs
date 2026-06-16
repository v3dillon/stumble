use serde::{Deserialize, Serialize};
use stumble_core::*;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncReport {
    pub peer_id: Uuid,
    pub pod_slug: String,
    pub fetched_events: usize,
    pub imported_events: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HubRefreshReport {
    pub checked_nodes: usize,
    pub refreshed_nodes: usize,
    pub refreshed_pods: usize,
    pub fetched_events: usize,
    pub imported_events: usize,
    pub errors: Vec<String>,
}

pub async fn sync_pod_from_peer(
    tools: &AgentTools,
    ctx: &AuthContext,
    peer: &TrustedPeer,
    pod_slug: &str,
) -> anyhow::Result<SyncReport> {
    let url = format!(
        "{}/federation/pods/{}/events",
        peer.base_url.trim_end_matches('/'),
        pod_slug
    );
    let events = reqwest::get(url).await?.json::<Vec<EventLog>>().await?;
    let fetched_events = events.len();
    let imported_events =
        import_peer_events_on_blocking(tools.clone(), ctx.clone(), peer.id, events).await?;
    Ok(SyncReport {
        peer_id: peer.id,
        pod_slug: pod_slug.to_string(),
        fetched_events,
        imported_events,
    })
}

pub async fn refresh_hub_index(
    tools: &AgentTools,
    ctx: &AuthContext,
) -> anyhow::Result<HubRefreshReport> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let nodes = tools.list_hub_nodes()?;
    let mut report = HubRefreshReport {
        checked_nodes: nodes
            .iter()
            .filter(|node| node.node_id != ctx.node_id)
            .count(),
        ..HubRefreshReport::default()
    };
    for node in nodes {
        if node.node_id == ctx.node_id {
            continue;
        }
        match refresh_registered_node(tools, ctx, &client, &node).await {
            Ok(node_report) => {
                report.refreshed_nodes += 1;
                report.refreshed_pods += node_report.refreshed_pods;
                report.fetched_events += node_report.fetched_events;
                report.imported_events += node_report.imported_events;
            }
            Err(error) => report
                .errors
                .push(format!("{}: {error}", node.base_url.trim_end_matches('/'))),
        }
    }
    Ok(report)
}

async fn refresh_registered_node(
    tools: &AgentTools,
    ctx: &AuthContext,
    client: &reqwest::Client,
    registered: &HubRegisteredNode,
) -> anyhow::Result<HubRefreshReport> {
    let base = registered.base_url.trim_end_matches('/');
    let remote_node = fetch_remote_node_info(client, base).await?;
    if remote_node.node_id != registered.node_id {
        anyhow::bail!(
            "remote node id {} did not match registered node id {}",
            remote_node.node_id,
            registered.node_id
        );
    }
    tools.register_hub_node(HubRegisterNodeRequest {
        node_id: remote_node.node_id,
        display_name: remote_node.display_name,
        base_url: base.to_string(),
        public_key: remote_node.public_key,
        protocol_version: remote_node.supported_protocol_version,
    })?;

    let pods = client
        .get(format!("{base}/federation/pods"))
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<Pod>>()
        .await?;
    let mut report = HubRefreshReport::default();
    for pod in pods
        .into_iter()
        .filter(|pod| pod.visibility == Visibility::Public)
    {
        match refresh_registered_pod(tools, ctx, client, registered.node_id, base, pod).await {
            Ok(pod_report) => {
                report.refreshed_pods += 1;
                report.fetched_events += pod_report.fetched_events;
                report.imported_events += pod_report.imported_events;
            }
            Err(error) => report.errors.push(error.to_string()),
        }
    }
    Ok(report)
}

async fn fetch_remote_node_info(client: &reqwest::Client, base: &str) -> anyhow::Result<NodeInfo> {
    let well_known = client
        .get(format!("{base}/.well-known/stumble-node"))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status);
    if let Ok(response) = well_known {
        let well_known = response.json::<WellKnownNode>().await?;
        return Ok(well_known.node);
    }
    Ok(client
        .get(format!("{base}/federation/node"))
        .send()
        .await?
        .error_for_status()?
        .json::<NodeInfo>()
        .await?)
}

async fn refresh_registered_pod(
    tools: &AgentTools,
    ctx: &AuthContext,
    client: &reqwest::Client,
    node_id: NodeIdentityId,
    base: &str,
    pod: Pod,
) -> anyhow::Result<HubRefreshReport> {
    let manifest_url = format!("{base}/federation/pods/{}/manifest", pod.slug);
    let events_url = format!("{base}/federation/pods/{}/events", pod.slug);
    let manifest = client
        .get(&manifest_url)
        .send()
        .await?
        .error_for_status()?
        .json::<PodManifest>()
        .await?;
    if manifest.pod.visibility != Visibility::Public {
        anyhow::bail!("manifest for pod {} was not public", pod.slug);
    }
    tools.register_hub_pod(HubRegisterPodRequest {
        node_id,
        node_base_url: base.to_string(),
        pod_slug: manifest.pod.slug.clone(),
        pod_name: manifest.pod.name.clone(),
        description: manifest.pod.description.clone(),
        tags: discovery_tags(&manifest.pod),
        skill_pack_version: manifest.skill_pack_version,
        latest_event_hash: manifest.latest_known_event_hash,
        manifest_url,
        events_url: events_url.clone(),
    })?;

    let events = client
        .get(events_url)
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<EventLog>>()
        .await?;
    let fetched_events = events.len();
    let imported_events =
        import_hub_events_on_blocking(tools.clone(), ctx.clone(), node_id, events).await?;
    Ok(HubRefreshReport {
        fetched_events,
        imported_events,
        ..HubRefreshReport::default()
    })
}

async fn import_peer_events_on_blocking(
    tools: AgentTools,
    ctx: AuthContext,
    peer_id: PeerId,
    events: Vec<EventLog>,
) -> anyhow::Result<usize> {
    tokio::task::spawn_blocking(move || tools.import_pod_events(&ctx, peer_id, events))
        .await?
        .map_err(Into::into)
}

async fn import_hub_events_on_blocking(
    tools: AgentTools,
    ctx: AuthContext,
    node_id: NodeIdentityId,
    events: Vec<EventLog>,
) -> anyhow::Result<usize> {
    tokio::task::spawn_blocking(move || {
        tools.import_public_events_from_hub_node(&ctx, node_id, events)
    })
    .await?
    .map_err(Into::into)
}

fn discovery_tags(pod: &Pod) -> Vec<String> {
    let text = format!("{} {} {}", pod.slug, pod.name, pod.description).to_lowercase();
    let stop = ["the", "and", "for", "with", "pod", "this", "that", "from"];
    let mut tags = Vec::new();
    for token in text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 3 && !stop.contains(token))
    {
        let token = token.to_string();
        if !tags.contains(&token) {
            tags.push(token);
        }
        if tags.len() >= 12 {
            break;
        }
    }
    tags
}

pub fn export_events_jsonl(events: &[EventLog]) -> anyhow::Result<String> {
    Ok(events
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n"))
}

pub fn import_events_jsonl(text: &str) -> anyhow::Result<Vec<EventLog>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}
