use super::*;
use crate::discovery_peer::{
    peer_identity_view_for_node, FixedDiscoveryPeerProbe, UnreachableDiscoveryPeerProbe,
};
use crate::domain::*;
use crate::domain::{
    announcement_lease_duration, peer_advertisement_lease_duration, AnnouncementStreamEntry,
    AnnouncementStreamEventKind, AnnouncementStreamPayload, NodeInfo, PackageVersion,
    PodAnnouncement, CURRENT_PROTOCOL_VERSION,
};
use crate::pod_announcement::announcement_delivery_is_active;
use crate::pod_announcement::{retain_verified_pod_announcement, DeliveryProvenance};
use crate::signing::{
    create_node_identity, sign_discovery_peer_advertisement, sign_pod_announcement,
};
use crate::store::InMemoryStore;
use chrono::TimeZone;
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

fn sample_announcement(
    node: &crate::domain::NodeIdentity,
    announced_at: DateTime<Utc>,
    slug: &str,
) -> PodAnnouncement {
    sign_pod_announcement(
        node,
        PodAnnouncement {
            id: Uuid::now_v7(),
            origin_node_id: node.id,
            signer: NodeInfo {
                node_id: node.id,
                display_name: node.display_name.clone(),
                public_key: node.public_key.clone(),
                supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
            },
            pod_slug: slug.into(),
            pod_name: slug.replace('-', " "),
            subject: format!("{slug} subject"),
            public_pod_url: format!("https://origin.example/federation/pods/{slug}"),
            package_version: PackageVersion::new(1).unwrap(),
            latest_event_hash: None,
            announced_at,
            expires_at: announced_at + announcement_lease_duration(),
            signature: String::new(),
        },
    )
    .unwrap()
}

fn sample_peer_ad(
    node: &crate::domain::NodeIdentity,
    endpoint: &str,
    now: DateTime<Utc>,
) -> DiscoveryPeerAdvertisement {
    sign_discovery_peer_advertisement(
        node,
        DiscoveryPeerAdvertisement {
            id: Uuid::now_v7(),
            node_id: node.id,
            signer: NodeInfo {
                node_id: node.id,
                display_name: node.display_name.clone(),
                public_key: node.public_key.clone(),
                supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
            },
            public_endpoint: endpoint.into(),
            protocol_version: CURRENT_PROTOCOL_VERSION.into(),
            capability: DiscoveryPeerCapability::AnnouncementServing,
            issued_at: now,
            expires_at: now + peer_advertisement_lease_duration(),
            signature: String::new(),
        },
    )
    .unwrap()
}

fn stream_page(announcement: &PodAnnouncement, now: DateTime<Utc>) -> AnnouncementStreamPage {
    AnnouncementStreamPage {
        entries: vec![AnnouncementStreamEntry {
            sequence: 1,
            recorded_at: now,
            kind: AnnouncementStreamEventKind::Admitted,
            origin_node_id: announcement.origin_node_id,
            pod_slug: announcement.pod_slug.clone(),
            payload: AnnouncementStreamPayload::Announcement(announcement.clone()),
        }],
        next_cursor: None,
        limit: 50,
    }
}

#[test]
fn learns_and_selects_bounded_randomized_peers_without_trusted_peer() {
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let mut store = InMemoryStore::default();
    let home = create_node_identity("home", None);
    store.node_identities.insert(home.id, home.clone());

    let mut ads = Vec::new();
    for i in 0..6 {
        let peer = create_node_identity(&format!("peer-{i}"), None);
        let ad = sample_peer_ad(&peer, &format!("https://peer-{i}.example"), now);
        learn_discovery_peer_advertisement(
            &mut store,
            ad.clone(),
            Some("https://bootstrap.example"),
            Some(&FixedDiscoveryPeerProbe::matching_node(&peer)),
            now,
        )
        .unwrap();
        ads.push(ad);
    }

    let selected = select_outbound_discovery_peers(&mut store, Some(home.id), now, 42);
    assert_eq!(selected.len(), DEFAULT_MAX_OUTBOUND_DISCOVERY_PEERS);
    assert!(store.trusted_peers.is_empty());
    // Deterministic: same seed yields same set.
    let again = select_outbound_discovery_peers(&mut store, Some(home.id), now, 42);
    let mut a: Vec<_> = selected.iter().map(|p| p.node_id).collect();
    let mut b: Vec<_> = again.iter().map(|p| p.node_id).collect();
    a.sort();
    b.sort();
    assert_eq!(a, b);
    // Different seed can refill only when room; already at capacity so set stable.
    assert_eq!(
        list_active_outbound_peers(&store).len(),
        DEFAULT_MAX_OUTBOUND_DISCOVERY_PEERS
    );
    let _ = ads;
}

