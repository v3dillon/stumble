//! Relay handlers: Origin-signed Pod snapshots and Explore samples, verbatim.
//!
//! All routes answer only while the independent Relay capability is enabled;
//! otherwise they return the same disabled pattern as Bootstrap
//! (`relay_disabled`, 404). The Relay never re-signs and never becomes the
//! Origin. Explore samples are a sibling Origin-signed artifact; this process
//! never produces them.

use crate::{federation::ExploreSamplesRequest, ApiError, ApiState};
use axum::{
    extract::{Path, State},
    Json,
};
use stumble_core::*;
use uuid::Uuid;

/// Admits an Origin-signed public Pod snapshot pushed by its Origin Node.
pub(crate) async fn relay_admit_snapshot(
    State(state): State<ApiState>,
    Path((origin_node_id, slug)): Path<(Uuid, String)>,
    Json(snapshot): Json<FederationPodSnapshot>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let publication =
        state
            .tools
            .admit_relay_snapshot(origin_node_id, &slug, snapshot, chrono::Utc::now())?;
    Ok(Json(serde_json::json!({
        "outcome": "admitted",
        "origin_node_id": publication.origin_node_id,
        "pod_slug": publication.pod_slug,
        "received_at": publication.received_at,
    })))
}

/// Serves the stored Origin snapshot (Origin `node` + manifest + events).
pub(crate) async fn relay_pod_snapshot(
    State(state): State<ApiState>,
    Path((origin_node_id, slug)): Path<(Uuid, String)>,
) -> Result<Json<FederationPodSnapshot>, ApiError> {
    let publication = state.tools.relay_publication(origin_node_id, &slug)?;
    Ok(Json(publication.snapshot))
}

/// Serves the stored Origin manifest unchanged.
pub(crate) async fn relay_pod_manifest(
    State(state): State<ApiState>,
    Path((origin_node_id, slug)): Path<(Uuid, String)>,
) -> Result<Json<PodManifest>, ApiError> {
    let publication = state.tools.relay_publication(origin_node_id, &slug)?;
    Ok(Json(publication.snapshot.manifest))
}

/// Serves the stored Origin-signed events unchanged.
pub(crate) async fn relay_pod_events(
    State(state): State<ApiState>,
    Path((origin_node_id, slug)): Path<(Uuid, String)>,
) -> Result<Json<Vec<EventLog>>, ApiError> {
    let publication = state.tools.relay_publication(origin_node_id, &slug)?;
    Ok(Json(publication.snapshot.events))
}

/// Admits an Origin-signed Explore sample artifact pushed by its Origin Node.
pub(crate) async fn relay_admit_explore_samples(
    State(state): State<ApiState>,
    Path((origin_node_id, slug)): Path<(Uuid, String)>,
    Json(samples): Json<PodExploreSamples>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let stored = state.tools.admit_relay_explore_samples(
        origin_node_id,
        &slug,
        samples,
        chrono::Utc::now(),
    )?;
    Ok(Json(serde_json::json!({
        "outcome": "admitted",
        "origin_node_id": stored.origin_node_id,
        "pod_slug": stored.pod_slug,
        "announcement_id": stored.announcement_id,
    })))
}

/// Serves the stored Origin-signed Explore samples unchanged.
///
/// The Relay never re-slices the signed `samples` vec: that would break the
/// Origin signature. Request `limit` is ignored.
pub(crate) async fn relay_serve_explore_samples(
    State(state): State<ApiState>,
    Path((origin_node_id, slug)): Path<(Uuid, String)>,
    Json(request): Json<ExploreSamplesRequest>,
) -> Result<Json<PodExploreSamples>, ApiError> {
    if request.announcement.pod_slug != slug
        || request.announcement.origin_node_id != origin_node_id
    {
        return Err(AgentToolsError::from(StoreError::Validation(
            "announcement does not describe the requested Pod".into(),
        ))
        .into());
    }
    let samples = state.tools.relay_explore_samples(origin_node_id, &slug)?;
    if samples.announcement_id != request.announcement.id {
        return Err(AgentToolsError::from(StoreError::AnnouncementStale).into());
    }
    Ok(Json(samples))
}
