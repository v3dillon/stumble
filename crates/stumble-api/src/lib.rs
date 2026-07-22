use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use stumble_core::*;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

#[derive(Clone)]
pub struct ApiState {
    pub tools: AgentTools,
    pub base_url: String,
    pub dev_tokens_allowed: bool,
    /// Whether missing bearer tokens may use the loopback owner context.
    pub owner_access_allowed: bool,
}

#[derive(Debug, Clone)]
pub struct RouterOptions {
    pub dev_tokens_allowed: bool,
    /// Whether missing bearer tokens may use the loopback owner context.
    pub owner_access_allowed: bool,
}

impl Default for RouterOptions {
    fn default() -> Self {
        Self {
            dev_tokens_allowed: true,
            owner_access_allowed: true,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ApiRouteDoc {
    pub method: &'static str,
    pub path: &'static str,
    pub description: &'static str,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    /// Machine-readable failure class for Agent Harnesses and operators.
    code: &'static str,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.message,
                "code": self.code,
            })),
        )
            .into_response()
    }
}

fn agent_tools_error_code(error: &AgentToolsError) -> &'static str {
    match error {
        AgentToolsError::Forbidden { .. } => "forbidden",
        AgentToolsError::BootstrapRejected { reason, .. } => reason.as_code(),
        AgentToolsError::DiscoveryPeerRejected { reason, .. } => reason.as_code(),
        AgentToolsError::IndexSearch(failure) => failure.kind.as_code(),
        AgentToolsError::Store(StoreError::InvalidSignature) | AgentToolsError::Signing(_) => {
            "invalid_signature"
        }
        AgentToolsError::Store(StoreError::AnnouncementExpired) => "announcement_expired",
        AgentToolsError::Store(StoreError::AnnouncementWithdrawn) => "announcement_withdrawn",
        AgentToolsError::Store(StoreError::AnnouncementStale) => "announcement_stale",
        AgentToolsError::Store(StoreError::WithdrawalStale) => "withdrawal_stale",
        AgentToolsError::Store(StoreError::NotFound(_)) => "not_found",
        AgentToolsError::Store(StoreError::UntrustedPeer) => "untrusted_peer",
        AgentToolsError::Store(StoreError::Validation(_)) | AgentToolsError::BadUrl(_) => {
            "validation_error"
        }
        AgentToolsError::Store(StoreError::Duplicate(_)) => "duplicate",
        AgentToolsError::Store(StoreError::TenantBoundary) => "tenant_boundary",
        AgentToolsError::LockPoisoned | AgentToolsError::Persistence(_) => "internal_error",
        AgentToolsError::IncompatibleProtocol { .. } => "incompatible_protocol",
        _ => "request_error",
    }
}

impl From<AgentToolsError> for ApiError {
    fn from(value: AgentToolsError) -> Self {
        let status = if matches!(value, AgentToolsError::Forbidden { .. }) {
            StatusCode::FORBIDDEN
        } else if matches!(
            value,
            AgentToolsError::LockPoisoned | AgentToolsError::Persistence(_)
        ) {
            StatusCode::INTERNAL_SERVER_ERROR
        } else if matches!(
            value,
            AgentToolsError::BootstrapRejected {
                reason: BootstrapAdmissionRejectionReason::RateLimited,
                ..
            } | AgentToolsError::IndexSearch(IndexSearchFailure {
                kind: IndexSearchFailureKind::RateLimited,
                ..
            }) | AgentToolsError::DiscoveryPeerRejected {
                reason: DiscoveryPeerAdmissionRejectionReason::RateLimited,
                ..
            }
        ) {
            StatusCode::TOO_MANY_REQUESTS
        } else if matches!(
            value,
            AgentToolsError::BootstrapRejected {
                reason: BootstrapAdmissionRejectionReason::BootstrapDisabled,
                ..
            } | AgentToolsError::IndexSearch(IndexSearchFailure {
                kind: IndexSearchFailureKind::IndexDisabled,
                ..
            }) | AgentToolsError::DiscoveryPeerRejected {
                reason: DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled
                    | DiscoveryPeerAdmissionRejectionReason::BootstrapDisabled,
                ..
            }
        ) {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        };
        Self {
            status,
            code: agent_tools_error_code(&value),
            message: value.to_string(),
        }
    }
}

impl From<stumble_sync::PeerSyncError> for ApiError {
    fn from(value: stumble_sync::PeerSyncError) -> Self {
        match value {
            stumble_sync::PeerSyncError::Core(source) => source.into(),
            source @ stumble_sync::PeerSyncError::Request { .. } => Self {
                status: StatusCode::BAD_GATEWAY,
                code: "upstream_error",
                message: source.to_string(),
            },
            source @ stumble_sync::PeerSyncError::ImportTask(_) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "internal_error",
                message: source.to_string(),
            },
            source => Self {
                status: StatusCode::BAD_REQUEST,
                code: "request_error",
                message: source.to_string(),
            },
        }
    }
}

pub fn router(tools: AgentTools) -> Router {
    router_with_base_url(tools, "http://127.0.0.1:8787")
}

pub fn router_with_base_url(tools: AgentTools, base_url: impl Into<String>) -> Router {
    router_with_options(tools, base_url, RouterOptions::default())
}

