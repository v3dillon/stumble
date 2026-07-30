use super::super::endpoint::normalize_discovery_peer_endpoint;
use super::super::probe::{DiscoveryPeerProbe, DiscoveryPeerProbeError};
use super::*;
use crate::domain::{
    DiscoveryPeerAdvertisement, DiscoveryPeerCapability, DiscoveryPeerGossipConfig,
    DiscoveryPeerHealth, DiscoveryPeerSyncFailure, DiscoveryPeerSyncFailureKind,
    KnownDiscoveryPeerAdvertisement, NodeIdentityId, OutboundDiscoveryPeer,
    OutboundDiscoveryPeerStatus, CURRENT_PROTOCOL_VERSION, DEFAULT_MAX_OUTBOUND_DISCOVERY_PEERS,
    MAX_OUTBOUND_DISCOVERY_PEERS,
};
use crate::store::InMemoryStore;
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;

/// Ensures gossip config exists with automatic gossip enabled by default.
pub fn ensure_discovery_peer_gossip_config(
    store: &mut InMemoryStore,
) -> &mut DiscoveryPeerGossipConfig {
    store
        .discovery_peer_gossip_config
        .get_or_insert_with(DiscoveryPeerGossipConfig::default)
}

/// Returns whether automatic Discovery Peer gossip is currently enabled.
#[must_use]
pub fn peer_gossip_is_enabled(store: &InMemoryStore) -> bool {
    store
        .discovery_peer_gossip_config
        .as_ref()
        .map(|config| config.automatic_gossip_enabled)
        .unwrap_or(true)
}

/// Enables or disables automatic peer gossip without deleting audit state.
///
/// Cached peer advertisements, outbound set rows, cursors, health, and Bootstrap
/// / direct-address paths remain intact.
pub fn set_automatic_peer_gossip_enabled(
    store: &mut InMemoryStore,
    enabled: bool,
    now: DateTime<Utc>,
) -> DiscoveryPeerGossipConfig {
    let config = ensure_discovery_peer_gossip_config(store);
    config.automatic_gossip_enabled = enabled;
    config.updated_at = Some(now);
    config.clone()
}

/// Returns the effective max outbound peer bound from config (clamped).
#[must_use]
pub fn max_outbound_peers(store: &InMemoryStore) -> usize {
    store
        .discovery_peer_gossip_config
        .as_ref()
        .map(|config| config.max_outbound_peers)
        .unwrap_or(DEFAULT_MAX_OUTBOUND_DISCOVERY_PEERS)
        .clamp(1, MAX_OUTBOUND_DISCOVERY_PEERS)
}

/// Locally verifies and retains a signed Discovery Peer Advertisement.
///
/// Verifies identity, capability, protocol version, endpoint policy, lease, and
/// signature. When `probe` is provided, also verifies reachability and live
/// identity match. Reachability is **optional** for outbound learning: callers
/// may pass `None` so signed-ad local verification alone is enough (production
/// default). Live probe checks remain required at Discovery Peer enable time.
/// Does not create a Trusted Peer relationship.
///
/// `learned_from` records the Bootstrap base URL or peer endpoint that delivered
/// the advertisement sample. Sources accumulate on the known advertisement and
/// are copied into [`OutboundDiscoveryPeer`] at selection.
///
/// A successful learn of a fresh verified advertisement **un-evicts** a prior
/// transport/policy eviction for that node so selection may re-admit it.
///
/// # Errors
///
/// Returns a typed failure when verification fails.
pub fn learn_discovery_peer_advertisement(
    store: &mut InMemoryStore,
    advertisement: DiscoveryPeerAdvertisement,
    learned_from: Option<&str>,
    probe: Option<&dyn DiscoveryPeerProbe>,
    now: DateTime<Utc>,
) -> Result<KnownDiscoveryPeerAdvertisement, DiscoveryPeerSyncFailure> {
    verify_peer_advertisement_locally(&advertisement, probe, now)?;

    let mut sources = store
        .known_discovery_peer_advertisements
        .get(&advertisement.node_id)
        .map(|known| known.learned_from.clone())
        .unwrap_or_default();
    if let Some(source) = learned_from {
        sources.insert(source.to_string());
    }

    let known = KnownDiscoveryPeerAdvertisement {
        advertisement: advertisement.clone(),
        received_at: now,
        learned_from: sources.clone(),
    };
    store
        .known_discovery_peer_advertisements
        .insert(advertisement.node_id, known.clone());

    // Fresh verified learn makes a previously evicted peer re-selectable.
    if let Some(sync) = store
        .discovery_peer_sync_states
        .get_mut(&advertisement.node_id)
    {
        if sync.health == DiscoveryPeerHealth::Evicted {
            sync.health = DiscoveryPeerHealth::Healthy;
            sync.consecutive_failures = 0;
            sync.backoff_until = None;
            sync.last_error = None;
        }
    }

    // If this peer is already outbound, refresh endpoint / advertisement identity
    // and accumulate sample provenance without changing Trusted Peer state.
    if let Some(outbound) = store
        .outbound_discovery_peers
        .get_mut(&advertisement.node_id)
    {
        outbound.public_endpoint = advertisement.public_endpoint.clone();
        outbound.advertisement_id = advertisement.id;
        outbound.protocol_version = advertisement.protocol_version.clone();
        outbound.learned_from = sources;
    }

    Ok(known)
}

