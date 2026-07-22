//! Shared Index search constants, rate-limit bookkeeping, and request privacy.

use crate::domain::{IndexRuntimeState, IndexSearchFailure, IndexSearchFailureKind};
use crate::store::InMemoryStore;
use chrono::{DateTime, Duration, Utc};

/// Maximum UTF-8 byte length accepted for an explicit Index search query.
pub const MAX_INDEX_QUERY_BYTES: usize = 256;

/// Default result limit for Index search when the caller omits one.
pub const DEFAULT_INDEX_SEARCH_LIMIT: usize = 10;

/// Hard upper bound on Index search result pages.
pub const MAX_INDEX_SEARCH_LIMIT: usize = 50;

/// Sliding window used for public Index search rate limits.
pub const INDEX_SEARCH_RATE_WINDOW: Duration = Duration::hours(1);

/// Maximum accepted Index searches across all callers in the rate window.
///
/// Index search is anonymous (no User account); this bounds shared resource use.
pub const MAX_INDEX_SEARCHES_PER_WINDOW: usize = 1_024;

/// Ensures Index runtime bookkeeping exists in the store.
pub fn ensure_index_runtime(store: &mut InMemoryStore) -> &mut IndexRuntimeState {
    store
        .index_runtime
        .get_or_insert_with(IndexRuntimeState::default)
}

/// Returns whether accepting one more search would exceed the network rate limit.
#[must_use]
pub fn search_rate_limit_would_exceed(store: &InMemoryStore, now: DateTime<Utc>) -> bool {
    let Some(runtime) = store.index_runtime.as_ref() else {
        return false;
    };
    let cutoff = now - INDEX_SEARCH_RATE_WINDOW;
    let recent = runtime
        .recent_search_attempts
        .iter()
        .filter(|attempt| **attempt > cutoff)
        .count();
    recent >= MAX_INDEX_SEARCHES_PER_WINDOW
}

/// Records a successful search attempt for rate-limit bookkeeping.
///
/// Stores only the timestamp—never the query string or any User identifier.
pub fn record_search_attempt(store: &mut InMemoryStore, now: DateTime<Utc>) {
    let runtime = ensure_index_runtime(store);
    let cutoff = now - INDEX_SEARCH_RATE_WINDOW;
    runtime
        .recent_search_attempts
        .retain(|attempt| *attempt > cutoff);
    runtime.recent_search_attempts.push(now);
}

/// Asserts an [`IndexSearchRequest`]-shaped payload carries only query + limit.
///
/// Used by tests and debug assertions so Home Nodes never attach private
/// evidence to outbound Index requests.
#[must_use]
pub fn index_request_is_public_only(request: &crate::domain::IndexSearchRequest) -> bool {
    // Structural guarantee: the type only has query + limit fields.
    // Serialize and ensure no extra keys appear on the wire.
    let Ok(value) = serde_json::to_value(request) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    object.keys().all(|key| key == "query" || key == "limit")
}

/// Builds a typed Index search failure.
#[must_use]
pub fn index_fail(kind: IndexSearchFailureKind, message: impl Into<String>) -> IndexSearchFailure {
    IndexSearchFailure::new(kind, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::IndexSearchRequest;

    #[test]
    fn request_serializes_only_query_and_limit() {
        let request = IndexSearchRequest {
            query: "rust systems".into(),
            limit: Some(10),
        };
        assert!(index_request_is_public_only(&request));
        let value = serde_json::to_value(&request).unwrap();
        assert!(value.get("user_id").is_none());
        assert!(value.get("taste_profile").is_none());
        assert!(value.get("subscriptions").is_none());
    }

    #[test]
    fn rate_limit_counts_only_timestamps() {
        let now = Utc::now();
        let mut store = InMemoryStore::default();
        for _ in 0..MAX_INDEX_SEARCHES_PER_WINDOW {
            assert!(!search_rate_limit_would_exceed(&store, now));
            record_search_attempt(&mut store, now);
        }
        assert!(search_rate_limit_would_exceed(&store, now));
        let runtime = store.index_runtime.as_ref().unwrap();
        assert_eq!(
            runtime.recent_search_attempts.len(),
            MAX_INDEX_SEARCHES_PER_WINDOW
        );
        // No query text is retained in runtime state.
        let serialized = serde_json::to_string(runtime).unwrap();
        assert!(!serialized.contains("rust"));
        assert!(!serialized.contains("query"));
    }
}