pub fn router_with_options(
    tools: AgentTools,
    base_url: impl Into<String>,
    options: RouterOptions,
) -> Router {
    let state = ApiState {
        tools,
        base_url: base_url.into(),
        dev_tokens_allowed: options.dev_tokens_allowed,
        owner_access_allowed: options.owner_access_allowed,
    };
    Router::new()
        .route("/health", get(health))
        .route("/.well-known/stumble-node", get(well_known_node))
        .route("/openapi-lite", get(openapi_lite))
        .route("/route-link", post(retired_submission_contract))
        .route("/intake-link", post(retired_submission_contract))
        .route("/pods", get(list_pods).post(create_pod))
        .route("/pod-packages", post(create_private_pod_with_package))
        .route("/pods/:slug/join", post(join_pod))
        .route("/pods/:slug/submit", post(retired_submission_contract))
        .route("/pods/:slug/intake-link", post(retired_submission_contract))
        .route(
            "/pods/:slug/submissions/:id",
            delete(retired_submission_contract),
        )
        .route(
            "/pods/:slug/package",
            get(get_skill_pack).patch(patch_skill_pack_handler),
        )
        .route(
            "/pods/:slug/package/export",
            post(export_skill_pack_handler),
        )
        .route(
            "/pods/:slug/package/import",
            post(import_skill_pack_handler),
        )
        .route("/pods/:slug/package/fork", post(fork_skill_pack_handler))
        .route(
            "/pods/:slug/package/validate",
            post(validate_skill_pack_handler),
        )
        .route(
            "/pods/:slug/skill-pack",
            get(retired_package_contract).patch(retired_package_contract),
        )
        .route(
            "/pods/:slug/skill-pack/export",
            post(retired_package_contract),
        )
        .route(
            "/pods/:slug/skill-pack/import",
            post(retired_package_contract),
        )
        .route(
            "/pods/:slug/skill-pack/fork",
            post(retired_package_contract),
        )
        .route(
            "/pods/:slug/skill-pack/validate",
            post(retired_package_contract),
        )
        .route("/pods/:slug/sources", post(retired_crawler_contract))
        .route("/pods/:slug/crawl", post(retired_crawler_contract))
        .route("/pods/:slug/discover", post(retired_presentation_contract))
        .route("/pods/:slug/stumble", post(retired_presentation_contract))
        .route("/briefs", get(retired_presentation_contract))
        .route("/briefs/generate", post(retired_presentation_contract))
        .route("/feed", get(get_feed_batch))
        .route("/feed/:id/complete", post(complete_feed_batch))
        .route("/feed/items/:id/feedback", post(record_feed_feedback))
        .route(
            "/subscriptions/:pod_id/priority",
            post(set_priority_subscription),
        )
        .route(
            "/taste-profile",
            get(get_taste_profile).patch(update_taste_profile),
        )
        .route("/taste-profile/learned/reset", post(reset_learned_taste))
        .route(
            "/taste-profile/interest-seeds/:candidate_id/retract",
            post(retract_interest_seed),
        )
        .route("/links/:id/assets", get(retired_submission_contract))
        .route("/links/:id/save", post(retired_feedback_contract))
        .route("/links/:id/rate", post(retired_feedback_contract))
        .route("/me/preferences", patch(update_preferences))
        .route(
            "/discovery/announcements",
            post(index_pod_announcement).get(search_pod_announcements),
        )
        .route(
            "/discovery/announcements/produce",
            post(produce_pod_announcement),
        )
        .route(
            "/discovery/announcements/receive",
            post(receive_pod_announcement),
        )
        .route("/discovery/withdrawals", post(index_pod_withdrawal))
        .route(
            "/discovery/withdrawals/produce",
            post(produce_pod_withdrawal),
        )
        .route(
            "/discovery/withdrawals/receive",
            post(receive_pod_withdrawal),
        )
        .route(
            "/bootstrap/announcements",
            post(bootstrap_admit_announcement),
        )
        .route(
            "/bootstrap/announcements/stream",
            get(bootstrap_announcement_stream),
        )
        .route("/bootstrap/withdrawals", post(bootstrap_admit_withdrawal))
        .route(
            "/bootstrap/peer-advertisements",
            post(bootstrap_admit_peer_advertisement).get(bootstrap_peer_advertisement_sample),
        )
        .route(
            "/discovery/peer/announcements/stream",
            get(peer_announcement_stream),
        )
        .route(
            "/discovery/peer/advertisements",
            get(peer_advertisement_sample),
        )
        .route(
            "/home/discovery-peer",
            get(discovery_peer_status)
                .post(enable_discovery_peer)
                .delete(disable_discovery_peer),
        )
        .route(
            "/home/discovery-peers",
            get(outbound_discovery_peers)
                .patch(set_peer_gossip)
                .post(sync_outbound_discovery_peers),
        )
        .route("/home/discovery-status", get(home_discovery_status))
        .route(
            "/home/bootstrap/endpoints",
            get(list_bootstrap_endpoints).post(add_bootstrap_endpoint),
        )
        .route(
            "/home/bootstrap/endpoints/:id",
            patch(set_bootstrap_endpoint_enabled).delete(remove_bootstrap_endpoint),
        )
        .route("/home/bootstrap/status", get(bootstrap_status))
        .route("/home/bootstrap/sync", post(sync_bootstrap_endpoints))
        .route("/home/discover-public-pods", get(home_discover_public_pods))
        .route("/auth/dev-token", post(dev_token))
        .route("/me", get(me))
        .route("/tenants", get(list_tenants).post(create_tenant))
        .route("/api-tokens", post(create_api_token))
        .route("/api-tokens/:id", delete(retired_api_token_contract))
        .route("/harnesses", post(register_agent_harness))
        .route("/harnesses/:id", delete(revoke_agent_harness))
        .route("/pending-proposals", post(create_pending_proposal))
        .route("/pending-proposals/:id", get(get_pending_proposal))
        .route(
            "/pending-proposals/:id/approve",
            post(approve_pending_proposal),
        )
        .route(
            "/pending-proposals/:id/reject",
            post(reject_pending_proposal),
        )
        .route("/candidates", post(submit_candidate))
        .route("/candidates/:id", get(inspect_candidate))
        .route(
            "/discovery-tasks",
            get(list_discovery_tasks).post(materialize_discovery_tasks),
        )
        .route(
            "/discovery-tasks/immediate",
            post(create_immediate_discovery_task),
        )
        .route(
            "/personal-discovery/readiness",
            get(personal_discovery_readiness),
        )
        .route("/personal-discovery", post(request_personal_discovery))
        .route(
            "/personal-discovery/schedules",
            get(list_personal_discovery_schedules).post(create_personal_discovery_schedule),
        )
        .route(
            "/personal-discovery/schedules/:id",
            get(personal_discovery_schedule)
                .patch(update_personal_discovery_schedule)
                .delete(remove_personal_discovery_schedule),
        )
        .route(
            "/personal-discovery/schedules/:id/disable",
            post(disable_personal_discovery_schedule),
        )
        .route(
            "/discovery-result-batches/:id/notify",
            post(attempt_discovery_results_ready_notification),
        )
        .route(
            "/discovery-result-batches",
            get(list_discovery_result_batches).post(complete_discovery_result_batch),
        )
        .route(
            "/discovery-tasks/:id/source-availability",
            get(discovery_task_source_availability).post(report_discovery_source_availability),
        )
        .route(
            "/personal-discovery/authentication-needed-notices",
            get(list_authentication_needed_notices),
        )
        .route("/discovery-result-batches/:id", get(discovery_result_batch))
        .route(
            "/discovery-result-batches/:id/dismiss",
            post(dismiss_discovery_result_batch),
        )
        .route(
            "/discovery-result-batches/:id/reviewed",
            post(mark_discovery_result_batch_reviewed),
        )
        .route(
            "/discovery-result-batches/:id/items/:candidate_id/review",
            post(review_discovery_result_item),
        )
        .route("/discovery-plans/:id", get(discovery_plan))
        .route("/discovery-tasks/ready", get(list_ready_discovery_tasks))
        .route("/discovery-tasks/:id", get(discovery_task_status))
        .route("/discovery-tasks/:id/claim", post(claim_discovery_task))
        .route("/discovery-tasks/:id/renew", post(renew_discovery_task))
        .route(
            "/discovery-tasks/:id/complete",
            post(complete_discovery_task),
        )
        .route("/discovery-tasks/:id/fail", post(fail_discovery_task))
        .route("/federation/node", get(federation_node))
        .route("/federation/pods", get(federation_pods))
        .route("/federation/pods/:slug/manifest", get(federation_manifest))
        .route(
            "/federation/pods/:slug/events",
            get(federation_events).post(federation_import_events),
        )
        .route(
            "/federation/sync/:peer_id/:pod_slug",
            post(federation_sync_pod),
        )
        .route(
            "/federation/sync/:peer_id",
            post(retired_peer_sync_contract),
        )
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub fn dev_tokens_allowed_for_bind(bind: SocketAddr, explicit_allow: bool) -> bool {
    explicit_allow || bind.ip().is_loopback()
}

pub fn bind_with_port(bind: SocketAddr, port: Option<u16>) -> SocketAddr {
    port.map(|port| SocketAddr::new(bind.ip(), port))
        .unwrap_or(bind)
}

fn route_docs() -> Vec<ApiRouteDoc> {
    vec![
        ApiRouteDoc {
            method: "GET",
            path: "/health",
            description: "health check",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/.well-known/stumble-node",
            description: "custom Stumble node metadata and endpoint discovery",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/pods",
            description: "list local or hosted pods visible to the auth context",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/feed",
            description: "retrieve the current stable finite Feed Batch",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/feed/:id/complete",
            description: "complete a Feed Batch before deliberately requesting another",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/feed/items/:id/feedback",
            description: "record a private explicit Feedback Signal for a Feed item",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/taste-profile",
            description: "inspect the authenticated User's private Taste Profile",
        },
        ApiRouteDoc {
            method: "PATCH",
            path: "/taste-profile",
            description: "edit explicit private Taste Profile preferences",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/taste-profile/learned/reset",
            description: "reset one or all private learned Taste Profile weights",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/taste-profile/interest-seeds/:candidate_id/retract",
            description: "retract one private Interest Seed without deleting its reference",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/pods",
            description: "create a pod and default skill pack",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/candidates",
            description: "submit an authenticated provenance-bearing Candidate",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/pods/:slug/package",
            description: "read the current versioned Pod Package",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/pods/:slug/package/validate",
            description: "validate the current Pod Package",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/pods/:slug/package/export",
            description: "export a portable Pod Package",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/pods/:slug/package/import",
            description: "import a validated portable Pod Package",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/pods/:slug/package/fork",
            description: "fork a Pod Package into a new Pod",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/discovery-tasks",
            description: "list visible Discovery Tasks",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery-tasks",
            description: "materialize due Discovery Tasks",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery-tasks/immediate",
            description: "create conversational discovery through the task contract",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/discovery-tasks/ready",
            description: "list claimable Discovery Tasks",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/personal-discovery/readiness",
            description: "inspect whether private User evidence can support Personal Discovery",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/personal-discovery",
            description: "request an immutable private Discovery Plan and User-scoped task",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/discovery-plans/:id",
            description: "read an authorized minimized Discovery Plan",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery-tasks/:id/claim",
            description: "claim a Discovery Task lease",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery-tasks/:id/renew",
            description: "renew a Discovery Task lease",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery-tasks/:id/complete",
            description: "complete a Discovery Task",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery-tasks/:id/fail",
            description: "record a failed Discovery Task attempt",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/candidates/:id",
            description: "inspect a private Candidate and its independent evidence",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/auth/dev-token",
            description: "hosted-mode simple token issue endpoint",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/home/discover-public-pods",
            description: "intentional Explore of verified public Pod announcements under local Trust Policy",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/home/bootstrap/endpoints",
            description: "list User-controlled Bootstrap endpoints in order",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/home/bootstrap/endpoints",
            description: "add a replaceable Bootstrap endpoint",
        },
        ApiRouteDoc {
            method: "PATCH",
            path: "/home/bootstrap/endpoints/:id",
            description: "enable or disable a Bootstrap endpoint",
        },
        ApiRouteDoc {
            method: "DELETE",
            path: "/home/bootstrap/endpoints/:id",
            description: "remove a Bootstrap endpoint from configuration",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/home/bootstrap/status",
            description: "report Bootstrap endpoints with cursor, last success, and typed failure",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/home/bootstrap/sync",
            description: "fetch Announcement Streams from enabled Bootstrap endpoints outbound",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/home/discovery-peer",
            description: "inspect Discovery Peer opt-in serving state",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/home/discovery-peer",
            description: "explicitly enable Discovery Peer announcement serving after reachability verification",
        },
        ApiRouteDoc {
            method: "DELETE",
            path: "/home/discovery-peer",
            description: "disable Discovery Peer serving without affecting outbound discovery",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/bootstrap/peer-advertisements",
            description: "open Bootstrap admission of a signed Discovery Peer Advertisement",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/bootstrap/peer-advertisements",
            description: "small randomized unranked sample of Bootstrap-admitted peer advertisements",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/discovery/peer/announcements/stream",
            description: "opt-in Discovery Peer Announcement Stream pages (Origin signatures unchanged)",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/discovery/peer/advertisements",
            description: "small randomized unranked sample of current peer advertisements",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/home/discovery-peers",
            description: "list rotating outbound Discovery Peer set with cursor, health, and last-success",
        },
        ApiRouteDoc {
            method: "PATCH",
            path: "/home/discovery-peers",
            description: "enable or disable automatic peer gossip without deleting audit state",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/home/discovery-peers",
            description: "learn peers from samples and/or sync Announcement Streams from outbound Discovery Peers",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/home/discovery-status",
            description: "report discovery readiness including Bootstrap-outage degraded mode",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery/announcements",
            description: "verify and index a signed public Pod Announcement with its Announcement Lease",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/discovery/announcements",
            description: "Index search of eligible Pod Announcements by explicit query only (no User id)",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery/announcements/produce",
            description: "produce an Origin-signed Pod Announcement with a renewable 30-day lease",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery/announcements/receive",
            description: "receive a peer-delivered Origin-signed Pod Announcement",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery/withdrawals",
            description: "verify and index an Origin-signed Pod Withdrawal",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery/withdrawals/produce",
            description: "produce an Origin-signed Pod Withdrawal, optionally making the Pod private",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/discovery/withdrawals/receive",
            description: "receive a peer-delivered Origin-signed Pod Withdrawal",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/federation/node",
            description: "node public identity and protocol version",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/federation/pods/:slug/events",
            description: "export signed public events",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/federation/pods/:slug/events",
            description: "import signed public events from a trusted peer",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/federation/sync/:peer_id/:pod_slug",
            description: "synchronize signed events from a trusted peer",
        },
    ]
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({"status":"ok","service":"stumble","version": env!("CARGO_PKG_VERSION")}))
}

