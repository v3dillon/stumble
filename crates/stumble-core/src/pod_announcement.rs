//! Announcement Lease retention, renewal preference, and Pod Withdrawal helpers.
//!
//! Discovery eligibility for public Pod Announcements is governed by a renewable
//! 30-day lease and Origin-signed withdrawals. Neither mechanism deletes
//! Subscriptions or synchronized content.

use crate::domain::{
    announcement_lease_duration, KnownPodAnnouncement, KnownPodWithdrawal, NodeIdentity, NodeInfo,
    PackageVersion, PeerId, Pod, PodAnnouncement, PodId, PodWithdrawal, TrustPolicy, Visibility,
    CURRENT_PROTOCOL_VERSION,
};
use crate::signing::{sign_pod_announcement, sign_pod_withdrawal};
use crate::store::{InMemoryStore, StoreError};
use chrono::{DateTime, Utc};
use std::net::IpAddr;
use url::Url;
use uuid::Uuid;

/// Immediate delivery provenance for a verified announcement retention.
///
/// Local/origin retains omit remote sources. Remote delivery records at most one
/// peer, one Index URL, and one Bootstrap URL per call; Index and Bootstrap URLs
/// accumulate across retains of the same signed announcement identity.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeliveryProvenance {
    /// Trusted peer that delivered the announcement, when peer-sourced.
    pub peer_id: Option<PeerId>,
    /// Configured Index Node base URL that returned the announcement.
    pub index_url: Option<String>,
    /// Bootstrap base URL that delivered the announcement.
    pub bootstrap_url: Option<String>,
}

impl DeliveryProvenance {
    /// Origin-local or direct retain with no remote delivery source.
    pub const LOCAL: Self = Self {
        peer_id: None,
        index_url: None,
        bootstrap_url: None,
    };

    /// Peer-delivered announcement provenance.
    #[must_use]
    pub const fn peer(peer_id: PeerId) -> Self {
        Self {
            peer_id: Some(peer_id),
            index_url: None,
            bootstrap_url: None,
        }
    }

    /// Index Node search result provenance.
    #[must_use]
    pub fn index(url: impl Into<String>) -> Self {
        Self {
            peer_id: None,
            index_url: Some(url.into()),
            bootstrap_url: None,
        }
    }

    /// Bootstrap Announcement Stream provenance.
    #[must_use]
    pub fn bootstrap(url: impl Into<String>) -> Self {
        Self {
            peer_id: None,
            index_url: None,
            bootstrap_url: Some(url.into()),
        }
    }
}

/// Whether any enabled Bootstrap endpoint still provides provenance for `base_url`.
#[must_use]
pub fn retains_bootstrap_url(store: &InMemoryStore, base_url: &str) -> bool {
    store
        .bootstrap_endpoints
        .values()
        .any(|endpoint| endpoint.enabled && endpoint.base_url == base_url)
}

/// Whether a retained announcement still has active delivery provenance.
///
/// Local/origin retains (no remote sources) remain eligible. Remote sources are
/// active when a peer delivered them, any recorded Index URL still matches Trust
/// Policy, or at least one enabled Bootstrap still matches a recorded delivery URL.
#[must_use]
pub fn announcement_delivery_is_active(
    store: &InMemoryStore,
    known: &KnownPodAnnouncement,
    policy: Option<&TrustPolicy>,
) -> bool {
    let has_peer = known.received_from_peer_id.is_some();
    let has_index = !known.received_from_index_urls.is_empty();
    let has_bootstrap = !known.received_from_bootstrap_urls.is_empty();
    if !has_peer && !has_index && !has_bootstrap {
        return true;
    }
    if has_peer {
        return true;
    }
    if known
        .received_from_index_urls
        .iter()
        .any(|url| policy.is_some_and(|policy| policy.retains_index_url(url)))
    {
        return true;
    }
    known
        .received_from_bootstrap_urls
        .iter()
        .any(|url| retains_bootstrap_url(store, url))
}

/// Compares two verified announcements for the same Pod.
///
/// Prefer an active lease over an expired one; among equal lease validity prefer
/// the later `announced_at`, then higher package version.
#[must_use]
pub fn compare_announcement_preference(
    left: &PodAnnouncement,
    right: &PodAnnouncement,
    now: DateTime<Utc>,
) -> std::cmp::Ordering {
    let left_active = left.lease_is_active(now);
    let right_active = right.lease_is_active(now);
    match (left_active, right_active) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => left
            .announced_at
            .cmp(&right.announced_at)
            .then_with(|| left.package_version.cmp(&right.package_version))
            .then_with(|| left.id.cmp(&right.id)),
    }
}

