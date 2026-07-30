//! Opt-in Discovery Peer service and outbound Home Node peer rotation.
//!
//! Ordinary Home Nodes remain outbound-only for discovery by default. A User
//! must explicitly enable announcement serving after declaring a public endpoint
//! and passing identity, protocol, HTTPS, and reachability verification.
//! Enabled peers advertise a narrow signed capability and serve only public
//! discovery artifacts—never Pod Events, Subscriptions, Taste Profiles,
//! credentials, or administrative surfaces.
//!
//! Home Nodes automatically maintain a small rotating outbound Discovery Peer
//! set learned from Bootstrap and peer samples, synchronize Origin-signed
//! announcement lifecycle artifacts through it, and survive Bootstrap outages
//! without granting Trusted Peer status.
//!
//! # Module layout
//!
//! - [`probe`] — public endpoint reachability + identity view port
//! - [`endpoint`] — public endpoint policy (HTTPS outside loopback, private hosts)
//! - [`advertise`] — enable/disable opt-in service and produce signed advertisements
//! - [`admit`] — open Bootstrap admission of peer advertisements
//! - [`serve`] — inbound Announcement Stream pages and unranked peer samples
//! - [`client`] — outbound peer learning, rotation, sync, eviction, discovery status
//! - [`types`] — bounds and store helpers

mod admit;
mod advertise;
mod client;
mod endpoint;
mod probe;
mod serve;
mod types;

