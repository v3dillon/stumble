//! Opt-in enable/disable of Discovery Peer announcement serving.

use super::endpoint::normalize_discovery_peer_endpoint;
use super::probe::{DiscoveryPeerProbe, DiscoveryPeerProbeError};
use super::types::{ensure_discovery_peer_service, prune_peer_stream_entries};
use crate::domain::{
    peer_advertisement_lease_duration, AnnouncementStreamEntry, AnnouncementStreamEventKind,
    AnnouncementStreamPayload, DiscoveryPeerAdmissionRejectionReason, DiscoveryPeerAdvertisement,
    DiscoveryPeerCapability, DiscoveryPeerIdentityView, NodeIdentity, NodeInfo, PodAnnouncement,
    CURRENT_PROTOCOL_VERSION,
};
use crate::signing::sign_discovery_peer_advertisement;
use crate::store::InMemoryStore;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Returns whether this node currently enables inbound Discovery Peer serving.
#[must_use]
pub fn peer_service_is_enabled(store: &InMemoryStore) -> bool {
    store
        .discovery_peer_service
        .as_ref()
        .is_some_and(|state| state.enabled)
}

/// Enables announcement serving after public-endpoint verification and issues a
/// signed Discovery Peer Advertisement.
///
/// On successful enable, verified lease-active known public announcements already
/// retained on this node are projected into the peer serving stream so pure Home
/// Nodes can serve discovery artifacts without Bootstrap capability.
///
/// # Errors
///
/// Returns a stable rejection reason when the endpoint policy fails, the probe
/// reports unreachability or identity mismatch, identity is missing, or signing
/// fails.
pub fn enable_discovery_peer_service(
    store: &mut InMemoryStore,
    node: &NodeIdentity,
    public_endpoint: &str,
    probe: &dyn DiscoveryPeerProbe,
    now: DateTime<Utc>,
) -> Result<DiscoveryPeerAdvertisement, DiscoveryPeerAdmissionRejectionReason> {
    let endpoint =
        normalize_discovery_peer_endpoint(public_endpoint).map_err(|error| error.as_rejection())?;

    if node.public_key.trim().is_empty() {
        return Err(DiscoveryPeerAdmissionRejectionReason::InvalidIdentity);
    }

    let view = probe
        .probe_peer_endpoint(&endpoint)
        .map_err(|error| match error {
            DiscoveryPeerProbeError::Unreachable => {
                DiscoveryPeerAdmissionRejectionReason::UnreachableEndpoint
            }
        })?;
    ensure_identity_matches_node(node, &view)?;

    let advertisement = build_signed_advertisement(node, &endpoint, now)?;
    let service = ensure_discovery_peer_service(store);
    service.enabled = true;
    service.public_endpoint = Some(endpoint);
    service.current_advertisement = Some(advertisement.clone());
    service.verified_at = Some(now);

    // Project currently known verified public announcements into the peer stream.
    project_known_announcements_into_peer_stream(store, now);
    Ok(advertisement)
}

/// Renews the current Discovery Peer Advertisement while service remains enabled.
///
/// # Errors
///
/// Returns a rejection reason when service is disabled, the endpoint is missing,
/// reachability/identity verification fails, or signing fails.
pub fn renew_discovery_peer_advertisement(
    store: &mut InMemoryStore,
    node: &NodeIdentity,
    probe: &dyn DiscoveryPeerProbe,
    now: DateTime<Utc>,
) -> Result<DiscoveryPeerAdvertisement, DiscoveryPeerAdmissionRejectionReason> {
    if !peer_service_is_enabled(store) {
        return Err(DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled);
    }
    let endpoint = store
        .discovery_peer_service
        .as_ref()
        .and_then(|state| state.public_endpoint.clone())
        .ok_or(DiscoveryPeerAdmissionRejectionReason::VerificationFailed)?;

    let view = probe
        .probe_peer_endpoint(&endpoint)
        .map_err(|error| match error {
            DiscoveryPeerProbeError::Unreachable => {
                DiscoveryPeerAdmissionRejectionReason::UnreachableEndpoint
            }
        })?;
    ensure_identity_matches_node(node, &view)?;

    let advertisement = build_signed_advertisement(node, &endpoint, now)?;
    let service = ensure_discovery_peer_service(store);
    service.current_advertisement = Some(advertisement.clone());
    service.verified_at = Some(now);
    Ok(advertisement)
}

/// Disables announcement serving: stops advertisement renewal and inbound serve.
///
/// Outbound discovery, Bootstrap client config, and direct Pod synchronization
/// are unaffected. Serving stream sequence high-water is retained for audit.
pub fn disable_discovery_peer_service(store: &mut InMemoryStore, _now: DateTime<Utc>) {
    let service = ensure_discovery_peer_service(store);
    service.enabled = false;
    service.current_advertisement = None;
    // Keep public_endpoint and next_stream_sequence as audit/resume state; they
    // are not advertised while disabled and enable must re-verify.
}

