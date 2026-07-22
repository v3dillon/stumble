//! Inbound Discovery Peer serving: Announcement Stream pages and peer samples.
//!
//! Peer endpoints expose only public discovery artifacts. They never surface
//! Pod Events, Subscriptions, Taste Profiles, feedback, credentials, private
//! projections, or administrative capability.

use super::advertise::peer_service_is_enabled;
use super::types::{
    DEFAULT_PEER_SAMPLE_LIMIT, DEFAULT_PEER_STREAM_PAGE_LIMIT, MAX_PEER_SAMPLE_LIMIT,
    MAX_PEER_STREAM_PAGE_LIMIT,
};
use crate::bootstrap::{encode_stream_cursor, parse_stream_cursor};
use crate::domain::{
    AnnouncementStreamPage, DiscoveryPeerAdmissionRejectionReason, DiscoveryPeerAdvertisement,
    DiscoveryPeerAdvertisementSample,
};
use crate::store::InMemoryStore;
use chrono::{DateTime, Utc};

/// Reads a bounded Announcement Stream page from an enabled Discovery Peer.
///
/// Entries preserve Origin announcement bytes and signatures unchanged.
/// Serving is gated solely on opt-in peer service state (not Trusted Peer).
/// Read-only: does not mutate store state or emit write-on-read transitions.
///
/// # Errors
///
/// Returns [`DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled`] when
/// the node is not opted in, or `Malformed` for invalid cursors.
pub fn read_peer_announcement_stream(
    store: &InMemoryStore,
    cursor: Option<&str>,
    limit: Option<usize>,
    _now: DateTime<Utc>,
) -> Result<AnnouncementStreamPage, DiscoveryPeerAdmissionRejectionReason> {
    if !peer_service_is_enabled(store) {
        return Err(DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled);
    }

    // Reuse Bootstrap cursor grammar so Home Nodes can treat peer and Bootstrap
    // streams interchangeably for announcement lifecycle pages.
    let after = parse_stream_cursor(cursor)
        .map_err(|_| DiscoveryPeerAdmissionRejectionReason::Malformed)?;

    let high_water = store
        .discovery_peer_service
        .as_ref()
        .map(|state| state.next_stream_sequence)
        .unwrap_or(0)
        .max(
            store
                .discovery_peer_stream_entries
                .keys()
                .next_back()
                .copied()
                .map(|max| max.saturating_add(1))
                .unwrap_or(0),
        );
    if after > high_water {
        return Err(DiscoveryPeerAdmissionRejectionReason::Malformed);
    }

    let limit = limit
        .unwrap_or(DEFAULT_PEER_STREAM_PAGE_LIMIT)
        .clamp(1, MAX_PEER_STREAM_PAGE_LIMIT);
    let entries: Vec<_> = store
        .discovery_peer_stream_entries
        .range((after + 1)..)
        .take(limit)
        .map(|(_, entry)| entry.clone())
        .collect();
    let next_cursor = entries
        .last()
        .map(|entry| encode_stream_cursor(entry.sequence));
    Ok(AnnouncementStreamPage {
        entries,
        next_cursor,
        limit,
    })
}

/// Returns a small randomized sample of currently valid peer advertisements.
///
/// Sampling is unranked and carries no trust assertions. `seed` makes selection
/// deterministic for tests; production callers should supply server entropy.
///
/// # Errors
///
/// Returns [`DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled`] when
/// the node is not opted in.
pub fn sample_discovery_peer_advertisements(
    store: &InMemoryStore,
    limit: Option<usize>,
    now: DateTime<Utc>,
    seed: u64,
) -> Result<DiscoveryPeerAdvertisementSample, DiscoveryPeerAdmissionRejectionReason> {
    if !peer_service_is_enabled(store) {
        return Err(DiscoveryPeerAdmissionRejectionReason::PeerServiceDisabled);
    }
    Ok(sample_known_discovery_peer_advertisements(
        store, limit, now, seed,
    ))
}

/// Samples currently valid known peer advertisements without opt-in gates.
///
/// Used by Bootstrap Nodes (open sample of admitted ads) and as the shared
/// selection core for Discovery Peer samples. Unranked; `seed` controls
/// deterministic shuffle for tests or server entropy.
#[must_use]
pub fn sample_known_discovery_peer_advertisements(
    store: &InMemoryStore,
    limit: Option<usize>,
    now: DateTime<Utc>,
    seed: u64,
) -> DiscoveryPeerAdvertisementSample {
    let limit = limit
        .unwrap_or(DEFAULT_PEER_SAMPLE_LIMIT)
        .clamp(1, MAX_PEER_SAMPLE_LIMIT);

    let mut eligible: Vec<DiscoveryPeerAdvertisement> = store
        .known_discovery_peer_advertisements
        .values()
        .map(|known| known.advertisement.clone())
        .filter(|ad| ad.lease_is_active(now) && ad.verify().unwrap_or(false))
        .collect();

    // Deterministic shuffle (Fisher–Yates) seeded for test control / server entropy.
    let mut state = seed;
    for i in (1..eligible.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = (state as usize) % (i + 1);
        eligible.swap(i, j);
    }
    eligible.truncate(limit);

    DiscoveryPeerAdvertisementSample::new(eligible, limit)
}

/// Asserts a peer sample carries only public advertisement fields (privacy seam).
#[must_use]
pub fn peer_advertisement_sample_is_public_only(sample: &DiscoveryPeerAdvertisementSample) -> bool {
    let Ok(value) = serde_json::to_value(sample) else {
        return false;
    };
    let forbidden = [
        "taste_profile",
        "subscription",
        "subscriptions",
        "feedback",
        "credential",
        "credentials",
        "user_id",
        "private",
        "admin",
        "rank",
        "trust",
        "score",
        "quality",
        "popularity",
    ];
    !contains_forbidden_key(&value, &forbidden)
}

fn contains_forbidden_key(value: &serde_json::Value, forbidden: &[&str]) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if forbidden.iter().any(|item| key.eq_ignore_ascii_case(item)) {
                    return true;
                }
                if contains_forbidden_key(child, forbidden) {
                    return true;
                }
            }
            false
        }
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| contains_forbidden_key(item, forbidden)),
        _ => false,
    }
}
