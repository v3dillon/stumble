use super::super::probe::DiscoveryPeerProbe;
use super::super::types::{DEFAULT_PEER_SAMPLE_LIMIT, MAX_PEER_SAMPLE_LIMIT};
use super::*;
use crate::domain::{
    AnnouncementStreamEventKind, AnnouncementStreamPage, BootstrapStreamRequest,
    DiscoveryPeerAdvertisementSample, DiscoveryPeerHealth, DiscoveryPeerSampleRequest,
    DiscoveryPeerSyncFailure, DiscoveryPeerSyncFailureKind, DiscoveryPeerSyncOutcome,
    DiscoveryPeerSyncReport, DiscoveryPeerSyncState, DiscoveryStatus, NodeIdentityId,
    OutboundDiscoveryPeer, MAX_PEER_INVALID_ENTRIES_PER_PAGE, PEER_FAILURES_BEFORE_EVICTION,
};
use crate::pod_announcement::{
    retain_verified_pod_announcement, retain_verified_pod_withdrawal, DeliveryProvenance,
};
use crate::store::{InMemoryStore, StoreError};
use crate::stream_sync::{
    apply_page_staged, drain_stream_pages, fetch_stream_pages, map_retain_error as classify_retain,
    FetchedStream, RetainFailure,
};
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeSet;

/// Base backoff after the first consecutive peer failure.
const BASE_BACKOFF_SECONDS: i64 = 30;

/// Backoff doublings cap: consecutive failures beyond this stop growing the delay.
const MAX_BACKOFF_DOUBLINGS: u32 = 5;

/// Snapshot of one peer for a lock-free fetch plan.
#[derive(Debug, Clone)]
pub struct DiscoveryPeerSyncPlan {
    /// Selected outbound peer.
    pub peer: OutboundDiscoveryPeer,
    /// Cursor to resume from, when previously persisted.
    pub cursor: Option<String>,
}

