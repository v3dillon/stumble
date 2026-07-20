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

#[tokio::test]
async fn http_completes_lists_and_dismisses_result_batches() {
    let tools = AgentTools::new(seed_store());
    let manager = tools
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
    let worker = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "worker".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![HarnessCapability::PersonalDiscoveryExecution],
                pod_ids: None,
            },
        )
        .unwrap();
    let manager_auth = format!("Bearer {}", manager.token.expose());
    let worker_auth = format!("Bearer {}", worker.token.expose());
    let app = router(tools);

    let response = app
        .clone()
        .oneshot(
            Request::post("/personal-discovery")
                .header("content-type", "application/json")
                .header("authorization", &manager_auth)
                .body(Body::from(
                    json!({"idempotency_key": "http-batch", "result_count": 4}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let created: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    let task_id = created["task"]["id"].as_str().unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/discovery-tasks/{task_id}/claim"))
                .header("content-type", "application/json")
                .header("authorization", &worker_auth)
                .body(Body::from(json!({"lease_seconds": 300}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::post("/candidates")
                .header("content-type", "application/json")
                .header("authorization", &worker_auth)
                .body(Body::from(
                    json!({
                        "source_url": "https://http.example/result",
                        "target": {
                            "kind": "personal_discovery",
                            "task_id": task_id,
                            "allocation_role": "proven"
                        },
                        "source_metadata": {},
                        "content_type": "article",
                        "tags": ["systems"],
                        "provenance": {
                            "discovered_at": "2026-07-20T12:00:00Z",
                            "discovery_method": "browser_search"
                        },
                        "harness_idempotency_key": "http-result-1",
                        "client_idempotency_key": "http-result-1"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let submitted: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    let submission_id = submitted["submission"]["id"].as_str().unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::post("/discovery-result-batches")
                .header("content-type", "application/json")
                .header("authorization", &worker_auth)
                .body(Body::from(
                    json!({
                        "task_id": task_id,
                        "submission_ids": [submission_id]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let batch: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(batch["state"], "ready");
    let batch_id = batch["id"].as_str().unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::get("/discovery-result-batches")
                .header("authorization", &manager_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::post(format!("/discovery-result-batches/{batch_id}/dismiss"))
                .header("authorization", &manager_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let dismissed: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(dismissed["state"], "dismissed");
}

#[tokio::test]
async fn http_reviews_result_item_with_learning_and_authorization_errors() {
    let tools = AgentTools::new(seed_store());
    let manager = tools
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
    let worker = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "worker".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![HarnessCapability::PersonalDiscoveryExecution],
                pod_ids: None,
            },
        )
        .unwrap();
    let manager_auth = format!("Bearer {}", manager.token.expose());
    let worker_auth = format!("Bearer {}", worker.token.expose());
    let app = router(tools);

    let response = app
        .clone()
        .oneshot(
            Request::post("/personal-discovery")
                .header("content-type", "application/json")
                .header("authorization", &manager_auth)
                .body(Body::from(
                    json!({"idempotency_key": "http-review", "result_count": 4}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let created: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    let task_id = created["task"]["id"].as_str().unwrap();

    app.clone()
        .oneshot(
            Request::post(format!("/discovery-tasks/{task_id}/claim"))
                .header("content-type", "application/json")
                .header("authorization", &worker_auth)
                .body(Body::from(json!({"lease_seconds": 300}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::post("/candidates")
                .header("content-type", "application/json")
                .header("authorization", &worker_auth)
                .body(Body::from(
                    json!({
                        "source_url": "https://http-review.example/result",
                        "target": {
                            "kind": "personal_discovery",
                            "task_id": task_id,
                            "allocation_role": "proven"
                        },
                        "source_metadata": {},
                        "content_type": "article",
                        "tags": ["systems"],
                        "provenance": {
                            "discovered_at": "2026-07-20T12:00:00Z",
                            "discovery_method": "browser_search"
                        },
                        "harness_idempotency_key": "http-review-1",
                        "client_idempotency_key": "http-review-1"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let submitted: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    let submission_id = submitted["submission"]["id"].as_str().unwrap();
    let candidate_id = submitted["candidate"]["id"].as_str().unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::post("/discovery-result-batches")
                .header("content-type", "application/json")
                .header("authorization", &worker_auth)
                .body(Body::from(
                    json!({
                        "task_id": task_id,
                        "submission_ids": [submission_id]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let batch: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    let batch_id = batch["id"].as_str().unwrap();

    // Worker cannot review.
    let response = app
        .clone()
        .oneshot(
            Request::post(format!(
                "/discovery-result-batches/{batch_id}/items/{candidate_id}/review"
            ))
            .header("content-type", "application/json")
            .header("authorization", &worker_auth)
            .body(Body::from(json!({"action": "more_like_this"}).to_string()))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::post(format!(
                "/discovery-result-batches/{batch_id}/items/{candidate_id}/review"
            ))
            .header("content-type", "application/json")
            .header("authorization", &manager_auth)
            .body(Body::from(json!({"action": "more_like_this"}).to_string()))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let outcome: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(outcome["item"]["review"]["state"], "reviewed");
    assert_eq!(outcome["item"]["review"]["action"], "more_like_this");
    assert_eq!(outcome["batch"]["state"], "ready");
    assert!(outcome["taste_profile"]["source_affinities"].is_array());
    assert!(outcome["allowed_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action == "more_like_this"));
}

#[tokio::test]
async fn http_manages_schedules_and_exposes_backpressure() {
    let tools = AgentTools::new(seed_store());
    let manager = tools
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
    let worker = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "worker".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![HarnessCapability::PersonalDiscoveryExecution],
                pod_ids: None,
            },
        )
        .unwrap();
    let manager_auth = format!("Bearer {}", manager.token.expose());
    let worker_auth = format!("Bearer {}", worker.token.expose());
    let app = router(tools);

    let response = app
        .clone()
        .oneshot(
            Request::post("/personal-discovery/schedules")
                .header("content-type", "application/json")
                .header("authorization", &manager_auth)
                .body(Body::from(
                    json!({
                        "name": "daily",
                        "cadence": "daily",
                        "result_count": 5,
                        "delivery_mode": "notify_when_supported"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let created: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(created["schedule"]["name"], "daily");
    assert_eq!(created["backpressure"]["kind"], "none");
    let schedule_id = created["schedule"]["id"].as_str().unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::get("/personal-discovery/schedules")
                .header("authorization", &worker_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let listed: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);

    let response = app
        .clone()
        .oneshot(
            Request::post(format!(
                "/personal-discovery/schedules/{schedule_id}/disable"
            ))
            .header("authorization", &worker_auth)
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
