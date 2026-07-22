//! Public Index catalog search over admitted valid announcements.
//!
//! Search is topic-relevant only to the explicit caller query. Results expose
//! Origin-signed announcements and retrieval evidence without quality, trust,
//! popularity, or personalized authority fields. Processing requires no User
//! account and retains no product analytics (only short-lived rate timestamps).

use super::types::{
    index_fail, record_search_attempt, search_rate_limit_would_exceed, DEFAULT_INDEX_SEARCH_LIMIT,
    MAX_INDEX_QUERY_BYTES, MAX_INDEX_SEARCH_LIMIT,
};
use crate::domain::{
    IndexSearchFailure, IndexSearchFailureKind, IndexSearchRequest, PodAnnouncementSearchResponse,
    PodAnnouncementSearchResult,
};
use crate::pod_announcement::announcement_is_discovery_eligible;
use crate::store::InMemoryStore;
use chrono::{DateTime, Utc};

/// Minimal Index catalog tokenization for explicit search queries.
///
/// Unlike [`crate::domain::discovery_tokens`], this keeps short intentional tokens
/// (`ai`, `go`, `web`, …) and does not apply a discovery stop list. Splits on
/// non-alphanumeric characters, lowercases, dedupes, and caps at 80 tokens.
#[must_use]
pub(crate) fn index_search_tokens(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in query
        .split(|c: char| !c.is_alphanumeric())
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        let lower = token.to_lowercase();
        if !out.iter().any(|existing| existing == &lower) {
            out.push(lower);
        }
        if out.len() >= 80 {
            break;
        }
    }
    out
}