/// Verifies a peer advertisement for local learning without Bootstrap rate limits.
pub(super) fn verify_peer_advertisement_locally(
    advertisement: &DiscoveryPeerAdvertisement,
    probe: Option<&dyn DiscoveryPeerProbe>,
    now: DateTime<Utc>,
) -> Result<(), DiscoveryPeerSyncFailure> {
    if advertisement.node_id != advertisement.signer.node_id
        || advertisement.signer.public_key.trim().is_empty()
    {
        return Err(DiscoveryPeerSyncFailure::new(
            DiscoveryPeerSyncFailureKind::Validation,
            "peer advertisement identity is inconsistent",
        ));
    }
    if advertisement.protocol_version != CURRENT_PROTOCOL_VERSION
        || advertisement.signer.supported_protocol_version != CURRENT_PROTOCOL_VERSION
    {
        return Err(DiscoveryPeerSyncFailure::new(
            DiscoveryPeerSyncFailureKind::IncompatibleProtocol,
            "peer advertisement protocol is incompatible",
        ));
    }
    if advertisement.capability != DiscoveryPeerCapability::AnnouncementServing {
        return Err(DiscoveryPeerSyncFailure::new(
            DiscoveryPeerSyncFailureKind::Validation,
            "peer advertisement capability is not announcement_serving",
        ));
    }
    let endpoint =
        normalize_discovery_peer_endpoint(&advertisement.public_endpoint).map_err(|error| {
            DiscoveryPeerSyncFailure::new(
                DiscoveryPeerSyncFailureKind::Validation,
                format!("peer endpoint policy violation: {error:?}"),
            )
        })?;
    if endpoint != advertisement.public_endpoint {
        return Err(DiscoveryPeerSyncFailure::new(
            DiscoveryPeerSyncFailureKind::Malformed,
            "peer advertisement endpoint is not normalized",
        ));
    }
    if !advertisement.lease_is_active(now) {
        return Err(DiscoveryPeerSyncFailure::new(
            DiscoveryPeerSyncFailureKind::ExpiredAdvertisement,
            "peer advertisement lease is expired",
        ));
    }
    match advertisement.verify() {
        Ok(true) => {}
        Ok(false) | Err(_) => {
            return Err(DiscoveryPeerSyncFailure::new(
                DiscoveryPeerSyncFailureKind::InvalidSignature,
                "peer advertisement signature verification failed",
            ));
        }
    }
    if let Some(probe) = probe {
        let view = probe
            .probe_peer_endpoint(&advertisement.public_endpoint)
            .map_err(|error| match error {
                DiscoveryPeerProbeError::Unreachable => DiscoveryPeerSyncFailure::new(
                    DiscoveryPeerSyncFailureKind::Unreachable,
                    "peer endpoint is not reachable",
                ),
            })?;
        if view.node_id != advertisement.node_id
            || view.public_key != advertisement.signer.public_key
            || view.protocol_version != advertisement.protocol_version
        {
            return Err(DiscoveryPeerSyncFailure::new(
                DiscoveryPeerSyncFailureKind::Validation,
                "reachable peer identity does not match advertisement",
            ));
        }
    }
    Ok(())
}

