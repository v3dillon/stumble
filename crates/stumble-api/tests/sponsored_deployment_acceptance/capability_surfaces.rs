//! Runtime capability independence, browser Candidate containment, and the
//! outbound-only default before peer-serving opt-in.

use crate::common::*;
use axum::http::StatusCode;
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use stumble_api::{router, router_with_base_url};
use stumble_core::*;

// ─── Runtime capability independence + Relay absent ──────────────────────────

#[tokio::test]
async fn runtime_enables_bootstrap_and_index_independently_without_relay() {
    let probe = Arc::new(UnreachableOriginProbe);

    let bootstrap_only =
        AgentTools::new(seed_store()).with_bootstrap_capability(true, probe.clone());
    assert!(bootstrap_only.bootstrap_enabled());
    assert!(!bootstrap_only.index_enabled());
    let app = router(bootstrap_only);
    let (status, wk) = http_json(&app, "GET", "/.well-known/stumble-node", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let endpoints = wk["endpoints"].as_object().unwrap();
    assert!(endpoints.contains_key("bootstrap_announcements"));
    assert!(!endpoints.contains_key("index_search_announcements"));
    assert!(!wk.to_string().to_lowercase().contains("relay"));

    let index_only = AgentTools::new(seed_store()).with_index_capability(true);
    assert!(!index_only.bootstrap_enabled());
    assert!(index_only.index_enabled());
    let app = router(index_only);
    let (status, wk) = http_json(&app, "GET", "/.well-known/stumble-node", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let endpoints = wk["endpoints"].as_object().unwrap();
    assert!(!endpoints.contains_key("bootstrap_announcements"));
    assert!(endpoints.contains_key("index_search_announcements"));
    assert!(!wk.to_string().to_lowercase().contains("relay"));

    // Neither capability: bootstrap routes report disabled.
    let neither = AgentTools::new(seed_store());
    let app = router(neither);
    let (status, body) = http_json(
        &app,
        "POST",
        "/bootstrap/announcements",
        None,
        Some(json!({
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
        })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "bootstrap_disabled");
}

// ─── Relay is a third independent capability flag ────────────────────────────

#[tokio::test]
async fn runtime_enables_relay_independently_and_advertises_only_when_on() {
    // Relay-only process: Relay keys advertised, Bootstrap/Index absent.
    let relay_only = AgentTools::new(seed_store()).with_relay_capability(true);
    assert!(!relay_only.bootstrap_enabled());
    assert!(!relay_only.index_enabled());
    assert!(relay_only.relay_enabled());
    let app = router(relay_only);
    let (status, wk) = http_json(&app, "GET", "/.well-known/stumble-node", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let endpoints = wk["endpoints"].as_object().unwrap();
    assert!(endpoints.contains_key("relay_publications"));
    assert!(endpoints.contains_key("relay_pod_snapshot_template"));
    assert!(endpoints.contains_key("relay_explore_samples_template"));
    assert!(!endpoints.contains_key("bootstrap_announcements"));
    assert!(!endpoints.contains_key("index_search_announcements"));
    // Bootstrap routes stay disabled on a Relay-only process.
    let (status, body) =
        http_json(&app, "GET", "/bootstrap/announcements/stream", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "bootstrap_disabled");

    // All three capabilities on one process, still independent flags.
    let all_three = AgentTools::new(seed_store())
        .with_bootstrap_capability(true, Arc::new(UnreachableOriginProbe))
        .with_index_capability(true)
        .with_relay_capability(true);
    assert!(all_three.bootstrap_enabled());
    assert!(all_three.index_enabled());
    assert!(all_three.relay_enabled());
    let app = router(all_three);
    let (status, wk) = http_json(&app, "GET", "/.well-known/stumble-node", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let endpoints = wk["endpoints"].as_object().unwrap();
    assert!(endpoints.contains_key("bootstrap_announcements"));
    assert!(endpoints.contains_key("index_search_announcements"));
    assert!(endpoints.contains_key("relay_publications"));
    assert!(endpoints.contains_key("relay_pod_snapshot_template"));
    assert!(endpoints.contains_key("relay_explore_samples_template"));

    // No capabilities: Relay routes report the Bootstrap-style disabled pattern.
    let none = AgentTools::new(seed_store());
    assert!(!none.relay_enabled());
    let app = router(none);
    let (status, body) = http_json(
        &app,
        "GET",
        "/relay/pods/00000000-0000-0000-0000-000000000002/example",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "relay_disabled");
}

// ─── Browser Candidates stay in Discovery Result Batches ─────────────────────

#[tokio::test]
async fn browser_candidates_remain_in_result_batches_not_feed() {
    let home_dir = TestDataDir::new("browser-home");
    let home = AgentTools::initialize_home_node(home_dir.path(), seed_store).unwrap();
    let manager = register_harness(
        &home,
        "pd manager",
        vec![
            HarnessCapability::PersonalDiscoveryManagement,
            HarnessCapability::FeedRead,
        ],
    );
    let worker = {
        let owner = home.default_auth_context().unwrap();
        let issued = home
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "pd worker".into(),
                    kind: AgentHarnessKind::Unattended,
                    capabilities: vec![HarnessCapability::PersonalDiscoveryExecution],
                    pod_ids: None,
                },
            )
            .unwrap();
        AuthLike {
            ctx: home
                .authenticate_token(issued.token.expose())
                .unwrap()
                .unwrap(),
        }
    };
    // The Personal Discovery workflow is a Harness surface (CLI/MCP); drive it
    // in-process while the HTTP app stays network-only.
    let created = home
        .request_personal_discovery(
            &manager.ctx,
            serde_json::from_value(json!({
                "idempotency_key": "sponsored-browser",
                "result_count": 4
            }))
            .unwrap(),
            Utc::now(),
        )
        .unwrap();
    let task_id = created.task.id;

    home.claim_discovery_task(
        &worker.ctx,
        task_id,
        Utc::now(),
        DiscoveryLeaseSeconds::new(300).unwrap(),
    )
    .unwrap();

    let submitted = home
        .submit_candidate(
            &worker.ctx,
            serde_json::from_value(json!({
                "source_url": "https://browser.example/unreviewed-find",
                "target": {
                    "kind": "personal_discovery",
                    "task_id": task_id,
                    "allocation_role": "proven"
                },
                "source_metadata": { "title": "Unreviewed browser find" },
                "content_type": "article",
                "tags": ["browser"],
                "provenance": {
                    "discovered_at": "2026-11-01T12:00:00Z",
                    "discovery_method": "browser_search"
                },
                "harness_idempotency_key": "browser-worker-1",
                "client_idempotency_key": "browser-client-1"
            }))
            .unwrap(),
        )
        .unwrap();
    let submission_id = submitted.submission.id;

    let batch = home
        .complete_discovery_result_batch(
            &worker.ctx,
            serde_json::from_value(json!({
                "task_id": task_id,
                "submission_ids": [submission_id]
            }))
            .unwrap(),
            Utc::now(),
        )
        .unwrap();
    let batch_id = batch.id;

    // Candidate lives in the Discovery Result Batch.
    let listed = home.list_discovery_result_batches(&manager.ctx).unwrap();
    assert!(listed.iter().any(|batch| batch.id == batch_id));

    // Feed must not surface the unreviewed browser Candidate without explicit User action.
    let feed = home
        .get_feed_batch(&manager.ctx, FeedBatchRequest::new(1).unwrap(), Utc::now())
        .unwrap();
    assert!(
        feed.items.iter().all(|item| {
            item.content_reference.canonical_url != "https://browser.example/unreviewed-find"
        }),
        "browser Candidate must not enter Feed without explicit User action"
    );
}

// ─── Outbound-only default: well-known omits peer serving until opt-in ────────

#[tokio::test]
async fn home_is_outbound_only_until_peer_serving_opt_in() {
    let home_dir = TestDataDir::new("outbound-home");
    let home = AgentTools::initialize_home_node(home_dir.path(), seed_store).unwrap();
    let node = local_node(&home);
    let home =
        home.with_discovery_peer_probe(Arc::new(FixedDiscoveryPeerProbe::matching_node(&node)));
    let admin = register_harness(
        &home,
        "outbound admin",
        vec![HarnessCapability::Administration],
    );
    let app = router_with_base_url(home.clone(), "http://127.0.0.1:9");

    let (status, wk) = http_json(&app, "GET", "/.well-known/stumble-node", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let endpoints = wk["endpoints"].as_object().unwrap();
    assert!(!endpoints.contains_key("discovery_peer_announcement_stream"));
    assert!(!endpoints.contains_key("discovery_peer_advertisement_sample"));

    let status_body = home.discovery_peer_service_status(&admin.ctx).unwrap();
    assert!(!status_body.enabled);

    // Opt-in enables serving advertisement (CLI `sync discovery serve enable`).
    let ad = home
        .enable_discovery_peer_service(&admin.ctx, "http://127.0.0.1:9", Utc::now())
        .unwrap();
    assert_eq!(ad.public_endpoint, "http://127.0.0.1:9");

    let (status, wk) = http_json(&app, "GET", "/.well-known/stumble-node", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let endpoints = wk["endpoints"].as_object().unwrap();
    assert!(endpoints.contains_key("discovery_peer_announcement_stream"));
}