async fn openapi_lite() -> Json<Vec<ApiRouteDoc>> {
    Json(route_docs())
}

async fn well_known_node(
    State(state): State<ApiState>,
    _headers: HeaderMap,
) -> Result<Json<WellKnownNode>, ApiError> {
    let ctx = state.tools.default_auth_context()?;
    Ok(Json(state.tools.well_known_node(&ctx, &state.base_url)?))
}

async fn list_pods(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Pod>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.list_pods_for_harness(&ctx)?))
}

async fn create_pod(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreatePodRequest>,
) -> Result<Json<CreatePodOutcome>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.request_create_pod(
        &ctx,
        request,
        chrono::Utc::now(),
    )?))
}

async fn create_private_pod_with_package(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreatePrivatePodWithPackageRequest>,
) -> Result<Json<CreatedPodPackage>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(
        state.tools.create_private_pod_with_package(&ctx, request)?,
    ))
}

async fn join_pod(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<StatusCode, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    state.tools.join_pod(&ctx, &slug)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_skill_pack(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<PodSkillPack>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.get_skill_pack(&ctx, &slug)?))
}

async fn patch_skill_pack_handler(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(patch): Json<SkillPackPatch>,
) -> Result<Json<PodSkillPack>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.patch_skill_pack(&ctx, &slug, patch)?))
}

async fn export_skill_pack_handler(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<ExportedSkillPack>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.export_skill_pack(&ctx, &slug)?))
}

async fn import_skill_pack_handler(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(files): Json<std::collections::BTreeMap<String, String>>,
) -> Result<Json<PodSkillPack>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.import_skill_pack(&ctx, &slug, files)?))
}

async fn fork_skill_pack_handler(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(request): Json<CreatePodRequest>,
) -> Result<Json<PodSkillPack>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.fork_skill_pack(&ctx, &slug, request)?))
}

async fn validate_skill_pack_handler(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<ValidationReport>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.validate_pod_skill_pack(&ctx, &slug)?))
}

#[derive(Debug, Deserialize)]
struct DiscoveryTaskLeaseRequest {
    lease_seconds: DiscoveryLeaseSeconds,
}

#[derive(Debug, Deserialize)]
struct FailDiscoveryTaskRequest {
    reason: String,
}

async fn materialize_discovery_tasks(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DiscoveryTask>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.materialize_due_discovery_tasks(
        &ctx,
        chrono::Utc::now(),
    )?))
}

async fn list_discovery_tasks(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DiscoveryTask>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(
        state.tools.list_discovery_tasks(&ctx, chrono::Utc::now())?,
    ))
}

async fn list_ready_discovery_tasks(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DiscoveryTask>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(
        state
            .tools
            .list_ready_discovery_tasks(&ctx, chrono::Utc::now())?,
    ))
}

async fn create_immediate_discovery_task(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateImmediateDiscoveryTaskRequest>,
) -> Result<Json<DiscoveryTask>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.create_immediate_discovery_task(
        &ctx,
        request,
        chrono::Utc::now(),
    )?))
}

async fn personal_discovery_readiness(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<PersonalDiscoveryReadiness>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.personal_discovery_readiness(&ctx)?))
}

async fn request_personal_discovery(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<RequestPersonalDiscovery>,
) -> Result<Json<RequestedPersonalDiscovery>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.request_personal_discovery(
        &ctx,
        request,
        chrono::Utc::now(),
    )?))
}

async fn list_personal_discovery_schedules(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<PersonalDiscoveryScheduleStatus>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.list_personal_discovery_schedules(
        &ctx,
        chrono::Utc::now(),
    )?))
}

