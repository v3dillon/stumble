use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use stumble_api::router;
use stumble_core::{
    seed_store, AgentHarnessKind, AgentTools, HarnessCapability, RegisterAgentHarnessRequest,
};
use tower::ServiceExt;

#[tokio::test]
async fn http_exposes_personal_readiness_request_and_plan_inspection() {
    let tools = AgentTools::new(seed_store());
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "manager".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::PersonalDiscoveryManagement],
                pod_ids: None,
            },
        )
        .unwrap();
    let authorization = format!("Bearer {}", issued.token.expose());
    let app = router(tools);

    let readiness = app
        .clone()
        .oneshot(
            Request::get("/personal-discovery/readiness")
                .header("authorization", &authorization)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(readiness.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::post("/personal-discovery")
                .header("content-type", "application/json")
                .header("authorization", &authorization)
                .body(Body::from(
                    json!({"idempotency_key": "http-personal"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let created: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(created["task"]["target"]["kind"], "personal");
    assert_eq!(created["plan"]["result_count"], 10);
    let plan_id = created["plan"]["id"].as_str().unwrap();

    let response = app
        .oneshot(
            Request::get(format!("/discovery-plans/{plan_id}"))
                .header("authorization", authorization)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
