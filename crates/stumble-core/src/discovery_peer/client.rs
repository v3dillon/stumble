//! Outbound Home Node Discovery Peer rotation and Bootstrap-outage survival.
//!
//! Home Nodes learn signed peer advertisements from Bootstrap Nodes and existing
//! Discovery Peers, select a small rotating outbound set without granting Trusted
//! Peer status, and synchronize only Origin-signed public announcement lifecycle
//! artifacts. Network I/O is separated from store mutation so callers avoid
//! holding store locks across HTTP.

use super::endpoint::normalize_discovery_peer_endpoint;
use super::probe::{DiscoveryPeerProbe, DiscoveryPeerProbeError};
use super::types::{DEFAULT_PEER_SAMPLE_LIMIT, MAX_PEER_SAMPLE_LIMIT};
use crate::domain::{
    AnnouncementStreamEventKind, AnnouncementStreamPage, BootstrapStreamRequest,
    DiscoveryPeerAdvertisement, DiscoveryPeerAdvertisementSample, DiscoveryPeerCapability,
    DiscoveryPeerGossipConfig, DiscoveryPeerHealth, DiscoveryPeerSampleRequest,
    DiscoveryPeerSyncFailure, DiscoveryPeerSyncFailureKind, DiscoveryPeerSyncOutcome,
    DiscoveryPeerSyncReport, DiscoveryPeerSyncState, DiscoveryStatus,
    KnownDiscoveryPeerAdvertisement, NodeIdentityId, OutboundDiscoveryPeer,
    OutboundDiscoveryPeerStatus, CURRENT_PROTOCOL_VERSION, DEFAULT_MAX_OUTBOUND_DISCOVERY_PEERS,
    MAX_OUTBOUND_DISCOVERY_PEERS, MAX_PEER_INVALID_ENTRIES_PER_PAGE, PEER_FAILURES_BEFORE_EVICTION,
};
use crate::pod_announcement::{
    retain_verified_pod_announcement, retain_verified_pod_withdrawal, DeliveryProvenance,
};
use crate::store::{InMemoryStore, StoreError};
use chrono::{DateTime, Duration, Utc};
use std::collections::{BTreeSet, HashMap};

/// Maximum pages fetched from one Discovery Peer during a single sync pass.
const MAX_PAGES_PER_PEER: usize = 32;

/// Default page size requested by outbound Discovery Peer stream sync.
const DEFAULT_SYNC_PAGE_LIMIT: usize = 50;

/// Base backoff after the first consecutive peer failure.
const BASE_BACKOFF_SECONDS: i64 = 30;

/// Transport port for fetching unranked peer-advertisement samples.
///
/// Production implementations perform HTTP GET against Bootstrap or peer sample
/// paths. Tests inject scripted samples. Requests must carry only
/// [`DiscoveryPeerSampleRequest`] fields.
pub trait PeerAdvertisementSampleClient: Send + Sync {
    /// Fetches one unranked peer-advertisement sample from `base_url`.
    ///
    /// # Errors
    ///
    /// Returns a typed [`DiscoveryPeerSyncFailure`] for transport or protocol errors.
    fn fetch_peer_advertisement_sample(
        &self,
        base_url: &str,
        request: &DiscoveryPeerSampleRequest,
    ) -> Result<DiscoveryPeerAdvertisementSample, DiscoveryPeerSyncFailure>;
}

