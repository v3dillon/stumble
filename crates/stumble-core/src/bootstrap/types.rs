//! Shared Bootstrap constants, audit helpers, and StoreError mapping.

use crate::domain::{
    BootstrapAdmissionRejectionReason, BootstrapAdmittedKey, BootstrapRejectionAudit,
    BootstrapRuntimeState, NodeIdentityId, PodAnnouncement, PodWithdrawal,
};
use crate::store::{InMemoryStore, StoreError};
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

/// Maximum UTF-8 JSON size accepted for an open announcement submission.
pub const MAX_ANNOUNCEMENT_PAYLOAD_BYTES: usize = 16_384;

/// Maximum UTF-8 JSON size accepted for an open withdrawal submission.
pub const MAX_WITHDRAWAL_PAYLOAD_BYTES: usize = 8_192;

/// Sliding window used for Bootstrap admission rate limits.
pub const ADMISSION_RATE_WINDOW: Duration = Duration::hours(1);

/// Maximum accepted admissions across all Origins in the rate window.
pub const MAX_NETWORK_ADMISSIONS_PER_WINDOW: usize = 512;

/// Maximum accepted admissions from one Origin in the rate window.
pub const MAX_ORIGIN_ADMISSIONS_PER_WINDOW: usize = 24;

/// Maximum concurrently active admitted public Pods per Origin.
///
/// Kept below the per-Origin rate window so open admission can fill the active
/// set without immediately colliding with submission rate limits.
pub const MAX_ACTIVE_ANNOUNCEMENTS_PER_ORIGIN: usize = 8;

/// Default Announcement Stream page size.
pub const DEFAULT_STREAM_PAGE_LIMIT: usize = 50;

/// Hard upper bound on Announcement Stream page size.
pub const MAX_STREAM_PAGE_LIMIT: usize = 100;

/// Maximum retained Bootstrap rejection audit rows (ring-buffer bound).
pub const MAX_REJECTION_AUDITS: usize = 1_024;

/// Maximum age of Bootstrap rejection audits before age-prune.
pub const MAX_REJECTION_AUDIT_AGE: Duration = Duration::days(7);

/// Maximum retained Announcement Stream entries (oldest sequences pruned first).
///
/// First-release prune policy: drop the lowest sequence numbers when the map
/// exceeds this constant. Cursors into pruned history resume from remaining
/// entries without replaying deleted transitions.
pub const MAX_STREAM_ENTRIES: usize = 8_192;

/// Subject fields recorded on a rejection audit (outer-edge audit helper).
#[derive(Debug, Clone, Default)]
pub struct RejectSubject {
    /// Origin Node when identity could be read from the submission.
    pub origin_node_id: Option<NodeIdentityId>,
    /// Origin public key when present on the submission.
    pub origin_public_key: Option<String>,
    /// Announced Pod slug when present.
    pub pod_slug: Option<String>,
}

impl RejectSubject {
    /// Builds a subject from a signed announcement submission.
    #[must_use]
    pub fn from_announcement(announcement: &PodAnnouncement) -> Self {
        Self {
            origin_node_id: Some(announcement.origin_node_id),
            origin_public_key: Some(announcement.signer.public_key.clone()),
            pod_slug: Some(announcement.pod_slug.clone()),
        }
    }

    /// Builds a subject from a signed withdrawal submission.
    #[must_use]
    pub fn from_withdrawal(withdrawal: &PodWithdrawal) -> Self {
        Self {
            origin_node_id: Some(withdrawal.origin_node_id),
            origin_public_key: Some(withdrawal.signer.public_key.clone()),
            pod_slug: Some(withdrawal.pod_slug.clone()),
        }
    }
}

/// Ensures Bootstrap runtime bookkeeping exists in the store.
pub fn ensure_bootstrap_runtime(store: &mut InMemoryStore) -> &mut BootstrapRuntimeState {
    store
        .bootstrap_runtime
        .get_or_insert_with(BootstrapRuntimeState::default)
}

/// Estimates serialized payload size for open-admission bounds.
#[must_use]
pub fn estimated_payload_bytes<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
}

/// Maps a verification/retain [`StoreError`] onto a stable Bootstrap rejection reason.
///
/// Exhaustive over [`StoreError`] so new store variants fail to compile here
/// rather than collapsing into `Malformed`.
#[must_use]
pub fn map_store_error(error: StoreError) -> BootstrapAdmissionRejectionReason {
    match error {
        StoreError::InvalidSignature => BootstrapAdmissionRejectionReason::InvalidSignature,
        StoreError::AnnouncementExpired
        | StoreError::AnnouncementStale
        | StoreError::WithdrawalStale => BootstrapAdmissionRejectionReason::StaleLease,
        StoreError::AnnouncementWithdrawn => {
            BootstrapAdmissionRejectionReason::AnnouncementWithdrawn
        }
        StoreError::Validation(_) => BootstrapAdmissionRejectionReason::Malformed,
        StoreError::NotFound(_)
        | StoreError::Duplicate(_)
        | StoreError::TenantBoundary
        | StoreError::UntrustedPeer => BootstrapAdmissionRejectionReason::Malformed,
    }
}

