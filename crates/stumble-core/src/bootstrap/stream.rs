//! Topic-neutral Announcement Stream read path and lease-expiry transitions.
//!
//! Stream GETs may perform a **bounded, Bootstrap-admitted-only** write-on-read
//! to emit pending expiry transitions. They never scan the full
//! `known_pod_announcements` table for non-Bootstrap state.

use super::types::{
    ensure_bootstrap_runtime, is_bootstrap_admitted, prune_stream_entries,
    unmark_bootstrap_admitted, DEFAULT_STREAM_PAGE_LIMIT, MAX_STREAM_PAGE_LIMIT,
};
use crate::domain::{
    AnnouncementStreamEntry, AnnouncementStreamEventKind, AnnouncementStreamPage,
    AnnouncementStreamPayload, BootstrapAdmissionRejectionReason, NodeIdentityId, PodAnnouncement,
    PodWithdrawal,
};
use crate::store::InMemoryStore;
use chrono::{DateTime, Utc};

/// Appends one lifecycle entry and prunes the stream to the retention cap.
pub(crate) fn append_stream_entry(
    store: &mut InMemoryStore,
    kind: AnnouncementStreamEventKind,
    origin_node_id: NodeIdentityId,
    pod_slug: String,
    payload: AnnouncementStreamPayload,
    now: DateTime<Utc>,
) -> u64 {
    let runtime = ensure_bootstrap_runtime(store);
    let sequence = runtime.next_stream_sequence;
    runtime.next_stream_sequence = runtime.next_stream_sequence.saturating_add(1);
    let entry = AnnouncementStreamEntry {
        sequence,
        recorded_at: now,
        kind,
        origin_node_id,
        pod_slug,
        payload,
    };
    store.announcement_stream_entries.insert(sequence, entry);
    prune_stream_entries(store);
    sequence
}

/// Emits `expired` stream transitions for Bootstrap-admitted leases that lapsed.
///
/// Only walks [`crate::domain::BootstrapRuntimeState::admitted_keys`]. Expiry is
/// **terminal** for the admitted set: after one `Expired` stream row the key is
/// unmarked, so markers and re-emits are unnecessary. Never deletes Subscriptions
/// or synchronized content. Work is bounded by the size of the admitted set.
///
/// Returns the number of newly emitted expiry transitions.
pub fn emit_expiry_transitions(store: &mut InMemoryStore, now: DateTime<Utc>) -> usize {
    ensure_bootstrap_runtime(store);

    let admitted_keys: Vec<_> = store
        .bootstrap_runtime
        .as_ref()
        .map(|runtime| runtime.admitted_keys.iter().cloned().collect())
        .unwrap_or_default();

    let mut candidates: Vec<PodAnnouncement> = Vec::new();
    let mut stale_keys = Vec::new();
    for key in admitted_keys {
        match store.known_pod_announcements.get(&key) {
            Some(known) if !known.announcement.lease_is_active(now) => {
                candidates.push(known.announcement.clone());
            }
            None => stale_keys.push(key),
            Some(_) => {}
        }
    }

    // Keys that left known state without a Bootstrap lifecycle row are dropped
    // from the admitted set. Prefer projecting withdrawals through
    // [`super::types::project_bootstrap_withdrawal`] so stream consumers still
    // see `Withdrawn` when Index/peer paths apply a verified withdrawal.
    for key in stale_keys {
        if let Some(runtime) = store.bootstrap_runtime.as_mut() {
            runtime.admitted_keys.remove(&key);
        }
    }

    let mut emitted = 0usize;
    for announcement in candidates {
        let key = (announcement.origin_node_id, announcement.pod_slug.clone());
        append_stream_entry(
            store,
            AnnouncementStreamEventKind::Expired,
            announcement.origin_node_id,
            announcement.pod_slug.clone(),
            AnnouncementStreamPayload::Announcement(announcement),
            now,
        );
        // Terminal: release the admitted slot (same lifecycle ownership as withdraw).
        if let Some(runtime) = store.bootstrap_runtime.as_mut() {
            runtime.admitted_keys.remove(&key);
        }
        emitted += 1;
    }
    emitted
}