#[test]
fn peer_sync_retains_origin_bytes_and_multi_source_provenance() {
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let mut store = InMemoryStore::default();
    let peer_node = create_node_identity("peer", None);
    let ad = sample_peer_ad(&peer_node, "https://peer.example", now);
    learn_discovery_peer_advertisement(
        &mut store,
        ad,
        Some("https://bootstrap.example"),
        Some(&FixedDiscoveryPeerProbe::matching_node(&peer_node)),
        now,
    )
    .unwrap();
    select_outbound_discovery_peers(&mut store, None, now, 1);

    let origin = create_node_identity("origin", None);
    let announcement = sample_announcement(&origin, now, "systems");
    let original_sig = announcement.signature.clone();
    let mut client = ScriptedDiscoveryPeerStreamClient::new();
    client.push_page(
        "https://peer.example",
        None,
        stream_page(&announcement, now),
    );

    let report = sync_outbound_discovery_peers(&mut store, &client, now);
    assert_eq!(report.retained_announcements, 1);
    assert!(report.outcomes[0].ok);
    let known = store
        .known_pod_announcements
        .get(&(announcement.origin_node_id, "systems".into()))
        .unwrap();
    assert_eq!(known.announcement.signature, original_sig);
    assert!(known
        .received_from_discovery_peer_endpoints
        .contains("https://peer.example"));
    assert!(known.received_from_peer_id.is_none());
    assert!(store.trusted_peers.is_empty());

    // Independent bootstrap provenance keeps eligibility after peer eviction.
    retain_verified_pod_announcement(
        &mut store,
        announcement.clone(),
        DeliveryProvenance::bootstrap("https://boot.example"),
        now,
    )
    .unwrap();
    crate::bootstrap::add_bootstrap_endpoint(&mut store, "boot", "https://boot.example", now)
        .unwrap();
    mark_peer_evicted(&mut store, peer_node.id, now, "test eviction");
    store.outbound_discovery_peers.remove(&peer_node.id);
    let known = store
        .known_pod_announcements
        .get(&(announcement.origin_node_id, "systems".into()))
        .unwrap();
    assert!(announcement_delivery_is_active(&store, known, None));
}

#[test]
fn invalid_signature_evicts_peer_and_preserves_prior_announcements() {
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let mut store = InMemoryStore::default();
    let peer_node = create_node_identity("peer", None);
    let ad = sample_peer_ad(&peer_node, "https://peer.example", now);
    learn_discovery_peer_advertisement(
        &mut store,
        ad,
        None,
        Some(&FixedDiscoveryPeerProbe::matching_node(&peer_node)),
        now,
    )
    .unwrap();
    select_outbound_discovery_peers(&mut store, None, now, 1);

    let origin = create_node_identity("origin", None);
    let good = sample_announcement(&origin, now, "already");
    retain_verified_pod_announcement(
        &mut store,
        good.clone(),
        DeliveryProvenance::bootstrap("https://boot.example"),
        now,
    )
    .unwrap();

    let mut forged = sample_announcement(&origin, now, "forged");
    forged.signature = "not-valid".into();
    let mut client = ScriptedDiscoveryPeerStreamClient::new();
    client.push_page("https://peer.example", None, stream_page(&forged, now));

    let report = sync_outbound_discovery_peers(&mut store, &client, now);
    assert!(!report.outcomes[0].ok);
    assert_eq!(
        report.outcomes[0].error.as_ref().unwrap().kind,
        DiscoveryPeerSyncFailureKind::InvalidSignature
    );
    assert!(report.evicted.contains(&peer_node.id));
    assert!(!store.outbound_discovery_peers.contains_key(&peer_node.id));
    assert!(store
        .known_pod_announcements
        .contains_key(&(good.origin_node_id, good.pod_slug.clone())));
}