/// Searches the Index Node's admitted valid announcement catalog.
///
/// # Errors
///
/// Returns a typed [`IndexSearchFailure`] when the Index capability is disabled,
/// the query is oversized or malformed, or the rate limit is exceeded.
pub fn search_index_catalog(
    store: &mut InMemoryStore,
    request: &IndexSearchRequest,
    index_enabled: bool,
    now: DateTime<Utc>,
) -> Result<PodAnnouncementSearchResponse, IndexSearchFailure> {
    if !index_enabled {
        return Err(index_fail(
            IndexSearchFailureKind::IndexDisabled,
            "this node does not enable Index search",
        ));
    }

    if request.query.len() > MAX_INDEX_QUERY_BYTES {
        return Err(index_fail(
            IndexSearchFailureKind::QueryTooLarge,
            format!("query exceeds maximum length of {MAX_INDEX_QUERY_BYTES} bytes"),
        ));
    }

    let limit = match request.limit {
        None => DEFAULT_INDEX_SEARCH_LIMIT,
        Some(0) => {
            return Err(index_fail(
                IndexSearchFailureKind::Malformed,
                "limit must be at least 1",
            ));
        }
        Some(limit) if limit > MAX_INDEX_SEARCH_LIMIT => {
            return Err(index_fail(
                IndexSearchFailureKind::Malformed,
                format!("limit must be at most {MAX_INDEX_SEARCH_LIMIT}"),
            ));
        }
        Some(limit) => limit,
    };

    if search_rate_limit_would_exceed(store, now) {
        return Err(index_fail(
            IndexSearchFailureKind::RateLimited,
            "Index search rate limit exceeded",
        ));
    }

    // Whitespace-only → catalog dump. Non-empty input that yields no tokens after
    // minimal tokenization is Malformed (not a silent empty result set).
    let trimmed = request.query.trim();
    let query = trimmed.to_lowercase();
    let query_tokens = index_search_tokens(&query);
    if !trimmed.is_empty() && query_tokens.is_empty() {
        return Err(index_fail(
            IndexSearchFailureKind::Malformed,
            "query produced no searchable tokens",
        ));
    }
    let mut results = store
        .known_pod_announcements
        .values()
        .filter_map(|known| {
            if !announcement_is_discovery_eligible(store, &known.announcement, now) {
                return None;
            }
            let searchable = format!(
                "{} {} {}",
                known.announcement.pod_slug,
                known.announcement.pod_name,
                known.announcement.subject
            )
            .to_lowercase();
            let matched = query_tokens
                .iter()
                .filter(|token| searchable.contains(token.as_str()))
                .count();
            if !query_tokens.is_empty() && matched == 0 {
                return None;
            }
            let relevance = if query_tokens.is_empty() {
                1.0
            } else {
                let matched = u16::try_from(matched).unwrap_or(u16::MAX);
                let token_count = u16::try_from(query_tokens.len()).unwrap_or(u16::MAX);
                f32::from(matched) / f32::from(token_count)
            };
            Some(PodAnnouncementSearchResult {
                announcement: known.announcement.clone(),
                relevance,
                reasons: vec![if query_tokens.is_empty() {
                    "Public Pod Announcement is available from this Index Node".into()
                } else {
                    "Pod subject matches the explicit Explore query".into()
                }],
            })
        })
        .collect::<Vec<_>>();
    results.sort_by(|left, right| {
        right
            .relevance
            .total_cmp(&left.relevance)
            .then_with(|| left.announcement.pod_slug.cmp(&right.announcement.pod_slug))
    });
    results.truncate(limit);

    // Rate bookkeeping only — no query text or User identity is retained.
    record_search_attempt(store, now);

    Ok(PodAnnouncementSearchResponse { query, results })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        announcement_lease_duration, NodeInfo, PackageVersion, CURRENT_PROTOCOL_VERSION,
    };
    use crate::pod_announcement::{retain_verified_pod_announcement, DeliveryProvenance};
    use crate::signing::{create_node_identity, sign_pod_announcement};
    use chrono::TimeZone;
    use uuid::Uuid;

    fn sample_announcement(
        node: &crate::domain::NodeIdentity,
        announced_at: DateTime<Utc>,
        slug: &str,
        subject: &str,
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
                subject: subject.into(),
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
    fn searches_admitted_catalog_for_explicit_query() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, now, "rust-systems", "Rust ownership");
        retain_verified_pod_announcement(
            &mut store,
            announcement.clone(),
            DeliveryProvenance::LOCAL,
            now,
        )
        .unwrap();

        let response = search_index_catalog(
            &mut store,
            &IndexSearchRequest {
                query: "rust".into(),
                limit: Some(10),
            },
            true,
            now,
        )
        .unwrap();
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].announcement.id, announcement.id);
        let wire = serde_json::to_value(&response).unwrap();
        assert!(wire.get("global_quality_score").is_none());
        assert!(wire.get("authority").is_none());
        assert!(wire.get("popularity").is_none());
        assert!(wire.get("trust").is_none());
    }

    #[test]
    fn empty_query_returns_bounded_catalog_results() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let node = create_node_identity("origin", None);
        retain_verified_pod_announcement(
            &mut store,
            sample_announcement(&node, now, "a", "alpha"),
            DeliveryProvenance::LOCAL,
            now,
        )
        .unwrap();
        let response = search_index_catalog(
            &mut store,
            &IndexSearchRequest {
                query: String::new(),
                limit: Some(10),
            },
            true,
            now,
        )
        .unwrap();
        assert_eq!(response.results.len(), 1);
    }

    #[test]
    fn rejects_disabled_oversized_malformed_and_rate_limited() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();

        let disabled = search_index_catalog(
            &mut store,
            &IndexSearchRequest {
                query: "rust".into(),
                limit: Some(10),
            },
            false,
            now,
        )
        .unwrap_err();
        assert_eq!(disabled.kind, IndexSearchFailureKind::IndexDisabled);

        let oversized = search_index_catalog(
            &mut store,
            &IndexSearchRequest {
                query: "x".repeat(MAX_INDEX_QUERY_BYTES + 1),
                limit: Some(10),
            },
            true,
            now,
        )
        .unwrap_err();
        assert_eq!(oversized.kind, IndexSearchFailureKind::QueryTooLarge);

        let malformed = search_index_catalog(
            &mut store,
            &IndexSearchRequest {
                query: "rust".into(),
                limit: Some(0),
            },
            true,
            now,
        )
        .unwrap_err();
        assert_eq!(malformed.kind, IndexSearchFailureKind::Malformed);

        let no_tokens = search_index_catalog(
            &mut store,
            &IndexSearchRequest {
                query: "...!!!".into(),
                limit: Some(10),
            },
            true,
            now,
        )
        .unwrap_err();
        assert_eq!(no_tokens.kind, IndexSearchFailureKind::Malformed);

        // Exhaust rate limit without retaining queries.
        for _ in 0..super::super::types::MAX_INDEX_SEARCHES_PER_WINDOW {
            search_index_catalog(
                &mut store,
                &IndexSearchRequest {
                    query: "ok".into(),
                    limit: Some(1),
                },
                true,
                now,
            )
            .unwrap();
        }
        let limited = search_index_catalog(
            &mut store,
            &IndexSearchRequest {
                query: "ok".into(),
                limit: Some(1),
            },
            true,
            now,
        )
        .unwrap_err();
        assert_eq!(limited.kind, IndexSearchFailureKind::RateLimited);
    }

    #[test]
    fn short_intentional_tokens_are_searchable() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let node = create_node_identity("origin", None);
        for (slug, subject) in [
            ("ai-notes", "Notes on modern ai tooling"),
            ("go-lang", "Practical go concurrency"),
            ("web-perf", "web performance budgets"),
        ] {
            retain_verified_pod_announcement(
                &mut store,
                sample_announcement(&node, now, slug, subject),
                DeliveryProvenance::LOCAL,
                now,
            )
            .unwrap();
        }

        for (query, expected_slug) in [("ai", "ai-notes"), ("go", "go-lang"), ("web", "web-perf")] {
            let response = search_index_catalog(
                &mut store,
                &IndexSearchRequest {
                    query: query.into(),
                    limit: Some(10),
                },
                true,
                now,
            )
            .unwrap();
            assert_eq!(
                response.results.len(),
                1,
                "query {query:?} should match one announcement"
            );
            assert_eq!(response.results[0].announcement.pod_slug, expected_slug);
        }
    }
}