/// Projects a verified Origin-signed announcement into the peer serving stream.
///
/// Preserves Origin announcement bytes and signatures unchanged. No-ops when
/// peer service is disabled so disabled nodes do not build inbound serve state.
/// Requires a valid signature and an active lease at `now`.
///
/// # Errors
///
/// Returns [`DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled`] when
/// service is not enabled, `InvalidSignature` when verification fails, or
/// `StaleLease` when the lease is not active.
pub fn project_peer_serving_announcement(
    store: &mut InMemoryStore,
    announcement: PodAnnouncement,
    now: DateTime<Utc>,
) -> Result<u64, DiscoveryPeerAdmissionRejectionReason> {
    if !peer_service_is_enabled(store) {
        return Err(DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled);
    }
    match announcement.verify() {
        Ok(true) => {}
        Ok(false) | Err(_) => {
            return Err(DiscoveryPeerAdmissionRejectionReason::InvalidSignature);
        }
    }
    if !announcement.lease_is_active(now) {
        return Err(DiscoveryPeerAdmissionRejectionReason::StaleLease);
    }
    Ok(append_peer_stream_entry(store, announcement, now))
}

/// Projects a retained announcement into the peer stream when serving is enabled.
///
/// Silent no-op when peer service is disabled or the announcement fails verify
/// / lease checks (retain paths already verified; this only gates peer serving).
pub fn maybe_project_peer_serving_announcement(
    store: &mut InMemoryStore,
    announcement: &PodAnnouncement,
    now: DateTime<Utc>,
) {
    if !peer_service_is_enabled(store) {
        return;
    }
    let _ = project_peer_serving_announcement(store, announcement.clone(), now);
}

fn project_known_announcements_into_peer_stream(store: &mut InMemoryStore, now: DateTime<Utc>) {
    let candidates: Vec<PodAnnouncement> = store
        .known_pod_announcements
        .values()
        .map(|known| known.announcement.clone())
        .filter(|announcement| {
            announcement.lease_is_active(now) && announcement.verify().unwrap_or(false)
        })
        .collect();
    for announcement in candidates {
        let _ = project_peer_serving_announcement(store, announcement, now);
    }
}

fn append_peer_stream_entry(
    store: &mut InMemoryStore,
    announcement: PodAnnouncement,
    now: DateTime<Utc>,
) -> u64 {
    let service = ensure_discovery_peer_service(store);
    let sequence = service.next_stream_sequence;
    service.next_stream_sequence = service.next_stream_sequence.saturating_add(1);
    let entry = AnnouncementStreamEntry {
        sequence,
        recorded_at: now,
        kind: AnnouncementStreamEventKind::Admitted,
        origin_node_id: announcement.origin_node_id,
        pod_slug: announcement.pod_slug.clone(),
        payload: AnnouncementStreamPayload::Announcement(announcement),
    };
    store.discovery_peer_stream_entries.insert(sequence, entry);
    prune_peer_stream_entries(store);
    sequence
}

fn ensure_identity_matches_node(
    node: &NodeIdentity,
    view: &DiscoveryPeerIdentityView,
) -> Result<(), DiscoveryPeerAdmissionRejectionReason> {
    if view.node_id != node.id
        || view.public_key != node.public_key
        || view.protocol_version != CURRENT_PROTOCOL_VERSION
    {
        return Err(DiscoveryPeerAdmissionRejectionReason::IdentityMismatch);
    }
    Ok(())
}

fn build_signed_advertisement(
    node: &NodeIdentity,
    public_endpoint: &str,
    now: DateTime<Utc>,
) -> Result<DiscoveryPeerAdvertisement, DiscoveryPeerAdmissionRejectionReason> {
    let unsigned = DiscoveryPeerAdvertisement {
        id: Uuid::now_v7(),
        node_id: node.id,
        signer: NodeInfo {
            node_id: node.id,
            display_name: node.display_name.clone(),
            public_key: node.public_key.clone(),
            supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
        },
        public_endpoint: public_endpoint.to_string(),
        protocol_version: CURRENT_PROTOCOL_VERSION.into(),
        capability: DiscoveryPeerCapability::AnnouncementServing,
        issued_at: now,
        expires_at: now + peer_advertisement_lease_duration(),
        signature: String::new(),
    };
    sign_discovery_peer_advertisement(node, unsigned)
        .map_err(|_| DiscoveryPeerAdmissionRejectionReason::InvalidIdentity)
}