/// Returns whether a retained announcement is eligible for new discovery/relaying.
#[must_use]
pub fn announcement_is_discovery_eligible(
    store: &InMemoryStore,
    announcement: &PodAnnouncement,
    now: DateTime<Utc>,
) -> bool {
    if !announcement.lease_is_active(now) {
        return false;
    }
    let key = (announcement.origin_node_id, announcement.pod_slug.clone());
    if let Some(known) = store.known_pod_withdrawals.get(&key) {
        if known.withdrawal.withdrawn_at >= announcement.announced_at {
            return false;
        }
    }
    true
}

/// Validates and canonicalizes a direct public Pod address.
///
/// # Errors
///
/// Returns a validation error unless the address uses HTTPS (or loopback HTTP)
/// and has the canonical `/federation/pods/<slug>` shape.
pub fn canonical_public_pod_url(value: &str) -> Result<String, StoreError> {
    let mut url =
        Url::parse(value).map_err(|error| StoreError::Validation(format!("bad url: {error}")))?;
    let host = url
        .host_str()
        .ok_or_else(|| StoreError::Validation("public Pod URL must include a host".to_string()))?;
    let is_loopback_http = url.scheme() == "http"
        && (host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback()));
    if url.scheme() != "https" && !is_loopback_http {
        return Err(StoreError::Validation(
            "public Pod URL must use HTTPS except on loopback".to_string(),
        ));
    }
    let path = url.path().trim_end_matches('/').to_string();
    let segments = path.split('/').collect::<Vec<_>>();
    if segments.len() != 4
        || !segments[0].is_empty()
        || segments[1] != "federation"
        || segments[2] != "pods"
        || segments[3].is_empty()
    {
        return Err(StoreError::Validation(
            "public Pod URL must use /federation/pods/<slug>".to_string(),
        ));
    }
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string().trim_end_matches('/').to_string())
}

/// Validates that a public Pod URL is canonical and matches `pod_slug`.
///
/// # Errors
///
/// Returns a validation error when the URL is malformed or the path slug does
/// not match the Pod identity.
pub fn validate_public_pod_url(value: &str, pod_slug: &str) -> Result<String, StoreError> {
    let canonical = canonical_public_pod_url(value)?;
    let url = Url::parse(&canonical)
        .map_err(|error| StoreError::Validation(format!("bad url: {error}")))?;
    if url.path().trim_end_matches('/') != format!("/federation/pods/{pod_slug}") {
        return Err(StoreError::Validation(
            "public Pod URL does not match the signed Pod slug".to_string(),
        ));
    }
    Ok(canonical)
}

/// Builds and signs a Pod Announcement from current Origin public state.
///
/// # Errors
///
/// Returns store errors when the package is missing, the URL is invalid, the
/// package version is invalid, or signing fails.
pub fn build_signed_pod_announcement(
    store: &InMemoryStore,
    node: &NodeIdentity,
    pod: &Pod,
    public_pod_url: &str,
    now: DateTime<Utc>,
) -> Result<PodAnnouncement, StoreError> {
    let public_pod_url = validate_public_pod_url(public_pod_url, &pod.slug)?;
    let package = store
        .pod_skill_packs
        .get(&pod.id)
        .ok_or_else(|| StoreError::NotFound("Pod Package".into()))?;
    let announced_at = now;
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
            pod_slug: pod.slug.clone(),
            pod_name: pod.name.clone(),
            subject: pod.description.clone(),
            public_pod_url,
            package_version: PackageVersion::new(package.version)
                .map_err(|error| StoreError::Validation(error.to_string()))?,
            latest_event_hash: store.latest_federated_event_hash(&pod.slug),
            announced_at,
            expires_at: announced_at + announcement_lease_duration(),
            signature: String::new(),
        },
    )
    .map_err(|error| StoreError::Validation(error.to_string()))
}