/// Transport port for fetching Discovery Peer Announcement Stream pages.
///
/// Production implementations perform HTTP GET against
/// `{endpoint}/discovery/peer/announcements/stream`. Tests inject scripted pages.
/// Requests must carry only cursor pagination fields.
pub trait DiscoveryPeerStreamClient: Send + Sync {
    /// Fetches one stream page from a Discovery Peer `base_url`.
    ///
    /// # Errors
    ///
    /// Returns a typed [`DiscoveryPeerSyncFailure`] for transport or protocol errors.
    fn fetch_peer_announcement_stream(
        &self,
        base_url: &str,
        request: &BootstrapStreamRequest,
    ) -> Result<AnnouncementStreamPage, DiscoveryPeerSyncFailure>;
}

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
fn verify_peer_advertisement_locally(
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

/// Snapshot of one peer for a lock-free fetch plan.
#[derive(Debug, Clone)]
pub struct DiscoveryPeerSyncPlan {
    /// Selected outbound peer.
    pub peer: OutboundDiscoveryPeer,
    /// Cursor to resume from, when previously persisted.
    pub cursor: Option<String>,
}

/// Pages fetched from one peer before any store mutation.
#[derive(Debug, Clone)]
pub struct FetchedDiscoveryPeerStream {
    /// Successfully fetched pages in order (may be empty).
    pub pages: Vec<AnnouncementStreamPage>,
    /// Cursor used for the first page request.
    pub start_cursor: Option<String>,
    /// Transport/protocol failure after the last successful page, if any.
    pub fetch_error: Option<DiscoveryPeerSyncFailure>,
}

/// Builds the ordered plan of outbound peers ready for sync (read-only).
#[must_use]
pub fn plan_discovery_peer_sync(
    store: &InMemoryStore,
    now: DateTime<Utc>,
) -> Vec<DiscoveryPeerSyncPlan> {
    if !peer_gossip_is_enabled(store) {
        return Vec::new();
    }
    list_active_outbound_peers(store)
        .into_iter()
        .filter(|peer| {
            let Some(state) = store.discovery_peer_sync_states.get(&peer.node_id) else {
                return true;
            };
            if state.health == DiscoveryPeerHealth::Evicted {
                return false;
            }
            state
                .backoff_until
                .map(|until| until <= now)
                .unwrap_or(true)
        })
        .map(|peer| {
            let cursor = store
                .discovery_peer_sync_states
                .get(&peer.node_id)
                .and_then(|state| state.cursor.clone());
            DiscoveryPeerSyncPlan { peer, cursor }
        })
        .collect()
}

/// Fetches stream pages for one peer without touching the store.
#[must_use]
pub fn fetch_discovery_peer_stream_pages(
    client: &dyn DiscoveryPeerStreamClient,
    base_url: &str,
    start_cursor: Option<String>,
) -> FetchedDiscoveryPeerStream {
    let mut pages = Vec::new();
    let mut cursor = start_cursor.clone();

    for _ in 0..MAX_PAGES_PER_PEER {
        let request = BootstrapStreamRequest {
            cursor: cursor.clone(),
            limit: Some(DEFAULT_SYNC_PAGE_LIMIT),
        };
        debug_assert!(peer_stream_request_is_public_only(&request));

        let page = match client.fetch_peer_announcement_stream(base_url, &request) {
            Ok(page) => page,
            Err(error) => {
                return FetchedDiscoveryPeerStream {
                    pages,
                    start_cursor,
                    fetch_error: Some(error),
                };
            }
        };

        let next_cursor = page.next_cursor.clone();
        pages.push(page);
        match next_cursor {
            Some(next) if next != cursor.clone().unwrap_or_default() => {
                cursor = Some(next);
            }
            _ => {
                return FetchedDiscoveryPeerStream {
                    pages,
                    start_cursor,
                    fetch_error: None,
                };
            }
        }
    }

    FetchedDiscoveryPeerStream {
        pages,
        start_cursor,
        fetch_error: None,
    }
}

/// Applies previously fetched peer stream pages and updates cursor / health.
pub fn apply_discovery_peer_stream_pages(
    store: &mut InMemoryStore,
    peer: &OutboundDiscoveryPeer,
    fetched: FetchedDiscoveryPeerStream,
    now: DateTime<Utc>,
) -> DiscoveryPeerSyncOutcome {
    let mut state = store
        .discovery_peer_sync_states
        .get(&peer.node_id)
        .cloned()
        .unwrap_or_else(|| empty_sync_state(peer.node_id));
    state.last_attempt_at = Some(now);

    let mut cursor = fetched.start_cursor;
    let mut pages_fetched = 0usize;
    let mut retained_announcements = 0usize;
    let mut retained_withdrawals = 0usize;

    for page in &fetched.pages {
        match apply_peer_stream_page_staged(store, &peer.public_endpoint, page, now) {
            Ok((announcements, withdrawals)) => {
                retained_announcements = retained_announcements.saturating_add(announcements);
                retained_withdrawals = retained_withdrawals.saturating_add(withdrawals);
                pages_fetched = pages_fetched.saturating_add(1);
                match &page.next_cursor {
                    Some(next) if next != &cursor.clone().unwrap_or_default() => {
                        cursor = Some(next.clone());
                        state.cursor = cursor.clone();
                    }
                    _ => {
                        cursor = page.next_cursor.clone().or(cursor);
                        state.cursor = cursor.clone();
                        state.last_success_at = Some(now);
                        state.last_error = None;
                        state.consecutive_failures = 0;
                        state.backoff_until = None;
                        state.health = DiscoveryPeerHealth::Healthy;
                        store
                            .discovery_peer_sync_states
                            .insert(peer.node_id, state.clone());
                        return peer_outcome(
                            peer,
                            true,
                            pages_fetched,
                            retained_announcements,
                            retained_withdrawals,
                            cursor,
                            state.health,
                            None,
                        );
                    }
                }
            }
            Err(error) => {
                record_peer_failure(store, peer, &mut state, error.clone(), now);
                return peer_outcome(
                    peer,
                    false,
                    pages_fetched,
                    retained_announcements,
                    retained_withdrawals,
                    cursor,
                    state.health,
                    Some(error),
                );
            }
        }
    }

    if let Some(error) = fetched.fetch_error {
        record_peer_failure(store, peer, &mut state, error.clone(), now);
        return peer_outcome(
            peer,
            false,
            pages_fetched,
            retained_announcements,
            retained_withdrawals,
            cursor,
            state.health,
            Some(error),
        );
    }

    state.cursor = cursor.clone();
    state.last_success_at = Some(now);
    state.last_error = None;
    state.consecutive_failures = 0;
    state.backoff_until = None;
    state.health = DiscoveryPeerHealth::Healthy;
    store
        .discovery_peer_sync_states
        .insert(peer.node_id, state.clone());
    peer_outcome(
        peer,
        true,
        pages_fetched,
        retained_announcements,
        retained_withdrawals,
        cursor,
        state.health,
        None,
    )
}

/// Synchronizes Announcement Streams from each viable outbound Discovery Peer.
///
/// Invalid data, flooding, incompatible versions, expired advertisements, or
/// repeated transport failures cause bounded backoff and automatic local eviction.
pub fn sync_outbound_discovery_peers(
    store: &mut InMemoryStore,
    client: &dyn DiscoveryPeerStreamClient,
    now: DateTime<Utc>,
) -> DiscoveryPeerSyncReport {
    let plans = plan_discovery_peer_sync(store, now);
    let mut outcomes = Vec::with_capacity(plans.len());
    let mut retained_announcements = 0usize;
    let mut retained_withdrawals = 0usize;
    let mut evicted = Vec::new();

    for plan in plans {
        // Skip peers whose advertisement expired between plan and fetch.
        if let Some(known) = store
            .known_discovery_peer_advertisements
            .get(&plan.peer.node_id)
        {
            if !known.advertisement.lease_is_active(now) {
                mark_peer_evicted(
                    store,
                    plan.peer.node_id,
                    now,
                    "peer advertisement expired before sync",
                );
                store.outbound_discovery_peers.remove(&plan.peer.node_id);
                evicted.push(plan.peer.node_id);
                outcomes.push(peer_outcome(
                    &plan.peer,
                    false,
                    0,
                    0,
                    0,
                    plan.cursor.clone(),
                    DiscoveryPeerHealth::Evicted,
                    Some(DiscoveryPeerSyncFailure::new(
                        DiscoveryPeerSyncFailureKind::ExpiredAdvertisement,
                        "peer advertisement expired before sync",
                    )),
                ));
                continue;
            }
        }

        let fetched =
            fetch_discovery_peer_stream_pages(client, &plan.peer.public_endpoint, plan.cursor);
        let outcome = apply_discovery_peer_stream_pages(store, &plan.peer, fetched, now);
        if outcome.health == DiscoveryPeerHealth::Evicted {
            evicted.push(outcome.node_id);
            store.outbound_discovery_peers.remove(&outcome.node_id);
        }
        retained_announcements =
            retained_announcements.saturating_add(outcome.retained_announcements);
        retained_withdrawals = retained_withdrawals.saturating_add(outcome.retained_withdrawals);
        outcomes.push(outcome);
    }

    DiscoveryPeerSyncReport {
        outcomes,
        retained_announcements,
        retained_withdrawals,
        evicted,
    }
}

/// One peer-advertisement sample fetch result keyed by delivery source URL.
///
/// Produced by [`fetch_peer_advertisement_samples`] without holding the store.
pub type FetchedPeerAdvertisementSample = (
    String,
    Result<DiscoveryPeerAdvertisementSample, DiscoveryPeerSyncFailure>,
);

/// Fetches peer-advertisement samples from each source without store access.
///
/// Callers must run this **outside** any store write lock (HTTP I/O). Sources
/// are sorted and deduplicated. Per-source transport failures are returned in
/// the result vector so callers can soft-skip them.
#[must_use]
pub fn fetch_peer_advertisement_samples(
    sample_client: &dyn PeerAdvertisementSampleClient,
    sources: &[String],
) -> Vec<FetchedPeerAdvertisementSample> {
    let request = DiscoveryPeerSampleRequest {
        limit: Some(DEFAULT_PEER_SAMPLE_LIMIT.clamp(1, MAX_PEER_SAMPLE_LIMIT)),
    };
    debug_assert!(peer_sample_request_is_public_only(&request));

    let mut ordered: Vec<String> = sources.to_vec();
    ordered.sort();
    ordered.dedup();

    ordered
        .into_iter()
        .map(|source| {
            let result = sample_client.fetch_peer_advertisement_sample(&source, &request);
            (source, result)
        })
        .collect()
}

/// Verifies and retains previously fetched samples, then selects outbound peers.
///
/// Intended to run under a short store write lock after
/// [`fetch_peer_advertisement_samples`] completed without holding the store.
/// Soft-skips invalid advertisements and failed sample sources.
///
/// `probe` is optional: pass `None` for signed-ad local verification only
/// (production default for outbound learning). Inject a real probe when live
/// reachability/identity match is required.
pub fn retain_learned_samples_and_select(
    store: &mut InMemoryStore,
    fetched: &[FetchedPeerAdvertisementSample],
    probe: Option<&dyn DiscoveryPeerProbe>,
    local_node_id: Option<NodeIdentityId>,
    now: DateTime<Utc>,
    selection_seed: u64,
) -> Vec<OutboundDiscoveryPeer> {
    if !peer_gossip_is_enabled(store) {
        return list_active_outbound_peers(store);
    }

    for (source, result) in fetched {
        match result {
            Ok(sample) => {
                for advertisement in &sample.advertisements {
                    // Soft-skip individual invalid ads; hard failures only on
                    // transport for the sample itself (already filtered).
                    let _ = learn_discovery_peer_advertisement(
                        store,
                        advertisement.clone(),
                        Some(source.as_str()),
                        probe,
                        now,
                    );
                }
            }
            Err(_error) => {
                // Sample source fallthrough: one unavailable Bootstrap/peer does
                // not block learning from others.
            }
        }
    }

    select_outbound_discovery_peers(store, local_node_id, now, selection_seed)
}

/// Learns peer advertisements from Bootstrap endpoints and existing outbound peers.
///
/// Convenience composition of [`fetch_peer_advertisement_samples`] then
/// [`retain_learned_samples_and_select`]. Prefer the split pair in AgentTools so
/// HTTP sample fetches never run under the store write lock.
///
/// `seed` controls local selection determinism after learning.
pub fn learn_peers_from_sample_sources(
    store: &mut InMemoryStore,
    sample_client: &dyn PeerAdvertisementSampleClient,
    bootstrap_base_urls: &[String],
    peer_endpoints: &[String],
    probe: Option<&dyn DiscoveryPeerProbe>,
    local_node_id: Option<NodeIdentityId>,
    now: DateTime<Utc>,
    selection_seed: u64,
) -> Result<Vec<OutboundDiscoveryPeer>, DiscoveryPeerSyncFailure> {
    if !peer_gossip_is_enabled(store) {
        return Ok(list_active_outbound_peers(store));
    }

    let mut sources: Vec<String> = bootstrap_base_urls
        .iter()
        .chain(peer_endpoints.iter())
        .cloned()
        .collect();
    sources.sort();
    sources.dedup();

    // Note: this convenience path still performs I/O while `store` is borrowed.
    // Production AgentTools must call fetch + retain separately.
    let fetched = fetch_peer_advertisement_samples(sample_client, &sources);
    Ok(retain_learned_samples_and_select(
        store,
        &fetched,
        probe,
        local_node_id,
        now,
        selection_seed,
    ))
}

/// Reports discovery readiness, including degraded mode for fresh nodes without
/// a viable Bootstrap while preserving direct Pod URL operation.
#[must_use]
pub fn discovery_status(store: &InMemoryStore) -> DiscoveryStatus {
    let automatic_gossip_enabled = peer_gossip_is_enabled(store);
    let enabled_bootstrap_count = store
        .bootstrap_endpoints
        .values()
        .filter(|endpoint| endpoint.enabled)
        .count();
    let active_outbound_peer_count = list_active_outbound_peers(store).len();
    let any_bootstrap_success = store.bootstrap_sync_states.values().any(|state| {
        state.last_success_at.is_some()
            && store
                .bootstrap_endpoints
                .get(&state.endpoint_id)
                .is_some_and(|endpoint| endpoint.enabled)
    });
    let any_peer_success = store.discovery_peer_sync_states.values().any(|state| {
        state.last_success_at.is_some() && state.health != DiscoveryPeerHealth::Evicted
    });

    // Degraded when there is no enabled Bootstrap, or every enabled Bootstrap has
    // never succeeded and there is also no successful peer path yet.
    let degraded = if enabled_bootstrap_count == 0 {
        true
    } else {
        !any_bootstrap_success && !any_peer_success
    };

    let (degraded_reason, message) = if !degraded {
        (
            None,
            "discovery is operating with configured Bootstrap and/or Discovery Peers".to_string(),
        )
    } else if enabled_bootstrap_count == 0 {
        (
            Some("no_enabled_bootstrap".to_string()),
            "discovery is degraded: no enabled Bootstrap endpoint; direct Pod URLs still work"
                .to_string(),
        )
    } else {
        (
            Some("no_viable_bootstrap".to_string()),
            "discovery is degraded: no viable Bootstrap contact yet; direct Pod URLs still work"
                .to_string(),
        )
    };

    DiscoveryStatus {
        automatic_gossip_enabled,
        enabled_bootstrap_count,
        active_outbound_peer_count,
        degraded,
        degraded_reason,
        message,
    }
}

/// Asserts an outbound peer sample request carries only public fields.
#[must_use]
pub fn peer_sample_request_is_public_only(request: &DiscoveryPeerSampleRequest) -> bool {
    let Ok(value) = serde_json::to_value(request) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    let allowed: BTreeSet<&str> = ["limit"].into_iter().collect();
    object.keys().all(|key| allowed.contains(key.as_str()))
        && !object.contains_key("taste_profile")
        && !object.contains_key("subscriptions")
        && !object.contains_key("feedback")
        && !object.contains_key("admin")
        && !object.contains_key("private")
}

/// Asserts an outbound peer stream request carries only public pagination fields.
#[must_use]
pub fn peer_stream_request_is_public_only(request: &BootstrapStreamRequest) -> bool {
    crate::bootstrap::request_is_public_only(request)
}

fn empty_sync_state(node_id: NodeIdentityId) -> DiscoveryPeerSyncState {
    DiscoveryPeerSyncState {
        node_id,
        cursor: None,
        last_success_at: None,
        last_attempt_at: None,
        consecutive_failures: 0,
        backoff_until: None,
        health: DiscoveryPeerHealth::Healthy,
        last_error: None,
    }
}

fn peer_outcome(
    peer: &OutboundDiscoveryPeer,
    ok: bool,
    pages_fetched: usize,
    retained_announcements: usize,
    retained_withdrawals: usize,
    cursor: Option<String>,
    health: DiscoveryPeerHealth,
    error: Option<DiscoveryPeerSyncFailure>,
) -> DiscoveryPeerSyncOutcome {
    DiscoveryPeerSyncOutcome {
        node_id: peer.node_id,
        public_endpoint: peer.public_endpoint.clone(),
        ok,
        pages_fetched,
        retained_announcements,
        retained_withdrawals,
        cursor,
        health,
        error,
    }
}

fn record_peer_failure(
    store: &mut InMemoryStore,
    peer: &OutboundDiscoveryPeer,
    state: &mut DiscoveryPeerSyncState,
    error: DiscoveryPeerSyncFailure,
    now: DateTime<Utc>,
) {
    state.last_error = Some(error.clone());
    state.consecutive_failures = state.consecutive_failures.saturating_add(1);

    // Immediate eviction for policy/abuse classes.
    let immediate = matches!(
        error.kind,
        DiscoveryPeerSyncFailureKind::InvalidSignature
            | DiscoveryPeerSyncFailureKind::Flooding
            | DiscoveryPeerSyncFailureKind::IncompatibleProtocol
            | DiscoveryPeerSyncFailureKind::ExpiredAdvertisement
    );
    if immediate || state.consecutive_failures >= PEER_FAILURES_BEFORE_EVICTION {
        state.health = DiscoveryPeerHealth::Evicted;
        state.backoff_until = None;
        store
            .discovery_peer_sync_states
            .insert(peer.node_id, state.clone());
        store.outbound_discovery_peers.remove(&peer.node_id);
        return;
    }

    let backoff_secs =
        BASE_BACKOFF_SECONDS.saturating_mul(1_i64 << (state.consecutive_failures.min(5) - 1));
    state.backoff_until = Some(now + Duration::seconds(backoff_secs));
    state.health = DiscoveryPeerHealth::BackedOff;
    store
        .discovery_peer_sync_states
        .insert(peer.node_id, state.clone());
}

fn mark_peer_evicted(
    store: &mut InMemoryStore,
    node_id: NodeIdentityId,
    now: DateTime<Utc>,
    message: &str,
) {
    let mut state = store
        .discovery_peer_sync_states
        .get(&node_id)
        .cloned()
        .unwrap_or_else(|| empty_sync_state(node_id));
    state.last_attempt_at = Some(now);
    state.health = DiscoveryPeerHealth::Evicted;
    state.last_error = Some(DiscoveryPeerSyncFailure::new(
        DiscoveryPeerSyncFailureKind::ExpiredAdvertisement,
        message,
    ));
    store.discovery_peer_sync_states.insert(node_id, state);
}

fn map_retain_error(error: StoreError, subject: &str) -> Result<(), DiscoveryPeerSyncFailure> {
    match error {
        StoreError::AnnouncementStale
        | StoreError::AnnouncementExpired
        | StoreError::AnnouncementWithdrawn
        | StoreError::WithdrawalStale => Ok(()),
        StoreError::InvalidSignature => Err(DiscoveryPeerSyncFailure::new(
            DiscoveryPeerSyncFailureKind::InvalidSignature,
            format!("{subject} signature verification failed"),
        )),
        StoreError::Validation(message) => Err(DiscoveryPeerSyncFailure::new(
            DiscoveryPeerSyncFailureKind::Validation,
            message,
        )),
        error => Err(DiscoveryPeerSyncFailure::new(
            DiscoveryPeerSyncFailureKind::Protocol,
            error.to_string(),
        )),
    }
}

fn apply_peer_stream_page_staged(
    store: &mut InMemoryStore,
    peer_endpoint: &str,
    page: &AnnouncementStreamPage,
    now: DateTime<Utc>,
) -> Result<(usize, usize), DiscoveryPeerSyncFailure> {
    let before_announcements = store.known_pod_announcements.clone();
    let before_withdrawals = store.known_pod_withdrawals.clone();
    match apply_peer_stream_page(store, peer_endpoint, page, now) {
        Ok(counts) => Ok(counts),
        Err(error) => {
            store.known_pod_announcements = before_announcements;
            store.known_pod_withdrawals = before_withdrawals;
            Err(error)
        }
    }
}

fn apply_peer_stream_page(
    store: &mut InMemoryStore,
    peer_endpoint: &str,
    page: &AnnouncementStreamPage,
    now: DateTime<Utc>,
) -> Result<(usize, usize), DiscoveryPeerSyncFailure> {
    let mut retained_announcements = 0usize;
    let mut retained_withdrawals = 0usize;
    let mut invalid_count = 0usize;

    for entry in &page.entries {
        match entry.kind {
            AnnouncementStreamEventKind::Admitted | AnnouncementStreamEventKind::Renewed => {
                let Some(announcement) = entry.payload.as_announcement().cloned() else {
                    invalid_count = invalid_count.saturating_add(1);
                    if invalid_count > MAX_PEER_INVALID_ENTRIES_PER_PAGE {
                        return Err(DiscoveryPeerSyncFailure::new(
                            DiscoveryPeerSyncFailureKind::Flooding,
                            "peer stream flooded with malformed announcement entries",
                        ));
                    }
                    continue;
                };
                match retain_verified_pod_announcement(
                    store,
                    announcement,
                    DeliveryProvenance::discovery_peer(peer_endpoint),
                    now,
                ) {
                    Ok(_) => retained_announcements = retained_announcements.saturating_add(1),
                    Err(error) => {
                        if matches!(error, StoreError::InvalidSignature) {
                            invalid_count = invalid_count.saturating_add(1);
                            if invalid_count > MAX_PEER_INVALID_ENTRIES_PER_PAGE {
                                return Err(DiscoveryPeerSyncFailure::new(
                                    DiscoveryPeerSyncFailureKind::Flooding,
                                    "peer stream flooded with invalid signatures",
                                ));
                            }
                            // Single invalid signature is a hard failure for the page
                            // (peer delivered forged Origin bytes).
                            return Err(DiscoveryPeerSyncFailure::new(
                                DiscoveryPeerSyncFailureKind::InvalidSignature,
                                "announcement signature verification failed",
                            ));
                        }
                        map_retain_error(error, "announcement")?;
                    }
                }
            }
            AnnouncementStreamEventKind::Withdrawn => {
                let Some(withdrawal) = entry.payload.as_withdrawal().cloned() else {
                    invalid_count = invalid_count.saturating_add(1);
                    if invalid_count > MAX_PEER_INVALID_ENTRIES_PER_PAGE {
                        return Err(DiscoveryPeerSyncFailure::new(
                            DiscoveryPeerSyncFailureKind::Flooding,
                            "peer stream flooded with malformed withdrawal entries",
                        ));
                    }
                    continue;
                };
                match retain_verified_pod_withdrawal(store, withdrawal, None, now) {
                    Ok(_) => retained_withdrawals = retained_withdrawals.saturating_add(1),
                    Err(error) => {
                        if matches!(error, StoreError::InvalidSignature) {
                            return Err(DiscoveryPeerSyncFailure::new(
                                DiscoveryPeerSyncFailureKind::InvalidSignature,
                                "withdrawal signature verification failed",
                            ));
                        }
                        map_retain_error(error, "withdrawal")?;
                    }
                }
            }
            AnnouncementStreamEventKind::Expired => {
                // Lease expiry is evaluated locally; stream notice needs no private state.
            }
        }
    }
    Ok((retained_announcements, retained_withdrawals))
}

/// In-memory scripted peer sample client for tests.
#[derive(Debug, Default)]
pub struct ScriptedPeerAdvertisementSampleClient {
    /// Samples keyed by normalized base URL.
    pub samples: HashMap<String, DiscoveryPeerAdvertisementSample>,
    /// Forced failures keyed by base URL.
    pub failures: HashMap<String, DiscoveryPeerSyncFailure>,
    /// Captured outbound requests for privacy assertions.
    pub captured: std::sync::Mutex<Vec<(String, DiscoveryPeerSampleRequest)>>,
}

impl ScriptedPeerAdvertisementSampleClient {
    /// Creates an empty scripted sample client.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a successful sample for `base_url`.
    pub fn push_sample(&mut self, base_url: &str, sample: DiscoveryPeerAdvertisementSample) {
        self.samples.insert(base_url.to_string(), sample);
    }

    /// Registers a forced failure for every sample fetch against `base_url`.
    pub fn fail(&mut self, base_url: &str, failure: DiscoveryPeerSyncFailure) {
        self.failures.insert(base_url.to_string(), failure);
    }
}

impl PeerAdvertisementSampleClient for ScriptedPeerAdvertisementSampleClient {
    fn fetch_peer_advertisement_sample(
        &self,
        base_url: &str,
        request: &DiscoveryPeerSampleRequest,
    ) -> Result<DiscoveryPeerAdvertisementSample, DiscoveryPeerSyncFailure> {
        if let Ok(mut captured) = self.captured.lock() {
            captured.push((base_url.to_string(), request.clone()));
        }
        if let Some(failure) = self.failures.get(base_url) {
            return Err(failure.clone());
        }
        self.samples.get(base_url).cloned().ok_or_else(|| {
            DiscoveryPeerSyncFailure::new(
                DiscoveryPeerSyncFailureKind::Transport,
                format!("no scripted peer sample for {base_url}"),
            )
        })
    }
}

/// In-memory scripted Discovery Peer stream client for tests.
#[derive(Debug, Default)]
pub struct ScriptedDiscoveryPeerStreamClient {
    /// Pages keyed by base URL, then by request cursor (`""` for start).
    pub pages: HashMap<String, HashMap<String, AnnouncementStreamPage>>,
    /// Forced failures keyed by base URL.
    pub failures: HashMap<String, DiscoveryPeerSyncFailure>,
    /// Captured outbound requests for privacy assertions.
    pub captured: std::sync::Mutex<Vec<(String, BootstrapStreamRequest)>>,
}

impl ScriptedDiscoveryPeerStreamClient {
    /// Creates an empty scripted stream client.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a successful page for `base_url` at the given cursor.
    pub fn push_page(
        &mut self,
        base_url: &str,
        cursor: Option<&str>,
        page: AnnouncementStreamPage,
    ) {
        let key = cursor.unwrap_or("").to_string();
        self.pages
            .entry(base_url.to_string())
            .or_default()
            .insert(key, page);
    }

    /// Registers a forced failure for every fetch against `base_url`.
    pub fn fail(&mut self, base_url: &str, failure: DiscoveryPeerSyncFailure) {
        self.failures.insert(base_url.to_string(), failure);
    }
}

impl DiscoveryPeerStreamClient for ScriptedDiscoveryPeerStreamClient {
    fn fetch_peer_announcement_stream(
        &self,
        base_url: &str,
        request: &BootstrapStreamRequest,
    ) -> Result<AnnouncementStreamPage, DiscoveryPeerSyncFailure> {
        if let Ok(mut captured) = self.captured.lock() {
            captured.push((base_url.to_string(), request.clone()));
        }
        if let Some(failure) = self.failures.get(base_url) {
            return Err(failure.clone());
        }
        let cursor_key = request.cursor.clone().unwrap_or_default();
        self.pages
            .get(base_url)
            .and_then(|pages| pages.get(&cursor_key))
            .cloned()
            .ok_or_else(|| {
                DiscoveryPeerSyncFailure::new(
                    DiscoveryPeerSyncFailureKind::Transport,
                    format!("no scripted peer stream page for {base_url} cursor {cursor_key:?}"),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery_peer::{
        peer_identity_view_for_node, FixedDiscoveryPeerProbe, UnreachableDiscoveryPeerProbe,
    };
    use crate::domain::{
        announcement_lease_duration, peer_advertisement_lease_duration, AnnouncementStreamEntry,
        AnnouncementStreamEventKind, AnnouncementStreamPayload, NodeInfo, PackageVersion,
        PodAnnouncement, CURRENT_PROTOCOL_VERSION,
    };
    use crate::pod_announcement::announcement_delivery_is_active;
    use crate::signing::{
        create_node_identity, sign_discovery_peer_advertisement, sign_pod_announcement,
    };
    use chrono::TimeZone;
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
        learn_discovery_peer_advertisement(
            &mut store,
            ad,
            Some("https://bootstrap.example"),
            None,
            t,
        )
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
}
