//! Relay admission + subscribe acceptance: a private Origin pushes signed
//! snapshots to a combined Bootstrap/Index/Relay sponsor, and a fresh Home
//! Node subscribes through the Relay URL while the Origin has no listener.

use crate::common::*;
use axum::http::StatusCode;
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use stumble_api::{
    republish_relay_publication, router, router_with_base_url, ReqwestOriginExploreSampleClient,
    ReqwestOriginProbe,
};
use stumble_core::*;

#[tokio::test]
async fn private_origin_publishes_through_relay_and_home_subscribes() {
    let origin_dir = TestDataDir::new("relay-origin");
    let sponsor_dir = TestDataDir::new("relay-sponsor");
    let home_dir = TestDataDir::new("relay-home");

    // ── Origin: private Home Node with a public Pod; never serves HTTP ──────
    let origin = AgentTools::open_home_node(origin_dir.path(), seed_store).unwrap();
    let origin_curator = register_harness(
        &origin,
        "relay origin curator",
        vec![HarnessCapability::PodCuration],
    );
    let pod = create_public_pod(
        &origin,
        "relayed-systems",
        "Distributed systems research relayed for private origins",
    );
    accept_public_item(
        &origin,
        &pod,
        "https://research.example/relayed-systems",
        "Relayed systems primer",
    );
    let origin_node = local_node(&origin);

    // ── Combined sponsor: Bootstrap + Index + Relay in one process ──────────
    let sponsor = AgentTools::open_home_node(sponsor_dir.path(), seed_store)
        .unwrap()
        .with_bootstrap_capability(true, Arc::new(ReqwestOriginProbe))
        .with_index_capability(true)
        .with_relay_capability(true);
    assert!(sponsor.bootstrap_enabled());
    assert!(sponsor.index_enabled());
    assert!(sponsor.relay_enabled());
    let sponsor_server =
        EphemeralHttpServer::start_with(|base| router_with_base_url(sponsor.clone(), base)).await;
    let sponsor_base = sponsor_server.base_url.clone();
    let client = reqwest::Client::new();

    // Relay-on well-known advertises the Relay keys.
    let (status, wk) = client_json(
        &client,
        reqwest::Method::GET,
        &format!("{sponsor_base}/.well-known/stumble-node"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let endpoints = wk["endpoints"].as_object().unwrap();
    assert!(endpoints.contains_key("relay_publications"));
    assert!(endpoints.contains_key("relay_pod_snapshot_template"));
    assert!(endpoints.contains_key("relay_explore_samples_template"));

    let relay_url = format!("{sponsor_base}/relay/pods/{}/{}", origin_node.id, pod.slug);

    // ── Origin pushes the signed snapshot to the Relay ──────────────────────
    let origin_owner = origin.default_auth_context().unwrap();
    let snapshot = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let snapshot_json = serde_json::to_value(&snapshot).unwrap();
    let (status, admitted) = client_json(
        &client,
        reqwest::Method::POST,
        &relay_url,
        None,
        Some(snapshot_json.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "relay admit: {admitted}");
    assert_eq!(admitted["outcome"], "admitted");

    // Identical replay is an idempotent upsert.
    let (status, replayed) = client_json(
        &client,
        reqwest::Method::POST,
        &relay_url,
        None,
        Some(snapshot_json.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "relay replay: {replayed}");

    // ── Bootstrap admit succeeds via the Relay URL, no Origin listener ──────
    let announcement = origin
        .pod_announcement_at(&origin_curator.ctx, &pod.slug, &relay_url, Utc::now())
        .unwrap();
    let (status, admit) = client_json(
        &client,
        reqwest::Method::POST,
        &format!("{sponsor_base}/bootstrap/announcements"),
        None,
        Some(serde_json::to_value(&announcement).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "bootstrap admit via relay: {admit}");
    assert_eq!(admit["outcome"], "admitted");

    // ── Fresh Home Node subscribes through the Relay URL ────────────────────
    let home = AgentTools::initialize_home_node(home_dir.path(), seed_store).unwrap();
    let subscriber = register_harness(
        &home,
        "relay subscriber",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::FeedRead,
        ],
    );
    let result = stumble_sync::subscribe_pod_from_url(&home, &subscriber.ctx, &relay_url)
        .await
        .expect("subscribe through the Relay URL");

    // Subscription pins the Origin, never the Relay.
    let sponsor_node = local_node(&sponsor);
    assert_eq!(result.subscription.origin_node_id, origin_node.id);
    assert_ne!(result.subscription.origin_node_id, sponsor_node.id);
    assert_eq!(
        result.subscription.origin_public_key,
        origin_node.public_key
    );
    assert_ne!(
        result.subscription.origin_public_key,
        sponsor_node.public_key
    );
    assert!(
        result.imported_events >= 1,
        "events verify with the Origin key"
    );

    // The sponsor never gains Home Node private state through the Relay.
    {
        let binding = sponsor.store();
        let store = binding.read().unwrap();
        assert!(store.subscriptions.is_empty());
        assert!(store
            .relay_publications
            .contains_key(&(origin_node.id, pod.slug.clone())));
    }

    // ── Forged / re-signed snapshot is rejected ─────────────────────────────
    let mut forged = snapshot_json.clone();
    forged["node"]["public_key"] = serde_json::Value::String(sponsor_node.public_key.clone());
    let (status, rejected) = client_json(
        &client,
        reqwest::Method::POST,
        &relay_url,
        None,
        Some(forged),
    )
    .await;
    assert!(
        status.is_client_error(),
        "forged snapshot must be rejected: {rejected}"
    );

    // ── Oversized snapshot is rejected before verification or storage ───────
    let mut oversized = snapshot_json.clone();
    oversized["manifest"]["pod"]["description"] =
        serde_json::Value::String("x".repeat(MAX_RELAY_SNAPSHOT_PAYLOAD_BYTES + 1));
    let (status, rejected) = client_json(
        &client,
        reqwest::Method::POST,
        &relay_url,
        None,
        Some(oversized),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "oversized: {rejected}");
    assert_eq!(rejected["code"], "payload_too_large");

    // ── Relay-disabled POST returns the Bootstrap-style disabled pattern ────
    let disabled = AgentTools::new(seed_store());
    let app = router(disabled);
    let (status, body) = http_json(
        &app,
        "POST",
        &format!("/relay/pods/{}/{}", origin_node.id, pod.slug),
        None,
        Some(snapshot_json),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "relay_disabled");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_origin_explore_samples_through_relay() {
    let origin_dir = TestDataDir::new("relay-samples-origin");
    let sponsor_dir = TestDataDir::new("relay-samples-sponsor");
    let home_dir = TestDataDir::new("relay-samples-home");

    let origin = AgentTools::open_home_node(origin_dir.path(), seed_store).unwrap();
    let origin_curator = register_harness(
        &origin,
        "relay sample origin curator",
        vec![HarnessCapability::PodCuration],
    );
    let pod = create_public_pod(
        &origin,
        "relayed-samples",
        "Origin-signed Explore samples served by a Relay",
    );
    accept_public_item(
        &origin,
        &pod,
        "https://research.example/relayed-samples",
        "Relayed sample primer",
    );
    let origin_node = local_node(&origin);
    let origin_owner = origin.default_auth_context().unwrap();

    let sponsor = AgentTools::open_home_node(sponsor_dir.path(), seed_store)
        .unwrap()
        .with_bootstrap_capability(true, Arc::new(ReqwestOriginProbe))
        .with_index_capability(true)
        .with_relay_capability(true);
    let sponsor_server =
        EphemeralHttpServer::start_with(|base| router_with_base_url(sponsor.clone(), base)).await;
    let sponsor_base = sponsor_server.base_url.clone();
    let client = reqwest::Client::new();
    let relay_url = format!("{sponsor_base}/relay/pods/{}/{}", origin_node.id, pod.slug);
    let samples_url = format!("{relay_url}/explore-samples");

    let snapshot = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let (status, admitted) = client_json(
        &client,
        reqwest::Method::POST,
        &relay_url,
        None,
        Some(serde_json::to_value(&snapshot).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "relay admit: {admitted}");

    let announcement = origin
        .pod_announcement_at(&origin_curator.ctx, &pod.slug, &relay_url, Utc::now())
        .unwrap();
    let samples = origin
        .pod_explore_samples(&origin_curator.ctx, &announcement, 10)
        .unwrap();
    assert!(
        !samples.samples.is_empty(),
        "Origin must sign at least one sample"
    );
    let samples_json = serde_json::to_value(&samples).unwrap();

    // Snapshot replay must not drop later sibling samples; PUT first, then replay.
    let (status, put) = client_json(
        &client,
        reqwest::Method::PUT,
        &samples_url,
        None,
        Some(samples_json.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "relay sample admit: {put}");
    assert_eq!(put["outcome"], "admitted");
    assert_eq!(put["announcement_id"], json!(announcement.id));

    let (status, replayed) = client_json(
        &client,
        reqwest::Method::POST,
        &relay_url,
        None,
        Some(serde_json::to_value(&snapshot).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "snapshot replay: {replayed}");

    let fetch_body = json!({ "announcement": announcement, "limit": 1 });
    let (status, fetched) = client_json(
        &client,
        reqwest::Method::POST,
        &samples_url,
        None,
        Some(fetch_body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "relay sample fetch: {fetched}");
    assert_eq!(fetched, samples_json);

    let (status, _) = client_json(
        &client,
        reqwest::Method::POST,
        &format!("{sponsor_base}/bootstrap/announcements"),
        None,
        Some(serde_json::to_value(&announcement).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let home = AgentTools::initialize_home_node(home_dir.path(), seed_store).unwrap();
    let reader = register_harness(
        &home,
        "relay sample reader",
        vec![HarnessCapability::FeedRead],
    );
    home.index_pod_announcement(announcement.clone()).unwrap();
    let home_for_fetch = home.clone();
    let reader_ctx = reader.ctx.clone();
    let origin_id = origin_node.id;
    let slug = pod.slug.clone();
    let accepted = tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let sample_client = ReqwestOriginExploreSampleClient::new(runtime.handle().clone());
        home_for_fetch.fetch_origin_explore_samples(
            &reader_ctx,
            origin_id,
            &slug,
            3,
            &sample_client,
        )
    })
    .await
    .unwrap()
    .expect("Home Node fetches Origin-signed samples from the Relay URL");
    assert_eq!(accepted, samples);

    // A refresh issues a new announcement id. The runner tick shares
    // republish_relay_publication so Relay samples bind that id.
    let refreshed = origin
        .pod_announcement_at(&origin_curator.ctx, &pod.slug, &relay_url, Utc::now())
        .unwrap();
    assert_ne!(refreshed.id, announcement.id);
    let report = republish_relay_publication(&origin, &origin_curator.ctx, &refreshed).await;
    assert!(
        report.snapshot.is_ok(),
        "relay snapshot republish: {:?}",
        report.snapshot
    );
    assert!(
        report.explore_samples.is_ok(),
        "relay sample republish: {:?}",
        report.explore_samples
    );
    let (status, fetched) = client_json(
        &client,
        reqwest::Method::POST,
        &samples_url,
        None,
        Some(json!({ "announcement": refreshed, "limit": 3 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "refreshed sample fetch: {fetched}");
    assert_eq!(fetched["announcement_id"], json!(refreshed.id));
    let (status, stale) = client_json(
        &client,
        reqwest::Method::POST,
        &samples_url,
        None,
        Some(json!({ "announcement": announcement, "limit": 3 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "prior samples: {stale}");
    assert_eq!(stale["code"], "announcement_stale");

    let mut forged = samples.clone();
    forged.signature = "not-a-signature".into();
    let (status, rejected) = client_json(
        &client,
        reqwest::Method::PUT,
        &samples_url,
        None,
        Some(serde_json::to_value(&forged).unwrap()),
    )
    .await;
    assert!(
        status.is_client_error(),
        "forged samples must be rejected: {rejected}"
    );
    assert_eq!(rejected["code"], "invalid_signature");

    let sponsor_node = local_node(&sponsor);
    let mut resigned = samples.clone();
    resigned.origin_node_id = sponsor_node.id;
    resigned.signer = NodeInfo {
        node_id: sponsor_node.id,
        display_name: sponsor_node.display_name.clone(),
        public_key: sponsor_node.public_key.clone(),
        supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
    };
    resigned = sign_pod_explore_samples(&sponsor_node, resigned).unwrap();
    let (status, rejected) = client_json(
        &client,
        reqwest::Method::PUT,
        &samples_url,
        None,
        Some(serde_json::to_value(&resigned).unwrap()),
    )
    .await;
    assert!(
        status.is_client_error(),
        "re-signed samples must be rejected: {rejected}"
    );

    let mut stale_request = announcement.clone();
    stale_request.id = uuid::Uuid::now_v7();
    let (status, stale) = client_json(
        &client,
        reqwest::Method::POST,
        &samples_url,
        None,
        Some(json!({ "announcement": stale_request, "limit": 3 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "stale samples: {stale}");
    assert_eq!(stale["code"], "announcement_stale");

    let mut mismatched = announcement.clone();
    mismatched.pod_slug = "other-slug".into();
    let (status, mismatch) = client_json(
        &client,
        reqwest::Method::POST,
        &samples_url,
        None,
        Some(json!({ "announcement": mismatched, "limit": 3 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "path mismatch: {mismatch}");
    assert_eq!(mismatch["code"], "validation_error");

    let mut oversized = samples_json.clone();
    oversized["signer"]["display_name"] =
        json!("x".repeat(MAX_RELAY_EXPLORE_SAMPLES_PAYLOAD_BYTES + 1));
    let (status, rejected) = client_json(
        &client,
        reqwest::Method::PUT,
        &samples_url,
        None,
        Some(oversized),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "oversized samples: {rejected}"
    );
    assert_eq!(rejected["code"], "payload_too_large");

    let disabled = AgentTools::new(seed_store());
    let app = router(disabled);
    let path = format!("/relay/pods/{}/{}", origin_node.id, pod.slug);
    let (status, body) = http_json(
        &app,
        "PUT",
        &format!("{path}/explore-samples"),
        None,
        Some(samples_json.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "relay_disabled");
    let (status, body) = http_json(
        &app,
        "POST",
        &format!("{path}/explore-samples"),
        None,
        Some(fetch_body),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "relay_disabled");
}