/// Issues a signed Origin announcement and retains it under lease preference rules.
///
/// # Errors
///
/// Same as [`build_signed_pod_announcement`] and [`retain_verified_pod_announcement`].
pub fn issue_and_retain_origin_pod_announcement(
    store: &mut InMemoryStore,
    node: &NodeIdentity,
    pod: &Pod,
    public_pod_url: &str,
    now: DateTime<Utc>,
) -> Result<PodAnnouncement, StoreError> {
    let announcement = build_signed_pod_announcement(store, node, pod, public_pod_url, now)?;
    retain_verified_pod_announcement(store, announcement.clone(), DeliveryProvenance::LOCAL, now)?;
    Ok(announcement)
}

/// Builds, signs, and retains an Origin-signed Pod Withdrawal for a Pod identity.
///
/// When `public_pod_url` is `None`, the last retained announcement's URL is used
/// if available. `covers_announcement_id` is taken from any retained announcement.
///
/// # Errors
///
/// Returns store errors for invalid URLs, signing failure, or retention rejection.
pub fn issue_origin_pod_withdrawal(
    store: &mut InMemoryStore,
    node: &NodeIdentity,
    pod_slug: &str,
    public_pod_url: Option<String>,
    now: DateTime<Utc>,
) -> Result<PodWithdrawal, StoreError> {
    let key = (node.id, pod_slug.to_string());
    let covers_announcement_id = store
        .known_pod_announcements
        .get(&key)
        .map(|known| known.announcement.id);
    let public_pod_url = match public_pod_url {
        Some(url) => Some(validate_public_pod_url(&url, pod_slug)?),
        None => store
            .known_pod_announcements
            .get(&key)
            .map(|known| known.announcement.public_pod_url.clone()),
    };
    let withdrawal = sign_pod_withdrawal(
        node,
        PodWithdrawal {
            id: Uuid::now_v7(),
            origin_node_id: node.id,
            signer: NodeInfo {
                node_id: node.id,
                display_name: node.display_name.clone(),
                public_key: node.public_key.clone(),
                supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
            },
            pod_slug: pod_slug.to_string(),
            public_pod_url,
            covers_announcement_id,
            withdrawn_at: now,
            signature: String::new(),
        },
    )
    .map_err(|error| StoreError::Validation(error.to_string()))?;
    retain_verified_pod_withdrawal(store, withdrawal.clone(), None, now)?;
    Ok(withdrawal)
}

/// Refreshes a retained Origin announcement when public state has changed.
///
/// No-op when the Pod is not public/origin-local, no prior announcement URL is
/// known, or the retained announcement already matches current public state.
///
/// # Errors
///
/// Returns store errors when signing or retention fails.
pub fn refresh_public_pod_announcement_if_needed(
    store: &mut InMemoryStore,
    pod_id: PodId,
    now: DateTime<Utc>,
) -> Result<Option<PodAnnouncement>, StoreError> {
    let pod = store
        .pods
        .get(&pod_id)
        .cloned()
        .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
    if pod.visibility != Visibility::Public {
        return Ok(None);
    }
    let node = store.node_for_tenant(pod.tenant_id)?;
    if pod
        .origin_node_id
        .is_some_and(|origin_node_id| origin_node_id != node.id)
    {
        return Ok(None);
    }
    let key = (node.id, pod.slug.clone());
    let Some(known) = store.known_pod_announcements.get(&key).cloned() else {
        return Ok(None);
    };
    // A withdrawal covering this Pod blocks refresh until re-announced explicitly.
    if let Some(withdrawal) = store.known_pod_withdrawals.get(&key) {
        if withdrawal.withdrawal.withdrawn_at >= known.announcement.announced_at {
            return Ok(None);
        }
    }
    let package = store
        .pod_skill_packs
        .get(&pod.id)
        .ok_or_else(|| StoreError::NotFound("Pod Package".into()))?;
    let package_version = PackageVersion::new(package.version)
        .map_err(|error| StoreError::Validation(error.to_string()))?;
    let latest_event_hash = store.latest_federated_event_hash(&pod.slug);
    let current = &known.announcement;
    if current.pod_name == pod.name
        && current.subject == pod.description
        && current.package_version == package_version
        && current.latest_event_hash == latest_event_hash
        && current.lease_is_active(now)
    {
        return Ok(None);
    }
    let refreshed =
        issue_and_retain_origin_pod_announcement(store, &node, &pod, &current.public_pod_url, now)?;
    Ok(Some(refreshed))
}