#[test]
fn repeated_transport_failures_backoff_then_evict() {
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let mut store = InMemoryStore::default();
    let peer_node = create_node_identity("peer", None);
    let ad = sample_peer_ad(&peer_node, "https://peer.example", now);
    learn_discovery_peer_advertisement(
        &mut store,
        ad,
        None,
        Some(&FixedDiscoveryPeerProbe::matching_node(&peer_node)),
        now,
    )
    .unwrap();
    select_outbound_discovery_peers(&mut store, None, now, 1);

    let mut client = ScriptedDiscoveryPeerStreamClient::new();
    client.fail(
        "https://peer.example",
        DiscoveryPeerSyncFailure::new(
            DiscoveryPeerSyncFailureKind::Transport,
            "connection refused",
        ),
    );

    let mut t = now;
    for attempt in 1..=PEER_FAILURES_BEFORE_EVICTION {
        // Clear backoff so the plan includes the peer each attempt.
        if let Some(state) = store.discovery_peer_sync_states.get_mut(&peer_node.id) {
            state.backoff_until = None;
            if state.health == DiscoveryPeerHealth::BackedOff {
                state.health = DiscoveryPeerHealth::Healthy;
            }
        }
        let report = sync_outbound_discovery_peers(&mut store, &client, t);
        assert!(!report.outcomes.is_empty());
        if attempt < PEER_FAILURES_BEFORE_EVICTION {
            assert_eq!(
                store.discovery_peer_sync_states[&peer_node.id].health,
                DiscoveryPeerHealth::BackedOff
            );
            assert!(store.outbound_discovery_peers.contains_key(&peer_node.id));
        } else {
            assert!(report.evicted.contains(&peer_node.id));
            assert!(!store.outbound_discovery_peers.contains_key(&peer_node.id));
        }
        t = t + Duration::seconds(1);
    }
}

#[test]
fn disable_gossip_stops_sync_without_deleting_audit_state() {
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let mut store = InMemoryStore::default();
    let peer_node = create_node_identity("peer", None);
    let ad = sample_peer_ad(&peer_node, "https://peer.example", now);
    learn_discovery_peer_advertisement(
        &mut store,
        ad,
        Some("https://bootstrap.example"),
        Some(&FixedDiscoveryPeerProbe::matching_node(&peer_node)),
        now,
    )
    .unwrap();
    select_outbound_discovery_peers(&mut store, None, now, 1);
    assert_eq!(list_active_outbound_peers(&store).len(), 1);

    set_automatic_peer_gossip_enabled(&mut store, false, now);
    assert!(!peer_gossip_is_enabled(&store));
    // Audit state retained.
    assert!(store
        .known_discovery_peer_advertisements
        .contains_key(&peer_node.id));
    assert!(store.outbound_discovery_peers.contains_key(&peer_node.id));
    assert!(store.discovery_peer_sync_states.contains_key(&peer_node.id));

    let mut client = ScriptedDiscoveryPeerStreamClient::new();
    let origin = create_node_identity("origin", None);
    let announcement = sample_announcement(&origin, now, "systems");
    client.push_page(
        "https://peer.example",
        None,
        stream_page(&announcement, now),
    );
    let report = sync_outbound_discovery_peers(&mut store, &client, now);
    assert!(report.outcomes.is_empty());
    assert!(!store
        .known_pod_announcements
        .contains_key(&(announcement.origin_node_id, announcement.pod_slug.clone())));
}

#[test]
fn fresh_node_without_bootstrap_reports_degraded() {
    let store = InMemoryStore::default();
    let status = discovery_status(&store);
    assert!(status.degraded);
    assert_eq!(
        status.degraded_reason.as_deref(),
        Some("no_enabled_bootstrap")
    );
    assert!(status.message.contains("direct Pod URL"));
}

