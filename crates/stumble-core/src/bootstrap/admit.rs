//! Open Bootstrap admission for announcements and withdrawals.
//!
//! Pipeline:
//! 1. Bootstrap-only gates (enabled, payload size, identity, protocol, rate
//!    limit, probe/manifest, origin quota)
//! 2. Single [`retain_verified_pod_announcement`] /
//!    [`retain_verified_pod_withdrawal`] call
//! 3. Exhaustive [`map_store_error`] for retain failures
//!
//! Quota, stream lifecycle, and expiry bookkeeping operate only on the
//! Bootstrap-admitted key set in [`crate::domain::BootstrapRuntimeState`].

use super::probe::{manifest_matches, OriginProbe, OriginProbeError};
use super::stream::{
    append_announcement_stream, append_withdrawal_stream, project_bootstrap_withdrawal,
};
use super::types::{
    estimated_payload_bytes, is_bootstrap_admitted, map_store_error, mark_bootstrap_admitted,
    rate_limit_would_exceed, record_admission_attempt, reject, RejectSubject,
    MAX_ACTIVE_ANNOUNCEMENTS_PER_ORIGIN, MAX_ANNOUNCEMENT_PAYLOAD_BYTES,
    MAX_WITHDRAWAL_PAYLOAD_BYTES,
};
use crate::domain::{
    AnnouncementStreamEventKind, BootstrapAdmissionAcceptance, BootstrapAdmissionOutcomeKind,
    BootstrapAdmissionRejectionReason, BootstrapWithdrawalAcceptance,
    BootstrapWithdrawalOutcomeKind, NodeIdentityId, PodAnnouncement, PodWithdrawal,
    CURRENT_PROTOCOL_VERSION,
};
use crate::pod_announcement::{
    announcement_is_discovery_eligible, retain_verified_pod_announcement,
    retain_verified_pod_withdrawal,
};
use crate::store::InMemoryStore;
use chrono::{DateTime, Utc};