/// Verifies and retains an Origin-signed announcement under lease preference rules.
///
/// Delivery provenance merges when the retained signed announcement is the same
/// identity: Bootstrap URLs accumulate, peer id and Index URL prefer newly
/// supplied values while preserving prior sources when the new delivery omits
/// them. A strictly preferred newer announcement starts provenance from this
/// delivery only.
///
/// # Errors
///
/// Returns typed store errors for invalid signatures, expired leases, withdrawn
/// Pods, stale renewals, or malformed direct addresses.
pub fn retain_verified_pod_announcement(
    store: &mut InMemoryStore,
    announcement: PodAnnouncement,
    provenance: DeliveryProvenance,
    now: DateTime<Utc>,
) -> Result<KnownPodAnnouncement, StoreError> {
    validate_public_pod_url(&announcement.public_pod_url, &announcement.pod_slug)?;
    if !announcement
        .verify()
        .map_err(|_| StoreError::InvalidSignature)?
    {
        return Err(StoreError::InvalidSignature);
    }
    if !announcement.lease_is_active(now) {
        return Err(StoreError::AnnouncementExpired);
    }

    let key = (announcement.origin_node_id, announcement.pod_slug.clone());
    if let Some(known_withdrawal) = store.known_pod_withdrawals.get(&key) {
        if known_withdrawal.withdrawal.withdrawn_at >= announcement.announced_at {
            return Err(StoreError::AnnouncementWithdrawn);
        }
        // A later re-announcement supersedes the prior withdrawal.
        store.known_pod_withdrawals.remove(&key);
    }

    if let Some(existing) = store.known_pod_announcements.get(&key) {
        match compare_announcement_preference(&existing.announcement, &announcement, now) {
            // Existing is strictly preferred over the candidate.
            std::cmp::Ordering::Greater => {
                return Err(StoreError::AnnouncementStale);
            }
            // Equal preference: allow overwrite so delivery provenance can refresh
            // (e.g. replacement Index Node for the same signed announcement).
            std::cmp::Ordering::Equal | std::cmp::Ordering::Less => {}
        }
    }

    let (peer_id, mut index_urls, mut bootstrap_urls) =
        if let Some(existing) = store.known_pod_announcements.get(&key) {
            if existing.announcement.id == announcement.id {
                (
                    provenance.peer_id.or(existing.received_from_peer_id),
                    existing.received_from_index_urls.clone(),
                    existing.received_from_bootstrap_urls.clone(),
                )
            } else {
                (
                    provenance.peer_id,
                    std::collections::BTreeSet::new(),
                    std::collections::BTreeSet::new(),
                )
            }
        } else {
            (
                provenance.peer_id,
                std::collections::BTreeSet::new(),
                std::collections::BTreeSet::new(),
            )
        };
    if let Some(url) = provenance.index_url {
        index_urls.insert(url);
    }
    if let Some(url) = provenance.bootstrap_url {
        bootstrap_urls.insert(url);
    }

    let known = KnownPodAnnouncement {
        announcement,
        received_from_peer_id: peer_id,
        received_from_index_urls: index_urls,
        received_from_bootstrap_urls: bootstrap_urls,
        received_at: now,
    };
    store.known_pod_announcements.insert(key, known.clone());
    // Opt-in Discovery Peers project retained verified announcements into the
    // peer-local stream so pure Home Nodes can serve without Bootstrap.
    crate::discovery_peer::maybe_project_peer_serving_announcement(store, &known.announcement, now);
    Ok(known)
}

