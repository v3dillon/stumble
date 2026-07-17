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
use url::Url;
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

#[derive(Debug, Clone)]
struct PageMetadata {
    title: Option<String>,
    summary: Option<String>,
    image_url: Option<String>,
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
        .route("/route-link", post(route_link))
        .route("/intake-link", post(auto_route_intake_link))
        .route("/pods", get(list_pods).post(create_pod))
        .route("/pod-packages", post(create_private_pod_with_package))
        .route("/pods/:slug/join", post(join_pod))
        .route("/pods/:slug/submit", post(submit_link))
        .route("/pods/:slug/intake-link", post(intake_link))
        .route(
            "/pods/:slug/submissions/:id",
            delete(remove_submission_from_pod),
        )
        .route(
            "/pods/:slug/skill-pack",
            get(get_skill_pack).patch(patch_skill_pack_handler),
        )
        .route(
            "/pods/:slug/skill-pack/export",
            post(export_skill_pack_handler),
        )
        .route(
            "/pods/:slug/skill-pack/import",
            post(import_skill_pack_handler),
        )
        .route("/pods/:slug/skill-pack/fork", post(fork_skill_pack_handler))
        .route(
            "/pods/:slug/skill-pack/validate",
            post(validate_skill_pack_handler),
        )
        .route("/pods/:slug/sources", post(add_source))
        .route("/pods/:slug/crawl", post(crawl_pod))
        .route("/pods/:slug/discover", post(discover))
        .route("/pods/:slug/stumble", post(stumble))
        .route("/briefs", get(list_briefs))
        .route("/briefs/generate", post(generate_brief))
        .route("/links/:id/assets", get(link_assets))
        .route("/links/:id/save", post(save_link))
        .route("/links/:id/rate", post(rate_link))
        .route("/me/preferences", patch(update_preferences))
        .route("/discovery/pods", get(pod_discovery_feed))
        .route("/home/discover-public-pods", get(home_discover_public_pods))
        .route("/auth/dev-token", post(dev_token))
        .route("/me", get(me))
        .route("/tenants", get(list_tenants).post(create_tenant))
        .route("/api-tokens", post(create_api_token))
        .route("/api-tokens/:id", delete(revoke_api_token))
        .route("/harnesses", post(register_agent_harness))
        .route("/harnesses/:id", delete(revoke_agent_harness))
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
        .route("/federation/sync/:peer_id", post(federation_sync))
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
            method: "POST",
            path: "/pods",
            description: "create a pod and default skill pack",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/route-link",
            description: "fetch metadata, rank pod candidates, and suggest a new pod when routing needs confirmation without storing the link",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/intake-link",
            description: "route a link to the best pod, store only when confidence is high, and otherwise return candidates plus a suggested new pod",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/pods/:slug/intake-link",
            description: "fetch a link, summarize metadata, submit it, and store a representative image asset",
        },
        ApiRouteDoc {
            method: "DELETE",
            path: "/pods/:slug/submissions/:id",
            description: "remove a link from a pod; purges the submission and its assets when no pod still references it",
        },
        ApiRouteDoc {
            method: "GET",
            path: "/links/:id/assets",
            description: "list representative image assets for a submitted link",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/pods/:slug/discover",
            description: "rank links using AgentTools",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/pods/:slug/stumble",
            description: "discover with controlled randomness",
        },
        ApiRouteDoc {
            method: "POST",
            path: "/briefs/generate",
            description: "generate private brief from one or more pods",
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
) -> Result<Json<Pod>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.create_pod(&ctx, request)?))
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

async fn submit_link(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(request): Json<SubmitLinkRequest>,
) -> Result<Json<Submission>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.submit_link_to_pod(&ctx, &slug, request)?))
}

async fn remove_submission_from_pod(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((slug, submission_id)): Path<(String, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    let purged = state
        .tools
        .remove_submission_from_pod(&ctx, &slug, submission_id)?;
    Ok(Json(json!({
        "removed_from_pod": slug,
        "submission_id": submission_id,
        "submission_purged": purged,
    })))
}

async fn route_link(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<RouteLinkRequest>,
) -> Result<Json<RouteLinkResponse>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    let metadata = fetch_page_metadata(&request.url).await?;
    Ok(Json(state.tools.route_link_to_pods(
        &ctx,
        RouteLinkRequest {
            url: request.url,
            title: request.title.or(metadata.title),
            summary: request.summary.or(metadata.summary),
            tags: request.tags,
        },
        2.5,
    )?))
}

