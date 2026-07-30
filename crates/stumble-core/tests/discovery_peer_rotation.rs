//! Focused acceptance for outbound Discovery Peer rotation and Bootstrap outages.
//!
//! Drives Core entry points with temporary SQLite, scripted sample/stream
//! transports, and deterministic clocks. Covers learn/select, peer sync under
//! Bootstrap outage, multi-source provenance, eviction, disable gossip, degraded
//! discovery, cursor resume across restart, and direct Pod URL independence.

use chrono::{TimeZone, Utc};
use std::sync::Arc;
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-peer-rotation-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn harness(tools: &AgentTools, label: &str, capabilities: Vec<HarnessCapability>) -> AuthContext {
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
    tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap()
}

fn origin_tools() -> AgentTools {
    AgentTools::new(seed_store())
}

fn sample_announcement(
    origin: &AgentTools,
    slug: &str,
    announced_at: chrono::DateTime<Utc>,
) -> PodAnnouncement {
    let ctx = origin.default_auth_context().unwrap();
    let curator = harness(
        origin,
        &format!("curator-{slug}"),
        vec![HarnessCapability::PodCuration],
    );
    let private = origin
        .create_pod(
            &curator,
            CreatePodRequest {
                name: slug.replace('-', " "),
                slug: slug.into(),
                description: format!("{slug} subject"),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let proposal = origin
        .create_pending_proposal(
            &curator,
            SensitiveChange::PublishPod { pod_id: private.id },
            announced_at,
            announced_at + chrono::Duration::hours(1),
        )
        .unwrap();
    let approver = harness(
        origin,
        &format!("approver-{slug}"),
        vec![HarnessCapability::Approval],
    );
    origin
        .approve_pending_proposal(&approver, proposal.id, announced_at)
        .unwrap();
    let url = format!("https://origin.example/federation/pods/{slug}");
    origin
        .pod_announcement_at(&ctx, slug, &url, announced_at)
        .unwrap()
}

/// Builds a signed peer advertisement by enabling service on a temporary node store.
fn peer_ad(
    endpoint: &str,
    now: chrono::DateTime<Utc>,
) -> (NodeIdentity, DiscoveryPeerAdvertisement) {
    let mut store = InMemoryStore::default();
    let node = create_node_identity("serving-peer", None);
    store.node_identities.insert(node.id, node.clone());
    let ad = enable_discovery_peer_service(
        &mut store,
        &node,
        endpoint,
        &FixedDiscoveryPeerProbe::matching_node(&node),
        now,
    )
    .unwrap();
    (node, ad)
}

fn stream_page(
    announcement: &PodAnnouncement,
    now: chrono::DateTime<Utc>,
) -> AnnouncementStreamPage {
    AnnouncementStreamPage::new(
        vec![AnnouncementStreamEntry::new(
            1,
            now,
            AnnouncementStreamEventKind::Admitted,
            announcement.origin_node_id,
            announcement.pod_slug.clone(),
            AnnouncementStreamPayload::Announcement(announcement.clone()),
        )],
        None,
        50,
    )
}

fn clear_default_bootstrap(tools: &AgentTools, admin: &AuthContext) {
    let default_id = tools.list_bootstrap_endpoints(admin).unwrap()[0].id;
    tools.remove_bootstrap_endpoint(admin, default_id).unwrap();
}

fn home_with_peer_probe(dir: &std::path::Path, peer: &NodeIdentity) -> AgentTools {
    AgentTools::initialize_home_node(dir, seed_store)
        .unwrap()
        .with_discovery_peer_probe(Arc::new(FixedDiscoveryPeerProbe::matching_node(peer)))
}

#[test]
fn learns_peers_from_bootstrap_sample_and_syncs_while_bootstrap_down() {
    let dir = TestDataDir::new("bootstrap-outage");
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let (peer_node, ad) = peer_ad("https://peer.example", now);
    let tools = home_with_peer_probe(&dir.0, &peer_node);
    let admin = harness(
        &tools,
        "peer admin",
        vec![HarnessCapability::Administration],
    );
    clear_default_bootstrap(&tools, &admin);

    let boot = tools
        .add_bootstrap_endpoint(&admin, "primary", "https://boot.example", now)
        .unwrap();

    let mut samples = ScriptedPeerAdvertisementSampleClient::new();
    samples.push_sample(
        &boot.base_url,
        DiscoveryPeerAdvertisementSample::new(vec![ad], 8),
    );

    let selected = tools
        .learn_and_select_discovery_peers(&admin, &samples, now, 7)
        .unwrap()
        .selected;
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].node_id, peer_node.id);
    // Sample provenance is populated through learn → select.
    assert!(selected[0].learned_from.contains(&boot.base_url));
    // Automatic selection never creates a Trusted Peer for the Discovery Peer.
    assert!(!tools
        .store()
        .read()
        .unwrap()
        .trusted_peers
        .values()
        .any(|peer| peer.node_id == peer_node.id));

    // Bootstrap is unavailable for announcement streams; peer still delivers.
    let origin = origin_tools();
    let announcement = sample_announcement(&origin, "after-outage", now);
    let mut stream = ScriptedDiscoveryPeerStreamClient::new();
    stream.push_page(
        "https://peer.example",
        None,
        stream_page(&announcement, now),
    );
    let report = tools
        .sync_outbound_discovery_peers(&admin, &stream, now)
        .unwrap();
    assert_eq!(report.retained_announcements, 1);
    assert!(report.outcomes[0].ok);

    let store = tools.store();
    let guard = store.read().unwrap();
    let known = guard
        .known_pod_announcements
        .get(&(announcement.origin_node_id, "after-outage".into()))
        .unwrap();
    assert!(known
        .received_from_discovery_peer_endpoints
        .contains("https://peer.example"));
    assert!(known.received_from_bootstrap_urls.is_empty());
    assert_eq!(known.announcement.signature, announcement.signature);
}