/// Counts active Bootstrap-admitted announcements for one Origin.
fn active_bootstrap_count_for_origin(
    store: &InMemoryStore,
    origin_node_id: NodeIdentityId,
    now: DateTime<Utc>,
) -> usize {
    let keys: Vec<_> = store
        .bootstrap_runtime
        .as_ref()
        .map(|runtime| {
            runtime
                .admitted_keys
                .iter()
                .filter(|(origin, _)| *origin == origin_node_id)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    keys.iter()
        .filter_map(|key| store.known_pod_announcements.get(key))
        .filter(|known| announcement_is_discovery_eligible(store, &known.announcement, now))
        .count()
}

/// Admits a public Pod Announcement on a Bootstrap-capable node.
///
/// # Errors
///
/// Returns a stable [`BootstrapAdmissionRejectionReason`] on policy or
/// verification failure. Store/signature transport errors surface as reasons.
pub fn admit_bootstrap_announcement(
    store: &mut InMemoryStore,
    announcement: PodAnnouncement,
    probe: &dyn OriginProbe,
    bootstrap_enabled: bool,
    now: DateTime<Utc>,
) -> Result<BootstrapAdmissionAcceptance, BootstrapAdmissionRejectionReason> {
    let subject = RejectSubject::from_announcement(&announcement);

    // --- Bootstrap-only gates ---
    if !bootstrap_enabled {
        return Err(reject(
            store,
            BootstrapAdmissionRejectionReason::BootstrapDisabled,
            &subject,
            now,
        ));
    }

    if estimated_payload_bytes(&announcement) > MAX_ANNOUNCEMENT_PAYLOAD_BYTES {
        return Err(reject(
            store,
            BootstrapAdmissionRejectionReason::PayloadTooLarge,
            &subject,
            now,
        ));
    }

    if announcement.origin_node_id != announcement.signer.node_id
        || announcement.signer.public_key.trim().is_empty()
        || announcement.pod_slug.trim().is_empty()
    {
        return Err(reject(
            store,
            BootstrapAdmissionRejectionReason::InvalidIdentity,
            &subject,
            now,
        ));
    }

    if announcement.signer.supported_protocol_version != CURRENT_PROTOCOL_VERSION {
        return Err(reject(
            store,
            BootstrapAdmissionRejectionReason::IncompatibleProtocol,
            &subject,
            now,
        ));
    }

    let key = (announcement.origin_node_id, announcement.pod_slug.clone());

    // Idempotent replay only applies to already Bootstrap-admitted state.
    if is_bootstrap_admitted(store, &key) {
        if let Some(existing) = store.known_pod_announcements.get(&key) {
            if existing.announcement.id == announcement.id && existing.announcement == announcement
            {
                return Ok(BootstrapAdmissionAcceptance {
                    outcome: BootstrapAdmissionOutcomeKind::Idempotent,
                    known: existing.clone(),
                    stream_sequence: None,
                });
            }
        }
    }

    if rate_limit_would_exceed(store, announcement.origin_node_id, now) {
        return Err(reject(
            store,
            BootstrapAdmissionRejectionReason::RateLimited,
            &subject,
            now,
        ));
    }

    let view =
        match probe.probe_public_manifest(&announcement.public_pod_url, &announcement.pod_slug) {
            Ok(view) => view,
            Err(OriginProbeError::Unreachable) => {
                return Err(reject(
                    store,
                    BootstrapAdmissionRejectionReason::UnreachableOrigin,
                    &subject,
                    now,
                ));
            }
            Err(OriginProbeError::ManifestUnavailable) => {
                return Err(reject(
                    store,
                    BootstrapAdmissionRejectionReason::ManifestUnavailable,
                    &subject,
                    now,
                ));
            }
        };

    if let Err(reason) = manifest_matches(&announcement, &view) {
        return Err(reject(store, reason, &subject, now));
    }

    let is_new_bootstrap_admission = !is_bootstrap_admitted(store, &key);
    if is_new_bootstrap_admission
        && active_bootstrap_count_for_origin(store, announcement.origin_node_id, now)
            >= MAX_ACTIVE_ANNOUNCEMENTS_PER_ORIGIN
    {
        return Err(reject(
            store,
            BootstrapAdmissionRejectionReason::OriginQuotaExceeded,
            &subject,
            now,
        ));
    }

    // --- Single retain/verification primitive ---
    let known = retain_verified_pod_announcement(
        store,
        announcement,
        crate::pod_announcement::DeliveryProvenance::LOCAL,
        now,
    )
    .map_err(|error| reject(store, map_store_error(error), &subject, now))?;

    mark_bootstrap_admitted(
        store,
        (
            known.announcement.origin_node_id,
            known.announcement.pod_slug.clone(),
        ),
    );

    record_admission_attempt(store, known.announcement.origin_node_id, now);

    let (outcome, kind) = if is_new_bootstrap_admission {
        (
            BootstrapAdmissionOutcomeKind::Admitted,
            AnnouncementStreamEventKind::Admitted,
        )
    } else {
        (
            BootstrapAdmissionOutcomeKind::Renewed,
            AnnouncementStreamEventKind::Renewed,
        )
    };
    let stream_sequence = append_announcement_stream(store, kind, &known.announcement, now);

    Ok(BootstrapAdmissionAcceptance {
        outcome,
        known,
        stream_sequence: Some(stream_sequence),
    })
}

/// Admits an Origin-signed Pod Withdrawal on a Bootstrap-capable node.
///
/// # Errors
///
/// Returns a stable rejection reason when verification or policy fails.
pub fn admit_bootstrap_withdrawal(
    store: &mut InMemoryStore,
    withdrawal: PodWithdrawal,
    bootstrap_enabled: bool,
    now: DateTime<Utc>,
) -> Result<BootstrapWithdrawalAcceptance, BootstrapAdmissionRejectionReason> {
    let subject = RejectSubject::from_withdrawal(&withdrawal);

    // --- Bootstrap-only gates ---
    if !bootstrap_enabled {
        return Err(reject(
            store,
            BootstrapAdmissionRejectionReason::BootstrapDisabled,
            &subject,
            now,
        ));
    }

    if estimated_payload_bytes(&withdrawal) > MAX_WITHDRAWAL_PAYLOAD_BYTES {
        return Err(reject(
            store,
            BootstrapAdmissionRejectionReason::PayloadTooLarge,
            &subject,
            now,
        ));
    }

    if withdrawal.origin_node_id != withdrawal.signer.node_id
        || withdrawal.signer.public_key.trim().is_empty()
        || withdrawal.pod_slug.trim().is_empty()
    {
        return Err(reject(
            store,
            BootstrapAdmissionRejectionReason::InvalidIdentity,
            &subject,
            now,
        ));
    }

    if withdrawal.signer.supported_protocol_version != CURRENT_PROTOCOL_VERSION {
        return Err(reject(
            store,
            BootstrapAdmissionRejectionReason::IncompatibleProtocol,
            &subject,
            now,
        ));
    }

    let key = (withdrawal.origin_node_id, withdrawal.pod_slug.clone());
    if let Some(existing) = store.known_pod_withdrawals.get(&key) {
        if existing.withdrawal.id == withdrawal.id && existing.withdrawal == withdrawal {
            return Ok(BootstrapWithdrawalAcceptance {
                outcome: BootstrapWithdrawalOutcomeKind::Idempotent,
                known: existing.clone(),
                stream_sequence: None,
            });
        }
    }

    if rate_limit_would_exceed(store, withdrawal.origin_node_id, now) {
        return Err(reject(
            store,
            BootstrapAdmissionRejectionReason::RateLimited,
            &subject,
            now,
        ));
    }

    // --- Single retain/verification primitive ---
    let known = retain_verified_pod_withdrawal(store, withdrawal, None, now)
        .map_err(|error| reject(store, map_store_error(error), &subject, now))?;

    record_admission_attempt(store, known.withdrawal.origin_node_id, now);
    // Shared projector: unmark admitted set + stream Withdrawn (also used by Index/peer paths).
    let stream_sequence =
        project_bootstrap_withdrawal(store, &known.withdrawal, now).or_else(|| {
            // Withdrawal of a never-Bootstrap-admitted Pod still gets an open-admission
            // stream row so Bootstrap consumers see the lifecycle event.
            Some(append_withdrawal_stream(store, &known.withdrawal, now))
        });

    Ok(BootstrapWithdrawalAcceptance {
        outcome: BootstrapWithdrawalOutcomeKind::Admitted,
        known,
        stream_sequence,
    })
}

/// Counts active Bootstrap-admitted announcements for one Origin (test/operator helper).
#[must_use]
pub fn count_active_origin_announcements(
    store: &InMemoryStore,
    origin_node_id: NodeIdentityId,
    now: DateTime<Utc>,
) -> usize {
    active_bootstrap_count_for_origin(store, origin_node_id, now)
}