async fn create_personal_discovery_schedule(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreatePersonalDiscoveryScheduleRequest>,
) -> Result<Json<PersonalDiscoveryScheduleStatus>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.create_personal_discovery_schedule(
        &ctx,
        request,
        chrono::Utc::now(),
    )?))
}

async fn personal_discovery_schedule(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<PersonalDiscoveryScheduleId>,
) -> Result<Json<PersonalDiscoveryScheduleStatus>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.personal_discovery_schedule(
        &ctx,
        id,
        chrono::Utc::now(),
    )?))
}

async fn update_personal_discovery_schedule(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<PersonalDiscoveryScheduleId>,
    Json(request): Json<UpdatePersonalDiscoveryScheduleRequest>,
) -> Result<Json<PersonalDiscoveryScheduleStatus>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.update_personal_discovery_schedule(
        &ctx,
        id,
        request,
        chrono::Utc::now(),
    )?))
}

async fn disable_personal_discovery_schedule(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<PersonalDiscoveryScheduleId>,
) -> Result<Json<PersonalDiscoveryScheduleStatus>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.disable_personal_discovery_schedule(
        &ctx,
        id,
        chrono::Utc::now(),
    )?))
}

async fn remove_personal_discovery_schedule(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<PersonalDiscoveryScheduleId>,
) -> Result<Json<PersonalDiscoverySchedule>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(
        state.tools.remove_personal_discovery_schedule(&ctx, id)?,
    ))
}

async fn attempt_discovery_results_ready_notification(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<DiscoveryResultBatchId>,
) -> Result<Json<DiscoveryResultsReadyNotificationOutcome>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(
        state
            .tools
            .attempt_discovery_results_ready_notification(&ctx, id, chrono::Utc::now())?,
    ))
}

async fn list_discovery_result_batches(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DiscoveryResultBatch>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.list_discovery_result_batches(&ctx)?))
}

async fn complete_discovery_result_batch(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CompleteDiscoveryResultBatchRequest>,
) -> Result<Json<DiscoveryResultBatch>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.complete_discovery_result_batch(
        &ctx,
        request,
        chrono::Utc::now(),
    )?))
}

async fn report_discovery_source_availability(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(task_id): Path<DiscoveryTaskId>,
    Json(mut request): Json<ReportDiscoverySourceAvailabilityRequest>,
) -> Result<Json<ReportedDiscoverySourceAvailability>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    request.task_id = task_id;
    Ok(Json(state.tools.report_discovery_source_availability(
        &ctx,
        request,
        chrono::Utc::now(),
    )?))
}

async fn discovery_task_source_availability(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(task_id): Path<DiscoveryTaskId>,
) -> Result<Json<DiscoveryTaskSourceAvailability>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(
        state
            .tools
            .discovery_task_source_availability(&ctx, task_id)?,
    ))
}

async fn list_authentication_needed_notices(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AuthenticationNeededNotice>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.list_authentication_needed_notices(&ctx)?))
}

async fn discovery_result_batch(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<DiscoveryResultBatchId>,
) -> Result<Json<DiscoveryResultBatch>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.discovery_result_batch(&ctx, id)?))
}

async fn dismiss_discovery_result_batch(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<DiscoveryResultBatchId>,
) -> Result<Json<DiscoveryResultBatch>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.dismiss_discovery_result_batch(
        &ctx,
        id,
        chrono::Utc::now(),
    )?))
}

async fn mark_discovery_result_batch_reviewed(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<DiscoveryResultBatchId>,
) -> Result<Json<DiscoveryResultBatch>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.mark_discovery_result_batch_reviewed(
        &ctx,
        id,
        chrono::Utc::now(),
    )?))
}

async fn review_discovery_result_item(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((batch_id, candidate_id)): Path<(DiscoveryResultBatchId, CandidateId)>,
    Json(action): Json<DiscoveryResultItemActionRequest>,
) -> Result<Json<DiscoveryResultItemReviewOutcome>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.review_discovery_result_item(
        &ctx,
        ReviewDiscoveryResultItemRequest {
            batch_id,
            candidate_id,
            action,
        },
        chrono::Utc::now(),
    )?))
}

async fn discovery_plan(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<DiscoveryPlanId>,
) -> Result<Json<DiscoveryPlan>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.discovery_plan(&ctx, id)?))
}

async fn discovery_task_status(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<DiscoveryTaskId>,
) -> Result<Json<DiscoveryTask>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.discovery_task_status(
        &ctx,
        id,
        chrono::Utc::now(),
    )?))
}

async fn claim_discovery_task(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<DiscoveryTaskId>,
    Json(request): Json<DiscoveryTaskLeaseRequest>,
) -> Result<Json<DiscoveryTask>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.claim_discovery_task(
        &ctx,
        id,
        chrono::Utc::now(),
        request.lease_seconds,
    )?))
}

async fn renew_discovery_task(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<DiscoveryTaskId>,
    Json(request): Json<DiscoveryTaskLeaseRequest>,
) -> Result<Json<DiscoveryTask>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.renew_discovery_task_lease(
        &ctx,
        id,
        chrono::Utc::now(),
        request.lease_seconds,
    )?))
}

async fn complete_discovery_task(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<DiscoveryTaskId>,
) -> Result<Json<DiscoveryTask>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.complete_discovery_task(
        &ctx,
        id,
        chrono::Utc::now(),
    )?))
}

async fn fail_discovery_task(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<DiscoveryTaskId>,
    Json(request): Json<FailDiscoveryTaskRequest>,
) -> Result<Json<DiscoveryTask>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.fail_discovery_task(
        &ctx,
        id,
        chrono::Utc::now(),
        request.reason,
    )?))
}

async fn submit_candidate(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CandidateSubmissionRequest>,
) -> Result<Json<SubmittedCandidate>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.submit_candidate(&ctx, request)?))
}

async fn inspect_candidate(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<CandidateId>,
) -> Result<Json<CandidateInspection>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.inspect_candidate(&ctx, id)?))
}

async fn retired_submission_contract() -> impl IntoResponse {
    (
        StatusCode::GONE,
        Json(LegacyContract::LegacySubmission.error()),
    )
}

async fn retired_crawler_contract() -> impl IntoResponse {
    (
        StatusCode::GONE,
        Json(LegacyContract::CrawlerSourceConnector.error()),
    )
}

async fn retired_presentation_contract() -> impl IntoResponse {
    (
        StatusCode::GONE,
        Json(LegacyContract::LegacyFeedPresentation.error()),
    )
}

async fn retired_package_contract() -> impl IntoResponse {
    (
        StatusCode::GONE,
        Json(LegacyContract::LegacySkillPack.error()),
    )
}

async fn retired_feedback_contract() -> impl IntoResponse {
    (
        StatusCode::GONE,
        Json(LegacyContract::LegacyFeedback.error()),
    )
}

async fn retired_peer_sync_contract() -> impl IntoResponse {
    (
        StatusCode::GONE,
        Json(LegacyContract::LegacyPeerSync.error()),
    )
}

#[derive(Debug, Deserialize)]
struct FeedQuery {
    size: Option<usize>,
    recurrence_penalty_days: Option<RecurrencePenaltyDays>,
    #[serde(flatten)]
    feed_mix: FeedMixOverrides,
    focus: Option<String>,
    avoid: Option<String>,
}

async fn get_feed_batch(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<FeedQuery>,
) -> Result<Json<FeedBatch>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    let mut request = FeedBatchRequest::new(query.size.unwrap_or(7))
        .map_err(|error| AgentToolsError::Store(StoreError::Validation(error.to_string())))?;
    if let Some(days) = query.recurrence_penalty_days {
        request.recurrence_penalty_days = Some(days);
    }
    request.feed_mix = query
        .feed_mix
        .resolve(FeedMix::default())
        .map_err(|error| AgentToolsError::Store(StoreError::Validation(error.to_string())))?;
    request.batch_intent = BatchIntent::new(
        query
            .focus
            .map(|topics| split_query_topics(&topics))
            .unwrap_or_default(),
        query
            .avoid
            .map(|topics| split_query_topics(&topics))
            .unwrap_or_default(),
    );
    Ok(Json(state.tools.get_feed_batch(
        &ctx,
        request,
        chrono::Utc::now(),
    )?))
}