#[test]
fn multi_source_survives_peer_eviction_and_disable_gossip_keeps_audit() {
    let dir = TestDataDir::new("multi-source");
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let (peer_node, ad) = peer_ad("https://peer.example", now);
    let tools = home_with_peer_probe(&dir.0, &peer_node);
    let admin = harness(&tools, "admin", vec![HarnessCapability::Administration]);
    let reader = harness(&tools, "reader", vec![HarnessCapability::FeedRead]);
    clear_default_bootstrap(&tools, &admin);

    let boot = tools
        .add_bootstrap_endpoint(&admin, "boot", "https://boot.example", now)
        .unwrap();

    {
        let store = tools.store();
        let mut guard = store.write().unwrap();
        learn_discovery_peer_advertisement(
            &mut guard,
            ad,
            Some(&boot.base_url),
            Some(&FixedDiscoveryPeerProbe::matching_node(&peer_node)),
            now,
        )
        .unwrap();
        select_outbound_discovery_peers(&mut guard, None, now, 1);
    }

    let origin = origin_tools();
    let shared = sample_announcement(&origin, "shared-rust", now);
    let sole = sample_announcement(&origin, "sole-peer-rust", now);
    {
        let store = tools.store();
        let mut guard = store.write().unwrap();
        retain_verified_pod_announcement(
            &mut guard,
            shared.clone(),
            DeliveryProvenance::bootstrap(boot.base_url.clone()),
            now,
        )
        .unwrap();
        retain_verified_pod_announcement(
            &mut guard,
            shared.clone(),
            DeliveryProvenance::discovery_peer("https://peer.example"),
            now,
        )
        .unwrap();
        retain_verified_pod_announcement(
            &mut guard,
            sole.clone(),
            DeliveryProvenance::discovery_peer("https://peer.example"),
            now,
        )
        .unwrap();
    }

    let before = tools
        .explore_public_pods(&reader, ExploreRequest::new("rust", 10, 0).unwrap())
        .unwrap();
    assert_eq!(before.results.len(), 2);

    // Evict the peer: sole-source leaves explore; multi-source stays.
    {
        let store = tools.store();
        let mut guard = store.write().unwrap();
        if let Some(state) = guard.discovery_peer_sync_states.get_mut(&peer_node.id) {
            state.health = DiscoveryPeerHealth::Evicted;
        }
        guard.outbound_discovery_peers.remove(&peer_node.id);
    }
    let after = tools
        .explore_public_pods(&reader, ExploreRequest::new("rust", 10, 0).unwrap())
        .unwrap();
    assert_eq!(after.results.len(), 1);
    assert_eq!(after.results[0].announcement.pod_slug, "shared-rust");

    // Disable gossip: audit remains; Bootstrap path unaffected.
    let config = tools
        .set_automatic_peer_gossip_enabled(&admin, false, now)
        .unwrap();
    assert!(!config.automatic_gossip_enabled);
    assert!(tools
        .store()
        .read()
        .unwrap()
        .known_discovery_peer_advertisements
        .contains_key(&peer_node.id));
    assert!(tools
        .store()
        .read()
        .unwrap()
        .known_pod_announcements
        .contains_key(&(sole.origin_node_id, sole.pod_slug.clone())));
    assert_eq!(tools.list_bootstrap_endpoints(&admin).unwrap().len(), 1);
}