pub use admit::admit_discovery_peer_advertisement;
pub use advertise::{
    disable_discovery_peer_service, enable_discovery_peer_service,
    maybe_project_peer_serving_announcement, peer_service_is_enabled,
    project_peer_serving_announcement, renew_discovery_peer_advertisement,
};
pub use client::{
    apply_discovery_peer_stream_pages, discovery_status, ensure_discovery_peer_gossip_config,
    evict_if_advertisement_expired, fetch_discovery_peer_stream_pages,
    fetch_peer_advertisement_samples, learn_discovery_peer_advertisement,
    learn_peers_from_sample_sources, list_active_outbound_peers, max_outbound_peers,
    outbound_discovery_peer_statuses, peer_gossip_is_enabled, peer_sample_request_is_public_only,
    peer_stream_request_is_public_only, plan_discovery_peer_sync,
    retain_learned_samples_and_select, select_outbound_discovery_peers,
    set_automatic_peer_gossip_enabled, sync_outbound_discovery_peers, DiscoveryPeerStreamClient,
    DiscoveryPeerSyncPlan, FetchedDiscoveryPeerStream, FetchedPeerAdvertisementSample,
    PeerAdvertisementSampleClient, PeerLearnReport, ScriptedDiscoveryPeerStreamClient,
    ScriptedPeerAdvertisementSampleClient,
};
pub use endpoint::{normalize_discovery_peer_endpoint, EndpointPolicyError};
pub use probe::{
    peer_identity_view_for_advertisement, peer_identity_view_for_node, DiscoveryPeerProbe,
    DiscoveryPeerProbeError, FixedDiscoveryPeerProbe, ScriptedDiscoveryPeerProbe,
    SimpleMatchingDiscoveryPeerProbe, UnreachableDiscoveryPeerProbe,
};
pub use serve::{
    peer_advertisement_sample_is_public_only, read_peer_announcement_stream,
    sample_discovery_peer_advertisements, sample_known_discovery_peer_advertisements,
};
pub use types::{
    ensure_discovery_peer_service, estimated_payload_bytes, DEFAULT_PEER_SAMPLE_LIMIT,
    DEFAULT_PEER_STREAM_PAGE_LIMIT, MAX_PEER_ADVERTISEMENT_PAYLOAD_BYTES,
    MAX_PEER_NETWORK_ADMISSIONS_PER_WINDOW, MAX_PEER_NODE_ADMISSIONS_PER_WINDOW,
    MAX_PEER_SAMPLE_LIMIT, MAX_PEER_STREAM_ENTRIES, MAX_PEER_STREAM_PAGE_LIMIT,
    PEER_ADMISSION_RATE_WINDOW,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        announcement_lease_duration, peer_advertisement_lease_duration,
        AnnouncementStreamEventKind, BootstrapAdmissionOutcomeKind,
        DiscoveryPeerAdmissionRejectionReason, DiscoveryPeerCapability, DiscoveryPeerIdentityView,
        NodeInfo, PackageVersion, PodAnnouncement, CURRENT_PROTOCOL_VERSION,
    };
    use crate::pod_announcement::{retain_verified_pod_announcement, DeliveryProvenance};
    use crate::signing::{create_node_identity, sign_pod_announcement};
    use crate::store::InMemoryStore;
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    fn sample_pod_announcement(
        node: &crate::domain::NodeIdentity,
        announced_at: chrono::DateTime<Utc>,
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

    #[test]
    fn default_service_is_disabled_and_does_not_serve() {
        let store = InMemoryStore::default();
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        assert!(!peer_service_is_enabled(&store));
        let err = read_peer_announcement_stream(&store, None, Some(10), now).unwrap_err();
        assert_eq!(
            err,
            DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled
        );
        let err = sample_discovery_peer_advertisements(&store, Some(5), now, 1).unwrap_err();
        assert_eq!(
            err,
            DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled
        );
    }

    #[test]
    fn enable_requires_public_endpoint_and_produces_signed_ad() {
        let mut store = InMemoryStore::default();
        let node = create_node_identity("home", None);
        store.node_identities.insert(node.id, node.clone());
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let probe = FixedDiscoveryPeerProbe::matching_node(&node);
        let ad =
            enable_discovery_peer_service(&mut store, &node, "https://peer.example", &probe, now)
                .unwrap();
        assert!(ad.verify().unwrap());
        assert_eq!(ad.capability, DiscoveryPeerCapability::AnnouncementServing);
        assert_eq!(ad.public_endpoint, "https://peer.example");
        assert_eq!(ad.protocol_version, CURRENT_PROTOCOL_VERSION);
        assert_eq!(ad.expires_at, now + peer_advertisement_lease_duration());
        assert!(peer_service_is_enabled(&store));
        assert_eq!(
            store
                .discovery_peer_service
                .as_ref()
                .unwrap()
                .public_endpoint
                .as_deref(),
            Some("https://peer.example")
        );
    }

    #[test]
    fn enable_rejects_private_and_insecure_endpoints() {
        let mut store = InMemoryStore::default();
        let node = create_node_identity("home", None);
        store.node_identities.insert(node.id, node.clone());
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let probe = FixedDiscoveryPeerProbe::matching_node(&node);
        let err =
            enable_discovery_peer_service(&mut store, &node, "http://peer.example", &probe, now)
                .unwrap_err();
        assert_eq!(err, DiscoveryPeerAdmissionRejectionReason::InsecureEndpoint);
        let err = enable_discovery_peer_service(&mut store, &node, "https://10.0.0.5", &probe, now)
            .unwrap_err();
        assert_eq!(err, DiscoveryPeerAdmissionRejectionReason::PrivateEndpoint);
        assert!(!peer_service_is_enabled(&store));
    }

    #[test]
    fn enable_rejects_unreachable_endpoint() {
        let mut store = InMemoryStore::default();
        let node = create_node_identity("home", None);
        store.node_identities.insert(node.id, node.clone());
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let err = enable_discovery_peer_service(
            &mut store,
            &node,
            "https://peer.example",
            &UnreachableDiscoveryPeerProbe,
            now,
        )
        .unwrap_err();
        assert_eq!(
            err,
            DiscoveryPeerAdmissionRejectionReason::UnreachableEndpoint
        );
    }

    #[test]
    fn enable_rejects_identity_mismatch() {
        let mut store = InMemoryStore::default();
        let node = create_node_identity("home", None);
        store.node_identities.insert(node.id, node.clone());
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let other = create_node_identity("other", None);
        let probe = FixedDiscoveryPeerProbe::matching_node(&other);
        let err =
            enable_discovery_peer_service(&mut store, &node, "https://peer.example", &probe, now)
                .unwrap_err();
        assert_eq!(err, DiscoveryPeerAdmissionRejectionReason::IdentityMismatch);
        assert!(!peer_service_is_enabled(&store));
    }

    #[test]
    fn disable_stops_inbound_serve_and_clears_renewable_ad() {
        let mut store = InMemoryStore::default();
        let node = create_node_identity("home", None);
        store.node_identities.insert(node.id, node.clone());
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        enable_discovery_peer_service(
            &mut store,
            &node,
            "https://peer.example",
            &FixedDiscoveryPeerProbe::matching_node(&node),
            now,
        )
        .unwrap();
        disable_discovery_peer_service(&mut store, now);
        assert!(!peer_service_is_enabled(&store));
        assert!(store
            .discovery_peer_service
            .as_ref()
            .unwrap()
            .current_advertisement
            .is_none());
        let err = read_peer_announcement_stream(&store, None, Some(10), now).unwrap_err();
        assert_eq!(
            err,
            DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled
        );
    }

    #[test]
    fn bootstrap_admits_valid_peer_ad_and_rejects_forged_stale_unreachable() {
        let mut store = InMemoryStore::default();
        let node = create_node_identity("peer", None);
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut service_store = InMemoryStore::default();
        service_store.node_identities.insert(node.id, node.clone());
        let ad = enable_discovery_peer_service(
            &mut service_store,
            &node,
            "https://peer.example",
            &FixedDiscoveryPeerProbe::matching_node(&node),
            now,
        )
        .unwrap();

        let accepted = admit_discovery_peer_advertisement(
            &mut store,
            ad.clone(),
            &FixedDiscoveryPeerProbe::matching_advertisement(&ad),
            true,
            now,
        )
        .unwrap();
        assert_eq!(accepted.outcome, BootstrapAdmissionOutcomeKind::Admitted);
        assert!(store
            .known_discovery_peer_advertisements
            .contains_key(&node.id));

        // Forged signature.
        let mut forged = ad.clone();
        forged.signature = "not-a-real-signature".into();
        let err = admit_discovery_peer_advertisement(
            &mut store,
            forged,
            &FixedDiscoveryPeerProbe::matching_advertisement(&ad),
            true,
            now,
        )
        .unwrap_err();
        assert_eq!(err, DiscoveryPeerAdmissionRejectionReason::InvalidSignature);

        // Stale lease.
        let stale_now = now + peer_advertisement_lease_duration() + Duration::seconds(1);
        let err = admit_discovery_peer_advertisement(
            &mut store,
            ad.clone(),
            &FixedDiscoveryPeerProbe::matching_advertisement(&ad),
            true,
            stale_now,
        )
        .unwrap_err();
        assert_eq!(err, DiscoveryPeerAdmissionRejectionReason::StaleLease);

        // Unreachable (fresh advertisement so admission is not short-circuited as idempotent).
        let other = create_node_identity("other-peer", None);
        let mut other_store = InMemoryStore::default();
        other_store.node_identities.insert(other.id, other.clone());
        let other_ad = enable_discovery_peer_service(
            &mut other_store,
            &other,
            "https://other-peer.example",
            &FixedDiscoveryPeerProbe::matching_node(&other),
            now,
        )
        .unwrap();
        let err = admit_discovery_peer_advertisement(
            &mut store,
            other_ad,
            &UnreachableDiscoveryPeerProbe,
            true,
            now,
        )
        .unwrap_err();
        assert_eq!(
            err,
            DiscoveryPeerAdmissionRejectionReason::UnreachableEndpoint
        );
    }

    #[test]
    fn admit_rejects_identity_mismatch_and_rate_limits() {
        let mut store = InMemoryStore::default();
        let node = create_node_identity("peer", None);
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut service_store = InMemoryStore::default();
        service_store.node_identities.insert(node.id, node.clone());
        let ad = enable_discovery_peer_service(
            &mut service_store,
            &node,
            "https://peer.example",
            &FixedDiscoveryPeerProbe::matching_node(&node),
            now,
        )
        .unwrap();

        let wrong = DiscoveryPeerIdentityView {
            node_id: Uuid::now_v7(),
            public_key: "not-the-key".into(),
            protocol_version: CURRENT_PROTOCOL_VERSION.into(),
        };
        let err = admit_discovery_peer_advertisement(
            &mut store,
            ad.clone(),
            &FixedDiscoveryPeerProbe::reachable(wrong),
            true,
            now,
        )
        .unwrap_err();
        assert_eq!(err, DiscoveryPeerAdmissionRejectionReason::IdentityMismatch);

        // Admit renewed advertisements from the same node until the per-node limit trips.
        for i in 0..MAX_PEER_NODE_ADMISSIONS_PER_WINDOW {
            let mut renewed_store = InMemoryStore::default();
            renewed_store.node_identities.insert(node.id, node.clone());
            let renewed = enable_discovery_peer_service(
                &mut renewed_store,
                &node,
                "https://peer.example",
                &FixedDiscoveryPeerProbe::matching_node(&node),
                now + Duration::seconds(i as i64 + 1),
            )
            .unwrap();
            admit_discovery_peer_advertisement(
                &mut store,
                renewed,
                &FixedDiscoveryPeerProbe::matching_node(&node),
                true,
                now + Duration::seconds(i as i64 + 1),
            )
            .unwrap_or_else(|error| panic!("admit {i} should succeed: {error}"));
        }
        let mut overflow_store = InMemoryStore::default();
        overflow_store.node_identities.insert(node.id, node.clone());
        let overflow = enable_discovery_peer_service(
            &mut overflow_store,
            &node,
            "https://peer.example",
            &FixedDiscoveryPeerProbe::matching_node(&node),
            now + Duration::minutes(30),
        )
        .unwrap();
        let err = admit_discovery_peer_advertisement(
            &mut store,
            overflow,
            &FixedDiscoveryPeerProbe::matching_node(&node),
            true,
            now + Duration::minutes(30),
        )
        .unwrap_err();
        assert_eq!(err, DiscoveryPeerAdmissionRejectionReason::RateLimited);
        assert_eq!(err.as_code(), "rate_limited");
    }

    #[test]
    fn enabled_peer_serves_origin_bytes_and_randomized_peer_samples() {
        let mut store = InMemoryStore::default();
        let node = create_node_identity("peer", None);
        store.node_identities.insert(node.id, node.clone());
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        enable_discovery_peer_service(
            &mut store,
            &node,
            "https://peer.example",
            &FixedDiscoveryPeerProbe::matching_node(&node),
            now,
        )
        .unwrap();

        let origin = create_node_identity("origin", None);
        let announcement = sample_pod_announcement(&origin, now, "systems");
        let original_signature = announcement.signature.clone();
        let original_bytes = serde_json::to_vec(&announcement).unwrap();
        project_peer_serving_announcement(&mut store, announcement.clone(), now).unwrap();

        let page = read_peer_announcement_stream(&store, None, Some(10), now).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].kind, AnnouncementStreamEventKind::Admitted);
        let served = page.entries[0].payload.as_announcement().unwrap();
        assert_eq!(served.signature, original_signature);
        assert_eq!(serde_json::to_vec(served).unwrap(), original_bytes);

        // Seed a second peer ad into known set for sampling.
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
        admit_discovery_peer_advertisement(
            &mut store,
            other_ad,
            &FixedDiscoveryPeerProbe::matching_node(&other),
            true,
            now,
        )
        .unwrap();
        // Also admit self so sample has local ads.
        let self_ad = store
            .discovery_peer_service
            .as_ref()
            .unwrap()
            .current_advertisement
            .clone()
            .unwrap();
        admit_discovery_peer_advertisement(
            &mut store,
            self_ad,
            &FixedDiscoveryPeerProbe::matching_node(&node),
            true,
            now,
        )
        .unwrap();

        let sample = sample_discovery_peer_advertisements(&store, Some(10), now, 42).unwrap();
        assert_eq!(sample.advertisements.len(), 2);
        // No rank/trust fields on the wire shape.
        let wire = serde_json::to_value(&sample).unwrap();
        assert!(wire.get("rank").is_none());
        assert!(wire.get("trust").is_none());
        assert!(wire.get("score").is_none());
        assert!(peer_advertisement_sample_is_public_only(&sample));
    }

    #[test]
    fn pure_home_node_projects_retained_announcements_into_peer_stream() {
        let mut store = InMemoryStore::default();
        let node = create_node_identity("home", None);
        store.node_identities.insert(node.id, node.clone());
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let origin = create_node_identity("origin", None);
        let announcement = sample_pod_announcement(&origin, now, "systems");
        retain_verified_pod_announcement(
            &mut store,
            announcement.clone(),
            DeliveryProvenance::LOCAL,
            now,
        )
        .unwrap();
        // Not yet enabled: peer stream empty / disabled.
        assert!(store.discovery_peer_stream_entries.is_empty());

        enable_discovery_peer_service(
            &mut store,
            &node,
            "https://peer.example",
            &FixedDiscoveryPeerProbe::matching_node(&node),
            now,
        )
        .unwrap();
        // Enable projects known verified announcements.
        assert!(!store.discovery_peer_stream_entries.is_empty());
        let page = read_peer_announcement_stream(&store, None, Some(10), now).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(
            page.entries[0].payload.as_announcement().unwrap().pod_slug,
            "systems"
        );

        // Subsequent retain while enabled also projects.
        let second = sample_pod_announcement(&origin, now + Duration::seconds(1), "networks");
        retain_verified_pod_announcement(&mut store, second, DeliveryProvenance::LOCAL, now)
            .unwrap();
        let page = read_peer_announcement_stream(&store, None, Some(10), now).unwrap();
        assert_eq!(page.entries.len(), 2);
    }

    #[test]
    fn peer_and_bootstrap_streams_use_independent_sequences() {
        let mut store = InMemoryStore::default();
        let node = create_node_identity("combined", None);
        store.node_identities.insert(node.id, node.clone());
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();

        // Bootstrap stream sequence advances independently.
        crate::bootstrap::ensure_bootstrap_runtime(&mut store);
        let bootstrap_seq = {
            let runtime = store.bootstrap_runtime.as_mut().unwrap();
            let seq = runtime.next_stream_sequence;
            runtime.next_stream_sequence = runtime.next_stream_sequence.saturating_add(1);
            seq
        };
        store.announcement_stream_entries.insert(
            bootstrap_seq,
            crate::domain::AnnouncementStreamEntry {
                sequence: bootstrap_seq,
                recorded_at: now,
                kind: AnnouncementStreamEventKind::Admitted,
                origin_node_id: node.id,
                pod_slug: "bootstrap-only".into(),
                payload: crate::domain::AnnouncementStreamPayload::Announcement(
                    sample_pod_announcement(&node, now, "bootstrap-only"),
                ),
            },
        );

        enable_discovery_peer_service(
            &mut store,
            &node,
            "https://peer.example",
            &FixedDiscoveryPeerProbe::matching_node(&node),
            now,
        )
        .unwrap();
        let origin = create_node_identity("origin", None);
        let announcement = sample_pod_announcement(&origin, now, "peer-only");
        let peer_seq =
            project_peer_serving_announcement(&mut store, announcement.clone(), now).unwrap();

        // Both streams keep independent maps; sequence numbers may coincide without
        // overwriting (this was the critical combined-role bug on a shared map).
        assert_eq!(peer_seq, 1);
        assert_eq!(bootstrap_seq, 1);
        assert_eq!(
            store
                .announcement_stream_entries
                .get(&bootstrap_seq)
                .unwrap()
                .pod_slug,
            "bootstrap-only"
        );
        assert_eq!(
            store
                .discovery_peer_stream_entries
                .get(&peer_seq)
                .unwrap()
                .pod_slug,
            "peer-only"
        );
        let peer_page = read_peer_announcement_stream(&store, None, Some(10), now).unwrap();
        assert_eq!(peer_page.entries.len(), 1);
        assert_eq!(peer_page.entries[0].pod_slug, "peer-only");
    }

    #[test]
    fn project_requires_verify_and_active_lease() {
        let mut store = InMemoryStore::default();
        let node = create_node_identity("peer", None);
        store.node_identities.insert(node.id, node.clone());
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        enable_discovery_peer_service(
            &mut store,
            &node,
            "https://peer.example",
            &FixedDiscoveryPeerProbe::matching_node(&node),
            now,
        )
        .unwrap();
        let origin = create_node_identity("origin", None);
        let mut bad = sample_pod_announcement(&origin, now, "systems");
        bad.signature = "forged".into();
        let err = project_peer_serving_announcement(&mut store, bad, now).unwrap_err();
        assert_eq!(err, DiscoveryPeerAdmissionRejectionReason::InvalidSignature);

        let stale = sample_pod_announcement(
            &origin,
            now - announcement_lease_duration() - Duration::hours(1),
            "stale",
        );
        let err = project_peer_serving_announcement(&mut store, stale, now).unwrap_err();
        assert_eq!(err, DiscoveryPeerAdmissionRejectionReason::StaleLease);
    }

    #[test]
    fn loopback_http_endpoint_is_allowed_for_local_tests() {
        let mut store = InMemoryStore::default();
        let node = create_node_identity("home", None);
        store.node_identities.insert(node.id, node.clone());
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let ad = enable_discovery_peer_service(
            &mut store,
            &node,
            "http://127.0.0.1:8080",
            &FixedDiscoveryPeerProbe::matching_node(&node),
            now,
        )
        .unwrap();
        assert_eq!(ad.public_endpoint, "http://127.0.0.1:8080");
    }

    #[test]
    fn simple_matching_probe_supports_identity_swap() {
        let node = create_node_identity("home", None);
        let probe = SimpleMatchingDiscoveryPeerProbe::from_node(&node);
        let view = probe.probe_peer_endpoint("https://peer.example").unwrap();
        assert_eq!(view.node_id, node.id);
        let other = create_node_identity("other", None);
        probe.set_identity(peer_identity_view_for_node(&other));
        let view = probe.probe_peer_endpoint("https://peer.example").unwrap();
        assert_eq!(view.node_id, other.id);
    }
}
