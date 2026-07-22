//! Open Bootstrap admission and topic-neutral Announcement Streams.
//!
//! A Bootstrap-capable node accepts verifiable public Pod Announcements without
//! User accounts or Trusted Peer relationships. Admission verifies origin
//! identity, signature, canonical URL, reachability, public manifest, protocol
//! compatibility, lease, and resource bounds. It never assigns trust, quality,
//! rank, or personalized ordering.
//!
//! # Module layout
//!
//! - [`probe`] — Origin reachability / public-manifest port
//! - [`admit`] — open announcement and withdrawal admission
//! - [`stream`] — cursor-paginated Announcement Stream and expiry transitions
//! - [`types`] — bounds, audit helpers, Bootstrap-admitted key bookkeeping

mod admit;
mod probe;
mod stream;
mod types;

pub use admit::{
    admit_bootstrap_announcement, admit_bootstrap_withdrawal, count_active_origin_announcements,
};
pub use probe::{
    manifest_matches, probe_view_matching, FixedOriginProbe, OriginProbe, OriginProbeError,
    OriginPublicManifestView, ScriptedMatchingOriginProbe, UnreachableOriginProbe,
};
pub use stream::{
    emit_expiry_transitions, encode_stream_cursor, parse_stream_cursor,
    project_bootstrap_withdrawal, read_announcement_stream,
};
pub use types::{
    ensure_bootstrap_runtime, estimated_payload_bytes, is_bootstrap_admitted, map_store_error,
    mark_bootstrap_admitted, prune_rejection_audits, prune_stream_entries, reject,
    unmark_bootstrap_admitted, RejectSubject, ADMISSION_RATE_WINDOW, DEFAULT_STREAM_PAGE_LIMIT,
    MAX_ACTIVE_ANNOUNCEMENTS_PER_ORIGIN, MAX_ANNOUNCEMENT_PAYLOAD_BYTES,
    MAX_NETWORK_ADMISSIONS_PER_WINDOW, MAX_ORIGIN_ADMISSIONS_PER_WINDOW, MAX_REJECTION_AUDITS,
    MAX_REJECTION_AUDIT_AGE, MAX_STREAM_ENTRIES, MAX_STREAM_PAGE_LIMIT,
    MAX_WITHDRAWAL_PAYLOAD_BYTES,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        announcement_lease_duration, AnnouncementStreamEventKind, BootstrapAdmissionOutcomeKind,
        BootstrapAdmissionRejectionReason, NodeInfo, PackageVersion, CURRENT_PROTOCOL_VERSION,
    };
    use crate::signing::{create_node_identity, sign_pod_announcement};
    use crate::store::InMemoryStore;
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    fn sample_announcement(
        node: &crate::domain::NodeIdentity,
        announced_at: chrono::DateTime<Utc>,
        slug: &str,
    ) -> crate::domain::PodAnnouncement {
        sign_pod_announcement(
            node,
            crate::domain::PodAnnouncement {
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
    fn admits_without_user_or_peer_and_streams() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, now, "systems");
        let probe = FixedOriginProbe {
            view: Some(probe_view_matching(&announcement)),
            error: None,
        };
        let accepted =
            admit_bootstrap_announcement(&mut store, announcement.clone(), &probe, true, now)
                .unwrap();
        assert_eq!(accepted.outcome, BootstrapAdmissionOutcomeKind::Admitted);
        assert!(accepted.stream_sequence.is_some());
        assert!(is_bootstrap_admitted(
            &store,
            &(announcement.origin_node_id, announcement.pod_slug.clone())
        ));
        let page = read_announcement_stream(&mut store, None, Some(10), true, now).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].kind, AnnouncementStreamEventKind::Admitted);
        assert_eq!(
            page.entries[0].payload.as_announcement().unwrap().id,
            announcement.id
        );
    }

    #[test]
    fn duplicate_submission_is_idempotent() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, now, "systems");
        let probe = FixedOriginProbe {
            view: Some(probe_view_matching(&announcement)),
            error: None,
        };
        admit_bootstrap_announcement(&mut store, announcement.clone(), &probe, true, now).unwrap();
        let again =
            admit_bootstrap_announcement(&mut store, announcement, &probe, true, now).unwrap();
        assert_eq!(again.outcome, BootstrapAdmissionOutcomeKind::Idempotent);
        assert!(again.stream_sequence.is_none());
        assert_eq!(store.announcement_stream_entries.len(), 1);
    }

    #[test]
    fn rejects_unreachable_and_manifest_unavailable_distinctly() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, now, "systems");
        let err = admit_bootstrap_announcement(
            &mut store,
            announcement.clone(),
            &UnreachableOriginProbe,
            true,
            now,
        )
        .unwrap_err();
        assert_eq!(err, BootstrapAdmissionRejectionReason::UnreachableOrigin);

        let probe = FixedOriginProbe {
            view: None,
            error: Some(OriginProbeError::ManifestUnavailable),
        };
        let err =
            admit_bootstrap_announcement(&mut store, announcement, &probe, true, now).unwrap_err();
        assert_eq!(err, BootstrapAdmissionRejectionReason::ManifestUnavailable);
        assert_eq!(store.bootstrap_rejection_audits.len(), 2);
    }

    #[test]
    fn stream_cursor_rejects_unknown_future_position() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let err =
            read_announcement_stream(&mut store, Some("99"), Some(10), true, now).unwrap_err();
        assert_eq!(err, BootstrapAdmissionRejectionReason::Malformed);
    }

    #[test]
    fn emits_expiry_only_for_bootstrap_admitted() {
        let announced = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, announced, "systems");
        let probe = FixedOriginProbe {
            view: Some(probe_view_matching(&announcement)),
            error: None,
        };
        admit_bootstrap_announcement(&mut store, announcement, &probe, true, announced).unwrap();

        // Peer-retained announcement that is NOT Bootstrap-admitted must not expiry-stream.
        let other = create_node_identity("other", None);
        let peer_only = sample_announcement(&other, announced, "peer-only");
        crate::pod_announcement::retain_verified_pod_announcement(
            &mut store, peer_only, None, None, announced,
        )
        .unwrap();

        let later = announced + announcement_lease_duration() + Duration::seconds(1);
        let page = read_announcement_stream(&mut store, None, Some(10), true, later).unwrap();
        let expired: Vec<_> = page
            .entries
            .iter()
            .filter(|entry| entry.kind == AnnouncementStreamEventKind::Expired)
            .collect();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].pod_slug, "systems");
        // Expiry is terminal for the admitted set: no re-emit on subsequent reads.
        let key = (node.id, "systems".to_string());
        assert!(!is_bootstrap_admitted(&store, &key));
        let page_again = read_announcement_stream(&mut store, None, Some(10), true, later).unwrap();
        let expired_again = page_again
            .entries
            .iter()
            .filter(|entry| entry.kind == AnnouncementStreamEventKind::Expired)
            .count();
        assert_eq!(expired_again, 1);
    }

    #[test]
    fn index_style_withdrawal_projects_bootstrap_stream() {
        let announced = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, announced, "systems");
        let probe = FixedOriginProbe {
            view: Some(probe_view_matching(&announcement)),
            error: None,
        };
        admit_bootstrap_announcement(&mut store, announcement.clone(), &probe, true, announced)
            .unwrap();
        let withdrawal = crate::signing::sign_pod_withdrawal(
            &node,
            crate::domain::PodWithdrawal {
                id: uuid::Uuid::now_v7(),
                origin_node_id: node.id,
                signer: announcement.signer.clone(),
                pod_slug: announcement.pod_slug.clone(),
                public_pod_url: Some(announcement.public_pod_url.clone()),
                covers_announcement_id: Some(announcement.id),
                withdrawn_at: announced + Duration::hours(1),
                signature: String::new(),
            },
        )
        .unwrap();
        // Simulate Index path: retain then project (no open Bootstrap admit).
        crate::pod_announcement::retain_verified_pod_withdrawal(
            &mut store,
            withdrawal.clone(),
            None,
            announced + Duration::hours(1),
        )
        .unwrap();
        let seq =
            project_bootstrap_withdrawal(&mut store, &withdrawal, announced + Duration::hours(1));
        assert!(seq.is_some());
        assert!(!is_bootstrap_admitted(
            &store,
            &(node.id, "systems".to_string())
        ));
        let page = read_announcement_stream(
            &mut store,
            None,
            Some(10),
            true,
            announced + Duration::hours(1),
        )
        .unwrap();
        assert!(page
            .entries
            .iter()
            .any(|entry| entry.kind == AnnouncementStreamEventKind::Withdrawn));
    }

    #[test]
    fn map_store_error_is_exhaustive_and_faithful() {
        assert_eq!(
            map_store_error(crate::store::StoreError::AnnouncementWithdrawn),
            BootstrapAdmissionRejectionReason::AnnouncementWithdrawn
        );
        assert_eq!(
            map_store_error(crate::store::StoreError::InvalidSignature),
            BootstrapAdmissionRejectionReason::InvalidSignature
        );
        assert_eq!(
            map_store_error(crate::store::StoreError::AnnouncementExpired),
            BootstrapAdmissionRejectionReason::StaleLease
        );
    }
}