/// Verifies and retains an Origin-signed Pod Withdrawal.
///
/// Successful retention removes the matching announcement from discovery while
/// leaving Subscriptions and synchronized content untouched.
///
/// # Errors
///
/// Returns typed store errors for invalid signatures, mismatched Origin identity,
/// or stale withdrawals.
pub fn retain_verified_pod_withdrawal(
    store: &mut InMemoryStore,
    withdrawal: PodWithdrawal,
    received_from_peer_id: Option<PeerId>,
    now: DateTime<Utc>,
) -> Result<KnownPodWithdrawal, StoreError> {
    if !withdrawal
        .verify()
        .map_err(|_| StoreError::InvalidSignature)?
    {
        return Err(StoreError::InvalidSignature);
    }
    if withdrawal.origin_node_id != withdrawal.signer.node_id {
        return Err(StoreError::Validation(
            "Pod Withdrawal origin does not match signer".into(),
        ));
    }

    let key = (withdrawal.origin_node_id, withdrawal.pod_slug.clone());
    if let Some(existing) = store.known_pod_withdrawals.get(&key) {
        if existing.withdrawal.withdrawn_at > withdrawal.withdrawn_at {
            return Err(StoreError::WithdrawalStale);
        }
        if existing.withdrawal.id == withdrawal.id && existing.withdrawal == withdrawal {
            return Ok(existing.clone());
        }
    }

    if let Some(covers) = withdrawal.covers_announcement_id {
        if let Some(known) = store.known_pod_announcements.get(&key) {
            if known.announcement.id != covers
                && known.announcement.announced_at > withdrawal.withdrawn_at
            {
                return Err(StoreError::WithdrawalStale);
            }
        }
    }

    // Remove discovery eligibility without touching subscriptions or content.
    if let Some(known) = store.known_pod_announcements.get(&key) {
        if known.announcement.announced_at <= withdrawal.withdrawn_at {
            store.known_pod_announcements.remove(&key);
        }
    }

    let known = KnownPodWithdrawal {
        withdrawal,
        received_from_peer_id,
        received_at: now,
    };
    store.known_pod_withdrawals.insert(key, known.clone());
    Ok(known)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        announcement_lease_duration, NodeInfo, PackageVersion, CURRENT_PROTOCOL_VERSION,
    };
    use crate::signing::{create_node_identity, sign_pod_announcement};
    use chrono::TimeZone;
    use uuid::Uuid;

    fn sample_announcement(
        node: &crate::domain::NodeIdentity,
        announced_at: DateTime<Utc>,
        package_version: i32,
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
                pod_slug: "systems".into(),
                pod_name: "Systems".into(),
                subject: "Distributed systems".into(),
                public_pod_url: "https://origin.example/federation/pods/systems".into(),
                package_version: PackageVersion::new(package_version).unwrap(),
                latest_event_hash: None,
                announced_at,
                expires_at: announced_at + announcement_lease_duration(),
                signature: String::new(),
            },
        )
        .unwrap()
    }

    #[test]
    fn prefers_later_active_lease_and_rejects_stale() {
        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let t1 = t0 + chrono::Duration::days(1);
        let now = t1 + chrono::Duration::hours(1);
        let mut store = InMemoryStore::default();
        let node = create_node_identity("origin", None);
        let first = sample_announcement(&node, t0, 1);
        retain_verified_pod_announcement(&mut store, first.clone(), DeliveryProvenance::LOCAL, now)
            .unwrap();
        let renewal = sample_announcement(&node, t1, 1);
        let retained = retain_verified_pod_announcement(
            &mut store,
            renewal.clone(),
            DeliveryProvenance::LOCAL,
            now,
        )
        .unwrap();
        assert_eq!(retained.announcement.id, renewal.id);
        assert!(matches!(
            retain_verified_pod_announcement(&mut store, first, DeliveryProvenance::LOCAL, now),
            Err(StoreError::AnnouncementStale)
        ));
    }

    #[test]
    fn rejects_expired_lease() {
        let announced = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let now = announced + announcement_lease_duration() + chrono::Duration::seconds(1);
        let mut store = InMemoryStore::default();
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, announced, 1);
        assert!(matches!(
            retain_verified_pod_announcement(
                &mut store,
                announcement,
                DeliveryProvenance::LOCAL,
                now
            ),
            Err(StoreError::AnnouncementExpired)
        ));
    }

    #[test]
    fn lease_is_inactive_at_exact_expiry_instant() {
        let announced = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let expires_at = announced + announcement_lease_duration();
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, announced, 1);
        assert_eq!(announcement.expires_at, expires_at);
        assert!(announcement.lease_is_active(expires_at - chrono::Duration::seconds(1)));
        assert!(
            !announcement.lease_is_active(expires_at),
            "lease end is exclusive: active only while expires_at > now"
        );
        assert!(!announcement.lease_is_active(expires_at + chrono::Duration::seconds(1)));
    }

    #[test]
    fn retain_rejects_malformed_public_pod_url() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let node = create_node_identity("origin", None);
        let mut announcement = sample_announcement(&node, now, 1);
        announcement.public_pod_url = "https://origin.example/wrong/path".into();
        announcement = sign_pod_announcement(&node, announcement).unwrap();
        assert!(matches!(
            retain_verified_pod_announcement(
                &mut store,
                announcement,
                DeliveryProvenance::LOCAL,
                now
            ),
            Err(StoreError::Validation(_))
        ));
    }
}
