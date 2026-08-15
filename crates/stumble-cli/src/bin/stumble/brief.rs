//! `stumble brief get`: one composed presentation payload.

use super::{agent_tools_error, internal_error, CliResult};
use crate::parser::BriefWorkflow;
use stumble_core::{AgentTools, AuthContext};

pub(super) fn execute(
    command: BriefWorkflow,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    match command {
        BriefWorkflow::Get => {
            maybe_sync_bootstrap(tools, actor);
            serde_json::to_value(
                tools
                    .compose_brief(actor, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
    }
}

/// Best-effort Bootstrap sync when local announcements are empty.
fn maybe_sync_bootstrap(tools: &AgentTools, actor: &AuthContext) {
    let Ok(request) = stumble_core::ExploreRequest::new("", 1, 3) else {
        return;
    };
    let empty = tools
        .explore_public_pods(actor, request)
        .map(|response| response.results.is_empty())
        .unwrap_or(true);
    if !empty {
        return;
    }
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
    else {
        return;
    };
    let client = stumble_api::ReqwestAnnouncementStreamClient::new(runtime.handle().clone());
    let _ = tools.sync_bootstrap_endpoints(actor, &client, chrono::Utc::now());
    drop(runtime);
}