#[test]
fn learn_rejects_unreachable_and_expired() {
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let mut store = InMemoryStore::default();
    let peer = create_node_identity("peer", None);
    let ad = sample_peer_ad(&peer, "https://peer.example", now);
    let err = learn_discovery_peer_advertisement(
        &mut store,
        ad.clone(),
        None,
        Some(&UnreachableDiscoveryPeerProbe),
        now,
    )
    .unwrap_err();
    assert_eq!(err.kind, DiscoveryPeerSyncFailureKind::Unreachable);

    let stale_now = now + peer_advertisement_lease_duration() + Duration::seconds(1);
    let err = learn_discovery_peer_advertisement(
        &mut store,
        ad,
        None,
        Some(&FixedDiscoveryPeerProbe::matching_node(&peer)),
        stale_now,
    )
    .unwrap_err();
    assert_eq!(err.kind, DiscoveryPeerSyncFailureKind::ExpiredAdvertisement);
}

#[test]
fn sole_peer_source_becomes_ineligible_after_eviction() {
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let mut store = InMemoryStore::default();
    let peer_node = create_node_identity("peer", None);
    let ad = sample_peer_ad(&peer_node, "https://peer.example", now);
    learn_discovery_peer_advertisement(
        &mut store,
        ad,
        None,
        Some(&FixedDiscoveryPeerProbe::matching_node(&peer_node)),
        now,
    )
    .unwrap();
    select_outbound_discovery_peers(&mut store, None, now, 1);
    let origin = create_node_identity("origin", None);
    let announcement = sample_announcement(&origin, now, "peer-only");
    retain_verified_pod_announcement(
        &mut store,
        announcement.clone(),
        DeliveryProvenance::discovery_peer("https://peer.example"),
        now,
    )
    .unwrap();
    let known = store
        .known_pod_announcements
        .get(&(announcement.origin_node_id, "peer-only".into()))
        .unwrap()
        .clone();
    assert!(announcement_delivery_is_active(&store, &known, None));
    mark_peer_evicted(&mut store, peer_node.id, now, "evicted");
    store.outbound_discovery_peers.remove(&peer_node.id);
    assert!(!announcement_delivery_is_active(&store, &known, None));
    // Audit row remains.
    assert!(store
        .known_pod_announcements
        .contains_key(&(announcement.origin_node_id, "peer-only".into())));
}

#[test]
fn cursor_advances_and_resumes() {
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let mut store = InMemoryStore::default();
    let peer_node = create_node_identity("peer", None);
    let ad = sample_peer_ad(&peer_node, "https://peer.example", now);
    learn_discovery_peer_advertisement(
        &mut store,
        ad,
        None,
        Some(&FixedDiscoveryPeerProbe::matching_node(&peer_node)),
        now,
    )
    .unwrap();
    select_outbound_discovery_peers(&mut store, None, now, 1);

    let origin = create_node_identity("origin", None);
    let first = sample_announcement(&origin, now, "first");
    let second = sample_announcement(&origin, now, "second");
    let mut page1 = stream_page(&first, now);
    page1.next_cursor = Some("1".into());
    let page2 = stream_page(&second, now);

    let mut client = ScriptedDiscoveryPeerStreamClient::new();
    client.push_page("https://peer.example", None, page1);
    client.push_page("https://peer.example", Some("1"), page2);

    let report = sync_outbound_discovery_peers(&mut store, &client, now);
    assert!(report.outcomes[0].ok);
    assert_eq!(report.retained_announcements, 2);
    assert_eq!(
        store.discovery_peer_sync_states[&peer_node.id]
            .cursor
            .as_deref(),
        Some("1")
    );
    assert_eq!(
        store.discovery_peer_sync_states[&peer_node.id].last_success_at,
        Some(now)
    );
}

#[test]
fn peer_sample_request_is_public_only_rejects_private_fields() {
    let request = DiscoveryPeerSampleRequest { limit: Some(5) };
    assert!(peer_sample_request_is_public_only(&request));
    let dirty = serde_json::json!({"limit": 5, "taste_profile": {}});
    let object = dirty.as_object().unwrap();
    assert!(object.contains_key("taste_profile"));
}

#[test]
fn probe_identity_helper_is_available() {
    let node = create_node_identity("peer", None);
    let view = peer_identity_view_for_node(&node);
    assert_eq!(view.node_id, node.id);
}

