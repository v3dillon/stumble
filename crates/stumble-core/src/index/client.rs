//! Outbound Home Node Index search for explicit User-authored Explore only.
//!
//! Remote Index relevance is retrieval evidence only. Home Nodes verify every
//! returned announcement, apply Trust Policy, discard remote ordering, and
//! recompute relevance locally through Explore ranking.

use super::types::{index_fail, index_request_is_public_only, MAX_INDEX_SEARCH_LIMIT};
use crate::domain::{
    IndexExploreImportOutcome, IndexExploreImportReport, IndexSearchFailure,
    IndexSearchFailureKind, IndexSearchRequest, KnownPodAnnouncement, KnownPodWithdrawal,
    NodeIdentityId, PodAnnouncementSearchResponse, TrustPolicy,
};
use crate::pod_announcement::{retain_verified_pod_announcement, DeliveryProvenance};
use crate::store::{InMemoryStore, StoreError};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Mutex;

/// Transport port for querying a replaceable Index Node.
///
/// Production implementations perform HTTP GET against
/// `{base_url}/discovery/announcements?q=&limit=`. Tests inject scripted
/// responses. Requests must carry only [`IndexSearchRequest`] fields—never User
/// identity or private discovery evidence.
pub trait IndexSearchClient: Send + Sync {
    /// Fetches a non-authoritative search response from `base_url`.
    ///
    /// # Errors
    ///
    /// Returns a typed [`IndexSearchFailure`] for transport or protocol errors.
    fn search_index(
        &self,
        base_url: &str,
        request: &IndexSearchRequest,
    ) -> Result<PodAnnouncementSearchResponse, IndexSearchFailure>;
}

/// Scripted Index client for deterministic tests.
#[derive(Debug, Default)]
pub struct ScriptedIndexSearchClient {
    responses:
        Mutex<HashMap<String, Vec<Result<PodAnnouncementSearchResponse, IndexSearchFailure>>>>,
    /// Captured outbound requests for privacy assertions.
    pub captured: Mutex<Vec<(String, IndexSearchRequest)>>,
}

impl ScriptedIndexSearchClient {
    /// Creates an empty scripted client.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues a successful response for `base_url` (FIFO).
    pub fn push_response(&self, base_url: &str, response: PodAnnouncementSearchResponse) {
        self.responses
            .lock()
            .expect("scripted index client lock")
            .entry(normalize_base(base_url))
            .or_default()
            .push(Ok(response));
    }

    /// Queues a typed failure for `base_url` (FIFO).
    pub fn push_error(&self, base_url: &str, error: IndexSearchFailure) {
        self.responses
            .lock()
            .expect("scripted index client lock")
            .entry(normalize_base(base_url))
            .or_default()
            .push(Err(error));
    }
}

impl IndexSearchClient for ScriptedIndexSearchClient {
    fn search_index(
        &self,
        base_url: &str,
        request: &IndexSearchRequest,
    ) -> Result<PodAnnouncementSearchResponse, IndexSearchFailure> {
        debug_assert!(index_request_is_public_only(request));
        self.captured
            .lock()
            .expect("scripted index client lock")
            .push((normalize_base(base_url), request.clone()));
        let mut map = self.responses.lock().expect("scripted index client lock");
        let queue = map.entry(normalize_base(base_url)).or_default();
        if queue.is_empty() {
            return Err(index_fail(
                IndexSearchFailureKind::Transport,
                format!("no scripted response for {base_url}"),
            ));
        }
        queue.remove(0)
    }
}

