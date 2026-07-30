//! Principal multi-node scenario plus lifecycle, rejection, block, index
//! policy, and peer eviction coverage.

use crate::common::*;
use axum::http::StatusCode;
use chrono::{Duration, TimeZone, Utc};
use std::sync::Arc;
use stumble_api::{
    router_with_base_url, ReqwestAnnouncementStreamClient, ReqwestDiscoveryPeerStreamClient,
    ReqwestIndexSearchClient, ReqwestPeerAdvertisementSampleClient,
};
use stumble_core::*;

// ─── Principal multi-node scenario ───────────────────────────────────────────

#[tokio::test]
async fn sponsored_multi_node_publish_sync_peer_outage_and_subscribe() {
    let now = Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap();
    let origin_dir = TestDataDir::new("origin");
    let sponsor_dir = TestDataDir::new("sponsor");
    let peer_dir = TestDataDir::new("peer");
    let home_dir = TestDataDir::new("home");

    // ── Origin ──────────────────────────────────────────────────────────────
    let origin = AgentTools::open_home_node(origin_dir.path(), seed_store).unwrap();
    let origin_curator = register_harness(
        &origin,
        "origin curator",
        vec![HarnessCapability::PodCuration],
    );
    let primary = create_public_pod(
        &origin,
        "sponsored-systems",
        "Distributed systems research for decentralized discovery",
    );
    accept_public_item(
        &origin,
        &primary,
        "https://research.example/distributed-systems",
        "Distributed systems research primer",
    );

    let origin_server =
        EphemeralHttpServer::start_with(|base| router_with_base_url(origin.clone(), base)).await;
    let origin_base = origin_server.base_url.clone();
    let primary_url = format!("{origin_base}/federation/pods/{}", primary.slug);

    let announcement = origin
        .pod_announcement_at(&origin_curator.ctx, &primary.slug, &primary_url, now)
        .unwrap();
    assert!(announcement.verify().unwrap());
    assert_eq!(
        announcement.expires_at - announcement.announced_at,
        announcement_lease_duration()
    );

    // ── Discovery Peer node (identity known before sponsor probe wiring) ────
    let peer_node_identity = {
        let opened = AgentTools::open_home_node(peer_dir.path(), seed_store).unwrap();
        local_node(&opened)
    };
    // Re-open with probes + bootstrap capability so Origins can admit after sponsor outage.
    let origin_probe_peer = Arc::new(ScriptedMatchingOriginProbe::default());
    origin_probe_peer.set_announcement(&announcement);
    let peer = AgentTools::open_initialized_home_node(peer_dir.path())
        .unwrap()
        .with_discovery_peer_probe(Arc::new(FixedDiscoveryPeerProbe::matching_node(
            &peer_node_identity,
        )))
        .with_bootstrap_capability(true, origin_probe_peer.clone());
    let peer_admin = register_harness(&peer, "peer admin", vec![HarnessCapability::Administration]);

    // ── Sponsored Bootstrap + Index ─────────────────────────────────────────
    let origin_probe_sponsor = Arc::new(ScriptedMatchingOriginProbe::default());
    origin_probe_sponsor.set_announcement(&announcement);
    let sponsor = AgentTools::open_home_node(sponsor_dir.path(), seed_store)
        .unwrap()
        .with_bootstrap_capability(true, origin_probe_sponsor.clone())
        .with_index_capability(true)
        .with_discovery_peer_probe(Arc::new(FixedDiscoveryPeerProbe::matching_node(
            &peer_node_identity,
        )));
    // Independent capability flags: both on; Relay never present.
    assert!(sponsor.bootstrap_enabled());
    assert!(sponsor.index_enabled());

    let sponsor_server =
        EphemeralHttpServer::start_with(|base| router_with_base_url(sponsor.clone(), base)).await;
    let sponsor_base = sponsor_server.base_url.clone();
    let client = reqwest::Client::new();

    // Well-known: Bootstrap + Index advertised; Relay absent.
    let (status, well_known) = client_json(
        &client,
        reqwest::Method::GET,
        &format!("{sponsor_base}/.well-known/stumble-node"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let endpoints = well_known["endpoints"].as_object().unwrap();
    assert!(endpoints.contains_key("bootstrap_announcements"));
    assert!(endpoints.contains_key("bootstrap_announcement_stream"));
    assert!(endpoints.contains_key("index_search_announcements"));
    let text = well_known.to_string().to_lowercase();
    assert!(!text.contains("relay"));
    assert!(!text.contains("hub"));

    // Open admission — no auth, no User account.
    let (status, admitted) = client_json(
        &client,
        reqwest::Method::POST,
        &format!("{sponsor_base}/bootstrap/announcements"),
        None,
        Some(serde_json::to_value(&announcement).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "admit body: {admitted}");
    assert_eq!(admitted["outcome"], "admitted");

    // Cursor-paginated neutral stream.
    let (status, stream) = client_json(
        &client,
        reqwest::Method::GET,
        &format!("{sponsor_base}/bootstrap/announcements/stream?limit=10"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stream["entries"].as_array().unwrap().len(), 1);
    assert_eq!(stream["entries"][0]["kind"], "admitted");
    let stream_text = stream.to_string().to_lowercase();
    assert!(!stream_text.contains("taste_profile"));
    assert!(!stream_text.contains("subscription"));
    assert!(!stream_text.contains("user_id"));

    // Index search returns the announcement without requiring an account.
    let (status, index_hits) = client_json(
        &client,
        reqwest::Method::GET,
        &format!("{sponsor_base}/discovery/announcements?q=distributed&limit=10"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(index_hits["results"].as_array().unwrap().len(), 1);

    // ── Peer: enable serving, admit same announcement, advertise to sponsor ─
    // Bind the peer HTTP listener first so the public endpoint URL is known.
    let peer_server =
        EphemeralHttpServer::start_with(|base| router_with_base_url(peer.clone(), base)).await;
    let peer_base = peer_server.base_url.clone();

    // Enable with the deterministic test clock so the 7-day peer-ad lease is
    // active under the same `now` used for learn/select (HTTP enable uses wall clock).
    let peer_ad = peer
        .enable_discovery_peer_service(&peer_admin.ctx, &peer_base, now)
        .unwrap();
    assert_eq!(peer_ad.public_endpoint, peer_base);

    // Node ops are a CLI surface; confirm the opt-in state in-process.
    let peer_status = peer.discovery_peer_service_status(&peer_admin.ctx).unwrap();
    assert!(peer_status.enabled);

    // Peer admits the Origin announcement (projects into peer stream).
    let (status, peer_admit) = client_json(
        &client,
        reqwest::Method::POST,
        &format!("{peer_base}/bootstrap/announcements"),
        None,
        Some(serde_json::to_value(&announcement).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "peer admit: {peer_admit}");

    // Sponsor admits the peer advertisement via public open HTTP (no account).
    // Use Core-time admission on the live tools so lease checks use `now`, then
    // also hit the public sample endpoint over HTTP for contract coverage.
    sponsor
        .admit_discovery_peer_advertisement_at(peer_ad.clone(), now)
        .unwrap();
    let (status, sample) = client_json(
        &client,
        reqwest::Method::GET,
        &format!("{sponsor_base}/bootstrap/peer-advertisements?limit=8"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "peer sample: {sample}");
    assert!(
        sample["advertisements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|ad| ad["public_endpoint"] == peer_base),
        "sponsor sample must include the opted-in peer: {sample}"
    );

    // ── Fresh Home Node ─────────────────────────────────────────────────────
    let home = AgentTools::initialize_home_node(home_dir.path(), seed_store).unwrap();
    let home_admin = register_harness(&home, "home admin", vec![HarnessCapability::Administration]);
    let home_reader = register_harness(
        &home,
        "home reader",
        vec![
            HarnessCapability::FeedRead,
            HarnessCapability::Feedback,
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::PersonalDiscoveryManagement,
            HarnessCapability::PersonalDiscoveryExecution,
        ],
    );

    // Sponsored default is present and removable (node ops are a CLI surface).
    let endpoints = home.list_bootstrap_endpoints(&home_admin.ctx).unwrap();
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].base_url, DEFAULT_SPONSORED_BOOTSTRAP_URL);
    assert!(endpoints[0].is_sponsored_default);

    // Replace sponsored default with the live multi-node sponsor.
    clear_default_bootstrap(&home, &home_admin.ctx);
    home.add_bootstrap_endpoint(
        &home_admin.ctx,
        "test-sponsor",
        &sponsor_base,
        chrono::Utc::now(),
    )
    .unwrap();

    // Private evidence on Home must never leave the node.
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec!["distributed systems research".into()]);
    home.update_taste_profile(&home_reader.ctx, taste).unwrap();

    // Outbound cursor-sync via public HTTP (spawn_blocking avoids nested runtime).
    let handle = tokio::runtime::Handle::current();
    let stream_client = ReqwestAnnouncementStreamClient::new(handle.clone());
    let home_for_sync = home.clone();
    let admin_ctx = home_admin.ctx.clone();
    let sync_report = tokio::task::spawn_blocking(move || {
        home_for_sync.sync_bootstrap_endpoints(&admin_ctx, &stream_client, now)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(
        sync_report.outcomes.iter().any(|e| e.ok),
        "bootstrap sync should succeed: {sync_report:?}"
    );
    assert!(sync_report.retained_announcements >= 1);

    // Cursor advanced; idempotent re-sync retains zero new rows.
    let cursor_before = home.bootstrap_status(&home_admin.ctx).unwrap()[0]
        .sync
        .cursor
        .clone();
    assert!(cursor_before.is_some());
    let home_for_sync = home.clone();
    let admin_ctx = home_admin.ctx.clone();
    let stream_client = ReqwestAnnouncementStreamClient::new(handle.clone());
    let again = tokio::task::spawn_blocking(move || {
        home_for_sync.sync_bootstrap_endpoints(&admin_ctx, &stream_client, now)
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(again.retained_announcements, 0);
    assert_eq!(
        home.bootstrap_status(&home_admin.ctx).unwrap()[0]
            .sync
            .cursor,
        cursor_before
    );

    // Sponsor store must not hold private Home evidence.
    {
        let binding = sponsor.store();
        let store = binding.read().unwrap();
        for prefs in store.user_preferences.values() {
            assert!(
                !prefs
                    .interests
                    .iter()
                    .any(|i| i.contains("distributed systems research")),
                "sponsor must not observe Home Taste Profile interests"
            );
        }
        assert!(
            store.subscriptions.is_empty(),
            "sponsor must not hold Home Subscriptions"
        );
    }

    // Local match + explain through the Explore surface (CLI `pod explore`).
    let explore = serde_json::to_value(
        home.explore_public_pods(
            &home_reader.ctx,
            ExploreRequest::new("distributed systems", 10, 5).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let results = explore["results"].as_array().unwrap();
    assert!(
        !results.is_empty(),
        "expected locally matched Pod: {explore}"
    );
    let matched = results
        .iter()
        .find(|r| r["announcement"]["pod_slug"] == "sponsored-systems")
        .expect("primary pod in explore");
    assert!(!matched["reasons"].as_array().unwrap().is_empty());
    assert!(matched["relevance"].as_f64().unwrap() > 0.0);

    // Preview: Origin-signed samples accepted locally (trial exposure path).
    let samples = origin
        .pod_explore_samples(&origin_curator.ctx, &announcement, 5)
        .unwrap();
    home.accept_pod_explore_samples(&home_reader.ctx, samples)
        .unwrap();
    let explored = home
        .explore_public_pods(
            &home_reader.ctx,
            ExploreRequest::new("distributed systems research", 10, 5).unwrap(),
        )
        .unwrap();
    let trial = explored
        .results
        .iter()
        .find(|r| r.announcement.pod_slug == "sponsored-systems")
        .expect("trial candidate");
    assert!(trial.endorsements.is_empty());
    assert!(trial.trial_exposure);
    assert!(trial.reasons.iter().any(|r| r.contains("trial exposure")));
    assert!(!trial.sample_content_references.is_empty());

    // Subscribe via direct Pod URL (canonical addressing, no sponsor required).
    let synchronized = stumble_sync::subscribe_pod_from_url(&home, &home_reader.ctx, &primary_url)
        .await
        .unwrap();
    assert!(synchronized.imported_events >= 1);
    let subscribed = home
        .explore_public_pods(
            &home_reader.ctx,
            ExploreRequest::new("distributed systems", 10, 0).unwrap(),
        )
        .unwrap();
    let sub_row = subscribed
        .results
        .iter()
        .find(|r| r.announcement.pod_slug == "sponsored-systems")
        .unwrap();
    assert!(sub_row.is_subscribed);

    // ── Learn Discovery Peer (seeded selection) while sponsor is up ─────────
    let sample_client = ReqwestPeerAdvertisementSampleClient::new(handle.clone());
    let home_for_learn = home.clone();
    let admin_ctx = home_admin.ctx.clone();
    let selected = tokio::task::spawn_blocking(move || {
        home_for_learn.learn_and_select_discovery_peers(&admin_ctx, &sample_client, now, 7)
    })
    .await
    .unwrap()
    .unwrap()
    .selected;
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].public_endpoint, peer_base);
    // Learning never creates a Trusted Peer.
    assert!(!home
        .store()
        .read()
        .unwrap()
        .trusted_peers
        .values()
        .any(|p| p.node_id == peer_node_identity.id));

    // ── Make sponsor unavailable; receive a new announcement through peer ───
    drop(sponsor_server);

    let second = create_public_pod(
        &origin,
        "peer-only-systems",
        "Peer delivered distributed systems catalog",
    );
    accept_public_item(
        &origin,
        &second,
        "https://research.example/peer-only",
        "Peer-only systems note",
    );
    let second_url = format!("{origin_base}/federation/pods/{}", second.slug);
    let second_announcement = origin
        .pod_announcement_at(&origin_curator.ctx, &second.slug, &second_url, now)
        .unwrap();
    origin_probe_peer.set_announcement(&second_announcement);

    // Peer open-admits the new announcement without the sponsor.
    let (status, peer_second) = client_json(
        &client,
        reqwest::Method::POST,
        &format!("{peer_base}/bootstrap/announcements"),
        None,
        Some(serde_json::to_value(&second_announcement).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "peer second admit: {peer_second}");

    let peer_stream_client = ReqwestDiscoveryPeerStreamClient::new(handle.clone());
    let home_for_peer = home.clone();
    let admin_ctx = home_admin.ctx.clone();
    let peer_sync = tokio::task::spawn_blocking(move || {
        home_for_peer.sync_outbound_discovery_peers(&admin_ctx, &peer_stream_client, now)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(
        peer_sync.retained_announcements >= 1,
        "peer sync should retain new announcement: {peer_sync:?}"
    );
    {
        let binding = home.store();
        let store = binding.read().unwrap();
        let known = store
            .known_pod_announcements
            .get(&(
                second_announcement.origin_node_id,
                second_announcement.pod_slug.clone(),
            ))
            .expect("second announcement learned via peer");
        assert!(known
            .received_from_discovery_peer_endpoints
            .contains(&peer_base));
        assert!(known.received_from_bootstrap_urls.is_empty());
    }

    // Bootstrap sync now fails transport; previously learned state remains.
    let stream_client = ReqwestAnnouncementStreamClient::new(handle.clone());
    let home_for_sync = home.clone();
    let admin_ctx = home_admin.ctx.clone();
    let failed_boot = tokio::task::spawn_blocking(move || {
        home_for_sync.sync_bootstrap_endpoints(&admin_ctx, &stream_client, now)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(failed_boot.outcomes.iter().all(|e| !e.ok));
    assert!(home
        .store()
        .read()
        .unwrap()
        .known_pod_announcements
        .contains_key(&(announcement.origin_node_id, announcement.pod_slug.clone())));

    // Established Home still has a viable outbound peer after sponsor outage.
    let status = home.discovery_status(&home_admin.ctx).unwrap();
    assert!(
        status.active_outbound_peer_count > 0,
        "established Home should keep outbound peers during sponsor outage: {status:?}"
    );
    assert!(
        status.message.contains("direct") || !status.degraded,
        "status should remain inspectable: {status:?}"
    );

    // Direct Pod URL still works with sponsor down.
    let second_sync = stumble_sync::subscribe_pod_from_url(&home, &home_reader.ctx, &second_url)
        .await
        .unwrap();
    assert!(second_sync.imported_events >= 1);

    // ── Restart recovery: cursors and announcements survive SQLite reopen ───
    let cursor_peer = home.outbound_discovery_peers(&home_admin.ctx).unwrap()[0]
        .sync
        .cursor
        .clone();
    let cursor_boot = home.bootstrap_status(&home_admin.ctx).unwrap()[0]
        .sync
        .cursor
        .clone();
    drop(home);
    let home = AgentTools::open_initialized_home_node(home_dir.path()).unwrap();
    let home_admin = register_harness(
        &home,
        "home admin reopen",
        vec![HarnessCapability::Administration],
    );
    let home_reader = register_harness(
        &home,
        "home reader reopen",
        vec![HarnessCapability::FeedRead],
    );
    assert_eq!(
        home.bootstrap_status(&home_admin.ctx).unwrap()[0]
            .sync
            .cursor,
        cursor_boot
    );
    assert_eq!(
        home.outbound_discovery_peers(&home_admin.ctx).unwrap()[0]
            .sync
            .cursor,
        cursor_peer
    );
    assert_eq!(
        home.explore_public_pods(
            &home_reader.ctx,
            ExploreRequest::new("distributed", 10, 0).unwrap(),
        )
        .unwrap()
        .results
        .len(),
        2
    );

    // Keep servers alive until the end of the test.
    drop(origin_server);
    drop(peer_server);
}

// ─── Lifecycle, rejection, block, index policy, peer eviction ────────────────

#[tokio::test]
async fn multi_node_renewal_withdrawal_rejections_block_index_policy_and_eviction() {
    let now = Utc.with_ymd_and_hms(2026, 11, 2, 12, 0, 0).unwrap();
    let origin_dir = TestDataDir::new("life-origin");
    let sponsor_dir = TestDataDir::new("life-sponsor");
    let home_dir = TestDataDir::new("life-home");

    let origin = AgentTools::open_home_node(origin_dir.path(), seed_store).unwrap();
    let origin_curator = register_harness(
        &origin,
        "life curator",
        vec![HarnessCapability::PodCuration],
    );
    let pod = create_public_pod(
        &origin,
        "lifecycle-systems",
        "Lifecycle distributed systems topic",
    );
    accept_public_item(
        &origin,
        &pod,
        "https://research.example/lifecycle",
        "Lifecycle systems article",
    );

    let origin_server =
        EphemeralHttpServer::start_with(|base| router_with_base_url(origin.clone(), base)).await;
    let origin_base = origin_server.base_url.clone();
    let pod_url = format!("{origin_base}/federation/pods/{}", pod.slug);

    let announcement = origin
        .pod_announcement_at(&origin_curator.ctx, &pod.slug, &pod_url, now)
        .unwrap();

    let probe = Arc::new(ScriptedMatchingOriginProbe::default());
    probe.set_announcement(&announcement);
    let sponsor = AgentTools::open_home_node(sponsor_dir.path(), seed_store)
        .unwrap()
        .with_bootstrap_capability(true, probe.clone())
        .with_index_capability(true);
    let sponsor_server =
        EphemeralHttpServer::start_with(|base| router_with_base_url(sponsor.clone(), base)).await;
    let sponsor_base = sponsor_server.base_url.clone();
    let client = reqwest::Client::new();

    // Admit
    let (status, body) = client_json(
        &client,
        reqwest::Method::POST,
        &format!("{sponsor_base}/bootstrap/announcements"),
        None,
        Some(serde_json::to_value(&announcement).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["outcome"], "admitted");

    // Renewal: later announcement for same pod → renewed outcome + stream entry.
    let renewed_at = now + Duration::hours(2);
    let renewed = origin
        .pod_announcement_at(&origin_curator.ctx, &pod.slug, &pod_url, renewed_at)
        .unwrap();
    probe.set_announcement(&renewed);
    let (status, body) = client_json(
        &client,
        reqwest::Method::POST,
        &format!("{sponsor_base}/bootstrap/announcements"),
        None,
        Some(serde_json::to_value(&renewed).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "renew: {body}");
    assert_eq!(body["outcome"], "renewed");

    // Malformed signature rejection.
    let mut forged = renewed.clone();
    forged.signature = "not-a-real-signature".into();
    let (status, body) = client_json(
        &client,
        reqwest::Method::POST,
        &format!("{sponsor_base}/bootstrap/announcements"),
        None,
        Some(serde_json::to_value(&forged).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_signature");

    // Incompatible protocol rejection (unsigned re-sign with bad version is hard;
    // submit a structurally valid body with wrong protocol on the signer field after
    // tampering bytes so verification fails as invalid_signature or incompatible).
    let mut incompatible = renewed.clone();
    incompatible.signer.supported_protocol_version = "stumble/0.1".into();
    // Signature no longer matches mutated signer → typed reject.
    let (status, body) = client_json(
        &client,
        reqwest::Method::POST,
        &format!("{sponsor_base}/bootstrap/announcements"),
        None,
        Some(serde_json::to_value(&incompatible).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let code = body["code"].as_str().unwrap_or("");
    assert!(
        code == "invalid_signature" || code == "incompatible_protocol",
        "expected signature/protocol reject, got {body}"
    );

    // Expired lease rejection via deterministic clock on Core admit path
    // (HTTP always uses wall clock; Core path proves lease check).
    // Use a dedicated Origin so we never compete with a fresher retained lease.
    let expired_origin_dir = TestDataDir::new("expired-origin");
    let expired_origin = AgentTools::open_home_node(expired_origin_dir.path(), seed_store).unwrap();
    let expired_curator = register_harness(
        &expired_origin,
        "expired curator",
        vec![HarnessCapability::PodCuration],
    );
    let expired_pod = create_public_pod(
        &expired_origin,
        "expired-systems",
        "Expired lease distributed systems",
    );
    let expired_at = now - announcement_lease_duration() - Duration::days(1);
    let expired = expired_origin
        .pod_announcement_at(
            &expired_curator.ctx,
            &expired_pod.slug,
            "https://origin.example/federation/pods/expired-systems",
            expired_at,
        )
        .unwrap();
    let expiry_dir = TestDataDir::new("expiry-sponsor");
    let expiry_probe = Arc::new(ScriptedMatchingOriginProbe::default());
    expiry_probe.set_announcement(&expired);
    let expiry_sponsor = AgentTools::open_home_node(expiry_dir.path(), seed_store)
        .unwrap()
        .with_bootstrap_capability(true, expiry_probe);
    let err = expiry_sponsor
        .admit_bootstrap_announcement_at(expired, now)
        .unwrap_err();
    match err {
        AgentToolsError::BootstrapRejected { reason, .. } => {
            assert_eq!(
                reason,
                BootstrapAdmissionRejectionReason::StaleLease,
                "expected stale lease for expired announcement"
            );
        }
        other => panic!("expected bootstrap reject, got {other:?}"),
    }

    // Signed withdrawal ends new discovery on sponsor.
    let withdrawal = origin
        .withdraw_public_pod(
            &origin_curator.ctx,
            &pod.slug,
            Some(&pod_url),
            false,
            now + Duration::hours(3),
        )
        .unwrap();
    let (status, body) = client_json(
        &client,
        reqwest::Method::POST,
        &format!("{sponsor_base}/bootstrap/withdrawals"),
        None,
        Some(serde_json::to_value(&withdrawal).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "withdrawal: {body}");
    let (status, search) = client_json(
        &client,
        reqwest::Method::GET,
        &format!("{sponsor_base}/discovery/announcements?q=lifecycle&limit=10"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        search["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["announcement"]["pod_slug"] != "lifecycle-systems"),
        "withdrawn pod must leave index: {search}"
    );

    // ── Home: Index score cannot override local Trust Policy block ──────────
    // Re-publish a fresh pod for the policy portion.
    let policy_pod = create_public_pod(
        &origin,
        "policy-systems",
        "Policy distributed systems research",
    );
    let policy_url = format!("{origin_base}/federation/pods/{}", policy_pod.slug);
    let policy_ann = origin
        .pod_announcement_at(
            &origin_curator.ctx,
            &policy_pod.slug,
            &policy_url,
            now + Duration::hours(4),
        )
        .unwrap();
    probe.set_announcement(&policy_ann);
    let (status, _) = client_json(
        &client,
        reqwest::Method::POST,
        &format!("{sponsor_base}/bootstrap/announcements"),
        None,
        Some(serde_json::to_value(&policy_ann).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Also index path for remote score evidence.
    let (status, _) = client_json(
        &client,
        reqwest::Method::POST,
        &format!("{sponsor_base}/discovery/announcements"),
        None,
        Some(serde_json::to_value(&policy_ann).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let home = AgentTools::initialize_home_node(home_dir.path(), seed_store).unwrap();
    let home_admin = register_harness(
        &home,
        "policy admin",
        vec![HarnessCapability::Administration],
    );
    let home_reader = register_harness(&home, "policy reader", vec![HarnessCapability::FeedRead]);
    clear_default_bootstrap(&home, &home_admin.ctx);
    home.add_bootstrap_endpoint(&home_admin.ctx, "sponsor", &sponsor_base, now)
        .unwrap();
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::AddIndexNode {
            label: "sponsor-index".into(),
            base_url: sponsor_base.clone(),
        },
    );

    let handle = tokio::runtime::Handle::current();
    let index_client = ReqwestIndexSearchClient::new(handle.clone());
    let home_for_idx = home.clone();
    let reader_ctx = home_reader.ctx.clone();
    let before_block = tokio::task::spawn_blocking(move || {
        home_for_idx.explore_public_pods_with_indexes(
            &reader_ctx,
            ExploreRequest::new("policy distributed systems", 10, 0).unwrap(),
            &index_client,
        )
    })
    .await
    .unwrap()
    .unwrap();
    assert!(
        before_block
            .results
            .iter()
            .any(|r| r.announcement.pod_slug == "policy-systems"),
        "index import should surface policy pod before block"
    );

    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockPod {
            origin_node_id: policy_ann.origin_node_id,
            pod_slug: "policy-systems".into(),
        },
    );
    let index_client = ReqwestIndexSearchClient::new(handle.clone());
    let home_for_idx = home.clone();
    let reader_ctx = home_reader.ctx.clone();
    let after_block = tokio::task::spawn_blocking(move || {
        home_for_idx.explore_public_pods_with_indexes(
            &reader_ctx,
            ExploreRequest::new("policy distributed systems", 10, 0).unwrap(),
            &index_client,
        )
    })
    .await
    .unwrap()
    .unwrap();
    assert!(
        after_block
            .results
            .iter()
            .all(|r| r.announcement.pod_slug != "policy-systems"),
        "remote Index score must not override local Trust Policy block"
    );

    // ── Peer eviction: forged stream entries evict the peer ─────────────────
    let bad_peer_now = now;
    let (bad_peer_node, bad_ad) = {
        let mut store = InMemoryStore::default();
        let node = create_node_identity("bad-peer", None);
        store.node_identities.insert(node.id, node.clone());
        let ad = enable_discovery_peer_service(
            &mut store,
            &node,
            "https://bad-peer.example",
            &FixedDiscoveryPeerProbe::matching_node(&node),
            bad_peer_now,
        )
        .unwrap();
        (node, ad)
    };
    {
        let store = home.store();
        let mut guard = store.write().unwrap();
        learn_discovery_peer_advertisement(
            &mut guard,
            bad_ad,
            None,
            Some(&FixedDiscoveryPeerProbe::matching_node(&bad_peer_node)),
            bad_peer_now,
        )
        .unwrap();
        select_outbound_discovery_peers(&mut guard, None, bad_peer_now, 1);
    }
    let mut forged_stream = ScriptedDiscoveryPeerStreamClient::new();
    let mut forged_ann = policy_ann.clone();
    forged_ann.signature = "forged".into();
    forged_stream.push_page(
        "https://bad-peer.example",
        None,
        AnnouncementStreamPage::new(
            vec![AnnouncementStreamEntry::new(
                1,
                bad_peer_now,
                AnnouncementStreamEventKind::Admitted,
                forged_ann.origin_node_id,
                forged_ann.pod_slug.clone(),
                AnnouncementStreamPayload::Announcement(forged_ann),
            )],
            None,
            50,
        ),
    );
    let report = home
        .sync_outbound_discovery_peers(&home_admin.ctx, &forged_stream, bad_peer_now)
        .unwrap();
    assert!(report.evicted.contains(&bad_peer_node.id));
    assert!(!home
        .outbound_discovery_peers(&home_admin.ctx)
        .unwrap()
        .iter()
        .any(|p| p.peer.node_id == bad_peer_node.id));

    drop(origin_server);
    drop(sponsor_server);
}