async fn auto_route_intake_link(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<AutoRouteIntakeRequest>,
) -> Result<Json<AutoRouteIntakeResponse>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    let metadata = fetch_page_metadata(&request.url).await?;
    let routing = state.tools.route_link_to_pods(
        &ctx,
        RouteLinkRequest {
            url: request.url.clone(),
            title: metadata.title.clone(),
            summary: metadata.summary.clone(),
            tags: request.tags.clone(),
        },
        request.min_confidence.unwrap_or(2.5),
    )?;
    let Some(selected) = routing.selected.clone() else {
        return Ok(Json(AutoRouteIntakeResponse {
            routing,
            intake: None,
        }));
    };
    let intake = intake_link_with_metadata(
        &state.tools,
        &ctx,
        &selected.pod_slug,
        LinkIntakeRequest {
            url: request.url,
            note: request.note,
            tags: request.tags,
            representative_image: request.representative_image,
        },
        metadata,
    )?;
    Ok(Json(AutoRouteIntakeResponse {
        routing,
        intake: Some(intake),
    }))
}

async fn intake_link(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(request): Json<LinkIntakeRequest>,
) -> Result<Json<LinkIntakeResponse>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    let metadata = fetch_page_metadata(&request.url).await?;
    Ok(Json(intake_link_with_metadata(
        &state.tools,
        &ctx,
        &slug,
        request,
        metadata,
    )?))
}

fn intake_link_with_metadata(
    tools: &AgentTools,
    ctx: &AuthContext,
    slug: &str,
    request: LinkIntakeRequest,
    metadata: PageMetadata,
) -> Result<LinkIntakeResponse, ApiError> {
    let image_request = request
        .representative_image
        .clone()
        .or_else(|| metadata.image_url.clone().map(page_image_request));
    let submission = tools.submit_link_to_pod(
        ctx,
        slug,
        SubmitLinkRequest {
            url: request.url,
            title: metadata.title.clone(),
            description: metadata.summary.clone(),
            note: request.note,
            tags: request.tags,
            discovered_by_crawler: false,
        },
    )?;
    let mut assets = tools.assets_for_submission(ctx, submission.id)?;
    if let Some(image_request) = image_request {
        let asset = tools.add_submission_asset(ctx, submission.id, image_request)?;
        if !assets.iter().any(|existing| existing.id == asset.id) {
            assets.push(asset);
        }
    }
    Ok(LinkIntakeResponse {
        submission,
        assets,
        fetched_title: metadata.title,
        fetched_summary: metadata.summary,
        representative_image_url: metadata.image_url,
    })
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
struct AddSourceRequest {
    source_type: CrawlerSourceType,
    url: String,
}

async fn add_source(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(request): Json<AddSourceRequest>,
) -> Result<Json<CrawlerSource>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.add_source_to_pod(
        &ctx,
        &slug,
        request.source_type,
        request.url,
    )?))
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

async fn crawl_pod(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    let manifest = state.tools.pod_manifest(&ctx, &slug)?;
    Ok(Json(json!({
        "status": "queued",
        "pod": manifest.pod.slug,
        "note": "MVP crawler boundary is available in stumble-crawler; HTTP endpoint records intent."
    })))
}

async fn discover(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(request): Json<DiscoverRequest>,
) -> Result<Json<Vec<DiscoveryItem>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.discover_in_pod(&ctx, &slug, request)?))
}

async fn stumble(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(mut request): Json<DiscoverRequest>,
) -> Result<Json<Vec<DiscoveryItem>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    request.mode = DiscoveryMode::Stumble;
    Ok(Json(state.tools.discover_in_pod(&ctx, &slug, request)?))
}

async fn list_briefs(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Brief>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.list_briefs_for_harness(&ctx)?))
}

async fn generate_brief(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<GenerateBriefRequest>,
) -> Result<Json<Brief>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.generate_brief(&ctx, request)?))
}

