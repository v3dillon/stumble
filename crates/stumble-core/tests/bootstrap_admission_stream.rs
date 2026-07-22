//! Focused acceptance for open Bootstrap admission and Announcement Streams.
//!
//! Drives Core entry points with temporary SQLite, a fake Origin probe, and a
//! deterministic clock. Origin and Bootstrap are separate nodes so admission
//! does not observe the Origin's local retain side-effects.

use chrono::{Duration, TimeZone, Utc};
use std::sync::Arc;
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-bootstrap-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn harness(tools: &AgentTools, label: &str, capabilities: Vec<HarnessCapability>) -> AuthContext {
    let owner = tools.default_auth_context().unwrap();
    let issued = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind: AgentHarnessKind::Interactive,
                capabilities,
                pod_ids: None,
            },
        )
        .unwrap();
    tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap()
}

fn create_public_pod(tools: &AgentTools, slug: &str, description: &str) -> Pod {
    let proposer = harness(
        tools,
        "public Pod proposer",
        vec![HarnessCapability::PodCuration],
    );
    let approver = harness(
        tools,
        "public Pod approver",
        vec![HarnessCapability::Approval],
    );
    let pod = tools
        .create_pod(
            &proposer,
            CreatePodRequest {
                name: slug.replace('-', " "),
                slug: slug.into(),
                description: description.into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let now = Utc::now();
    let proposal = tools
        .create_pending_proposal(
            &proposer,
            SensitiveChange::PublishPod { pod_id: pod.id },
            now,
            now + Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

fn signed_announcement(
    origin: &AgentTools,
    slug: &str,
    at: chrono::DateTime<Utc>,
) -> PodAnnouncement {
    let ctx = origin.default_auth_context().unwrap();
    let url = format!("https://origin.example/federation/pods/{slug}");
    origin.pod_announcement_at(&ctx, slug, &url, at).unwrap()
}

fn pod_owner(tools: &AgentTools, pod_id: PodId) -> AuthContext {
    let store = tools.store();
    let store = store.read().unwrap();
    let owner_user = store
        .pod_roles
        .iter()
        .find(|assignment| assignment.pod_id == pod_id && assignment.role == PodRole::Owner)
        .map(|assignment| assignment.user_id)
        .expect("public Pod has an Owner");
    let mut ctx = tools.default_auth_context().unwrap();
    ctx.user_id = Some(owner_user);
    ctx
}

fn bootstrap_tools(data_dir: &TestDataDir, probe: Arc<dyn OriginProbe>) -> AgentTools {
    AgentTools::open_home_node(&data_dir.0, seed_store)
        .unwrap()
        .with_bootstrap_capability(true, probe)
}

fn origin_tools(data_dir: &TestDataDir) -> AgentTools {
    AgentTools::open_home_node(&data_dir.0, seed_store).unwrap()
}

#[test]
fn public_origin_admits_without_user_or_trusted_peer() {
    let origin_dir = TestDataDir::new("open-admit-origin");
    let bootstrap_dir = TestDataDir::new("open-admit-bootstrap");
    let origin = origin_tools(&origin_dir);
    let probe = Arc::new(ScriptedMatchingOriginProbe::default());
    let bootstrap = bootstrap_tools(&bootstrap_dir, probe.clone());
    let _pod = create_public_pod(&origin, "open-systems", "Open systems subject");
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    let announcement = signed_announcement(&origin, "open-systems", now);
    probe.set_announcement(&announcement);

    // No AuthContext / trusted peer: open admission only needs the signed artifact.
    let accepted = bootstrap
        .admit_bootstrap_announcement_at(announcement.clone(), now)
        .unwrap();
    assert_eq!(accepted.outcome, BootstrapAdmissionOutcomeKind::Admitted);
    assert!(accepted.stream_sequence.is_some());
    assert_eq!(accepted.known.announcement.id, announcement.id);
    assert!(accepted.known.received_from_peer_id.is_none());
}

#[test]
fn admission_verifies_signature_lease_protocol_url_and_manifest() {
    let origin_dir = TestDataDir::new("verify-origin");
    let bootstrap_dir = TestDataDir::new("verify-bootstrap");
    let origin = origin_tools(&origin_dir);
    let probe = Arc::new(ScriptedMatchingOriginProbe::default());
    let bootstrap = bootstrap_tools(&bootstrap_dir, probe.clone());
    let _pod = create_public_pod(&origin, "verify-pod", "Verify subject");
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    let mut announcement = signed_announcement(&origin, "verify-pod", now);
    probe.set_announcement(&announcement);
    bootstrap
        .admit_bootstrap_announcement_at(announcement.clone(), now)
        .unwrap();

    // Invalid signature.
    announcement.signature = "not-a-signature".into();
    let err = bootstrap
        .admit_bootstrap_announcement_at(announcement, now)
        .unwrap_err();
    match err {
        AgentToolsError::BootstrapRejected { reason, .. } => {
            assert_eq!(reason, BootstrapAdmissionRejectionReason::InvalidSignature);
            assert_eq!(reason.as_code(), "invalid_signature");
        }
        other => panic!("unexpected error: {other}"),
    }

    // Stale lease: produce at past issuance so the lease is inactive at `now`.
    let _stale_pod = create_public_pod(&origin, "stale-pod", "Stale subject");
    let announced = now - announcement_lease_duration() - Duration::hours(1);
    let stale = signed_announcement(&origin, "stale-pod", announced);
    probe.set_announcement(&stale);
    let err = bootstrap
        .admit_bootstrap_announcement_at(stale, now)
        .unwrap_err();
    match err {
        AgentToolsError::BootstrapRejected { reason, .. } => {
            assert_eq!(reason, BootstrapAdmissionRejectionReason::StaleLease);
        }
        other => panic!("unexpected error: {other}"),
    }

    // Unreachable origin.
    let unreachable_dir = TestDataDir::new("verify-unreachable");
    let unreachable = bootstrap_tools(&unreachable_dir, Arc::new(UnreachableOriginProbe));
    let live = signed_announcement(&origin, "verify-pod", now + Duration::minutes(1));
    let err = unreachable
        .admit_bootstrap_announcement_at(live, now + Duration::minutes(1))
        .unwrap_err();
    match err {
        AgentToolsError::BootstrapRejected { reason, .. } => {
            assert_eq!(reason, BootstrapAdmissionRejectionReason::UnreachableOrigin);
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn rejection_reasons_are_stable_and_audited() {
    let origin_dir = TestDataDir::new("audit-origin");
    let bootstrap_dir = TestDataDir::new("audit-bootstrap");
    let origin = origin_tools(&origin_dir);
    let bootstrap = bootstrap_tools(&bootstrap_dir, Arc::new(UnreachableOriginProbe));
    let _pod = create_public_pod(&origin, "audit-pod", "Audit subject");
    let now = Utc.with_ymd_and_hms(2026, 9, 2, 0, 0, 0).unwrap();
    let announcement = signed_announcement(&origin, "audit-pod", now);
    let _ = bootstrap
        .admit_bootstrap_announcement_at(announcement.clone(), now)
        .unwrap_err();
    let store = bootstrap.store();
    let store = store.read().unwrap();
    assert_eq!(store.bootstrap_rejection_audits.len(), 1);
    let audit = &store.bootstrap_rejection_audits[0];
    assert_eq!(
        audit.reason,
        BootstrapAdmissionRejectionReason::UnreachableOrigin
    );
    assert_eq!(audit.origin_node_id, Some(announcement.origin_node_id));
    assert_eq!(audit.pod_slug.as_deref(), Some("audit-pod"));
    let serialized = serde_json::to_value(audit).unwrap();
    let text = serialized.to_string();
    assert!(!text.contains("user_id"));
    assert!(!text.contains("taste"));
    assert!(!text.contains("subscription"));
}

#[test]
fn rate_limits_and_per_origin_quota_bound_monopoly() {
    let origin_dir = TestDataDir::new("limits-origin");
    let bootstrap_dir = TestDataDir::new("limits-bootstrap");
    let origin = origin_tools(&origin_dir);
    let probe = Arc::new(ScriptedMatchingOriginProbe::default());
    let bootstrap = bootstrap_tools(&bootstrap_dir, probe.clone());
    let now = Utc.with_ymd_and_hms(2026, 9, 3, 0, 0, 0).unwrap();

    for i in 0..MAX_ACTIVE_ANNOUNCEMENTS_PER_ORIGIN {
        let slug = format!("quota-pod-{i}");
        let _pod = create_public_pod(&origin, &slug, &format!("Quota {i}"));
        let announcement = signed_announcement(&origin, &slug, now);
        probe.set_announcement(&announcement);
        bootstrap
            .admit_bootstrap_announcement_at(announcement, now)
            .unwrap();
    }
    let overflow_slug = format!("quota-pod-{}", MAX_ACTIVE_ANNOUNCEMENTS_PER_ORIGIN);
    let _pod = create_public_pod(&origin, &overflow_slug, "Overflow");
    let overflow = signed_announcement(&origin, &overflow_slug, now);
    probe.set_announcement(&overflow);
    let err = bootstrap
        .admit_bootstrap_announcement_at(overflow, now)
        .unwrap_err();
    match err {
        AgentToolsError::BootstrapRejected { reason, .. } => {
            assert_eq!(
                reason,
                BootstrapAdmissionRejectionReason::OriginQuotaExceeded
            );
        }
        other => panic!("unexpected error: {other}"),
    }

    // Rate limit renewals of a single Pod on a fresh Bootstrap node.
    let rate_origin_dir = TestDataDir::new("rate-origin");
    let rate_bootstrap_dir = TestDataDir::new("rate-bootstrap");
    let rate_origin = origin_tools(&rate_origin_dir);
    let rate_probe = Arc::new(ScriptedMatchingOriginProbe::default());
    let rate_bootstrap = bootstrap_tools(&rate_bootstrap_dir, rate_probe.clone());
    let _pod = create_public_pod(&rate_origin, "rate-pod", "Rate subject");
    let base = signed_announcement(&rate_origin, "rate-pod", now);
    rate_probe.set_announcement(&base);
    rate_bootstrap
        .admit_bootstrap_announcement_at(base, now)
        .unwrap();
    for i in 1..=MAX_ORIGIN_ADMISSIONS_PER_WINDOW {
        let renewal_at = now + Duration::seconds(i as i64);
        let renewal = signed_announcement(&rate_origin, "rate-pod", renewal_at);
        rate_probe.set_announcement(&renewal);
        let result = rate_bootstrap.admit_bootstrap_announcement_at(renewal, renewal_at);
        if i < MAX_ORIGIN_ADMISSIONS_PER_WINDOW {
            result.unwrap();
        } else {
            match result.unwrap_err() {
                AgentToolsError::BootstrapRejected { reason, .. } => {
                    assert_eq!(reason, BootstrapAdmissionRejectionReason::RateLimited);
                    assert_eq!(reason.as_code(), "rate_limited");
                }
                other => panic!("unexpected error: {other}"),
            }
        }
    }
}

#[test]
fn canonical_duplicates_idempotent_and_renewals_stream() {
    let origin_dir = TestDataDir::new("idempotent-origin");
    let bootstrap_dir = TestDataDir::new("idempotent-bootstrap");
    let origin = origin_tools(&origin_dir);
    let probe = Arc::new(ScriptedMatchingOriginProbe::default());
    let bootstrap = bootstrap_tools(&bootstrap_dir, probe.clone());
    let _pod = create_public_pod(&origin, "idem-pod", "Idem subject");
    let now = Utc.with_ymd_and_hms(2026, 9, 4, 0, 0, 0).unwrap();
    let announcement = signed_announcement(&origin, "idem-pod", now);
    probe.set_announcement(&announcement);
    bootstrap
        .admit_bootstrap_announcement_at(announcement.clone(), now)
        .unwrap();
    let again = bootstrap
        .admit_bootstrap_announcement_at(announcement, now)
        .unwrap();
    assert_eq!(again.outcome, BootstrapAdmissionOutcomeKind::Idempotent);
    assert!(again.stream_sequence.is_none());

    let later = now + Duration::days(1);
    let renewal = signed_announcement(&origin, "idem-pod", later);
    probe.set_announcement(&renewal);
    let renewed = bootstrap
        .admit_bootstrap_announcement_at(renewal, later)
        .unwrap();
    assert_eq!(renewed.outcome, BootstrapAdmissionOutcomeKind::Renewed);

    let page = bootstrap
        .announcement_stream_at(None, Some(10), later)
        .unwrap();
    assert!(page
        .entries
        .iter()
        .any(|e| e.kind == AnnouncementStreamEventKind::Admitted));
    assert!(page
        .entries
        .iter()
        .any(|e| e.kind == AnnouncementStreamEventKind::Renewed));
    assert_eq!(
        page.entries
            .iter()
            .filter(|e| e.kind == AnnouncementStreamEventKind::Admitted)
            .count(),
        1,
        "idempotent replay must not duplicate stream effects"
    );
}

#[test]
fn announcement_stream_is_topic_neutral_cursor_paginated_and_emits_lifecycle() {
    let origin_dir = TestDataDir::new("stream-origin");
    let bootstrap_dir = TestDataDir::new("stream-bootstrap");
    let origin = origin_tools(&origin_dir);
    let probe = Arc::new(ScriptedMatchingOriginProbe::default());
    let bootstrap = bootstrap_tools(&bootstrap_dir, probe.clone());
    let now = Utc.with_ymd_and_hms(2026, 9, 5, 0, 0, 0).unwrap();
    let mut alpha_pod = None;
    for (slug, subject) in [("alpha-pod", "Alpha"), ("beta-pod", "Beta")] {
        let pod = create_public_pod(&origin, slug, subject);
        if slug == "alpha-pod" {
            alpha_pod = Some(pod);
        }
        let announcement = signed_announcement(&origin, slug, now);
        probe.set_announcement(&announcement);
        bootstrap
            .admit_bootstrap_announcement_at(announcement, now)
            .unwrap();
    }

    let page1 = bootstrap
        .announcement_stream_at(None, Some(1), now)
        .unwrap();
    assert_eq!(page1.entries.len(), 1);
    assert!(page1.next_cursor.is_some());
    let page2 = bootstrap
        .announcement_stream_at(page1.next_cursor.as_deref(), Some(1), now)
        .unwrap();
    assert_eq!(page2.entries.len(), 1);
    assert_ne!(page1.entries[0].sequence, page2.entries[0].sequence);

    let alpha = alpha_pod.expect("alpha pod");
    let withdrawal = origin
        .withdraw_public_pod(
            &pod_owner(&origin, alpha.id),
            "alpha-pod",
            Some("https://origin.example/federation/pods/alpha-pod"),
            false,
            now + Duration::minutes(5),
        )
        .unwrap();
    bootstrap
        .admit_bootstrap_withdrawal_at(withdrawal, now + Duration::minutes(5))
        .unwrap();

    let expired_at = now + announcement_lease_duration() + Duration::seconds(1);
    let page = bootstrap
        .announcement_stream_at(None, Some(50), expired_at)
        .unwrap();
    assert!(page
        .entries
        .iter()
        .any(|e| e.kind == AnnouncementStreamEventKind::Withdrawn));
    assert!(page
        .entries
        .iter()
        .any(|e| e.kind == AnnouncementStreamEventKind::Expired));

    let serialized = serde_json::to_value(&page).unwrap();
    let text = serialized.to_string();
    assert!(!text.contains("taste_profile"));
    assert!(!text.contains("subscription"));
    assert!(!text.contains("feedback"));
    assert!(!text.contains("popularity"));
    assert!(!text.contains("endorsement"));
    assert!(!text.contains("user_id"));
}

#[test]
fn stream_cursors_resume_across_restart_and_reject_invalid() {
    let origin_dir = TestDataDir::new("cursor-origin");
    let bootstrap_dir = TestDataDir::new("cursor-bootstrap");
    let origin = origin_tools(&origin_dir);
    let probe = Arc::new(ScriptedMatchingOriginProbe::default());
    let bootstrap = bootstrap_tools(&bootstrap_dir, probe.clone());
    let now = Utc.with_ymd_and_hms(2026, 9, 6, 0, 0, 0).unwrap();
    for i in 0..3 {
        let slug = format!("cursor-pod-{i}");
        let _pod = create_public_pod(&origin, &slug, &format!("Cursor {i}"));
        let announcement = signed_announcement(&origin, &slug, now);
        probe.set_announcement(&announcement);
        bootstrap
            .admit_bootstrap_announcement_at(announcement, now)
            .unwrap();
    }
    let first = bootstrap
        .announcement_stream_at(None, Some(2), now)
        .unwrap();
    assert_eq!(first.entries.len(), 2);
    let cursor = first.next_cursor.clone().unwrap();
    drop(bootstrap);

    let restarted = bootstrap_tools(&bootstrap_dir, Arc::new(UnreachableOriginProbe));
    let second = restarted
        .announcement_stream_at(Some(&cursor), Some(10), now)
        .unwrap();
    assert_eq!(second.entries.len(), 1);
    assert!(!first
        .entries
        .iter()
        .any(|e| e.sequence == second.entries[0].sequence));

    let full = restarted
        .announcement_stream_at(None, Some(50), now)
        .unwrap();
    let mut sequences: Vec<u64> = full.entries.iter().map(|e| e.sequence).collect();
    let original = sequences.clone();
    sequences.dedup();
    assert_eq!(sequences, original);
    assert!(sequences.windows(2).all(|w| w[0] < w[1]));

    let invalid = restarted.announcement_stream_at(Some("not-a-cursor"), Some(10), now);
    match invalid.unwrap_err() {
        AgentToolsError::BootstrapRejected { reason, .. } => {
            assert_eq!(reason, BootstrapAdmissionRejectionReason::Malformed);
        }
        other => panic!("unexpected error: {other}"),
    }
    let future = restarted.announcement_stream_at(Some("999999"), Some(10), now);
    match future.unwrap_err() {
        AgentToolsError::BootstrapRejected { reason, .. } => {
            assert_eq!(reason, BootstrapAdmissionRejectionReason::Malformed);
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn admission_stream_rejection_and_leases_persist_in_sqlite() {
    let origin_dir = TestDataDir::new("persist-origin");
    let bootstrap_dir = TestDataDir::new("persist-bootstrap");
    let origin = origin_tools(&origin_dir);
    let probe = Arc::new(ScriptedMatchingOriginProbe::default());
    let bootstrap = bootstrap_tools(&bootstrap_dir, probe.clone());
    let _pod = create_public_pod(&origin, "persist-pod", "Persist subject");
    let now = Utc.with_ymd_and_hms(2026, 9, 7, 0, 0, 0).unwrap();
    let announcement = signed_announcement(&origin, "persist-pod", now);
    probe.set_announcement(&announcement);
    let accepted = bootstrap
        .admit_bootstrap_announcement_at(announcement.clone(), now)
        .unwrap();
    let _ = bootstrap
        .admit_bootstrap_announcement_at(
            {
                let mut bad = announcement.clone();
                bad.signature = "forged".into();
                bad
            },
            now,
        )
        .unwrap_err();
    drop(bootstrap);

    let restarted = bootstrap_tools(&bootstrap_dir, Arc::new(UnreachableOriginProbe));
    let store = restarted.store();
    let store = store.read().unwrap();
    assert!(store
        .known_pod_announcements
        .contains_key(&(announcement.origin_node_id, "persist-pod".into())));
    assert!(!store.announcement_stream_entries.is_empty());
    assert_eq!(
        store
            .announcement_stream_entries
            .get(&accepted.stream_sequence.unwrap())
            .unwrap()
            .payload
            .as_announcement()
            .unwrap()
            .id,
        announcement.id
    );
    assert!(!store.bootstrap_rejection_audits.is_empty());
    assert!(store.bootstrap_runtime.is_some());
    assert!(store
        .known_pod_announcements
        .values()
        .next()
        .unwrap()
        .announcement
        .lease_is_active(now));
}

#[test]
fn public_protocol_excludes_private_and_ranking_fields() {
    let origin_dir = TestDataDir::new("privacy-origin");
    let bootstrap_dir = TestDataDir::new("privacy-bootstrap");
    let origin = origin_tools(&origin_dir);
    let probe = Arc::new(ScriptedMatchingOriginProbe::default());
    let bootstrap = bootstrap_tools(&bootstrap_dir, probe.clone());
    let _pod = create_public_pod(&origin, "privacy-pod", "Privacy subject");
    let now = Utc.with_ymd_and_hms(2026, 9, 8, 0, 0, 0).unwrap();
    let announcement = signed_announcement(&origin, "privacy-pod", now);
    probe.set_announcement(&announcement);
    let accepted = bootstrap
        .admit_bootstrap_announcement_at(announcement, now)
        .unwrap();
    let page = bootstrap
        .announcement_stream_at(None, Some(10), now)
        .unwrap();

    for value in [
        serde_json::to_value(&accepted).unwrap(),
        serde_json::to_value(&page).unwrap(),
    ] {
        let text = value.to_string().to_lowercase();
        for forbidden in [
            "taste_profile",
            "subscription",
            "feedback",
            "popularity",
            "endorsement",
            "user_id",
            "personalized",
            "ranking_score",
        ] {
            assert!(
                !text.contains(forbidden),
                "public bootstrap payload must not contain {forbidden}: {text}"
            );
        }
    }
}