fn split_query_topics(topics: &str) -> Vec<String> {
    topics
        .split(',')
        .map(str::trim)
        .filter(|topic| !topic.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, Deserialize)]
struct PrioritySubscriptionUpdate {
    is_priority: bool,
}

async fn set_priority_subscription(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(pod_id): Path<PodId>,
    Json(update): Json<PrioritySubscriptionUpdate>,
) -> Result<StatusCode, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    state
        .tools
        .set_priority_subscription(&ctx, pod_id, update.is_priority)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn complete_feed_batch(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<FeedBatch>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.complete_feed_batch(
        &ctx,
        id,
        chrono::Utc::now(),
    )?))
}

#[derive(Debug, Deserialize)]
struct FeedFeedbackBody {
    kind: FeedbackKind,
    topic: Option<String>,
    reason: Option<String>,
}

async fn record_feed_feedback(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<ContentItemId>,
    Json(body): Json<FeedFeedbackBody>,
) -> Result<Json<FeedFeedbackState>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.record_feed_feedback(
        &ctx,
        id,
        body.kind,
        body.topic,
        body.reason,
        chrono::Utc::now(),
    )?))
}

async fn get_taste_profile(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<TasteProfile>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.taste_profile(&ctx)?))
}

async fn update_taste_profile(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<UpdateTasteProfileRequest>,
) -> Result<Json<TasteProfile>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.update_taste_profile(&ctx, request)?))
}

async fn reset_learned_taste(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<ResetLearnedTasteRequest>,
) -> Result<Json<TasteProfile>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.reset_learned_taste(&ctx, request)?))
}

async fn retract_interest_seed(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(candidate_id): Path<CandidateId>,
) -> Result<Json<TasteProfile>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.retract_interest_seed(&ctx, candidate_id)?))
}

async fn update_preferences(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<UpdatePreferencesRequest>,
) -> Result<Json<UserPreferences>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.update_preferences(&ctx, request)?))
}

#[derive(Debug, Deserialize)]
struct PublicPodDiscoveryQuery {
    /// Explicit Explore query; `topics` is accepted as a comma-joined alias.
    q: Option<String>,
    topics: Option<String>,
    limit: Option<usize>,
    sample_size: Option<usize>,
}

async fn home_discover_public_pods(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<PublicPodDiscoveryQuery>,
) -> Result<Json<ExploreResponse>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    let query_text = query.q.unwrap_or_else(|| {
        query
            .topics
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|topic| !topic.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    });
    let request = ExploreRequest::new(
        query_text,
        query.limit.unwrap_or(10),
        query.sample_size.unwrap_or(0),
    )
    .map_err(AgentToolsError::from)?;
    Ok(Json(state.tools.explore_public_pods(&ctx, request)?))
}

async fn dev_token(
    State(state): State<ApiState>,
    Json(request): Json<DevTokenRequest>,
) -> Result<Json<DevTokenResponse>, ApiError> {
    ensure_dev_tokens_allowed(&state)?;
    Ok(Json(state.tools.create_dev_token(request)?))
}

async fn me(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<AuthContext>, ApiError> {
    Ok(Json(auth_or_default(&state, &headers)?))
}

async fn list_tenants(State(state): State<ApiState>) -> Result<Json<Vec<Tenant>>, ApiError> {
    let store = state.tools.store();
    let store = store.read().map_err(|_| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "internal_error",
        message: "lock poisoned".to_string(),
    })?;
    Ok(Json(store.tenants.values().cloned().collect()))
}

async fn create_tenant(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateTenantRequest>,
) -> Result<Json<Tenant>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.create_tenant_as(&ctx, request)?))
}

async fn create_api_token(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<DevTokenRequest>,
) -> Result<Json<DevTokenResponse>, ApiError> {
    ensure_dev_tokens_allowed(&state)?;
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.create_dev_token_as(&ctx, request)?))
}

async fn retired_api_token_contract() -> impl IntoResponse {
    (
        StatusCode::GONE,
        Json(LegacyContract::LegacyApiToken.error()),
    )
}

async fn register_agent_harness(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<RegisterAgentHarnessRequest>,
) -> Result<Json<RegisterAgentHarnessResponse>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.register_agent_harness(&ctx, request)?))
}