fn normalize_base(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

/// Builds the public-only outbound request for an explicit Explore query.
#[must_use]
pub fn explicit_index_search_request(query: &str, limit: usize) -> IndexSearchRequest {
    IndexSearchRequest {
        query: query.to_string(),
        limit: Some(limit.clamp(1, MAX_INDEX_SEARCH_LIMIT)),
    }
}

/// Queries every configured Index Node with an explicit User-authored query and
/// retains verified announcements with per-Index provenance.
///
/// Remote relevance scores are discarded; callers recompute local Explore order
/// after import. Empty queries do not contact remote Indexes (catalog browse is
/// local-only). Failures on one Index fall through to the next without discarding
/// previously retained results.
///
/// # Errors
///
/// Returns store validation errors only when the Trust Policy or request setup
/// is invalid. Per-Index transport failures are recorded on the report.
pub fn import_from_configured_indexes(
    store: &mut InMemoryStore,
    policy: &TrustPolicy,
    query: &str,
    limit: usize,
    client: &dyn IndexSearchClient,
    now: DateTime<Utc>,
) -> Result<IndexExploreImportReport, StoreError> {
    let query = query.trim();
    let mut report = IndexExploreImportReport {
        query: query.to_string(),
        outcomes: Vec::new(),
        retained_announcements: 0,
    };

    // Explicit empty Explore stays local; never fan out for inferred catalog dumps.
    if query.is_empty() {
        return Ok(report);
    }

    let request = explicit_index_search_request(query, limit);
    debug_assert!(index_request_is_public_only(&request));

    for index in &policy.index_nodes {
        let base_url = normalize_base(&index.base_url);
        if base_url.is_empty() {
            report.outcomes.push(IndexExploreImportOutcome {
                index_base_url: index.base_url.clone(),
                ok: false,
                result_count: 0,
                retained: 0,
                error: Some(index_fail(
                    IndexSearchFailureKind::Malformed,
                    "Index base URL is empty",
                )),
            });
            continue;
        }

        match client.search_index(&base_url, &request) {
            Ok(response) => {
                let result_count = response.results.len();
                match retain_index_search_results(store, &base_url, response, now) {
                    Ok(retained) => {
                        report.retained_announcements =
                            report.retained_announcements.saturating_add(retained);
                        report.outcomes.push(IndexExploreImportOutcome {
                            index_base_url: base_url,
                            ok: true,
                            result_count,
                            retained,
                            error: None,
                        });
                    }
                    Err(error) => {
                        report.outcomes.push(IndexExploreImportOutcome {
                            index_base_url: base_url,
                            ok: false,
                            result_count,
                            retained: 0,
                            error: Some(store_error_to_index_failure(error)),
                        });
                    }
                }
            }
            Err(error) => {
                report.outcomes.push(IndexExploreImportOutcome {
                    index_base_url: base_url,
                    ok: false,
                    result_count: 0,
                    retained: 0,
                    error: Some(error),
                });
            }
        }
    }

    Ok(report)
}

/// Verifies and retains Index search results with provenance for one base URL.
///
/// Remote relevance is ignored. Invalid announcements fail the whole batch for
/// that Index so partial untrusted imports are not mixed into the local catalog.
/// On failure, only keys touched by this batch are rolled back.
///
/// # Errors
///
/// Returns verification or retain errors from the announcement pipeline.
pub fn retain_index_search_results(
    store: &mut InMemoryStore,
    index_base_url: &str,
    response: PodAnnouncementSearchResponse,
    now: DateTime<Utc>,
) -> Result<usize, StoreError> {
    let index_base_url = normalize_base(index_base_url);
    // Key-level rollback snapshots: only announcements/withdrawals this batch touches.
    let mut announcement_snapshots: HashMap<
        (NodeIdentityId, String),
        Option<KnownPodAnnouncement>,
    > = HashMap::new();
    let mut withdrawal_snapshots: HashMap<(NodeIdentityId, String), Option<KnownPodWithdrawal>> =
        HashMap::new();
    let mut retained = 0usize;
    for result in response.results {
        let key = (
            result.announcement.origin_node_id,
            result.announcement.pod_slug.clone(),
        );
        announcement_snapshots
            .entry(key.clone())
            .or_insert_with(|| store.known_pod_announcements.get(&key).cloned());
        withdrawal_snapshots
            .entry(key.clone())
            .or_insert_with(|| store.known_pod_withdrawals.get(&key).cloned());
        // Discard result.relevance — Home Node recomputes locally.
        match retain_verified_pod_announcement(
            store,
            result.announcement,
            DeliveryProvenance::index(index_base_url.clone()),
            now,
        ) {
            Ok(_) => retained = retained.saturating_add(1),
            Err(error) => {
                for (key, previous) in announcement_snapshots {
                    match previous {
                        Some(known) => {
                            store.known_pod_announcements.insert(key, known);
                        }
                        None => {
                            store.known_pod_announcements.remove(&key);
                        }
                    }
                }
                for (key, previous) in withdrawal_snapshots {
                    match previous {
                        Some(known) => {
                            store.known_pod_withdrawals.insert(key, known);
                        }
                        None => {
                            store.known_pod_withdrawals.remove(&key);
                        }
                    }
                }
                return Err(error);
            }
        }
    }
    Ok(retained)
}

fn store_error_to_index_failure(error: StoreError) -> IndexSearchFailure {
    let kind = match error {
        StoreError::InvalidSignature => IndexSearchFailureKind::Protocol,
        StoreError::AnnouncementExpired
        | StoreError::AnnouncementStale
        | StoreError::AnnouncementWithdrawn
        | StoreError::WithdrawalStale => IndexSearchFailureKind::Protocol,
        StoreError::Validation(_) => IndexSearchFailureKind::Malformed,
        StoreError::NotFound(_)
        | StoreError::Duplicate(_)
        | StoreError::TenantBoundary
        | StoreError::UntrustedPeer => IndexSearchFailureKind::Protocol,
    };
    index_fail(kind, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        announcement_lease_duration, IndexNode, NodeInfo, PackageVersion,
        PodAnnouncementSearchResult, CURRENT_PROTOCOL_VERSION,
    };
    use crate::signing::{create_node_identity, sign_pod_announcement};
    use chrono::TimeZone;
    use uuid::Uuid;

    fn sample_announcement(
        node: &crate::domain::NodeIdentity,
        announced_at: DateTime<Utc>,
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
    fn import_discards_remote_order_and_records_provenance() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let user_id = Uuid::now_v7();
        let mut policy = TrustPolicy::new(user_id, None);
        policy.index_nodes.push(IndexNode {
            label: "primary".into(),
            base_url: "https://index-a.example".into(),
        });
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, now, "rust-systems");
        let client = ScriptedIndexSearchClient::new();
        client.push_response(
            "https://index-a.example",
            PodAnnouncementSearchResponse {
                query: "rust".into(),
                results: vec![PodAnnouncementSearchResult {
                    announcement: announcement.clone(),
                    relevance: 0.01, // remote low score must not control eligibility
                    reasons: vec!["remote reason".into()],
                }],
            },
        );

        let report =
            import_from_configured_indexes(&mut store, &policy, "rust", 10, &client, now).unwrap();
        assert_eq!(report.retained_announcements, 1);
        assert!(report.outcomes[0].ok);
        let known = store
            .known_pod_announcements
            .get(&(announcement.origin_node_id, announcement.pod_slug.clone()))
            .unwrap();
        assert!(known
            .received_from_index_urls
            .contains("https://index-a.example"));

        let captured = client.captured.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].1.query, "rust");
        assert!(index_request_is_public_only(&captured[0].1));
    }

    #[test]
    fn multi_index_provenance_accumulates_and_any_active_keeps_eligible() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let user_id = Uuid::now_v7();
        let mut policy = TrustPolicy::new(user_id, None);
        policy.index_nodes.push(IndexNode {
            label: "a".into(),
            base_url: "https://index-a.example".into(),
        });
        policy.index_nodes.push(IndexNode {
            label: "b".into(),
            base_url: "https://index-b.example".into(),
        });
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, now, "shared");
        let client = ScriptedIndexSearchClient::new();
        client.push_response(
            "https://index-a.example",
            PodAnnouncementSearchResponse {
                query: "shared".into(),
                results: vec![PodAnnouncementSearchResult {
                    announcement: announcement.clone(),
                    relevance: 1.0,
                    reasons: vec![],
                }],
            },
        );
        client.push_response(
            "https://index-b.example",
            PodAnnouncementSearchResponse {
                query: "shared".into(),
                results: vec![PodAnnouncementSearchResult {
                    announcement: announcement.clone(),
                    relevance: 0.5,
                    reasons: vec![],
                }],
            },
        );

        let report =
            import_from_configured_indexes(&mut store, &policy, "shared", 10, &client, now)
                .unwrap();
        assert_eq!(report.retained_announcements, 2);
        let known = store
            .known_pod_announcements
            .get(&(announcement.origin_node_id, announcement.pod_slug.clone()))
            .unwrap()
            .clone();
        assert!(known
            .received_from_index_urls
            .contains("https://index-a.example"));
        assert!(known
            .received_from_index_urls
            .contains("https://index-b.example"));
        assert!(crate::pod_announcement::announcement_delivery_is_active(
            &store,
            &known,
            Some(&policy)
        ));

        // Remove B; still eligible via A.
        policy
            .index_nodes
            .retain(|node| node.base_url != "https://index-b.example");
        assert!(crate::pod_announcement::announcement_delivery_is_active(
            &store,
            &known,
            Some(&policy)
        ));

        // Remove A as well; no remaining Index provenance is active.
        policy.index_nodes.clear();
        assert!(!crate::pod_announcement::announcement_delivery_is_active(
            &store,
            &known,
            Some(&policy)
        ));
        // Audit row retained.
        assert!(store
            .known_pod_announcements
            .contains_key(&(announcement.origin_node_id, announcement.pod_slug)));
    }

    #[test]
    fn empty_query_does_not_contact_indexes() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let user_id = Uuid::now_v7();
        let mut policy = TrustPolicy::new(user_id, None);
        policy.index_nodes.push(IndexNode {
            label: "primary".into(),
            base_url: "https://index-a.example".into(),
        });
        let client = ScriptedIndexSearchClient::new();
        let report =
            import_from_configured_indexes(&mut store, &policy, "  ", 10, &client, now).unwrap();
        assert!(report.outcomes.is_empty());
        assert!(client.captured.lock().unwrap().is_empty());
    }

    #[test]
    fn removing_index_from_policy_excludes_sole_source() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let user_id = Uuid::now_v7();
        let mut policy = TrustPolicy::new(user_id, None);
        policy.index_nodes.push(IndexNode {
            label: "only".into(),
            base_url: "https://index-only.example".into(),
        });
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, now, "sole");
        retain_index_search_results(
            &mut store,
            "https://index-only.example",
            PodAnnouncementSearchResponse {
                query: "sole".into(),
                results: vec![PodAnnouncementSearchResult {
                    announcement: announcement.clone(),
                    relevance: 1.0,
                    reasons: vec![],
                }],
            },
            now,
        )
        .unwrap();
        let known = store
            .known_pod_announcements
            .get(&(announcement.origin_node_id, announcement.pod_slug.clone()))
            .unwrap()
            .clone();
        assert!(crate::pod_announcement::announcement_delivery_is_active(
            &store,
            &known,
            Some(&policy)
        ));
        policy.index_nodes.clear();
        assert!(!crate::pod_announcement::announcement_delivery_is_active(
            &store,
            &known,
            Some(&policy)
        ));
        // Audit row retained.
        assert!(store
            .known_pod_announcements
            .contains_key(&(announcement.origin_node_id, announcement.pod_slug)));
    }

    #[test]
    fn legacy_singular_index_url_deserializes_into_set() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, now, "legacy");
        let wire = serde_json::json!({
            "announcement": announcement,
            "received_from_peer_id": null,
            "received_from_index_url": "https://legacy-index.example",
            "received_from_bootstrap_urls": [],
            "received_at": now,
        });
        let known: KnownPodAnnouncement = serde_json::from_value(wire).unwrap();
        assert!(known
            .received_from_index_urls
            .contains("https://legacy-index.example"));
        assert_eq!(known.received_from_index_urls.len(), 1);
    }

    #[test]
    fn multi_index_fallthrough_on_failure() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let user_id = Uuid::now_v7();
        let mut policy = TrustPolicy::new(user_id, None);
        policy.index_nodes.push(IndexNode {
            label: "down".into(),
            base_url: "https://index-down.example".into(),
        });
        policy.index_nodes.push(IndexNode {
            label: "up".into(),
            base_url: "https://index-up.example".into(),
        });
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, now, "rust");
        let client = ScriptedIndexSearchClient::new();
        client.push_error(
            "https://index-down.example",
            index_fail(IndexSearchFailureKind::Transport, "connection refused"),
        );
        client.push_response(
            "https://index-up.example",
            PodAnnouncementSearchResponse {
                query: "rust".into(),
                results: vec![PodAnnouncementSearchResult {
                    announcement,
                    relevance: 1.0,
                    reasons: vec![],
                }],
            },
        );
        let report =
            import_from_configured_indexes(&mut store, &policy, "rust", 10, &client, now).unwrap();
        assert!(!report.outcomes[0].ok);
        assert!(report.outcomes[1].ok);
        assert_eq!(report.retained_announcements, 1);
    }
}