/// Records a rejection audit under retention bounds and returns the reason.
pub fn reject(
    store: &mut InMemoryStore,
    reason: BootstrapAdmissionRejectionReason,
    subject: &RejectSubject,
    now: DateTime<Utc>,
) -> BootstrapAdmissionRejectionReason {
    store
        .bootstrap_rejection_audits
        .push(BootstrapRejectionAudit {
            id: Uuid::now_v7(),
            origin_node_id: subject.origin_node_id,
            origin_public_key: subject.origin_public_key.clone(),
            pod_slug: subject.pod_slug.clone(),
            reason,
            rejected_at: now,
        });
    prune_rejection_audits(store, now);
    reason
}

/// Age- and count-prunes rejection audits (ADR-0046 retention bounds).
pub fn prune_rejection_audits(store: &mut InMemoryStore, now: DateTime<Utc>) {
    let cutoff = now - MAX_REJECTION_AUDIT_AGE;
    store
        .bootstrap_rejection_audits
        .retain(|audit| audit.rejected_at > cutoff);
    let len = store.bootstrap_rejection_audits.len();
    if len > MAX_REJECTION_AUDITS {
        let excess = len - MAX_REJECTION_AUDITS;
        store.bootstrap_rejection_audits.drain(0..excess);
    }
}

/// Prunes oldest stream entries when the retained map exceeds the constant cap.
pub fn prune_stream_entries(store: &mut InMemoryStore) {
    while store.announcement_stream_entries.len() > MAX_STREAM_ENTRIES {
        let Some(oldest) = store.announcement_stream_entries.keys().next().copied() else {
            break;
        };
        store.announcement_stream_entries.remove(&oldest);
    }
}

/// Returns whether `key` is currently Bootstrap-admitted.
#[must_use]
pub fn is_bootstrap_admitted(store: &InMemoryStore, key: &BootstrapAdmittedKey) -> bool {
    store
        .bootstrap_runtime
        .as_ref()
        .is_some_and(|runtime| runtime.admitted_keys.contains(key))
}

/// Marks a public Pod key as Bootstrap-admitted.
pub fn mark_bootstrap_admitted(store: &mut InMemoryStore, key: BootstrapAdmittedKey) {
    ensure_bootstrap_runtime(store).admitted_keys.insert(key);
}

/// Removes a public Pod key from the Bootstrap-admitted set.
pub fn unmark_bootstrap_admitted(store: &mut InMemoryStore, key: &BootstrapAdmittedKey) {
    if let Some(runtime) = store.bootstrap_runtime.as_mut() {
        runtime.admitted_keys.remove(key);
    }
}

pub(crate) fn prune_attempts(attempts: &mut Vec<DateTime<Utc>>, now: DateTime<Utc>) {
    let window_start = now - ADMISSION_RATE_WINDOW;
    attempts.retain(|at| *at > window_start);
}

pub(crate) fn rate_limit_would_exceed(
    store: &InMemoryStore,
    origin_node_id: NodeIdentityId,
    now: DateTime<Utc>,
) -> bool {
    let Some(runtime) = store.bootstrap_runtime.as_ref() else {
        return false;
    };
    let window_start = now - ADMISSION_RATE_WINDOW;
    let network = runtime
        .recent_network_admissions
        .iter()
        .filter(|at| **at > window_start)
        .count();
    if network >= MAX_NETWORK_ADMISSIONS_PER_WINDOW {
        return true;
    }
    let origin = runtime
        .recent_origin_admissions
        .get(&origin_node_id)
        .map(|entries| entries.iter().filter(|at| **at > window_start).count())
        .unwrap_or(0);
    origin >= MAX_ORIGIN_ADMISSIONS_PER_WINDOW
}

pub(crate) fn record_admission_attempt(
    store: &mut InMemoryStore,
    origin_node_id: NodeIdentityId,
    now: DateTime<Utc>,
) {
    let runtime = ensure_bootstrap_runtime(store);
    prune_attempts(&mut runtime.recent_network_admissions, now);
    runtime.recent_network_admissions.push(now);
    let origin_attempts = runtime
        .recent_origin_admissions
        .entry(origin_node_id)
        .or_default();
    prune_attempts(origin_attempts, now);
    origin_attempts.push(now);
}