/// Deterministically selects a bounded outbound peer set from known advertisements.
///
/// Selection is randomized under `seed` for test control. It never inserts
/// Trusted Peer rows. Expired, incompatible, and self peers are skipped.
/// Peers still marked [`DiscoveryPeerHealth::Evicted`] are skipped until a
/// fresh verified advertisement is learned (which un-evicts them). Existing
/// healthy/backed-off outbound peers are retained until capacity is refilled.
///
/// Copies multi-source `learned_from` provenance from the known advertisement
/// into each newly selected [`OutboundDiscoveryPeer`].
///
/// Returns the selected peers after mutation.
pub fn select_outbound_discovery_peers(
    store: &mut InMemoryStore,
    local_node_id: Option<NodeIdentityId>,
    now: DateTime<Utc>,
    seed: u64,
) -> Vec<OutboundDiscoveryPeer> {
    if !peer_gossip_is_enabled(store) {
        return list_active_outbound_peers(store);
    }
    let max = max_outbound_peers(store);

    // Drop outbound peers whose advertisements expired or became invalid.
    let stale: Vec<NodeIdentityId> = store
        .outbound_discovery_peers
        .iter()
        .filter_map(|(node_id, peer)| {
            let known = store.known_discovery_peer_advertisements.get(node_id)?;
            let ad = &known.advertisement;
            if !ad.lease_is_active(now)
                || ad.protocol_version != CURRENT_PROTOCOL_VERSION
                || ad.verify().unwrap_or(false) == false
                || ad.public_endpoint != peer.public_endpoint
            {
                Some(*node_id)
            } else {
                None
            }
        })
        .collect();
    for node_id in stale {
        mark_peer_evicted(store, node_id, now, "peer advertisement no longer valid");
        store.outbound_discovery_peers.remove(&node_id);
    }

    // Active outbound count after pruning.
    let mut active: Vec<NodeIdentityId> = store
        .outbound_discovery_peers
        .keys()
        .copied()
        .filter(|node_id| {
            store
                .discovery_peer_sync_states
                .get(node_id)
                .map(|state| state.health != DiscoveryPeerHealth::Evicted)
                .unwrap_or(true)
        })
        .collect();

    if active.len() >= max {
        active.sort();
        return active
            .into_iter()
            .filter_map(|id| store.outbound_discovery_peers.get(&id).cloned())
            .collect();
    }

    let mut candidates: Vec<(DiscoveryPeerAdvertisement, BTreeSet<String>)> = store
        .known_discovery_peer_advertisements
        .values()
        .filter(|known| {
            let ad = &known.advertisement;
            ad.lease_is_active(now)
                && ad.protocol_version == CURRENT_PROTOCOL_VERSION
                && ad.capability == DiscoveryPeerCapability::AnnouncementServing
                && ad.verify().unwrap_or(false)
                && local_node_id != Some(ad.node_id)
                && !store.outbound_discovery_peers.contains_key(&ad.node_id)
                && store
                    .discovery_peer_sync_states
                    .get(&ad.node_id)
                    .map(|state| state.health != DiscoveryPeerHealth::Evicted)
                    .unwrap_or(true)
        })
        .map(|known| (known.advertisement.clone(), known.learned_from.clone()))
        .collect();

    // Deterministic shuffle (Fisher–Yates) seeded for test control.
    let mut state = seed;
    for i in (1..candidates.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = (state as usize) % (i + 1);
        candidates.swap(i, j);
    }

    let need = max.saturating_sub(active.len());
    for (ad, learned_from) in candidates.into_iter().take(need) {
        let peer = OutboundDiscoveryPeer {
            node_id: ad.node_id,
            public_endpoint: ad.public_endpoint.clone(),
            advertisement_id: ad.id,
            protocol_version: ad.protocol_version.clone(),
            selected_at: now,
            learned_from,
        };
        store.outbound_discovery_peers.insert(ad.node_id, peer);
        store
            .discovery_peer_sync_states
            .entry(ad.node_id)
            .or_insert_with(|| empty_sync_state(ad.node_id));
        // Ensure health is healthy when (re)selected.
        if let Some(sync) = store.discovery_peer_sync_states.get_mut(&ad.node_id) {
            if sync.health == DiscoveryPeerHealth::Evicted {
                sync.health = DiscoveryPeerHealth::Healthy;
                sync.consecutive_failures = 0;
                sync.backoff_until = None;
                sync.last_error = None;
            }
        }
    }

    list_active_outbound_peers(store)
}

/// Returns currently selected non-evicted outbound peers.
#[must_use]
pub fn list_active_outbound_peers(store: &InMemoryStore) -> Vec<OutboundDiscoveryPeer> {
    let mut peers: Vec<_> = store
        .outbound_discovery_peers
        .values()
        .filter(|peer| {
            store
                .discovery_peer_sync_states
                .get(&peer.node_id)
                .map(|state| state.health != DiscoveryPeerHealth::Evicted)
                .unwrap_or(true)
        })
        .cloned()
        .collect();
    peers.sort_by(|left, right| {
        left.selected_at
            .cmp(&right.selected_at)
            .then_with(|| left.node_id.cmp(&right.node_id))
    });
    peers
}

/// Operator view of outbound peers joined with sync state.
#[must_use]
pub fn outbound_discovery_peer_statuses(store: &InMemoryStore) -> Vec<OutboundDiscoveryPeerStatus> {
    list_active_outbound_peers(store)
        .into_iter()
        .map(|peer| {
            let sync = store
                .discovery_peer_sync_states
                .get(&peer.node_id)
                .cloned()
                .unwrap_or_else(|| empty_sync_state(peer.node_id));
            OutboundDiscoveryPeerStatus { peer, sync }
        })
        .collect()
}