async fn revoke_agent_harness(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<AgentHarnessId>,
) -> Result<StatusCode, ApiError> {
    let ctx = auth_required(&state.tools, &headers)?;
    state.tools.revoke_agent_harness(&ctx, id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_pending_proposal(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreatePendingProposalRequest>,
) -> Result<Json<PendingProposal>, ApiError> {
    let ctx = auth_required(&state.tools, &headers)?;
    Ok(Json(state.tools.create_pending_proposal_from_request(
        &ctx,
        request,
        chrono::Utc::now(),
    )?))
}

async fn get_pending_proposal(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<PendingProposalId>,
) -> Result<Json<PendingProposal>, ApiError> {
    let ctx = auth_required(&state.tools, &headers)?;
    Ok(Json(state.tools.pending_proposal(
        &ctx,
        id,
        chrono::Utc::now(),
    )?))
}

async fn approve_pending_proposal(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<PendingProposalId>,
) -> Result<Json<PendingProposal>, ApiError> {
    let ctx = auth_required(&state.tools, &headers)?;
    Ok(Json(state.tools.approve_pending_proposal(
        &ctx,
        id,
        chrono::Utc::now(),
    )?))
}

async fn reject_pending_proposal(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<PendingProposalId>,
    Json(request): Json<RejectPendingProposalRequest>,
) -> Result<Json<PendingProposal>, ApiError> {
    let ctx = auth_required(&state.tools, &headers)?;
    Ok(Json(state.tools.reject_pending_proposal(
        &ctx,
        id,
        chrono::Utc::now(),
        request.reason,
    )?))
}

async fn federation_node(
    State(state): State<ApiState>,
    _headers: HeaderMap,
) -> Result<Json<NodeInfo>, ApiError> {
    let ctx = state.tools.default_auth_context()?;
    Ok(Json(state.tools.node_info(&ctx)?))
}

async fn federation_pods(
    State(state): State<ApiState>,
    _headers: HeaderMap,
) -> Result<Json<Vec<Pod>>, ApiError> {
    let ctx = state.tools.default_auth_context()?;
    Ok(Json(state.tools.list_public_pods(&ctx)?))
}

async fn federation_manifest(
    State(state): State<ApiState>,
    _headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<PodManifest>, ApiError> {
    let ctx = state.tools.default_auth_context()?;
    Ok(Json(state.tools.federation_pod_manifest(&ctx, &slug)?))
}

async fn federation_events(
    State(state): State<ApiState>,
    _headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<Vec<EventLog>>, ApiError> {
    let ctx = state.tools.default_auth_context()?;
    Ok(Json(state.tools.federation_pod_events(&ctx, &slug)?))
}

#[derive(Debug, Deserialize)]
struct ImportEventsRequest {
    peer_id: Uuid,
    events: Vec<EventLog>,
}

async fn federation_import_events(
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

async fn federation_sync_pod(
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
struct ProduceAnnouncementRequest {
    pod_slug: String,
    public_pod_url: String,
}

#[derive(Debug, Deserialize)]
struct ReceiveAnnouncementRequest {
    peer_id: Uuid,
    announcement: PodAnnouncement,
}

#[derive(Debug, Deserialize)]
struct ProduceWithdrawalRequest {
    pod_slug: String,
    public_pod_url: Option<String>,
    #[serde(default = "default_true")]
    make_private: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct ReceiveWithdrawalRequest {
    peer_id: Uuid,
    withdrawal: PodWithdrawal,
}

#[derive(Debug, Deserialize)]
struct AnnouncementSearchQuery {
    q: Option<String>,
    limit: Option<usize>,
}

async fn produce_pod_announcement(
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
async fn bootstrap_admit_announcement(
    State(state): State<ApiState>,
    Json(announcement): Json<PodAnnouncement>,
) -> Result<Json<BootstrapAdmissionAcceptance>, ApiError> {
    Ok(Json(
        state.tools.admit_bootstrap_announcement(announcement)?,
    ))
}

/// Open Bootstrap withdrawal admission: no User account or Trusted Peer required.
async fn bootstrap_admit_withdrawal(
    State(state): State<ApiState>,
    Json(withdrawal): Json<PodWithdrawal>,
) -> Result<Json<BootstrapWithdrawalAcceptance>, ApiError> {
    Ok(Json(state.tools.admit_bootstrap_withdrawal(withdrawal)?))
}

#[derive(Debug, Deserialize)]
struct AnnouncementStreamQuery {
    cursor: Option<String>,
    limit: Option<usize>,
}

/// Topic-neutral cursor-paginated Announcement Stream (no personalization).
async fn bootstrap_announcement_stream(
    State(state): State<ApiState>,
    Query(query): Query<AnnouncementStreamQuery>,
) -> Result<Json<AnnouncementStreamPage>, ApiError> {
    Ok(Json(state.tools.announcement_stream(
        query.cursor.as_deref(),
        query.limit,
    )?))
}

/// Open Bootstrap admission of a signed Discovery Peer Advertisement.
async fn bootstrap_admit_peer_advertisement(
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
async fn bootstrap_peer_advertisement_sample(
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
async fn peer_announcement_stream(
    State(state): State<ApiState>,
    Query(query): Query<AnnouncementStreamQuery>,
) -> Result<Json<AnnouncementStreamPage>, ApiError> {
    Ok(Json(state.tools.peer_announcement_stream(
        query.cursor.as_deref(),
        query.limit,
    )?))
}

#[derive(Debug, Deserialize)]
struct PeerAdvertisementSampleQuery {
    limit: Option<usize>,
}

/// Small randomized unranked sample of current peer advertisements.
///
/// Shuffle seed is server entropy only; clients cannot supply a seed.
async fn peer_advertisement_sample(
    State(state): State<ApiState>,
    Query(query): Query<PeerAdvertisementSampleQuery>,
) -> Result<Json<DiscoveryPeerAdvertisementSample>, ApiError> {
    Ok(Json(state.tools.peer_advertisement_sample(query.limit)?))
}

#[derive(Debug, Deserialize)]
struct EnableDiscoveryPeerRequest {
    public_endpoint: String,
}

/// Reports Discovery Peer opt-in state (admin).
async fn discovery_peer_status(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<DiscoveryPeerServiceState>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.discovery_peer_service_status(&ctx)?))
}

/// Explicitly enables inbound Discovery Peer announcement serving.
async fn enable_discovery_peer(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<EnableDiscoveryPeerRequest>,
) -> Result<Json<DiscoveryPeerAdvertisement>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.enable_discovery_peer_service(
        &ctx,
        &request.public_endpoint,
        chrono::Utc::now(),
    )?))
}

/// Disables inbound Discovery Peer serving without affecting outbound discovery.
async fn disable_discovery_peer(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<DiscoveryPeerServiceState>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.disable_discovery_peer_service(
        &ctx,
        chrono::Utc::now(),
    )?))
}

/// Lists the rotating outbound Discovery Peer set (not Trusted Peers).
async fn outbound_discovery_peers(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<OutboundDiscoveryPeerStatus>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.outbound_discovery_peers(&ctx)?))
}

#[derive(Debug, Deserialize)]
struct SetPeerGossipRequest {
    automatic_gossip_enabled: bool,
}

/// Enables or disables automatic peer gossip without deleting audit state.
async fn set_peer_gossip(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<SetPeerGossipRequest>,
) -> Result<Json<DiscoveryPeerGossipConfig>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.set_automatic_peer_gossip_enabled(
        &ctx,
        request.automatic_gossip_enabled,
        chrono::Utc::now(),
    )?))
}

#[derive(Debug, Deserialize)]
struct OutboundDiscoveryPeerAction {
    /// When true, learn peer samples and rotate the outbound set before sync.
    #[serde(default)]
    learn: bool,
    /// When true (default), sync Announcement Streams from outbound peers.
    #[serde(default = "default_true")]
    sync: bool,
}

/// Learns and/or syncs the rotating outbound Discovery Peer set.
async fn sync_outbound_discovery_peers(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<OutboundDiscoveryPeerAction>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    let handle = tokio::runtime::Handle::current();
    let sample_client = ReqwestPeerAdvertisementSampleClient::new(handle.clone());
    let stream_client = ReqwestDiscoveryPeerStreamClient::new(handle);
    Ok(Json(
        tokio::task::spawn_blocking(move || {
            let now = chrono::Utc::now();
            let mut body = serde_json::Map::new();
            if request.learn {
                let selected = state.tools.learn_and_select_discovery_peers(
                    &ctx,
                    &sample_client,
                    now,
                    // Production rotation uses server entropy; HTTP clients cannot force seeds.
                    {
                        use rand_core::{OsRng, RngCore};
                        OsRng.next_u64()
                    },
                )?;
                body.insert(
                    "selected".into(),
                    serde_json::to_value(selected).unwrap_or_default(),
                );
            }
            if request.sync {
                let report =
                    state
                        .tools
                        .sync_outbound_discovery_peers(&ctx, &stream_client, now)?;
                body.insert(
                    "sync".into(),
                    serde_json::to_value(report).unwrap_or_default(),
                );
            }
            Ok::<_, AgentToolsError>(serde_json::Value::Object(body))
        })
        .await
        .map_err(|error| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal",
            message: error.to_string(),
        })??,
    ))
}

/// Home Node discovery readiness including degraded Bootstrap-outage mode.
async fn home_discovery_status(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<DiscoveryStatus>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.discovery_status(&ctx)?))
}

#[derive(Debug, Deserialize)]
struct AddBootstrapEndpointRequest {
    label: String,
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct SetBootstrapEndpointEnabledRequest {
    enabled: bool,
}

/// Lists configured Bootstrap endpoints (Home Node owner/admin).
async fn list_bootstrap_endpoints(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<BootstrapEndpointConfig>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.list_bootstrap_endpoints(&ctx)?))
}

/// Adds a Bootstrap endpoint to the ordered User-controlled list.
async fn add_bootstrap_endpoint(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<AddBootstrapEndpointRequest>,
) -> Result<Json<BootstrapEndpointConfig>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.add_bootstrap_endpoint(
        &ctx,
        &request.label,
        &request.base_url,
        chrono::Utc::now(),
    )?))
}

/// Enables or disables one Bootstrap endpoint.
async fn set_bootstrap_endpoint_enabled(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<BootstrapEndpointId>,
    Json(request): Json<SetBootstrapEndpointEnabledRequest>,
) -> Result<Json<BootstrapEndpointConfig>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.set_bootstrap_endpoint_enabled(
        &ctx,
        id,
        request.enabled,
    )?))
}

/// Removes a Bootstrap endpoint from configuration.
async fn remove_bootstrap_endpoint(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<BootstrapEndpointId>,
) -> Result<Json<BootstrapEndpointConfig>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.remove_bootstrap_endpoint(&ctx, id)?))
}

/// Reports Bootstrap endpoints with cursor and typed failure state.
async fn bootstrap_status(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<BootstrapEndpointStatus>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.bootstrap_status(&ctx)?))
}

/// Outbound sync against enabled Bootstrap endpoints using HTTP transport.
async fn sync_bootstrap_endpoints(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<BootstrapSyncReport>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    let client = ReqwestAnnouncementStreamClient::new(tokio::runtime::Handle::current());
    Ok(Json(
        tokio::task::spawn_blocking(move || {
            state
                .tools
                .sync_bootstrap_endpoints(&ctx, &client, chrono::Utc::now())
        })
        .await
        .map_err(|error| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: error.to_string(),
        })??,
    ))
}

