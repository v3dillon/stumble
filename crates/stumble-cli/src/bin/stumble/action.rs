use super::{agent_tools_error, internal_error, resolve_pod, CliResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use stumble_cli::{render_text, ErrorBody, ExitStatusCategory};
use stumble_core::{
    AgentTools, AuthContext, ExploreRequest, FeedBatch, FeedBatchItem, FeedBatchRequest,
    SubmissionId,
};

/// Samples remembered so later runs keep surfacing new network finds.
const NETWORK_HISTORY_LIMIT: usize = 200;

/// Presentation cursor for the bare `stumble` command. This is surface state,
/// not domain state: delivery and completion facts live in the Home Node
/// store; this file only remembers what this surface already showed so every
/// run lands on something new.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct SurfaceState {
    #[serde(default)]
    batch_id: Option<uuid::Uuid>,
    #[serde(default)]
    shown_content_item_ids: Vec<String>,
    #[serde(default)]
    shown_network_urls: Vec<String>,
}

fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join("stumble_surface.json")
}

fn load_state(data_dir: &Path) -> SurfaceState {
    std::fs::read_to_string(state_path(data_dir))
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

fn save_state(
    data_dir: &Path,
    state: &SurfaceState,
) -> Result<(), (ErrorBody, ExitStatusCategory)> {
    let contents = serde_json::to_string_pretty(state).map_err(internal_error)?;
    std::fs::write(state_path(data_dir), contents).map_err(internal_error)
}

/// One Stumble: the next unseen item from the current Feed Batch, a fresh
/// batch once that one is walked, or — when the local Feed is caught up — a
/// clearly labeled sample from an unsubscribed public Pod on the network.
/// Nothing new anywhere reports `caught_up` with next steps.
pub(super) fn execute(data_dir: &Path, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    let mut state = load_state(data_dir);
    let request = FeedBatchRequest::new(7).expect("the default Feed Batch size is valid");
    let mut batch = tools
        .get_feed_batch(actor, request.clone(), chrono::Utc::now())
        .map_err(agent_tools_error)?;
    for recomposed in [false, true] {
        if state.batch_id != Some(batch.id) {
            state.batch_id = Some(batch.id);
            state.shown_content_item_ids.clear();
        }
        let unseen = batch.items.iter().find(|item| {
            !state
                .shown_content_item_ids
                .contains(&item.content_reference.content_item_id.to_string())
        });
        if let Some(item) = unseen {
            state
                .shown_content_item_ids
                .push(item.content_reference.content_item_id.to_string());
            let position = state.shown_content_item_ids.len();
            save_state(data_dir, &state)?;
            return feed_item_result(tools, actor, &batch, item, position);
        }
        // The current batch is fully shown (or arrived caught-up). Complete it
        // once so composition sees content added since, then try the fresh one.
        if !recomposed {
            tools
                .complete_feed_batch(actor, batch.id, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            batch = tools
                .get_feed_batch(actor, request.clone(), chrono::Utc::now())
                .map_err(agent_tools_error)?;
        }
    }
    state.batch_id = Some(batch.id);
    state.shown_content_item_ids.clear();
    let network = network_sample(tools, actor, &mut state)?;
    save_state(data_dir, &state)?;
    Ok(network.unwrap_or_else(caught_up_result))
}

fn feed_item_result(
    tools: &AgentTools,
    actor: &AuthContext,
    batch: &FeedBatch,
    item: &FeedBatchItem,
    position: usize,
) -> CliResult {
    let content_item_id = item.content_reference.content_item_id;
    let assets = tools
        .assets_for_submission(actor, SubmissionId::from(content_item_id))
        .map_err(agent_tools_error)?;
    let mut item_value = serde_json::to_value(item).map_err(internal_error)?;
    if let Some(placements) = item_value["placements"].as_array_mut() {
        for placement in placements {
            let pod_id = placement["pod_id"]
                .as_str()
                .ok_or_else(|| internal_error("Feed placement did not contain a Pod ID"))?;
            let pod = resolve_pod(tools, actor, pod_id)?;
            placement["slug"] = json!(pod.slug);
        }
    }
    Ok(json!({
        "kind": "feed_item",
        "batch": {
            "id": batch.id,
            "state": batch.state,
            "position": position,
            "total": batch.items.len(),
        },
        "item": item_value,
        "assets": assets,
        "hints": [
            format!("stumble feed feedback record {content_item_id} --kind saved"),
            format!("stumble feed feedback record {content_item_id} --kind interesting"),
            format!("stumble feed feedback record {content_item_id} --kind not-for-me"),
            "stumble",
        ],
    }))
}

/// Local-first network fallback: rank already-synchronized announcements with
/// an empty query (no interest-derived remote queries), fetch Origin-signed
/// samples for the top unsubscribed Pods best-effort, and surface the first
/// sample this surface has not shown before.
fn network_sample(
    tools: &AgentTools,
    actor: &AuthContext,
    state: &mut SurfaceState,
) -> Result<Option<Value>, (ErrorBody, ExitStatusCategory)> {
    let request = ExploreRequest::new(String::new(), 25, 3)
        .map_err(|error| agent_tools_error(error.into()))?;
    let mut explored = tools
        .explore_public_pods(actor, request.clone())
        .map_err(agent_tools_error)?;
    let missing_samples = explored
        .results
        .iter()
        .any(|result| !result.is_subscribed && result.sample_content_references.is_empty());
    if missing_samples {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(internal_error)?;
        let sample_client =
            stumble_api::ReqwestOriginExploreSampleClient::new(runtime.handle().clone());
        // Unreachable Origins never fail the action; they just yield no sample.
        let enriched = explored
            .results
            .iter()
            .take(5)
            .filter(|result| !result.is_subscribed && result.sample_content_references.is_empty())
            .filter(|result| {
                tools
                    .fetch_origin_explore_samples(
                        actor,
                        result.announcement.origin_node_id,
                        &result.announcement.pod_slug,
                        3,
                        &sample_client,
                    )
                    .is_ok()
            })
            .count();
        if enriched > 0 {
            explored = tools
                .explore_public_pods(actor, request)
                .map_err(agent_tools_error)?;
        }
    }
    for result in explored
        .results
        .iter()
        .filter(|result| !result.is_subscribed)
    {
        for sample in &result.sample_content_references {
            if state.shown_network_urls.contains(&sample.canonical_url) {
                continue;
            }
            state.shown_network_urls.push(sample.canonical_url.clone());
            if state.shown_network_urls.len() > NETWORK_HISTORY_LIMIT {
                let excess = state.shown_network_urls.len() - NETWORK_HISTORY_LIMIT;
                state.shown_network_urls.drain(..excess);
            }
            return Ok(Some(json!({
                "kind": "network_sample",
                "pod": {
                    "name": result.announcement.pod_name,
                    "slug": result.announcement.pod_slug,
                    "subject": result.announcement.subject,
                    "public_pod_url": result.announcement.public_pod_url,
                    "origin": result.announcement.signer.display_name,
                    "relevance": result.relevance,
                    "reasons": result.reasons,
                },
                "sample": sample,
                "hints": [
                    format!("stumble pod subscribe {}", result.announcement.public_pod_url),
                    "stumble",
                ],
            })));
        }
    }
    Ok(None)
}

fn caught_up_result() -> Value {
    json!({
        "kind": "caught_up",
        "message": "You're caught up — nothing new in the Feed or from the network.",
        "hints": [
            "stumble add <url>                        # save something you just read",
            "stumble sync bootstrap run               # pull fresh network announcements",
            "stumble pod explore --query \"<topic>\"    # go looking for new Pods",
        ],
    })
}

const CARD_WIDTH: usize = 64;
const TEXT_WIDTH: usize = 60;

/// Renders the Stumble result as a terminal card; other shapes fall back to
/// the generic text rendering so `--format text` never loses information.
pub(super) fn render_card(data: &Value) -> String {
    match data["kind"].as_str() {
        Some("feed_item") => feed_card(data),
        Some("network_sample") => network_card(data),
        Some("caught_up") => caught_up_card(data),
        _ => render_text(data),
    }
}

fn feed_card(data: &Value) -> String {
    let item = &data["item"];
    let reference = &item["content_reference"];
    let mut lines = vec![header("stumble")];
    push_reference(&mut lines, reference);
    let mut meta = Vec::new();
    if let Some(kind) = item["kind"].as_str() {
        meta.push(kind.replace('_', " "));
    }
    let pods = strings_at(&item["placements"], |placement| placement["slug"].as_str());
    if !pods.is_empty() {
        meta.push(format!("pod: {}", pods.join(", ")));
    }
    if let (Some(position), Some(total)) = (
        data["batch"]["position"].as_u64(),
        data["batch"]["total"].as_u64(),
    ) {
        meta.push(format!("{position} of {total}"));
    }
    let tags = strings_at(&reference["tags"], Value::as_str);
    if !tags.is_empty() {
        meta.push(format!("tags: {}", tags.join(", ")));
    }
    lines.push(format!("  {}", meta.join(" · ")));
    let local_asset = |asset_type: &str| {
        data["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|asset| asset["asset_type"] == asset_type)
            .find_map(|asset| asset["local_path"].as_str())
    };
    if let Some(path) = local_asset("representative_image") {
        lines.push(format!("  cover: {path}"));
    } else if let Some(url) = reference["media_references"][0]["url"].as_str() {
        lines.push(format!("  image: {url}"));
    }
    if let Some(path) = local_asset("readable_snapshot") {
        lines.push(format!("  archive: {path}"));
    }
    if let Some(reason) = item["ranking_evidence"]["reasons"][0].as_str() {
        lines.push(format!("  because: {reason}"));
    }
    lines.push(String::new());
    let id = reference["content_item_id"].as_str().unwrap_or("<id>");
    lines.push(format!(
        "  react: stumble feed feedback record {id} --kind saved|interesting|not-for-me"
    ));
    lines.push("  next:  stumble".to_string());
    finish(lines)
}

fn network_card(data: &Value) -> String {
    let pod = &data["pod"];
    let mut lines = vec![header("stumble · from the network")];
    push_reference(&mut lines, &data["sample"]);
    if let (Some(name), Some(slug)) = (pod["name"].as_str(), pod["slug"].as_str()) {
        lines.push(format!("  from Pod \"{name}\" ({slug}) — not subscribed"));
    }
    if let Some(subject) = pod["subject"].as_str() {
        for line in wrap(subject, TEXT_WIDTH) {
            lines.push(format!("  {line}"));
        }
    }
    if let Some(url) = pod["public_pod_url"].as_str() {
        lines.push(String::new());
        lines.push(format!("  subscribe: stumble pod subscribe {url}"));
        lines.push("  next:      stumble".to_string());
    }
    finish(lines)
}

fn caught_up_card(data: &Value) -> String {
    let mut lines = vec![header("stumble")];
    let message = data["message"]
        .as_str()
        .unwrap_or("You're caught up — nothing new right now.");
    lines.push(format!("  {message}"));
    lines.push(String::new());
    for hint in strings_at(&data["hints"], Value::as_str) {
        lines.push(format!("  {hint}"));
    }
    finish(lines)
}

fn push_reference(lines: &mut Vec<String>, reference: &Value) {
    if let Some(title) = reference["title"].as_str() {
        lines.push(format!("  {title}"));
    }
    if let Some(url) = reference["source_url"].as_str() {
        lines.push(format!("  {url}"));
    }
    let summary = reference["summary"]
        .as_str()
        .or_else(|| reference["permitted_description"].as_str());
    if let Some(summary) = summary {
        lines.push(String::new());
        for line in wrap(summary, TEXT_WIDTH) {
            lines.push(format!("  {line}"));
        }
    }
    lines.push(String::new());
}

fn strings_at<'value>(
    value: &'value Value,
    pick: impl Fn(&'value Value) -> Option<&'value str>,
) -> Vec<&'value str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(pick)
        .collect()
}

fn header(label: &str) -> String {
    let prefix = format!("── {label} ");
    let fill = CARD_WIDTH.saturating_sub(prefix.chars().count());
    format!("{prefix}{}", "─".repeat(fill))
}

fn finish(mut lines: Vec<String>) -> String {
    lines.push("─".repeat(CARD_WIDTH));
    lines.push(String::new());
    lines.join("\n")
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
