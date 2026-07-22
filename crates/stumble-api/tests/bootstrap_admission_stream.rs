//! HTTP acceptance for open Bootstrap admission and Announcement Streams.

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use std::sync::Arc;
use stumble_api::router;
use stumble_core::{
    seed_store, AgentHarnessKind, AgentTools, CreatePodRequest, HarnessCapability,
    RegisterAgentHarnessRequest, ScriptedMatchingOriginProbe, SensitiveChange, Visibility,
};
use tower::ServiceExt;

struct AuthLike {
    ctx: stumble_core::AuthContext,
}

fn register_harness(
    tools: &AgentTools,
    label: &str,
    capabilities: Vec<HarnessCapability>,
) -> AuthLike {
    let owner = tools.default_auth_context().unwrap();
    let issued = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind: AgentHarnessKind::Interactive,
                capabilities,
                pod_ids: None,
            },
        )
        .unwrap();
    AuthLike {
        ctx: tools
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap(),
    }
}

fn create_public_pod(tools: &AgentTools, proposer: &AuthLike, slug: &str) -> stumble_core::Pod {
    let approver = register_harness(
        tools,
        &format!("{slug} approver"),
        vec![HarnessCapability::Approval],
    );
    let pod = tools
        .create_pod(
            &proposer.ctx,
            CreatePodRequest {
                name: slug.replace('-', " "),
                slug: slug.into(),
                description: format!("{slug} subject"),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let now = Utc::now();
    let proposal = tools
        .create_pending_proposal(
            &proposer.ctx,
            SensitiveChange::PublishPod { pod_id: pod.id },
            now,
            now + chrono::Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver.ctx, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

#[tokio::test]
async fn http_open_bootstrap_admission_and_stream_without_auth() {
    // Origin produces the signed announcement; Bootstrap admits it on a separate node.
    let origin = AgentTools::new(seed_store());
    let auth = register_harness(
        &origin,
        "bootstrap origin curator",
        vec![HarnessCapability::PodCuration],
    );
    let pod = create_public_pod(&origin, &auth, "http-bootstrap-systems");
    let now = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap();
    let announcement = origin
        .pod_announcement_at(
            &auth.ctx,
            &pod.slug,
            "https://origin.example/federation/pods/http-bootstrap-systems",
            now,
        )
        .unwrap();
    let probe = Arc::new(ScriptedMatchingOriginProbe::default());
    probe.set_announcement(&announcement);
    let bootstrap = AgentTools::new(seed_store()).with_bootstrap_capability(true, probe);
    let app = router(bootstrap);

    // No Authorization header — open Bootstrap admission.
    let admit = app
        .clone()
        .oneshot(
            Request::post("/bootstrap/announcements")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&announcement).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admit.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(admit.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["outcome"], "admitted");
    assert!(body["stream_sequence"].is_number());

    // Idempotent replay.
    let again = app
        .clone()
        .oneshot(
            Request::post("/bootstrap/announcements")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&announcement).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(again.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["outcome"], "idempotent");

    // Topic-neutral stream, unauthenticated.
    let stream = app
        .clone()
        .oneshot(
            Request::get("/bootstrap/announcements/stream?limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stream.status(), StatusCode::OK);
    let page: Value =
        serde_json::from_slice(&to_bytes(stream.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(page["entries"].as_array().unwrap().len(), 1);
    assert_eq!(page["entries"][0]["kind"], "admitted");
    let text = page.to_string().to_lowercase();
    assert!(!text.contains("taste_profile"));
    assert!(!text.contains("user_id"));
    assert!(!text.contains("subscription"));
}

#[tokio::test]
async fn http_bootstrap_rejection_codes_are_stable() {
    let origin = AgentTools::new(seed_store());
    let auth = register_harness(
        &origin,
        "reject curator",
        vec![HarnessCapability::PodCuration],
    );
    let pod = create_public_pod(&origin, &auth, "http-bootstrap-unreachable");
    let announcement = origin
        .pod_announcement(
            &auth.ctx,
            &pod.slug,
            "https://origin.example/federation/pods/http-bootstrap-unreachable",
        )
        .unwrap();
    let bootstrap = AgentTools::new(seed_store())
        .with_bootstrap_capability(true, Arc::new(stumble_core::UnreachableOriginProbe));
    let app = router(bootstrap);

    let response = app
        .oneshot(
            Request::post("/bootstrap/announcements")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&announcement).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["code"], "unreachable_origin");
}

#[tokio::test]
async fn http_bootstrap_disabled_returns_not_found() {
    let tools = AgentTools::new(seed_store());
    let app = router(tools);
    let response = app
        .oneshot(
            Request::post("/bootstrap/announcements")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "id": "00000000-0000-0000-0000-000000000001",
                        "origin_node_id": "00000000-0000-0000-0000-000000000002",
                        "signer": {
                            "node_id": "00000000-0000-0000-0000-000000000002",
                            "display_name": "x",
                            "public_key": "y",
                            "supported_protocol_version": "stumble/1.0"
                        },
                        "pod_slug": "x",
                        "pod_name": "x",
                        "subject": "x",
                        "public_pod_url": "https://origin.example/federation/pods/x",
                        "package_version": 1,
                        "latest_event_hash": null,
                        "announced_at": "2026-10-01T00:00:00Z",
                        "expires_at": "2026-10-31T00:00:00Z",
                        "signature": "nope"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["code"], "bootstrap_disabled");
}