/// Production HTTP client for topic-neutral Announcement Stream pages.
///
/// Uses a Tokio handle so the Core inject seam can stay synchronous without
/// adding a blocking HTTP dependency to `stumble-core`. Shared by the API
/// `/home/bootstrap/sync` route and the CLI `stumble sync bootstrap run` path.
pub struct ReqwestAnnouncementStreamClient {
    client: reqwest::Client,
    handle: tokio::runtime::Handle,
}

impl ReqwestAnnouncementStreamClient {
    /// Builds a client that drives HTTP on `handle`.
    #[must_use]
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            client: reqwest::Client::new(),
            handle,
        }
    }
}

impl AnnouncementStreamClient for ReqwestAnnouncementStreamClient {
    fn fetch_announcement_stream(
        &self,
        base_url: &str,
        request: &BootstrapStreamRequest,
    ) -> Result<AnnouncementStreamPage, BootstrapSyncFailure> {
        let base = base_url.trim_end_matches('/');
        let url = format!("{base}/bootstrap/announcements/stream");
        let client = self.client.clone();
        let cursor = request.cursor.clone();
        let limit = request.limit;
        self.handle.block_on(async move {
            let mut http = client.get(&url);
            if let Some(cursor) = &cursor {
                http = http.query(&[("cursor", cursor.as_str())]);
            }
            if let Some(limit) = limit {
                http = http.query(&[("limit", limit.to_string())]);
            }
            let response = http.send().await.map_err(|error| {
                BootstrapSyncFailure::new(BootstrapSyncFailureKind::Transport, error.to_string())
            })?;
            if !response.status().is_success() {
                return Err(BootstrapSyncFailure::new(
                    BootstrapSyncFailureKind::Protocol,
                    format!("bootstrap stream HTTP {}", response.status()),
                ));
            }
            response.json().await.map_err(|error| {
                BootstrapSyncFailure::new(BootstrapSyncFailureKind::Protocol, error.to_string())
            })
        })
    }
}

/// Production HTTP client for Bootstrap/peer advertisement samples.
pub struct ReqwestPeerAdvertisementSampleClient {
    client: reqwest::Client,
    handle: tokio::runtime::Handle,
}

impl ReqwestPeerAdvertisementSampleClient {
    /// Builds a client that drives HTTP on `handle`.
    #[must_use]
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            client: reqwest::Client::new(),
            handle,
        }
    }
}

impl PeerAdvertisementSampleClient for ReqwestPeerAdvertisementSampleClient {
    fn fetch_peer_advertisement_sample(
        &self,
        base_url: &str,
        request: &DiscoveryPeerSampleRequest,
    ) -> Result<DiscoveryPeerAdvertisementSample, DiscoveryPeerSyncFailure> {
        debug_assert!(peer_sample_request_is_public_only(request));
        let base = base_url.trim_end_matches('/');
        // Prefer Bootstrap open sample path; peer sample path is used for peer endpoints.
        let bootstrap_url = format!("{base}/bootstrap/peer-advertisements");
        let peer_url = format!("{base}/discovery/peer/advertisements");
        let client = self.client.clone();
        let limit = request.limit;
        self.handle.block_on(async move {
            for url in [bootstrap_url, peer_url] {
                let mut http = client.get(&url);
                if let Some(limit) = limit {
                    http = http.query(&[("limit", limit.to_string())]);
                }
                let response = match http.send().await {
                    Ok(response) => response,
                    Err(error) => {
                        return Err(DiscoveryPeerSyncFailure::new(
                            DiscoveryPeerSyncFailureKind::Transport,
                            error.to_string(),
                        ));
                    }
                };
                if response.status().is_success() {
                    return response.json().await.map_err(|error| {
                        DiscoveryPeerSyncFailure::new(
                            DiscoveryPeerSyncFailureKind::Protocol,
                            error.to_string(),
                        )
                    });
                }
                if response.status().as_u16() == 404 || response.status().as_u16() == 403 {
                    continue;
                }
                return Err(DiscoveryPeerSyncFailure::new(
                    DiscoveryPeerSyncFailureKind::Protocol,
                    format!("peer sample HTTP {}", response.status()),
                ));
            }
            Err(DiscoveryPeerSyncFailure::new(
                DiscoveryPeerSyncFailureKind::Transport,
                format!("no peer advertisement sample available at {base}"),
            ))
        })
    }
}

/// Production HTTP client for Discovery Peer Announcement Stream pages.
pub struct ReqwestDiscoveryPeerStreamClient {
    client: reqwest::Client,
    handle: tokio::runtime::Handle,
}

impl ReqwestDiscoveryPeerStreamClient {
    /// Builds a client that drives HTTP on `handle`.
    #[must_use]
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            client: reqwest::Client::new(),
            handle,
        }
    }
}

impl DiscoveryPeerStreamClient for ReqwestDiscoveryPeerStreamClient {
    fn fetch_peer_announcement_stream(
        &self,
        base_url: &str,
        request: &BootstrapStreamRequest,
    ) -> Result<AnnouncementStreamPage, DiscoveryPeerSyncFailure> {
        debug_assert!(peer_stream_request_is_public_only(request));
        let base = base_url.trim_end_matches('/');
        let url = format!("{base}/discovery/peer/announcements/stream");
        let client = self.client.clone();
        let cursor = request.cursor.clone();
        let limit = request.limit;
        self.handle.block_on(async move {
            let mut http = client.get(&url);
            if let Some(cursor) = &cursor {
                http = http.query(&[("cursor", cursor.as_str())]);
            }
            if let Some(limit) = limit {
                http = http.query(&[("limit", limit.to_string())]);
            }
            let response = http.send().await.map_err(|error| {
                DiscoveryPeerSyncFailure::new(
                    DiscoveryPeerSyncFailureKind::Transport,
                    error.to_string(),
                )
            })?;
            if !response.status().is_success() {
                return Err(DiscoveryPeerSyncFailure::new(
                    DiscoveryPeerSyncFailureKind::Protocol,
                    format!("discovery peer stream HTTP {}", response.status()),
                ));
            }
            response.json().await.map_err(|error| {
                DiscoveryPeerSyncFailure::new(
                    DiscoveryPeerSyncFailureKind::Protocol,
                    error.to_string(),
                )
            })
        })
    }
}

/// Production HTTP client for replaceable Index Node search.
///
/// Uses a Tokio handle so the Core inject seam can stay synchronous without
/// adding a blocking HTTP dependency to `stumble-core`. Shared by intentional
/// Explore paths (CLI `stumble pod explore` when Indexes are configured).
pub struct ReqwestIndexSearchClient {
    client: reqwest::Client,
    handle: tokio::runtime::Handle,
}

impl ReqwestIndexSearchClient {
    /// Builds a client that drives HTTP on `handle`.
    #[must_use]
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            client: reqwest::Client::new(),
            handle,
        }
    }
}

impl IndexSearchClient for ReqwestIndexSearchClient {
    fn search_index(
        &self,
        base_url: &str,
        request: &IndexSearchRequest,
    ) -> Result<PodAnnouncementSearchResponse, IndexSearchFailure> {
        debug_assert!(index_request_is_public_only(request));
        let base = base_url.trim().trim_end_matches('/');
        let url = format!("{base}/discovery/announcements");
        let client = self.client.clone();
        let query = request.query.clone();
        let limit = request.limit;
        self.handle.block_on(async move {
            let mut http = client.get(&url).query(&[("q", query.as_str())]);
            if let Some(limit) = limit {
                http = http.query(&[("limit", limit.to_string())]);
            }
            let response = http.send().await.map_err(|error| {
                IndexSearchFailure::new(IndexSearchFailureKind::Transport, error.to_string())
            })?;
            let status = response.status();
            if !status.is_success() {
                // Prefer structured error bodies when present.
                if let Ok(body) = response.json::<serde_json::Value>().await {
                    if let Some(code) = body.get("code").and_then(|v| v.as_str()) {
                        let kind = match code {
                            "malformed" => IndexSearchFailureKind::Malformed,
                            "query_too_large" => IndexSearchFailureKind::QueryTooLarge,
                            "rate_limited" => IndexSearchFailureKind::RateLimited,
                            "incompatible_protocol" => IndexSearchFailureKind::IncompatibleProtocol,
                            "index_disabled" => IndexSearchFailureKind::IndexDisabled,
                            _ => IndexSearchFailureKind::Protocol,
                        };
                        let message = body
                            .get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or(code)
                            .to_string();
                        return Err(IndexSearchFailure::new(kind, message));
                    }
                }
                return Err(IndexSearchFailure::new(
                    IndexSearchFailureKind::Protocol,
                    format!("index search HTTP {status}"),
                ));
            }
            response.json().await.map_err(|error| {
                IndexSearchFailure::new(IndexSearchFailureKind::Protocol, error.to_string())
            })
        })
    }
}