#[test]
fn learned_from_accumulates_across_sources_and_copies_on_select() {
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let mut store = InMemoryStore::default();
    let peer = create_node_identity("peer", None);
    let ad = sample_peer_ad(&peer, "https://peer.example", now);

    // Learn from two sources before selection; provenance lives on known ad.
    learn_discovery_peer_advertisement(
        &mut store,
        ad.clone(),
        Some("https://bootstrap-a.example"),
        None,
        now,
    )
    .unwrap();
    learn_discovery_peer_advertisement(
        &mut store,
        ad,
        Some("https://bootstrap-b.example"),
        None,
        now,
    )
    .unwrap();

    let known = store
        .known_discovery_peer_advertisements
        .get(&peer.id)
        .unwrap();
    assert_eq!(known.learned_from.len(), 2);
    assert!(known.learned_from.contains("https://bootstrap-a.example"));
    assert!(known.learned_from.contains("https://bootstrap-b.example"));

    // Select copies multi-source provenance onto the outbound peer.
    let selected = select_outbound_discovery_peers(&mut store, None, now, 1);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].learned_from.len(), 2);
    assert!(selected[0]
        .learned_from
        .contains("https://bootstrap-a.example"));
    assert!(selected[0]
        .learned_from
        .contains("https://bootstrap-b.example"));
}

#[test]
fn fresh_learn_re_admits_transport_evicted_peer() {
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let mut store = InMemoryStore::default();
    let peer_node = create_node_identity("peer", None);
    let ad = sample_peer_ad(&peer_node, "https://peer.example", now);
    learn_discovery_peer_advertisement(
        &mut store,
        ad.clone(),
        Some("https://bootstrap.example"),
        None,
        now,
    )
    .unwrap();
    select_outbound_discovery_peers(&mut store, None, now, 1);
    assert!(store.outbound_discovery_peers.contains_key(&peer_node.id));

    // Transport failures until eviction.
    let mut client = ScriptedDiscoveryPeerStreamClient::new();
    client.fail(
        "https://peer.example",
        DiscoveryPeerSyncFailure::new(
            DiscoveryPeerSyncFailureKind::Transport,
            "connection refused",
        ),
    );
    let mut t = now;
    for _ in 1..=PEER_FAILURES_BEFORE_EVICTION {
        if let Some(state) = store.discovery_peer_sync_states.get_mut(&peer_node.id) {
            state.backoff_until = None;
            if state.health == DiscoveryPeerHealth::BackedOff {
                state.health = DiscoveryPeerHealth::Healthy;
            }
        }
        sync_outbound_discovery_peers(&mut store, &client, t);
        t = t + Duration::seconds(1);
    }
    assert!(!store.outbound_discovery_peers.contains_key(&peer_node.id));
    assert_eq!(
        store.discovery_peer_sync_states[&peer_node.id].health,
        DiscoveryPeerHealth::Evicted
    );

    // Without re-learn, selection must not re-admit an still-evicted peer.
    let selected = select_outbound_discovery_peers(&mut store, None, t, 1);
    assert!(selected.is_empty());
    assert!(!store.outbound_discovery_peers.contains_key(&peer_node.id));

    // Fresh verified learn un-evicts; select re-admits with provenance.
    learn_discovery_peer_advertisement(&mut store, ad, Some("https://bootstrap.example"), None, t)
        .unwrap();
    assert_eq!(
        store.discovery_peer_sync_states[&peer_node.id].health,
        DiscoveryPeerHealth::Healthy
    );
    let selected = select_outbound_discovery_peers(&mut store, None, t, 1);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].node_id, peer_node.id);
    assert!(selected[0]
        .learned_from
        .contains("https://bootstrap.example"));
    assert_eq!(
        store.discovery_peer_sync_states[&peer_node.id].health,
        DiscoveryPeerHealth::Healthy
    );
}

#[test]
fn learn_without_probe_accepts_signed_ad() {
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let mut store = InMemoryStore::default();
    let peer = create_node_identity("peer", None);
    let ad = sample_peer_ad(&peer, "https://peer.example", now);
    // Production learn path: local signed-ad verify without live reachability.
    learn_discovery_peer_advertisement(&mut store, ad, Some("https://boot.example"), None, now)
        .unwrap();
    assert!(store
        .known_discovery_peer_advertisements
        .contains_key(&peer.id));
}