#[test]
fn fresh_node_degraded_status_and_cursor_resume_survive_restart() {
    let dir = TestDataDir::new("restart");
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let (peer_node, ad) = peer_ad("https://peer.example", now);
    let tools = home_with_peer_probe(&dir.0, &peer_node);
    let admin = harness(&tools, "admin", vec![HarnessCapability::Administration]);

    // Fresh with only never-succeeded sponsored Bootstrap → degraded.
    let status = tools.discovery_status(&admin).unwrap();
    assert!(status.degraded);
    assert!(status.message.contains("direct Pod URL"));

    clear_default_bootstrap(&tools, &admin);
    let status = tools.discovery_status(&admin).unwrap();
    assert!(status.degraded);
    assert_eq!(
        status.degraded_reason.as_deref(),
        Some("no_enabled_bootstrap")
    );

    let mut samples = ScriptedPeerAdvertisementSampleClient::new();
    // Persist via public learn path using a Bootstrap URL that only serves peer samples.
    let boot = tools
        .add_bootstrap_endpoint(
            &admin,
            "sample-only",
            "https://sample.bootstrap.example",
            now,
        )
        .unwrap();
    samples.push_sample(
        &boot.base_url,
        DiscoveryPeerAdvertisementSample::new(vec![ad], 8),
    );
    let selected = tools
        .learn_and_select_discovery_peers(&admin, &samples, now, 3)
        .unwrap()
        .selected;
    assert_eq!(selected.len(), 1);

    let origin = origin_tools();
    let first = sample_announcement(&origin, "first-page", now);
    let mut page = stream_page(&first, now);
    page.next_cursor = Some("1".into());
    let mut stream = ScriptedDiscoveryPeerStreamClient::new();
    stream.push_page("https://peer.example", None, page);
    stream.push_page(
        "https://peer.example",
        Some("1"),
        AnnouncementStreamPage::new(vec![], None, 50),
    );
    let report = tools
        .sync_outbound_discovery_peers(&admin, &stream, now)
        .unwrap();
    assert!(report.outcomes[0].ok);
    assert_eq!(
        tools.outbound_discovery_peers(&admin).unwrap()[0]
            .sync
            .cursor
            .as_deref(),
        Some("1")
    );

    // Restart: cursors and outbound set survive (sync path; no async runtime hold).
    drop(tools);
    let reopened = AgentTools::open_initialized_home_node(&dir.0)
        .unwrap()
        .with_discovery_peer_probe(Arc::new(FixedDiscoveryPeerProbe::matching_node(&peer_node)));
    let admin = harness(
        &reopened,
        "admin-reopen",
        vec![HarnessCapability::Administration],
    );
    let statuses = reopened.outbound_discovery_peers(&admin).unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].sync.cursor.as_deref(), Some("1"));
    assert_eq!(statuses[0].sync.last_success_at, Some(now));
    assert!(reopened
        .store()
        .read()
        .unwrap()
        .known_pod_announcements
        .contains_key(&(first.origin_node_id, first.pod_slug.clone())));
}

