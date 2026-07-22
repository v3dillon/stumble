//! Shared Discovery Peer constants and store helpers.

use crate::bootstrap::ensure_bootstrap_runtime;
use crate::domain::{BootstrapRuntimeState, DiscoveryPeerServiceState, NodeIdentityId};
use crate::store::InMemoryStore;
use chrono::{DateTime, Duration, Utc};

/// Maximum UTF-8 JSON size accepted for a peer advertisement submission.
pub const MAX_PEER_ADVERTISEMENT_PAYLOAD_BYTES: usize = 8_192;

/// Default page size for peer-served Announcement Streams.
pub const DEFAULT_PEER_STREAM_PAGE_LIMIT: usize = 50;

/// Hard upper bound on peer-served Announcement Stream page size.
pub const MAX_PEER_STREAM_PAGE_LIMIT: usize = 100;

/// Default sample size for unranked peer-advertisement exchange.
pub const DEFAULT_PEER_SAMPLE_LIMIT: usize = 8;

/// Hard upper bound on peer-advertisement sample size.
pub const MAX_PEER_SAMPLE_LIMIT: usize = 16;

/// Sliding window used for Discovery Peer advertisement admission rate limits.
pub const PEER_ADMISSION_RATE_WINDOW: Duration = Duration::hours(1);

/// Maximum accepted peer-advertisement admissions across all nodes in the window.
pub const MAX_PEER_NETWORK_ADMISSIONS_PER_WINDOW: usize = 128;

/// Maximum accepted peer-advertisement admissions from one node in the window.
pub const MAX_PEER_NODE_ADMISSIONS_PER_WINDOW: usize = 8;

/// Maximum retained peer Announcement Stream entries (oldest sequences pruned).
pub const MAX_PEER_STREAM_ENTRIES: usize = 8_192;

/// Ensures Discovery Peer service bookkeeping exists in the store.
pub fn ensure_discovery_peer_service(store: &mut InMemoryStore) -> &mut DiscoveryPeerServiceState {
    store
        .discovery_peer_service
        .get_or_insert_with(DiscoveryPeerServiceState::default)
}

/// Estimates serialized payload size for open-admission bounds.
#[must_use]
pub fn estimated_payload_bytes<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
}

fn prune_peer_attempts(attempts: &mut Vec<DateTime<Utc>>, now: DateTime<Utc>) {
    let window_start = now - PEER_ADMISSION_RATE_WINDOW;
    attempts.retain(|at| *at > window_start);
}

/// Returns whether admitting another peer advertisement would exceed rate limits.
pub(crate) fn peer_rate_limit_would_exceed(
    store: &InMemoryStore,
    node_id: NodeIdentityId,
    now: DateTime<Utc>,
) -> bool {
    let Some(runtime) = store.bootstrap_runtime.as_ref() else {
        return false;
    };
    let window_start = now - PEER_ADMISSION_RATE_WINDOW;
    let network = runtime
        .recent_peer_admissions
        .iter()
        .filter(|at| **at > window_start)
        .count();
    if network >= MAX_PEER_NETWORK_ADMISSIONS_PER_WINDOW {
        return true;
    }
    let per_node = runtime
        .recent_peer_admissions_by_node
        .get(&node_id)
        .map(|entries| entries.iter().filter(|at| **at > window_start).count())
        .unwrap_or(0);
    per_node >= MAX_PEER_NODE_ADMISSIONS_PER_WINDOW
}

/// Records a successful peer-advertisement admission for rate-limit bookkeeping.
pub(crate) fn record_peer_admission_attempt(
    store: &mut InMemoryStore,
    node_id: NodeIdentityId,
    now: DateTime<Utc>,
) {
    let runtime: &mut BootstrapRuntimeState = ensure_bootstrap_runtime(store);
    prune_peer_attempts(&mut runtime.recent_peer_admissions, now);
    runtime.recent_peer_admissions.push(now);
    let node_attempts = runtime
        .recent_peer_admissions_by_node
        .entry(node_id)
        .or_default();
    prune_peer_attempts(node_attempts, now);
    node_attempts.push(now);
}

/// Prunes the oldest peer stream entries when retention exceeds the cap.
pub(crate) fn prune_peer_stream_entries(store: &mut InMemoryStore) {
    while store.discovery_peer_stream_entries.len() > MAX_PEER_STREAM_ENTRIES {
        let Some(oldest) = store.discovery_peer_stream_entries.keys().next().copied() else {
            break;
        };
        store.discovery_peer_stream_entries.remove(&oldest);
    }
}