/// Pages fetched from one peer before any store mutation.
pub type FetchedDiscoveryPeerStream = FetchedStream<DiscoveryPeerSyncFailure>;

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
    fetch_stream_pages(start_cursor, |request| {
        // Privacy invariant: request serializes only cursor + limit.
        debug_assert!(peer_stream_request_is_public_only(request));
        client.fetch_peer_announcement_stream(base_url, request)
    })
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

    let drain = drain_stream_pages(
        store,
        fetched,
        |cursor| state.cursor = cursor.clone(),
        |store, page| apply_peer_stream_page_staged(store, &peer.public_endpoint, page, now),
    );

    if let Some(error) = drain.failure {
        record_peer_failure(store, peer, &mut state, error.clone(), now);
        return peer_outcome(
            peer,
            false,
            drain.pages_applied,
            drain.retained_announcements,
            drain.retained_withdrawals,
            drain.cursor,
            state.health,
            Some(error),
        );
    }

    state.cursor = drain.cursor.clone();
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
        drain.pages_applied,
        drain.retained_announcements,
        drain.retained_withdrawals,
        drain.cursor,
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
        if let Some(outcome) = evict_if_advertisement_expired(store, &plan, now) {
            evicted.push(plan.peer.node_id);
            outcomes.push(outcome);
            continue;
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

/// Evicts the planned peer when its advertisement lease expired between plan
/// and sync, recording an Evicted sync state even for peers with no prior
/// state row. Returns the eviction outcome, or `None` when the lease is
/// still active.
pub fn evict_if_advertisement_expired(
    store: &mut InMemoryStore,
    plan: &DiscoveryPeerSyncPlan,
    now: DateTime<Utc>,
) -> Option<DiscoveryPeerSyncOutcome> {
    const EXPIRED_MESSAGE: &str = "peer advertisement expired before sync";
    let expired = store
        .known_discovery_peer_advertisements
        .get(&plan.peer.node_id)
        .is_some_and(|known| !known.advertisement.lease_is_active(now));
    if !expired {
        return None;
    }
    mark_peer_evicted(store, plan.peer.node_id, now, EXPIRED_MESSAGE);
    store.outbound_discovery_peers.remove(&plan.peer.node_id);
    Some(peer_outcome(
        &plan.peer,
        false,
        0,
        0,
        0,
        plan.cursor.clone(),
        DiscoveryPeerHealth::Evicted,
        Some(DiscoveryPeerSyncFailure::new(
            DiscoveryPeerSyncFailureKind::ExpiredAdvertisement,
            EXPIRED_MESSAGE,
        )),
    ))
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

/// Evidence from one sample-learning pass. Soft-skipped advertisements and
/// failed sources are counted and carried, never silently discarded, so a
/// caller can tell "learned nothing because the network is quiet" apart from
/// "every advertisement was forged and every source timed out".
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PeerLearnReport {
    /// Advertisements that verified and were retained (or refreshed).
    pub retained: usize,
    /// Advertisements rejected during verification and skipped.
    pub skipped_invalid: usize,
    /// Sample sources that failed, with the typed transport/protocol failure.
    pub failed_sources: Vec<(String, DiscoveryPeerSyncFailure)>,
    /// Selected outbound peers after learning.
    pub selected: Vec<OutboundDiscoveryPeer>,
}

/// Verifies and retains previously fetched samples, then selects outbound peers.
///
/// Intended to run under a short store write lock after
/// [`fetch_peer_advertisement_samples`] completed without holding the store.
/// Soft-skips invalid advertisements and failed sample sources, reporting both.
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
) -> PeerLearnReport {
    let mut report = PeerLearnReport::default();
    if !peer_gossip_is_enabled(store) {
        report.selected = list_active_outbound_peers(store);
        return report;
    }

    for (source, result) in fetched {
        match result {
            Ok(sample) => {
                for advertisement in &sample.advertisements {
                    // Soft-skip individual invalid ads; hard failures only on
                    // transport for the sample itself (already filtered).
                    match learn_discovery_peer_advertisement(
                        store,
                        advertisement.clone(),
                        Some(source.as_str()),
                        probe,
                        now,
                    ) {
                        Ok(_) => report.retained += 1,
                        Err(_) => report.skipped_invalid += 1,
                    }
                }
            }
            Err(error) => {
                // Sample source fallthrough: one unavailable Bootstrap/peer does
                // not block learning from others.
                report.failed_sources.push((source.clone(), error.clone()));
            }
        }
    }

    report.selected = select_outbound_discovery_peers(store, local_node_id, now, selection_seed);
    report
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
) -> Result<PeerLearnReport, DiscoveryPeerSyncFailure> {
    if !peer_gossip_is_enabled(store) {
        return Ok(PeerLearnReport {
            selected: list_active_outbound_peers(store),
            ..PeerLearnReport::default()
        });
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

pub(super) fn empty_sync_state(node_id: NodeIdentityId) -> DiscoveryPeerSyncState {
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

pub(super) fn peer_outcome(
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

pub(super) fn record_peer_failure(
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

    let backoff_secs = BASE_BACKOFF_SECONDS
        .saturating_mul(1_i64 << (state.consecutive_failures.min(MAX_BACKOFF_DOUBLINGS) - 1));
    state.backoff_until = Some(now + Duration::seconds(backoff_secs));
    state.health = DiscoveryPeerHealth::BackedOff;
    store
        .discovery_peer_sync_states
        .insert(peer.node_id, state.clone());
}

pub(super) fn mark_peer_evicted(
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

pub(super) fn map_retain_error(
    error: StoreError,
    subject: &str,
) -> Result<(), DiscoveryPeerSyncFailure> {
    classify_retain(error, subject).map_err(|failure| match failure {
        RetainFailure::InvalidSignature(message) => {
            DiscoveryPeerSyncFailure::new(DiscoveryPeerSyncFailureKind::InvalidSignature, message)
        }
        RetainFailure::Validation(message) => {
            DiscoveryPeerSyncFailure::new(DiscoveryPeerSyncFailureKind::Validation, message)
        }
        RetainFailure::Protocol(message) => {
            DiscoveryPeerSyncFailure::new(DiscoveryPeerSyncFailureKind::Protocol, message)
        }
    })
}

pub(super) fn apply_peer_stream_page_staged(
    store: &mut InMemoryStore,
    peer_endpoint: &str,
    page: &AnnouncementStreamPage,
    now: DateTime<Utc>,
) -> Result<(usize, usize), DiscoveryPeerSyncFailure> {
    apply_page_staged(store, |store| {
        apply_peer_stream_page(store, peer_endpoint, page, now)
    })
}

pub(super) fn apply_peer_stream_page(
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
                            // A single invalid signature is a hard failure for
                            // the page: the peer delivered forged Origin bytes.
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