async fn save_link(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    state.tools.save_link(&ctx, id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn link_assets(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<SubmissionAsset>>, ApiError> {
    let ctx = auth_or_default(&state, &headers)?;
    Ok(Json(state.tools.assets_for_submission(&ctx, id)?))
}

async fn rate_link(
    State(_state): State<ApiState>,
    Path(_id): Path<Uuid>,
) -> Json<serde_json::Value> {
    Json(
        json!({"status":"accepted","note":"rate_link is represented as local feedback in the core MVP"}),
    )
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

async fn revoke_api_token(Path(_id): Path<Uuid>) -> StatusCode {
    StatusCode::NO_CONTENT
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

async fn federation_sync(Path(peer_id): Path<Uuid>) -> Json<serde_json::Value> {
    Json(json!({
        "peer_id": peer_id,
        "status": "accepted",
        "note": "Use stumble-sync for outbound HTTP sync; endpoint exists to trigger worker integration."
    }))
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

async fn fetch_page_metadata(url: &str) -> Result<PageMetadata, ApiError> {
    let base_url = Url::parse(url).map_err(|error| ApiError {
        status: StatusCode::BAD_REQUEST,
        message: format!("bad url: {error}"),
    })?;
    let client = reqwest::Client::builder()
        .user_agent("stumble-link-intake/0.1")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|error| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("failed to build HTTP client: {error}"),
        })?;
    let html = client
        .get(url)
        .send()
        .await
        .map_err(|error| ApiError {
            status: StatusCode::BAD_GATEWAY,
            message: format!("failed to fetch link: {error}"),
        })?
        .error_for_status()
        .map_err(|error| ApiError {
            status: StatusCode::BAD_GATEWAY,
            message: format!("link returned an error: {error}"),
        })?
        .text()
        .await
        .map_err(|error| ApiError {
            status: StatusCode::BAD_GATEWAY,
            message: format!("failed to read link body: {error}"),
        })?;
    let title = extract_title(&html).or_else(|| extract_meta(&html, "property", "og:title"));
    let summary = extract_meta(&html, "name", "description")
        .or_else(|| extract_meta(&html, "property", "og:description"))
        .or_else(|| extract_meta(&html, "name", "twitter:description"))
        .or_else(|| title.as_ref().map(|title| format!("Page titled {title}.")));
    let image_url = extract_x_media_image(&html)
        .or_else(|| extract_meta(&html, "property", "og:image"))
        .or_else(|| extract_meta(&html, "name", "twitter:image"))
        .and_then(|image| resolve_url(&base_url, &image));
    Ok(PageMetadata {
        title,
        summary,
        image_url,
    })
}

fn page_image_request(url: String) -> RepresentativeImageRequest {
    RepresentativeImageRequest {
        source: SubmissionAssetSource::PageImage,
        url: Some(url),
        local_path: None,
        mime_type: None,
        alt_text: Some("Representative image extracted from page metadata.".to_string()),
    }
}

fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let start = lower.find("<title")?;
    let open_end = lower[start..].find('>')? + start + 1;
    let close = lower[open_end..].find("</title>")? + open_end;
    clean_html_value(&html[open_end..close])
}

fn extract_meta(html: &str, attr_name: &str, attr_value: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let mut offset = 0;
    while let Some(relative_start) = lower[offset..].find("<meta") {
        let start = offset + relative_start;
        let Some(relative_end) = lower[start..].find('>') else {
            break;
        };
        let end = start + relative_end + 1;
        let tag = &html[start..end];
        let tag_lower = tag.to_lowercase();
        if has_attr_value(&tag_lower, attr_name, attr_value) {
            if let Some(content) = attr(tag, "content") {
                return clean_html_value(&content);
            }
        }
        offset = end;
    }
    None
}

fn extract_x_media_image(html: &str) -> Option<String> {
    let markers = [
        "media_url_https:&quot;",
        "\"media_url_https\":\"",
        "https://pbs.twimg.com/amplify_video_thumb/",
        "https://pbs.twimg.com/media/",
    ];
    for marker in markers {
        if let Some(start) = html.find(marker) {
            let value_start = start + marker.len();
            if marker.starts_with("https://") {
                let value = &html[start..];
                let end = value
                    .find(|c: char| c == '"' || c == '\'' || c == '<' || c.is_whitespace())
                    .unwrap_or(value.len());
                return clean_html_value(&value[..end]);
            }
            let value = &html[value_start..];
            let end = value.find(['"', '&', '<']).unwrap_or(value.len());
            return clean_html_value(&value[..end]);
        }
    }
    None
}

fn has_attr_value(tag_lower: &str, attr_name: &str, attr_value: &str) -> bool {
    let attr_value = attr_value.to_lowercase();
    attr(tag_lower, attr_name).is_some_and(|value| value.to_lowercase() == attr_value)
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let needle = format!("{name}=");
    let start = lower.find(&needle)? + needle.len();
    let bytes = tag.as_bytes();
    let quote = *bytes.get(start)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let value_start = start + 1;
    let value_end = tag[value_start..].find(quote as char)? + value_start;
    Some(tag[value_start..value_end].to_string())
}

fn clean_html_value(value: &str) -> Option<String> {
    let cleaned = value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn resolve_url(base: &Url, value: &str) -> Option<String> {
    Url::parse(value)
        .or_else(|_| base.join(value))
        .ok()
        .map(|url| url.to_string())
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
                Request::post(format!("/links/{}/save", Uuid::nil()))
                    .header("authorization", format!("Bearer {}", issued.token.expose()))
                    .body(Body::empty())
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
}
