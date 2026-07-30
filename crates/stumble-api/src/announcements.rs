//! Announcement, withdrawal, and peer-advertisement admission and serving.

use crate::{auth_or_default, ApiError, ApiState};
use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use stumble_core::*;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub(crate) struct ProduceAnnouncementRequest {
    pod_slug: String,
    public_pod_url: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReceiveAnnouncementRequest {
    peer_id: Uuid,
    announcement: PodAnnouncement,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProduceWithdrawalRequest {
    pod_slug: String,
    public_pod_url: Option<String>,
    #[serde(default = "default_true")]
    make_private: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReceiveWithdrawalRequest {
    peer_id: Uuid,
    withdrawal: PodWithdrawal,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnnouncementSearchQuery {
    q: Option<String>,
    limit: Option<usize>,
}

pub(crate) async fn produce_pod_announcement(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<ProduceAnnouncementRequest>,
) -> Result<Json<PodAnnouncement>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.pod_announcement(
        &ctx,
        &request.pod_slug,
        &request.public_pod_url,
    )?))
}

/// Open Bootstrap admission: no User account or Trusted Peer required.
pub(crate) async fn bootstrap_admit_announcement(
    State(state): State<ApiState>,
    Json(announcement): Json<PodAnnouncement>,
) -> Result<Json<BootstrapAdmissionAcceptance>, ApiError> {
    Ok(Json(
        state.tools.admit_bootstrap_announcement(announcement)?,
    ))
}

/// Open Bootstrap withdrawal admission: no User account or Trusted Peer required.
pub(crate) async fn bootstrap_admit_withdrawal(
    State(state): State<ApiState>,
    Json(withdrawal): Json<PodWithdrawal>,
) -> Result<Json<BootstrapWithdrawalAcceptance>, ApiError> {
    Ok(Json(state.tools.admit_bootstrap_withdrawal(withdrawal)?))
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnnouncementStreamQuery {
    cursor: Option<String>,
    limit: Option<usize>,
}

/// Topic-neutral cursor-paginated Announcement Stream (no personalization).
pub(crate) async fn bootstrap_announcement_stream(
    State(state): State<ApiState>,
    Query(query): Query<AnnouncementStreamQuery>,
) -> Result<Json<AnnouncementStreamPage>, ApiError> {
    Ok(Json(state.tools.announcement_stream(
        query.cursor.as_deref(),
        query.limit,
    )?))
}

/// Open Bootstrap admission of a signed Discovery Peer Advertisement.
pub(crate) async fn bootstrap_admit_peer_advertisement(
    State(state): State<ApiState>,
    Json(advertisement): Json<DiscoveryPeerAdvertisement>,
) -> Result<Json<DiscoveryPeerAdmissionAcceptance>, ApiError> {
    Ok(Json(
        state
            .tools
            .admit_discovery_peer_advertisement(advertisement)?,
    ))
}

/// Bootstrap-open unranked sample of admitted peer advertisements.
pub(crate) async fn bootstrap_peer_advertisement_sample(
    State(state): State<ApiState>,
    Query(query): Query<PeerAdvertisementSampleQuery>,
) -> Result<Json<DiscoveryPeerAdvertisementSample>, ApiError> {
    Ok(Json(
        state
            .tools
            .bootstrap_peer_advertisement_sample(query.limit)?,
    ))
}

/// Discovery Peer Announcement Stream (opt-in serving only).
pub(crate) async fn peer_announcement_stream(
    State(state): State<ApiState>,
    Query(query): Query<AnnouncementStreamQuery>,
) -> Result<Json<AnnouncementStreamPage>, ApiError> {
    Ok(Json(state.tools.peer_announcement_stream(
        query.cursor.as_deref(),
        query.limit,
    )?))
}

#[derive(Debug, Deserialize)]
pub(crate) struct PeerAdvertisementSampleQuery {
    limit: Option<usize>,
}

/// Small randomized unranked sample of current peer advertisements.
///
/// Shuffle seed is server entropy only; clients cannot supply a seed.
pub(crate) async fn peer_advertisement_sample(
    State(state): State<ApiState>,
    Query(query): Query<PeerAdvertisementSampleQuery>,
) -> Result<Json<DiscoveryPeerAdvertisementSample>, ApiError> {
    Ok(Json(state.tools.peer_advertisement_sample(query.limit)?))
}

/// Verifies and indexes a signed public Pod Announcement (Index role).
pub(crate) async fn index_pod_announcement(
    State(state): State<ApiState>,
    Json(announcement): Json<PodAnnouncement>,
) -> Result<Json<KnownPodAnnouncement>, ApiError> {
    Ok(Json(state.tools.index_pod_announcement(announcement)?))
}

pub(crate) async fn receive_pod_announcement(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<ReceiveAnnouncementRequest>,
) -> Result<Json<KnownPodAnnouncement>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.receive_pod_announcement(
        &ctx,
        request.peer_id,
        request.announcement,
    )?))
}

/// Public Index search: explicit query only; no User account or stable User id.
pub(crate) async fn search_pod_announcements(
    State(state): State<ApiState>,
    Query(query): Query<AnnouncementSearchQuery>,
) -> Result<Json<PodAnnouncementSearchResponse>, ApiError> {
    Ok(Json(state.tools.search_pod_announcements_at(
        &IndexSearchRequest::new(query.q.unwrap_or_default(), query.limit),
        chrono::Utc::now(),
    )?))
}

pub(crate) async fn produce_pod_withdrawal(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<ProduceWithdrawalRequest>,
) -> Result<Json<PodWithdrawal>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.withdraw_public_pod(
        &ctx,
        &request.pod_slug,
        request.public_pod_url.as_deref(),
        request.make_private,
        chrono::Utc::now(),
    )?))
}

pub(crate) async fn index_pod_withdrawal(
    State(state): State<ApiState>,
    Json(withdrawal): Json<PodWithdrawal>,
) -> Result<Json<KnownPodWithdrawal>, ApiError> {
    Ok(Json(state.tools.index_pod_withdrawal(withdrawal)?))
}

pub(crate) async fn receive_pod_withdrawal(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<ReceiveWithdrawalRequest>,
) -> Result<Json<KnownPodWithdrawal>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.receive_pod_withdrawal(
        &ctx,
        request.peer_id,
        request.withdrawal,
    )?))
}

/// Open Bootstrap admission of a signed Pod Endorsement.
pub(crate) async fn bootstrap_admit_endorsement(
    State(state): State<ApiState>,
    Json(endorsement): Json<PodEndorsement>,
) -> Result<Json<PodEndorsement>, ApiError> {
    Ok(Json(state.tools.admit_bootstrap_endorsement(endorsement)?))
}

#[derive(Debug, Deserialize)]
pub(crate) struct EndorsementQuery {
    endorsed_node_id: NodeIdentityId,
    endorsed_pod_slug: String,
}

/// Serves valid endorsements of one endorsed Pod (Bootstrap role).
pub(crate) async fn bootstrap_list_endorsements(
    State(state): State<ApiState>,
    Query(query): Query<EndorsementQuery>,
) -> Result<Json<Vec<PodEndorsement>>, ApiError> {
    Ok(Json(state.tools.bootstrap_endorsements_for(
        query.endorsed_node_id,
        &query.endorsed_pod_slug,
    )?))
}
