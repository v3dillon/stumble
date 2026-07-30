//! Stumble's node-to-node network API.
//!
//! This crate serves only the surface other Stumble nodes reach: federation,
//! Bootstrap admission and streams, and Discovery Peer serving. The private
//! User and Harness surface lives in the CLI and MCP adapters.

mod announcements;
mod clients;
mod docs;
mod error;
mod federation;

pub use clients::{
    ReqwestAnnouncementStreamClient, ReqwestDiscoveryPeerStreamClient, ReqwestIndexSearchClient,
    ReqwestPeerAdvertisementSampleClient,
};
pub use docs::ApiRouteDoc;
pub use error::ApiError;

use announcements::*;
use axum::{
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use docs::route_docs;
use federation::*;
use serde_json::json;
use std::net::SocketAddr;
use stumble_core::*;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

#[derive(Clone)]
pub struct ApiState {
    pub tools: AgentTools,
    pub base_url: String,
    /// Whether missing bearer tokens may use the loopback owner context.
    pub owner_access_allowed: bool,
}

#[derive(Debug, Clone)]
pub struct RouterOptions {
    /// Whether missing bearer tokens may use the loopback owner context.
    pub owner_access_allowed: bool,
}

impl Default for RouterOptions {
    fn default() -> Self {
        Self {
            owner_access_allowed: true,
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
        owner_access_allowed: options.owner_access_allowed,
    };
    Router::new()
        .route("/health", get(health))
        .route("/.well-known/stumble-node", get(well_known_node))
        .route("/openapi-lite", get(openapi_lite))
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
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub fn bind_with_port(bind: SocketAddr, port: Option<u16>) -> SocketAddr {
    port.map(|port| SocketAddr::new(bind.ip(), port))
        .unwrap_or(bind)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({"status":"ok","service":"stumble","version": env!("CARGO_PKG_VERSION")}))
}

async fn openapi_lite() -> Json<Vec<ApiRouteDoc>> {
    Json(route_docs())
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


#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    use uuid::Uuid;

    #[test]
    fn bind_with_port_overrides_only_the_port() {
        let bind: SocketAddr = "127.0.0.1:8787".parse().unwrap();
        let updated = bind_with_port(bind, Some(9000));
        assert_eq!(updated.to_string(), "127.0.0.1:9000");
    }

    #[tokio::test]
    async fn user_and_harness_surfaces_are_absent_from_the_network_api() {
        let app = router(AgentTools::new(seed_store()));
        for (method, path) in [
            ("GET", "/feed"),
            ("GET", "/pods"),
            ("POST", "/candidates"),
            ("GET", "/taste-profile"),
            ("POST", "/harnesses"),
            ("POST", "/personal-discovery"),
            ("GET", "/discovery-tasks/ready"),
            ("POST", "/auth/dev-token"),
            ("GET", "/tenants"),
            ("GET", "/me"),
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
                "user-facing route must be absent from the network API: {method} {path}"
            );
        }
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
        assert!(routes.iter().all(|route| !route.path.starts_with("/home")));
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
    async fn unscoped_peer_sync_is_absent_without_redirect() {
        let response = router(AgentTools::new(seed_store()))
            .oneshot(
                Request::post(format!("/federation/sync/{}", Uuid::nil()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
