//! Federation handlers: node identity, public Pods, manifests, and signed events.

use crate::{auth_or_default, ApiError, ApiState};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use stumble_core::*;
use uuid::Uuid;

pub(crate) async fn well_known_node(
    State(state): State<ApiState>,
    _headers: HeaderMap,
) -> Result<Json<WellKnownNode>, ApiError> {
    let ctx = state.tools.default_auth_context()?;
    Ok(Json(state.tools.well_known_node(&ctx, &state.base_url)?))
}

pub(crate) async fn federation_node(
    State(state): State<ApiState>,
    _headers: HeaderMap,
) -> Result<Json<NodeInfo>, ApiError> {
    let ctx = state.tools.default_auth_context()?;
    Ok(Json(state.tools.node_info(&ctx)?))
}

pub(crate) async fn federation_pods(
    State(state): State<ApiState>,
    _headers: HeaderMap,
) -> Result<Json<Vec<Pod>>, ApiError> {
    let ctx = state.tools.default_auth_context()?;
    Ok(Json(state.tools.list_public_pods(&ctx)?))
}

pub(crate) async fn federation_manifest(
    State(state): State<ApiState>,
    _headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<PodManifest>, ApiError> {
    let ctx = state.tools.default_auth_context()?;
    Ok(Json(state.tools.federation_pod_manifest(&ctx, &slug)?))
}

pub(crate) async fn federation_events(
    State(state): State<ApiState>,
    _headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<Vec<EventLog>>, ApiError> {
    let ctx = state.tools.default_auth_context()?;
    Ok(Json(state.tools.federation_pod_events(&ctx, &slug)?))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ImportEventsRequest {
    peer_id: Uuid,
    events: Vec<EventLog>,
}

pub(crate) async fn federation_import_events(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(_slug): Path<String>,
    Json(request): Json<ImportEventsRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    let imported = state
        .tools
        .import_pod_events(&ctx, request.peer_id, request.events)?;
    Ok(Json(json!({"imported": imported})))
}

pub(crate) async fn federation_sync_pod(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((peer_id, pod_slug)): Path<(Uuid, String)>,
) -> Result<Json<stumble_sync::SyncReport>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    let peer = state.tools.trusted_peer(&ctx, peer_id)?;
    Ok(Json(
        stumble_sync::sync_pod_from_peer(&state.tools, &ctx, &peer, &pod_slug).await?,
    ))
}


#[derive(Debug, Deserialize)]
pub(crate) struct ExploreSamplesRequest {
    announcement: PodAnnouncement,
    #[serde(default = "default_sample_limit")]
    limit: usize,
}

fn default_sample_limit() -> usize {
    3
}

/// Serves bounded Origin-signed Explore samples for the current announcement.
pub(crate) async fn federation_explore_samples(
    State(state): State<ApiState>,
    _headers: HeaderMap,
    Path(slug): Path<String>,
    Json(request): Json<ExploreSamplesRequest>,
) -> Result<Json<PodExploreSamples>, ApiError> {
    if request.announcement.pod_slug != slug {
        return Err(AgentToolsError::from(StoreError::Validation(
            "announcement does not describe the requested Pod".into(),
        ))
        .into());
    }
    let ctx = state.tools.default_auth_context()?;
    Ok(Json(state.tools.pod_explore_samples(
        &ctx,
        &request.announcement,
        request.limit,
    )?))
}
