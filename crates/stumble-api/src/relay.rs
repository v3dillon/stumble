//! Relay handlers: Origin-signed Pod snapshots admitted and served verbatim.
//!
//! All routes answer only while the independent Relay capability is enabled;
//! otherwise they return the same disabled pattern as Bootstrap
//! (`relay_disabled`, 404). The Relay never re-signs and never becomes the
//! Origin.

use crate::{ApiError, ApiState};
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