/// Parses an Announcement Stream cursor.
///
/// Empty input means the beginning of the stream (`after = 0` exclusive start).
///
/// # Errors
///
/// Returns `Err` for unknown or malformed cursor values.
pub fn parse_stream_cursor(cursor: Option<&str>) -> Result<u64, BootstrapAdmissionRejectionReason> {
    match cursor.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(0),
        Some(value) => value
            .parse::<u64>()
            .map_err(|_| BootstrapAdmissionRejectionReason::Malformed),
    }
}

/// Encodes a stream resume cursor for the wire.
#[must_use]
pub fn encode_stream_cursor(after_sequence: u64) -> String {
    after_sequence.to_string()
}

/// Reads a topic-neutral, bounded page of the Announcement Stream.
///
/// Performs a bounded Bootstrap-admitted-only expiry advance at `now` before
/// serving so Home Nodes see lifecycle changes without a separate sweep.
/// Rejects unknown cursors safely.
///
/// # Errors
///
/// Returns [`BootstrapAdmissionRejectionReason::Malformed`] for invalid cursors
/// and [`BootstrapAdmissionRejectionReason::BootstrapDisabled`] when the node is
/// not serving Bootstrap streams.
pub fn read_announcement_stream(
    store: &mut InMemoryStore,
    cursor: Option<&str>,
    limit: Option<usize>,
    bootstrap_enabled: bool,
    now: DateTime<Utc>,
) -> Result<AnnouncementStreamPage, BootstrapAdmissionRejectionReason> {
    if !bootstrap_enabled {
        return Err(BootstrapAdmissionRejectionReason::BootstrapDisabled);
    }
    let after = parse_stream_cursor(cursor)?;
    // Unknown future cursors are rejected rather than silently skipping gaps.
    let max_sequence = store
        .announcement_stream_entries
        .keys()
        .next_back()
        .copied()
        .map(|max| max.saturating_add(1))
        .unwrap_or(0);
    let next_assigned = store
        .bootstrap_runtime
        .as_ref()
        .map(|runtime| runtime.next_stream_sequence)
        .unwrap_or(0);
    let high_water = max_sequence.max(next_assigned);
    if after > high_water {
        return Err(BootstrapAdmissionRejectionReason::Malformed);
    }

    // Isolated write-on-read: only walks Bootstrap-admitted keys (see emit_expiry_transitions).
    emit_expiry_transitions(store, now);

    let limit = limit
        .unwrap_or(DEFAULT_STREAM_PAGE_LIMIT)
        .clamp(1, MAX_STREAM_PAGE_LIMIT);
    let entries: Vec<AnnouncementStreamEntry> = store
        .announcement_stream_entries
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

/// Helper used by admission when appending announcement lifecycle rows.
pub(crate) fn append_announcement_stream(
    store: &mut InMemoryStore,
    kind: AnnouncementStreamEventKind,
    announcement: &PodAnnouncement,
    now: DateTime<Utc>,
) -> u64 {
    append_stream_entry(
        store,
        kind,
        announcement.origin_node_id,
        announcement.pod_slug.clone(),
        AnnouncementStreamPayload::Announcement(announcement.clone()),
        now,
    )
}

/// Helper used by withdrawal admission when appending withdrawal lifecycle rows.
pub(crate) fn append_withdrawal_stream(
    store: &mut InMemoryStore,
    withdrawal: &PodWithdrawal,
    now: DateTime<Utc>,
) -> u64 {
    append_stream_entry(
        store,
        AnnouncementStreamEventKind::Withdrawn,
        withdrawal.origin_node_id,
        withdrawal.pod_slug.clone(),
        AnnouncementStreamPayload::Withdrawal(withdrawal.clone()),
        now,
    )
}

/// Projects a verified withdrawal onto the Bootstrap stream when the Pod was admitted.
///
/// Call after any successful `retain_verified_pod_withdrawal` so Index/peer paths
/// cannot dissolve Bootstrap-admitted state without a lifecycle effect. Returns the
/// stream sequence when a `Withdrawn` row was appended.
pub fn project_bootstrap_withdrawal(
    store: &mut InMemoryStore,
    withdrawal: &PodWithdrawal,
    now: DateTime<Utc>,
) -> Option<u64> {
    let key = (withdrawal.origin_node_id, withdrawal.pod_slug.clone());
    if !is_bootstrap_admitted(store, &key) {
        return None;
    }
    unmark_bootstrap_admitted(store, &key);
    Some(append_withdrawal_stream(store, withdrawal, now))
}
