use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use stumble_api::router;
use stumble_core::{
    announcement_lease_duration, seed_store, AgentHarnessKind, AgentTools, CreatePodRequest,
    HarnessCapability, RegisterAgentHarnessRequest, SensitiveChange, Visibility,
};
use tower::ServiceExt;

struct AuthLike {
    ctx: stumble_core::AuthContext,
    authorization: String,
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
        authorization: format!("Bearer {}", issued.token.expose()),
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
async fn http_produce_index_and_search_announcement_with_lease() {
    let tools = AgentTools::new(seed_store());
    let auth = register_harness(
        &tools,
        "lease curator",
        vec![HarnessCapability::PodCuration],
    );
    let pod = create_public_pod(&tools, &auth, "http-lease-systems");
    let app = router(tools);

    let produce = app
        .clone()
        .oneshot(
            Request::post("/discovery/announcements/produce")
                .header("authorization", &auth.authorization)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "pod_slug": pod.slug,
                        "public_pod_url": "https://origin.example/federation/pods/http-lease-systems"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(produce.status(), StatusCode::OK);
    let announcement: Value =
        serde_json::from_slice(&to_bytes(produce.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(announcement["signature"].is_string());
    assert!(announcement["expires_at"].is_string());
    let announced_at = announcement["announced_at"].as_str().unwrap();
    let expires_at = announcement["expires_at"].as_str().unwrap();
    let announced = chrono::DateTime::parse_from_rfc3339(announced_at)
        .unwrap()
        .with_timezone(&Utc);
    let expires = chrono::DateTime::parse_from_rfc3339(expires_at)
        .unwrap()
        .with_timezone(&Utc);
    assert_eq!(expires - announced, announcement_lease_duration());

    let index = app
        .clone()
        .oneshot(
            Request::post("/discovery/announcements")
                .header("content-type", "application/json")
                .body(Body::from(announcement.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(index.status(), StatusCode::OK);

    let search = app
        .oneshot(
            Request::get("/discovery/announcements?q=http-lease&limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(search.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(search.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["results"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn http_rejects_forged_announcement_with_typed_code() {
    let tools = AgentTools::new(seed_store());
    let auth = register_harness(
        &tools,
        "forged curator",
        vec![HarnessCapability::PodCuration],
    );
    let pod = create_public_pod(&tools, &auth, "http-forged-systems");
    let app = router(tools);

    let produce = app
        .clone()
        .oneshot(
            Request::post("/discovery/announcements/produce")
                .header("authorization", &auth.authorization)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "pod_slug": pod.slug,
                        "public_pod_url": "https://origin.example/federation/pods/http-forged-systems"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let mut announcement: Value =
        serde_json::from_slice(&to_bytes(produce.into_body(), usize::MAX).await.unwrap()).unwrap();
    announcement["subject"] = json!("tampered subject");

    let response = app
        .oneshot(
            Request::post("/discovery/announcements")
                .header("content-type", "application/json")
                .body(Body::from(announcement.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["code"], "invalid_signature");
    assert!(body["error"].as_str().unwrap().contains("signature"));
}

#[tokio::test]
async fn http_produce_and_index_withdrawal_removes_discovery() {
    let tools = AgentTools::new(seed_store());
    let auth = register_harness(
        &tools,
        "withdraw curator",
        vec![HarnessCapability::PodCuration],
    );
    let pod = create_public_pod(&tools, &auth, "http-withdraw-systems");
    let app = router(tools);

    let produce = app
        .clone()
        .oneshot(
            Request::post("/discovery/announcements/produce")
                .header("authorization", &auth.authorization)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "pod_slug": pod.slug,
                        "public_pod_url": "https://origin.example/federation/pods/http-withdraw-systems"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let announcement: Value =
        serde_json::from_slice(&to_bytes(produce.into_body(), usize::MAX).await.unwrap()).unwrap();
    let index = app
        .clone()
        .oneshot(
            Request::post("/discovery/announcements")
                .header("content-type", "application/json")
                .body(Body::from(announcement.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(index.status(), StatusCode::OK);

    let withdraw = app
        .clone()
        .oneshot(
            Request::post("/discovery/withdrawals/produce")
                .header("authorization", &auth.authorization)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "pod_slug": pod.slug,
                        "public_pod_url": "https://origin.example/federation/pods/http-withdraw-systems",
                        "make_private": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(withdraw.status(), StatusCode::OK);
    let withdrawal: Value =
        serde_json::from_slice(&to_bytes(withdraw.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(withdrawal["signature"].is_string());
    assert_eq!(withdrawal["pod_slug"], pod.slug);

    let index_withdrawal = app
        .clone()
        .oneshot(
            Request::post("/discovery/withdrawals")
                .header("content-type", "application/json")
                .body(Body::from(withdrawal.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(index_withdrawal.status(), StatusCode::OK);

    let search = app
        .oneshot(
            Request::get("/discovery/announcements?q=http-withdraw&limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(search.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(search.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(body["results"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn http_rejects_expired_announcement_with_typed_code() {
    let tools = AgentTools::new(seed_store());
    let auth = register_harness(
        &tools,
        "expired curator",
        vec![HarnessCapability::PodCuration],
    );
    let pod = create_public_pod(&tools, &auth, "http-expired-systems");
    let issued = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let announcement = tools
        .pod_announcement_at(
            &auth.ctx,
            &pod.slug,
            "https://origin.example/federation/pods/http-expired-systems",
            issued,
        )
        .unwrap();
    let app = router(tools);

    let response = app
        .oneshot(
            Request::post("/discovery/announcements")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&announcement).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["code"], "announcement_expired");
}
