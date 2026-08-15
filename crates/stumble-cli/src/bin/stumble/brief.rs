//! `stumble brief get`: one composed presentation payload.
//!
//! The Home Node fills every section from existing operations; the harness
//! only presents. Every top-level key is always present, with empty arrays
//! instead of missing sections.

use super::{agent_tools_error, internal_error, CliResult};
use crate::parser::BriefWorkflow;
use serde_json::{json, Value};
use stumble_core::{
    AgentTools, AuthContext, DiscoveryResultBatchState, ExploreRequest, FeedBatchRequest,
};

pub(super) fn execute(
    command: BriefWorkflow,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    match command {
        BriefWorkflow::Get => compose(tools, actor),
    }
}

fn compose(tools: &AgentTools, actor: &AuthContext) -> CliResult {
    let packet = tools
        .user_context_packet(actor)
        .map_err(agent_tools_error)?;
    let taste_summary = taste_summary(&packet.taste);

    // Outside: the latest ready, unreviewed Discovery Result Batch.
    let mut batches = tools
        .list_discovery_result_batches(actor)
        .map_err(agent_tools_error)?;
    batches.retain(|batch| batch.state == DiscoveryResultBatchState::Ready);
    batches.sort_by_key(|batch| (batch.created_at, batch.id));
    let outside = match batches.pop() {
        Some(batch) => json!({
            "batch_id": batch.id,
            "items": batch.items,
            "source_availability": batch.source_availability,
        }),
        None => json!({
            "batch_id": null,
            "items": [],
            "reason": "no_unreviewed_batch",
        }),
    };

    // Network feed: the current stable Feed Batch.
    let feed_request = FeedBatchRequest::new(7).expect("the default Feed Batch size is valid");
    let feed = tools
        .get_feed_batch(actor, feed_request, chrono::Utc::now())
        .map_err(agent_tools_error)?;

    // Network explore: at most one public Pod from local announcements.
    // Best-effort bootstrap sync only when nothing is known locally.
    let request = ExploreRequest::new("", 1, 3).expect("bounded explore request is valid");
    let mut explore = tools
        .explore_public_pods(actor, request.clone())
        .map_err(agent_tools_error)?
        .results;
    let mut bootstrap_gap: Option<Value> = None;
    if explore.is_empty() {
        if run_bootstrap_best_effort(tools, actor).is_err() {
            bootstrap_gap = Some(json!({ "state": "bootstrap_not_synced" }));
        }
        explore = tools
            .explore_public_pods(actor, request)
            .map_err(agent_tools_error)?
            .results;
    }
    explore.truncate(1);

    let mut gaps: Vec<Value> = Vec::new();
    for watch in &packet.watches {
        if let Some(availability) = &watch.last_availability {
            if availability.state.authentication_required() {
                gaps.push(json!({
                    "state": availability.state,
                    "source": availability.source,
                    "watch_id": watch.id,
                    "url": watch.url,
                }));
            }
        }
    }
    let notices = tools
        .list_authentication_needed_notices(actor)
        .map_err(agent_tools_error)?;
    for notice in notices.iter().filter(|notice| notice.delivery_pending) {
        if gaps
            .iter()
            .any(|gap| gap["source"].as_str() == Some(notice.source.as_str()))
        {
            continue;
        }
        gaps.push(json!({
            "state": "authentication_required",
            "source": notice.source,
            "fingerprint": notice.state_fingerprint,
        }));
    }
    if explore.is_empty() {
        gaps.push(bootstrap_gap.unwrap_or_else(|| json!({ "state": "no_announcements" })));
    }

    serde_json::to_value(json!({
        "user": {
            "context_md": packet.context_md,
            "taste_summary": taste_summary,
        },
        "outside": outside,
        "network": {
            "feed": feed.items,
            "explore": explore,
        },
        "gaps": gaps,
    }))
    .map_err(internal_error)
}

fn taste_summary(taste: &stumble_core::TasteProfile) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !taste.explicit.interests.is_empty() {
        parts.push(format!(
            "interests: {}",
            taste.explicit.interests.join(", ")
        ));
    }
    if !taste.explicit.blocked_topics.is_empty() {
        parts.push(format!(
            "blocked topics: {}",
            taste.explicit.blocked_topics.join(", ")
        ));
    }
    if !taste.explicit.blocked_sources.is_empty() {
        parts.push(format!(
            "blocked sources: {}",
            taste.explicit.blocked_sources.join(", ")
        ));
    }
    parts.join("; ")
}

/// Synchronizes Bootstrap Announcement Streams; failures never fail the brief.
fn run_bootstrap_best_effort(tools: &AgentTools, actor: &AuthContext) -> Result<(), ()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|_| ())?;
    let client = stumble_api::ReqwestAnnouncementStreamClient::new(runtime.handle().clone());
    let outcome = tools
        .sync_bootstrap_endpoints(actor, &client, chrono::Utc::now())
        .map(|_| ())
        .map_err(|_| ());
    drop(runtime);
    outcome
}