#[test]
fn invalid_signature_evicts_and_does_not_create_trusted_peer() {
    let dir = TestDataDir::new("evict");
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let (peer_node, ad) = peer_ad("https://peer.example", now);
    let tools = home_with_peer_probe(&dir.0, &peer_node);
    let admin = harness(&tools, "admin", vec![HarnessCapability::Administration]);
    clear_default_bootstrap(&tools, &admin);

    {
        let store = tools.store();
        let mut guard = store.write().unwrap();
        learn_discovery_peer_advertisement(
            &mut guard,
            ad,
            None,
            Some(&FixedDiscoveryPeerProbe::matching_node(&peer_node)),
            now,
        )
        .unwrap();
        select_outbound_discovery_peers(&mut guard, None, now, 1);
    }

    let origin = origin_tools();
    let mut forged = sample_announcement(&origin, "forged", now);
    forged.signature = "forged".into();
    let mut stream = ScriptedDiscoveryPeerStreamClient::new();
    stream.push_page("https://peer.example", None, stream_page(&forged, now));
    let report = tools
        .sync_outbound_discovery_peers(&admin, &stream, now)
        .unwrap();
    assert!(!report.outcomes[0].ok);
    assert!(report.evicted.contains(&peer_node.id));
    assert!(tools.outbound_discovery_peers(&admin).unwrap().is_empty());
    assert!(!tools
        .store()
        .read()
        .unwrap()
        .trusted_peers
        .values()
        .any(|peer| peer.node_id == peer_node.id));
}

#[test]
fn multi_source_learned_from_and_relearn_after_transport_eviction() {
    let dir = TestDataDir::new("relearn");
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let (peer_node, ad) = peer_ad("https://peer.example", now);
    // Default Unreachable probe must not block learn (AgentTools passes None).
    let tools = AgentTools::initialize_home_node(&dir.0, seed_store).unwrap();
    let admin = harness(&tools, "admin", vec![HarnessCapability::Administration]);
    clear_default_bootstrap(&tools, &admin);

    let boot_a = tools
        .add_bootstrap_endpoint(&admin, "a", "https://boot-a.example", now)
        .unwrap();
    let boot_b = tools
        .add_bootstrap_endpoint(&admin, "b", "https://boot-b.example", now)
        .unwrap();

    let mut samples = ScriptedPeerAdvertisementSampleClient::new();
    samples.push_sample(
        &boot_a.base_url,
        DiscoveryPeerAdvertisementSample::new(vec![ad.clone()], 8),
    );
    samples.push_sample(
        &boot_b.base_url,
        DiscoveryPeerAdvertisementSample::new(vec![ad.clone()], 8),
    );

    let selected = tools
        .learn_and_select_discovery_peers(&admin, &samples, now, 5)
        .unwrap()
        .selected;
    assert_eq!(selected.len(), 1);
    assert!(selected[0].learned_from.contains(&boot_a.base_url));
    assert!(selected[0].learned_from.contains(&boot_b.base_url));

    // Transport failures until eviction.
    let mut stream = ScriptedDiscoveryPeerStreamClient::new();
    stream.fail(
        "https://peer.example",
        DiscoveryPeerSyncFailure::new(
            DiscoveryPeerSyncFailureKind::Transport,
            "connection refused",
        ),
    );
    let mut t = now;
    for _ in 0..PEER_FAILURES_BEFORE_EVICTION {
        // Clear backoff so each attempt runs.
        {
            let store = tools.store();
            let mut guard = store.write().unwrap();
            if let Some(state) = guard.discovery_peer_sync_states.get_mut(&peer_node.id) {
                state.backoff_until = None;
                if state.health == DiscoveryPeerHealth::BackedOff {
                    state.health = DiscoveryPeerHealth::Healthy;
                }
            }
        }
        let report = tools
            .sync_outbound_discovery_peers(&admin, &stream, t)
            .unwrap();
        let _ = report;
        t = t + chrono::Duration::seconds(1);
    }
    assert!(tools.outbound_discovery_peers(&admin).unwrap().is_empty());
    assert_eq!(
        tools
            .store()
            .read()
            .unwrap()
            .discovery_peer_sync_states
            .get(&peer_node.id)
            .map(|s| s.health),
        Some(DiscoveryPeerHealth::Evicted)
    );

    // Re-learn after eviction re-admits the peer with provenance.
    let reselected = tools
        .learn_and_select_discovery_peers(&admin, &samples, t, 5)
        .unwrap()
        .selected;
    assert_eq!(reselected.len(), 1);
    assert_eq!(reselected[0].node_id, peer_node.id);
    assert!(reselected[0].learned_from.contains(&boot_a.base_url));
    assert!(reselected[0].learned_from.contains(&boot_b.base_url));
}
