//! Opt-in Discovery Peer service acceptance (ticket 06).
//!
//! Covers outbound-only default, enable/disable operator surface, signed
//! advertisements, Bootstrap admission, peer stream/sample serving, privacy
//! bounds, and SQLite restart of opt-in state.

use chrono::{Duration, TimeZone, Utc};
use std::sync::Arc;
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-discovery-peer-{label}-{}",
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

fn admin(tools: &AgentTools) -> AuthContext {
    harness(
        tools,
        "discovery peer admin",
        vec![HarnessCapability::Administration],
    )
}

fn local_node(tools: &AgentTools) -> NodeIdentity {
    let binding = tools.store();
    let store = binding.read().unwrap();
    // Match enable_discovery_peer_service: tenant-less Home Node identity.
    store
        .node_identities
        .values()
        .find(|node| node.tenant_id.is_none())
        .cloned()
        .expect("local node identity")
}

fn matching_probe(tools: &AgentTools) -> Arc<dyn DiscoveryPeerProbe> {
    Arc::new(FixedDiscoveryPeerProbe::matching_node(&local_node(tools)))
}

fn create_public_pod(tools: &AgentTools, slug: &str, description: &str) -> Pod {
    let proposer = harness(
        tools,
        &format!("proposer-{slug}"),
        vec![HarnessCapability::PodCuration],
    );
    let approver = harness(
        tools,
        &format!("approver-{slug}"),
        vec![HarnessCapability::Approval],
    );
    let pod = tools
        .create_pod(
            &proposer,
            CreatePodRequest {
                name: slug.replace('-', " "),
                slug: slug.into(),
                description: description.into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let now = Utc::now();
    let proposal = tools
        .create_pending_proposal(
            &proposer,
            SensitiveChange::PublishPod { pod_id: pod.id },
            now,
            now + Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

fn sample_announcement(
    origin: &AgentTools,
    slug: &str,
    announced_at: chrono::DateTime<chrono::Utc>,
) -> PodAnnouncement {
    create_public_pod(origin, slug, &format!("{slug} subject"));
    let ctx = origin.default_auth_context().unwrap();
    let url = format!("https://origin.example/federation/pods/{slug}");
    origin
        .pod_announcement_at(&ctx, slug, &url, announced_at)
        .unwrap()
}

#[test]
fn newly_initialized_home_node_is_outbound_only_for_discovery() {
    let dir = TestDataDir::new("outbound-default");
    let home = AgentTools::open_home_node(&dir.0, seed_store).unwrap();
    assert!(!home.discovery_peer_service_enabled());
    assert!(!home.bootstrap_enabled());
    assert!(!home.index_enabled());

    let owner = home.default_auth_context().unwrap();
    let well_known = home
        .well_known_node(&owner, "https://home.example")
        .unwrap();
    assert!(!well_known
        .endpoints
        .contains_key("discovery_peer_announcement_stream"));
    assert!(!well_known
        .endpoints
        .contains_key("discovery_peer_advertisement_sample"));

    let err = home.peer_announcement_stream(None, Some(10)).unwrap_err();
    match err {
        AgentToolsError::DiscoveryPeerRejected { reason, .. } => {
            assert_eq!(
                reason,
                DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled
            );
        }
        other => panic!("expected peer disabled, got {other:?}"),
    }

    let status = home.discovery_peer_service_status(&admin(&home)).unwrap();
    assert!(!status.enabled);
    assert!(status.current_advertisement.is_none());
}

#[test]
fn authorized_user_can_enable_and_disable_announcement_serving() {
    let dir = TestDataDir::new("enable-disable");
    let home = {
        let opened = AgentTools::open_home_node(&dir.0, seed_store).unwrap();
        let probe = matching_probe(&opened);
        opened.with_discovery_peer_probe(probe)
    };
    let admin = admin(&home);
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();

    let ad = home
        .enable_discovery_peer_service(&admin, "https://peer.example", now)
        .unwrap();
    assert!(ad.verify().unwrap());
    assert_eq!(ad.capability, DiscoveryPeerCapability::AnnouncementServing);
    assert!(home.discovery_peer_service_enabled());

    let owner = home.default_auth_context().unwrap();
    let well_known = home
        .well_known_node(&owner, "https://peer.example")
        .unwrap();
    assert!(well_known
        .endpoints
        .contains_key("discovery_peer_announcement_stream"));
    assert!(well_known
        .endpoints
        .contains_key("discovery_peer_advertisement_sample"));

    let state = home
        .disable_discovery_peer_service(&admin, now + Duration::minutes(1))
        .unwrap();
    assert!(!state.enabled);
    assert!(state.current_advertisement.is_none());
    assert!(!home.discovery_peer_service_enabled());

    let well_known = home
        .well_known_node(&owner, "https://peer.example")
        .unwrap();
    assert!(!well_known
        .endpoints
        .contains_key("discovery_peer_announcement_stream"));
}

#[test]
fn enable_requires_endpoint_identity_protocol_https_and_reachability() {
    let dir = TestDataDir::new("enable-preconditions");
    let home = AgentTools::open_home_node(&dir.0, seed_store)
        .unwrap()
        .with_discovery_peer_probe(Arc::new(UnreachableDiscoveryPeerProbe));
    let admin = admin(&home);
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();

    let err = home
        .enable_discovery_peer_service(&admin, "http://peer.example", now)
        .unwrap_err();
    match err {
        AgentToolsError::DiscoveryPeerRejected { reason, .. } => {
            assert_eq!(
                reason,
                DiscoveryPeerAdmissionRejectionReason::InsecureEndpoint
            );
        }
        other => panic!("expected insecure, got {other:?}"),
    }

    let err = home
        .enable_discovery_peer_service(&admin, "https://10.0.0.8", now)
        .unwrap_err();
    match err {
        AgentToolsError::DiscoveryPeerRejected { reason, .. } => {
            assert_eq!(
                reason,
                DiscoveryPeerAdmissionRejectionReason::PrivateEndpoint
            );
        }
        other => panic!("expected private, got {other:?}"),
    }

    let err = home
        .enable_discovery_peer_service(&admin, "https://peer.example", now)
        .unwrap_err();
    match err {
        AgentToolsError::DiscoveryPeerRejected { reason, .. } => {
            assert_eq!(
                reason,
                DiscoveryPeerAdmissionRejectionReason::UnreachableEndpoint
            );
        }
        other => panic!("expected unreachable, got {other:?}"),
    }
    assert!(!home.discovery_peer_service_enabled());
}

#[test]
fn bootstrap_admits_valid_peer_ads_and_rejects_invalid() {
    let peer_dir = TestDataDir::new("admit-peer");
    let bootstrap_dir = TestDataDir::new("admit-bootstrap");
    let peer = {
        let opened = AgentTools::open_home_node(&peer_dir.0, seed_store).unwrap();
        let probe = matching_probe(&opened);
        opened.with_discovery_peer_probe(probe)
    };
    let peer_node = local_node(&peer);
    let bootstrap = AgentTools::open_home_node(&bootstrap_dir.0, seed_store)
        .unwrap()
        .with_bootstrap_capability(true, Arc::new(UnreachableOriginProbe))
        .with_discovery_peer_probe(Arc::new(FixedDiscoveryPeerProbe::matching_node(&peer_node)));

    let now = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    let ad = peer
        .enable_discovery_peer_service(&admin(&peer), "https://peer.example", now)
        .unwrap();

    let accepted = bootstrap
        .admit_discovery_peer_advertisement_at(ad.clone(), now)
        .unwrap();
    assert_eq!(accepted.outcome, BootstrapAdmissionOutcomeKind::Admitted);

    // Forged.
    let mut forged = ad.clone();
    forged.signature = "forged".into();
    let err = bootstrap
        .admit_discovery_peer_advertisement_at(forged, now)
        .unwrap_err();
    match err {
        AgentToolsError::DiscoveryPeerRejected { reason, .. } => {
            assert_eq!(
                reason,
                DiscoveryPeerAdmissionRejectionReason::InvalidSignature
            );
        }
        other => panic!("expected forged reject, got {other:?}"),
    }

    // Stale.
    let stale = now + peer_advertisement_lease_duration() + Duration::seconds(1);
    let err = bootstrap
        .admit_discovery_peer_advertisement_at(ad.clone(), stale)
        .unwrap_err();
    match err {
        AgentToolsError::DiscoveryPeerRejected { reason, .. } => {
            assert_eq!(reason, DiscoveryPeerAdmissionRejectionReason::StaleLease);
        }
        other => panic!("expected stale reject, got {other:?}"),
    }

    // Incompatible protocol.
    let node = local_node(&peer);
    let mut unsigned = ad.clone();
    unsigned.protocol_version = "stumble/0.1".into();
    unsigned.signer.supported_protocol_version = "stumble/0.1".into();
    unsigned.signature = String::new();
    let signed_bad = sign_discovery_peer_advertisement(&node, unsigned).unwrap();
    let err = bootstrap
        .admit_discovery_peer_advertisement_at(signed_bad, now)
        .unwrap_err();
    match err {
        AgentToolsError::DiscoveryPeerRejected { reason, .. } => {
            assert_eq!(
                reason,
                DiscoveryPeerAdmissionRejectionReason::IncompatibleProtocol
            );
        }
        other => panic!("expected protocol reject, got {other:?}"),
    }

    // Unreachable endpoint (fresh Bootstrap so admission is not short-circuited).
    let unreachable_dir = TestDataDir::new("admit-unreachable");
    let unreachable_bootstrap = AgentTools::open_home_node(&unreachable_dir.0, seed_store)
        .unwrap()
        .with_bootstrap_capability(true, Arc::new(UnreachableOriginProbe))
        .with_discovery_peer_probe(Arc::new(UnreachableDiscoveryPeerProbe));
    let err = unreachable_bootstrap
        .admit_discovery_peer_advertisement_at(ad, now)
        .unwrap_err();
    match err {
        AgentToolsError::DiscoveryPeerRejected { reason, .. } => {
            assert_eq!(
                reason,
                DiscoveryPeerAdmissionRejectionReason::UnreachableEndpoint
            );
        }
        other => panic!("expected unreachable reject, got {other:?}"),
    }
}

#[test]
fn enabled_peer_serves_streams_and_unranked_samples_without_private_surfaces() {
    let dir = TestDataDir::new("serve");
    let peer = {
        let opened = AgentTools::open_home_node(&dir.0, seed_store).unwrap();
        let probe = matching_probe(&opened);
        opened
            .with_discovery_peer_probe(probe)
            .with_bootstrap_capability(true, Arc::new(UnreachableOriginProbe))
    };
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    let admin = admin(&peer);
    let ad = peer
        .enable_discovery_peer_service(&admin, "https://peer.example", now)
        .unwrap();

    let origin_dir = TestDataDir::new("serve-origin");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let announcement = sample_announcement(&origin, "systems", now);
    let original = serde_json::to_vec(&announcement).unwrap();
    peer.project_peer_serving_announcement(&admin, announcement.clone(), now)
        .unwrap();

    let page = peer
        .peer_announcement_stream_at(None, Some(10), now)
        .unwrap();
    assert_eq!(page.entries.len(), 1);
    let served = page.entries[0].payload.as_announcement().unwrap();
    assert_eq!(serde_json::to_vec(served).unwrap(), original);
    assert_eq!(served.signature, announcement.signature);

    // Admit self + another peer for sample via a scripted probe that can match
    // each advertisement's identity in turn.
    let admit_probe = Arc::new(SimpleMatchingDiscoveryPeerProbe::from_advertisement(&ad));
    let peer = AgentTools::open_initialized_home_node(&dir.0)
        .unwrap()
        .with_bootstrap_capability(true, Arc::new(UnreachableOriginProbe))
        .with_discovery_peer_probe(admit_probe.clone());
    peer.admit_discovery_peer_advertisement_at(ad, now).unwrap();
    let other = create_node_identity("other", None);
    let mut other_store = InMemoryStore::default();
    other_store.node_identities.insert(other.id, other.clone());
    let other_ad = enable_discovery_peer_service(
        &mut other_store,
        &other,
        "https://other.example",
        &FixedDiscoveryPeerProbe::matching_node(&other),
        now,
    )
    .unwrap();
    admit_probe.set_identity(peer_identity_view_for_advertisement(&other_ad));
    peer.admit_discovery_peer_advertisement_at(other_ad, now)
        .unwrap();

    let sample = peer.peer_advertisement_sample_at(Some(10), 7, now).unwrap();
    assert_eq!(sample.advertisements.len(), 2);
    assert!(peer_advertisement_sample_is_public_only(&sample));
    let wire = serde_json::to_value(&sample).unwrap();
    for key in [
        "taste_profile",
        "subscription",
        "feedback",
        "credentials",
        "rank",
        "trust",
        "admin",
    ] {
        assert!(wire.get(key).is_none(), "sample must not expose {key}");
    }
}

#[test]
fn pure_home_node_enable_plus_retain_populates_peer_stream() {
    let dir = TestDataDir::new("pure-home-stream");
    let home = {
        let opened = AgentTools::open_home_node(&dir.0, seed_store).unwrap();
        let probe = matching_probe(&opened);
        // No bootstrap capability — pure Home Node.
        opened.with_discovery_peer_probe(probe)
    };
    assert!(!home.bootstrap_enabled());
    let admin = admin(&home);
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();

    home.enable_discovery_peer_service(&admin, "https://peer.example", now)
        .unwrap();

    let origin_dir = TestDataDir::new("pure-home-origin");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let announcement = sample_announcement(&origin, "systems", now);
    retain_verified_pod_announcement(
        &mut home.store().write().unwrap(),
        announcement.clone(),
        DeliveryProvenance::LOCAL,
        now,
    )
    .unwrap();
    // Persist after direct store mutation so stream is durable if needed.
    // Stream read is from live store.
    let page = home
        .peer_announcement_stream_at(None, Some(10), now)
        .unwrap();
    assert!(
        !page.entries.is_empty(),
        "pure Home Node peer stream must be non-empty after retain while enabled"
    );
    assert_eq!(
        page.entries[0].payload.as_announcement().unwrap().pod_slug,
        "systems"
    );
}

#[test]
fn combined_bootstrap_and_peer_streams_do_not_overwrite() {
    let dir = TestDataDir::new("combined-role");
    let origin_probe = Arc::new(ScriptedMatchingOriginProbe::default());
    let home = {
        let opened = AgentTools::open_home_node(&dir.0, seed_store).unwrap();
        let probe = matching_probe(&opened);
        opened
            .with_discovery_peer_probe(probe)
            .with_bootstrap_capability(true, origin_probe.clone())
    };
    let admin = admin(&home);
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    home.enable_discovery_peer_service(&admin, "https://peer.example", now)
        .unwrap();

    let origin_dir = TestDataDir::new("combined-origin");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let announcement = sample_announcement(&origin, "systems", now);
    origin_probe.set_announcement(&announcement);

    let admitted = home
        .admit_bootstrap_announcement_at(announcement.clone(), now)
        .unwrap();
    assert!(admitted.stream_sequence.is_some());
    // Peer stream also receives a projection via retain hook while peer is enabled.
    let peer_page = home
        .peer_announcement_stream_at(None, Some(10), now)
        .unwrap();
    let bootstrap_page = home.announcement_stream_at(None, Some(10), now).unwrap();
    assert!(!peer_page.entries.is_empty());
    assert!(!bootstrap_page.entries.is_empty());
    // Same sequence number may appear in both maps, but payloads live in separate
    // stores so Bootstrap admit cannot clobber peer stream rows (and vice versa).
    let bootstrap_seq = admitted.stream_sequence.unwrap();
    {
        let binding = home.store();
        let store = binding.read().unwrap();
        assert!(store
            .announcement_stream_entries
            .contains_key(&bootstrap_seq));
        assert!(store
            .discovery_peer_stream_entries
            .values()
            .any(|entry| entry.pod_slug == "systems"));
        // Peer map must still hold its entry even if sequence equals bootstrap's.
        if let Some(peer_entry) = store.discovery_peer_stream_entries.get(&bootstrap_seq) {
            assert_eq!(peer_entry.pod_slug, "systems");
        }
        assert_eq!(
            store
                .announcement_stream_entries
                .get(&bootstrap_seq)
                .unwrap()
                .pod_slug,
            "systems"
        );
    }
}

#[test]
fn disable_stops_renewal_and_inbound_without_affecting_outbound_bootstrap() {
    let dir = TestDataDir::new("disable-outbound");
    let home = {
        let opened = AgentTools::open_home_node(&dir.0, seed_store).unwrap();
        let probe = matching_probe(&opened);
        opened.with_discovery_peer_probe(probe)
    };
    let admin = admin(&home);
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();

    home.enable_discovery_peer_service(&admin, "https://peer.example", now)
        .unwrap();
    // Outbound bootstrap config still available while peer serving is enabled.
    let endpoints = home.list_bootstrap_endpoints(&admin).unwrap();
    assert!(!endpoints.is_empty());

    home.disable_discovery_peer_service(&admin, now).unwrap();
    // Outbound bootstrap config remains after disable.
    let endpoints_after = home.list_bootstrap_endpoints(&admin).unwrap();
    assert_eq!(endpoints_after.len(), endpoints.len());
    // Inbound peer serve rejects.
    assert!(matches!(
        home.peer_announcement_stream(None, Some(5)),
        Err(AgentToolsError::DiscoveryPeerRejected {
            reason: DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled,
            ..
        })
    ));
    // Renewal while disabled fails.
    assert!(matches!(
        home.renew_discovery_peer_advertisement(&admin, now),
        Err(AgentToolsError::DiscoveryPeerRejected {
            reason: DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled,
            ..
        })
    ));
}

#[test]
fn opt_in_state_ad_lease_and_serving_cursors_survive_sqlite_restart() {
    let dir = TestDataDir::new("restart");
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    let sequence = {
        let home = {
            let opened = AgentTools::open_home_node(&dir.0, seed_store).unwrap();
            let probe = matching_probe(&opened);
            opened.with_discovery_peer_probe(probe)
        };
        let admin = admin(&home);
        let ad = home
            .enable_discovery_peer_service(&admin, "https://peer.example", now)
            .unwrap();
        let origin_dir = TestDataDir::new("restart-origin");
        let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
        let announcement = sample_announcement(&origin, "systems", now);
        let seq = home
            .project_peer_serving_announcement(&admin, announcement, now)
            .unwrap();
        let status = home.discovery_peer_service_status(&admin).unwrap();
        assert!(status.enabled);
        assert_eq!(
            status.current_advertisement.as_ref().map(|a| a.id),
            Some(ad.id)
        );
        assert!(status.next_stream_sequence > seq);
        seq
    };

    let restarted = {
        let opened = AgentTools::open_initialized_home_node(&dir.0).unwrap();
        let probe = matching_probe(&opened);
        opened.with_discovery_peer_probe(probe)
    };
    assert!(restarted.discovery_peer_service_enabled());
    let admin = admin(&restarted);
    let status = restarted.discovery_peer_service_status(&admin).unwrap();
    assert!(status.enabled);
    assert_eq!(
        status.public_endpoint.as_deref(),
        Some("https://peer.example")
    );
    assert!(status.current_advertisement.is_some());
    assert!(status.next_stream_sequence > sequence);

    // Cursor resume continues after the previously served sequence.
    let page = restarted
        .peer_announcement_stream_at(Some(&sequence.to_string()), Some(10), now)
        .unwrap();
    assert!(page.entries.is_empty() || page.entries[0].sequence > sequence);
    // Full stream still serves the projected entry.
    let full = restarted
        .peer_announcement_stream_at(None, Some(10), now)
        .unwrap();
    assert_eq!(full.entries.len(), 1);
    assert_eq!(full.entries[0].sequence, sequence);
}
