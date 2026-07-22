//! Replaceable private Index Node search.
//!
//! An Index-capable node searches its admitted valid announcement catalog for
//! explicit bounded queries without User accounts or retained product analytics.
//! Home Nodes may call configured Indexes only from explicit User-authored
//! Explore actions; remote scores are discarded and relevance is recomputed
//! locally under Trust Policy.
//!
//! # Module layout
//!
//! - [`search`] — Index-side catalog search with capability, bounds, rate limits
//! - [`client`] — Home Node outbound Index client and import with provenance
//! - [`types`] — bounds, rate-limit bookkeeping, request privacy helpers

mod client;
mod search;
mod types;

pub use client::{
    explicit_index_search_request, import_from_configured_indexes, retain_index_search_results,
    IndexSearchClient, ScriptedIndexSearchClient,
};
pub use search::search_index_catalog;
pub use types::{
    ensure_index_runtime, index_fail, index_request_is_public_only, record_search_attempt,
    search_rate_limit_would_exceed, DEFAULT_INDEX_SEARCH_LIMIT, INDEX_SEARCH_RATE_WINDOW,
    MAX_INDEX_QUERY_BYTES, MAX_INDEX_SEARCHES_PER_WINDOW, MAX_INDEX_SEARCH_LIMIT,
};
