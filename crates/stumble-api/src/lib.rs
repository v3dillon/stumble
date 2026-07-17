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
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error": self.message}))).into_response()
    }
}

impl From<AgentToolsError> for ApiError {
    fn from(value: AgentToolsError) -> Self {
        let status = if matches!(value, AgentToolsError::Forbidden { .. }) {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::BAD_REQUEST
        };
        Self {
            status,
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
                message: source.to_string(),
            },
            source @ stumble_sync::PeerSyncError::ImportTask(_) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: source.to_string(),
            },
            source => Self {
                status: StatusCode::BAD_REQUEST,
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
        .route("/links/:id/assets", get(retired_submission_contract))
        .route("/links/:id/save", post(retired_feedback_contract))
        .route("/links/:id/rate", post(retired_feedback_contract))
        .route("/me/preferences", patch(update_preferences))
        .route("/discovery/pods", get(pod_discovery_feed))
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
        .route("/hub/register-node", post(hub_register_node))
        .route("/hub/register-pod", post(hub_register_pod))
        .route("/hub/refresh", post(hub_refresh))
        .route("/hub/search-pods", get(hub_search_pods))
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
            description: "home-node public pod discovery from explicit topics or private interests",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/discovery/pods",
            description: "combined public pod discovery feed split into local public pods and global hub-indexed pods",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/hub/register-node",
            description: "register public node metadata in a custom discovery hub",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/hub/register-pod",
            description: "register a public pod manifest in a custom discovery hub",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/hub/refresh",
            description: "pull registered remote nodes' public federation surfaces into the global discovery index",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/hub/search-pods",
            description: "refresh this node's public pod index, then search public pod manifests indexed by a custom discovery hub",
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
    topics: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct PodDiscoveryFeedQuery {
    q: Option<String>,
    limit: Option<usize>,
}

async fn pod_discovery_feed(
    State(state): State<ApiState>,
    _headers: HeaderMap,
    Query(query): Query<PodDiscoveryFeedQuery>,
) -> Result<Json<PodDiscoveryFeedResponse>, ApiError> {
    let ctx = state.tools.default_auth_context()?;
    Ok(Json(state.tools.pod_discovery_feed(
        &ctx,
        &state.base_url,
        &query.q.unwrap_or_default(),
        query.limit.unwrap_or(10),
    )?))
}

async fn home_discover_public_pods(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<PublicPodDiscoveryQuery>,
) -> Result<Json<HomePublicPodDiscoveryResponse>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    let topics = query
        .topics
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|topic| !topic.is_empty())
        .map(ToString::to_string)
        .collect();
    Ok(Json(state.tools.discover_public_pods_for_home(
        &ctx,
        topics,
        query.limit.unwrap_or(10),
    )?))
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

async fn hub_register_node(
    State(state): State<ApiState>,
    Json(request): Json<HubRegisterNodeRequest>,
) -> Result<Json<HubRegisteredNode>, ApiError> {
    Ok(Json(state.tools.register_hub_node(request)?))
}

async fn hub_register_pod(
    State(state): State<ApiState>,
    Json(request): Json<HubRegisterPodRequest>,
) -> Result<Json<HubRegisteredPod>, ApiError> {
    Ok(Json(state.tools.register_hub_pod(request)?))
}

async fn hub_refresh(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<stumble_sync::HubRefreshReport>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    stumble_sync::refresh_hub_index(&state.tools, &ctx)
        .await
        .map(Json)
        .map_err(|error| ApiError {
            status: StatusCode::BAD_GATEWAY,
            message: error.to_string(),
        })
}

#[derive(Debug, Deserialize)]
struct HubSearchQuery {
    q: Option<String>,
    limit: Option<usize>,
}

async fn hub_search_pods(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<HubSearchQuery>,
) -> Result<Json<HubSearchPodsResponse>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    state
        .tools
        .index_local_public_pods_in_hub(&ctx, &state.base_url)?;
    Ok(Json(state.tools.search_hub_pods(
        &query.q.unwrap_or_default(),
        query.limit.unwrap_or(10),
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
                message: "invalid token".to_string(),
            });
        }
    }
    if !state.owner_access_allowed {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "bearer token required".to_string(),
        });
    }
    let store = tools.store();
    let store = store.read().map_err(|_| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
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
            message: "bearer token required".to_string(),
        });
    };
    tools.authenticate_token(token)?.ok_or_else(|| ApiError {
        status: StatusCode::UNAUTHORIZED,
        message: "invalid token".to_string(),
    })
}

fn ensure_dev_tokens_allowed(state: &ApiState) -> Result<(), ApiError> {
    if state.dev_tokens_allowed {
        return Ok(());
    }
    Err(ApiError {
        status: StatusCode::FORBIDDEN,
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