async fn index_pod_announcement(
    State(state): State<ApiState>,
    Json(announcement): Json<PodAnnouncement>,
) -> Result<Json<KnownPodAnnouncement>, ApiError> {
    Ok(Json(state.tools.index_pod_announcement(announcement)?))
}

async fn receive_pod_announcement(
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
async fn search_pod_announcements(
    State(state): State<ApiState>,
    Query(query): Query<AnnouncementSearchQuery>,
) -> Result<Json<PodAnnouncementSearchResponse>, ApiError> {
    Ok(Json(state.tools.search_pod_announcements_at(
        &IndexSearchRequest::new(query.q.unwrap_or_default(), query.limit),
        chrono::Utc::now(),
    )?))
}

async fn produce_pod_withdrawal(
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

async fn index_pod_withdrawal(
    State(state): State<ApiState>,
    Json(withdrawal): Json<PodWithdrawal>,
) -> Result<Json<KnownPodWithdrawal>, ApiError> {
    Ok(Json(state.tools.index_pod_withdrawal(withdrawal)?))
}

async fn receive_pod_withdrawal(
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

fn auth_or_default(state: &ApiState, headers: &HeaderMap) -> Result<AuthContext, ApiError> {
    let tools = &state.tools;
    if let Some(value) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = value.strip_prefix("Bearer ") {
            if let Some(ctx) = tools.authenticate_token(token)? {
                return Ok(ctx);
            }
            return Err(ApiError {
                status: StatusCode::UNAUTHORIZED,
                code: "unauthorized",
                message: "invalid token".to_string(),
            });
        }
    }
    if !state.owner_access_allowed {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "bearer token required".to_string(),
        });
    }
    let store = tools.store();
    let store = store.read().map_err(|_| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "internal_error",
        message: "lock poisoned".to_string(),
    })?;
    let node = store.default_node().map_err(AgentToolsError::Store)?;
    Ok(AuthContext {
        user_id: store.users.keys().next().copied(),
        tenant_id: None,
        node_id: node.id,
        harness_id: None,
    })
}

fn auth_required(tools: &AgentTools, headers: &HeaderMap) -> Result<AuthContext, ApiError> {
    let Some(token) = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "bearer token required".to_string(),
        });
    };
    tools.authenticate_token(token)?.ok_or_else(|| ApiError {
        status: StatusCode::UNAUTHORIZED,
        code: "unauthorized",
        message: "invalid token".to_string(),
    })
}

fn ensure_dev_tokens_allowed(state: &ApiState) -> Result<(), ApiError> {
    if state.dev_tokens_allowed {
        return Ok(());
    }
    Err(ApiError {
        status: StatusCode::FORBIDDEN,
        code: "forbidden",
        message: "dev token minting is disabled for this bind address; use a loopback bind or explicitly allow public dev tokens".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[test]
    fn bind_with_port_overrides_only_the_port() {
        let bind: SocketAddr = "127.0.0.1:8787".parse().unwrap();
        let updated = bind_with_port(bind, Some(9000));
        assert_eq!(updated.to_string(), "127.0.0.1:9000");
    }

    #[test]
    fn dev_tokens_are_public_bind_opt_in() {
        let loopback: SocketAddr = "127.0.0.1:8787".parse().unwrap();
        let public: SocketAddr = "0.0.0.0:8787".parse().unwrap();
        assert!(dev_tokens_allowed_for_bind(loopback, false));
        assert!(!dev_tokens_allowed_for_bind(public, false));
        assert!(dev_tokens_allowed_for_bind(public, true));
    }

    #[tokio::test]
    async fn harness_capability_denial_is_http_forbidden() {
        let tools = AgentTools::new(seed_store());
        let owner = tools.default_auth_context().unwrap();
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "submitter".into(),
                    kind: AgentHarnessKind::Unattended,
                    capabilities: vec![HarnessCapability::CandidateSubmission],
                    pod_ids: None,
                },
            )
            .unwrap();
        let response = router(tools)
            .oneshot(
                Request::post(format!("/feed/items/{}/feedback", Uuid::nil()))
                    .header("authorization", format!("Bearer {}", issued.token.expose()))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"kind":"save"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn public_bind_rejects_unauthenticated_harness_registration() {
        let response = router_with_options(
            AgentTools::new(seed_store()),
            "https://pods.example.com",
            RouterOptions {
                dev_tokens_allowed: false,
                owner_access_allowed: false,
            },
        )
        .oneshot(
            Request::post("/harnesses")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&RegisterAgentHarnessRequest {
                        label: "attacker".into(),
                        kind: AgentHarnessKind::Unattended,
                        capabilities: vec![HarnessCapability::FeedRead],
                        pod_ids: None,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn federation_catalog_exposes_only_the_pod_scoped_sync_contract() {
        let routes = route_docs();
        assert!(routes.iter().any(|route| {
            route.method == "POST" && route.path == "/federation/sync/:peer_id/:pod_slug"
        }));
        assert!(!routes
            .iter()
            .any(|route| { route.method == "POST" && route.path == "/federation/sync/:peer_id" }));
    }

    #[test]
    fn public_route_docs_contain_no_legacy_hub_routes_or_terminology() {
        let routes = route_docs();
        assert!(!routes.iter().any(|route| route.path.starts_with("/hub")));
        assert!(!routes.iter().any(|route| route.path == "/discovery/pods"));
        for route in &routes {
            let blob = format!("{} {} {}", route.method, route.path, route.description);
            assert!(
                !blob.to_lowercase().contains("hub"),
                "public route docs must not use Hub terminology: {blob}"
            );
        }
        assert!(routes
            .iter()
            .any(|route| { route.method == "GET" && route.path == "/home/discover-public-pods" }));
        assert!(routes
            .iter()
            .any(|route| { route.method == "GET" && route.path == "/discovery/announcements" }));
    }

    #[tokio::test]
    async fn retired_hub_http_routes_are_absent_without_redirect() {
        let app = router(AgentTools::new(seed_store()));
        for (method, path) in [
            ("POST", "/hub/register-node"),
            ("POST", "/hub/register-pod"),
            ("POST", "/hub/refresh"),
            ("GET", "/hub/search-pods"),
            ("GET", "/discovery/pods"),
        ] {
            let request = Request::builder()
                .method(method)
                .uri(path)
                .body(Body::empty())
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "retired route must be absent: {method} {path}"
            );
            assert!(
                response
                    .headers()
                    .get(axum::http::header::LOCATION)
                    .is_none(),
                "retired route must not redirect: {method} {path}"
            );
        }
    }

    #[tokio::test]
    async fn unscoped_peer_sync_returns_the_versioned_retirement_contract() {
        let response = router(AgentTools::new(seed_store()))
            .oneshot(
                Request::post(format!("/federation/sync/{}", Uuid::nil()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::GONE);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let error: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(error["code"], "legacy_contract_retired");
        assert_eq!(error["protocol_version"], CURRENT_PROTOCOL_VERSION);
        assert_eq!(error["replacement"], "sync_pod");
    }
}
