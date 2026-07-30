//! Local full-text search over everything saved on this node.
//!
//! The index is SQLite FTS5 with BM25 ranking, stored alongside the
//! authoritative records and treated strictly as derived data: a search
//! compares the store generation with the last indexed generation and
//! rebuilds the whole index in one transaction only when they differ. The
//! write path never touches the index, so it can never drift or conflict
//! with the record diff in `persist_sqlite_store_changes`.
//!
//! A Home Node has one human Owner, so the index spans the node: it covers
//! every Submission of the actor's tenant, including Private Notes from any
//! local User. Readable Snapshot text is read from the media directory at
//! index-build time and never leaves the node (ADR-0052).

use super::prelude::*;
use super::{harness_for_context, AgentTools, AgentToolsError};
use crate::store::{apply_sqlite_schema, open_sqlite_store};
use serde::{Deserialize, Serialize};

/// Metadata key holding the store generation the index was last built from.
const SEARCH_INDEXED_GENERATION_KEY: &str = "search_indexed_generation";
/// Readable Snapshots are indexed up to this many bytes; the rest of an
/// unusually large page is archived but not searchable.
const MAX_INDEXED_SNAPSHOT_BYTES: usize = 2 * 1024 * 1024;
/// Snippets are truncated to this many tokens by FTS5.
const SNIPPET_TOKENS: u32 = 12;
pub const DEFAULT_SEARCH_LIMIT: usize = 10;
pub const MAX_SEARCH_LIMIT: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    /// Free-text query; terms are combined with implicit AND.
    pub query: String,
    /// Maximum number of hits, 1 to [`MAX_SEARCH_LIMIT`].
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub content_item_id: ContentItemId,
    pub title: String,
    pub url: String,
    pub domain: String,
    /// Slugs of the local Pods holding this item.
    pub pods: Vec<String>,
    /// Best matching passage with matched terms wrapped in brackets.
    pub snippet: String,
    /// BM25 relevance; higher is better, comparable only within one search.
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub query: String,
    pub hits: Vec<SearchHit>,
}

impl AgentTools {
    /// Searches everything saved on this node with BM25 ranking.
    ///
    /// # Errors
    ///
    /// Returns an error when the query is empty, the limit is out of range,
    /// the store lock is poisoned, or the index cannot be read or rebuilt.
    pub fn search_saved(
        &self,
        ctx: &AuthContext,
        request: SearchRequest,
    ) -> Result<SearchResults, AgentToolsError> {
        let limit = request.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        if !(1..=MAX_SEARCH_LIMIT).contains(&limit) {
            return Err(StoreError::Validation(format!(
                "limit must be between 1 and {MAX_SEARCH_LIMIT}"
            ))
            .into());
        }
        let Some(match_expression) = fts_match_expression(&request.query) else {
            return Err(StoreError::Validation("query must not be empty".to_string()).into());
        };
        self.refresh_if_stale()?;
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        harness_for_context(&store, ctx)?;
        let mut connection = match self.persistence_path() {
            Some(path) => open_sqlite_store(path)?,
            None => ephemeral_store_database().map_err(sqlite_error)?,
        };
        refresh_index(&mut connection, &store).map_err(sqlite_error)?;
        let hits = query_index(&mut connection, &store, ctx, &match_expression, limit)
            .map_err(sqlite_error)?;
        Ok(SearchResults {
            query: request.query,
            hits,
        })
    }
}

fn sqlite_error(error: rusqlite::Error) -> AgentToolsError {
    AgentToolsError::Persistence(StorePersistenceError::Sqlite(error))
}

/// Turns free text into an FTS5 MATCH expression by quoting every term, so
/// user input can never inject FTS5 query syntax. Returns `None` when the
/// query holds no terms.
fn fts_match_expression(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

/// Rebuilds the index when the store generation has moved since the last
/// build. The cheap read-only check runs first so searches of a fresh index
/// never contend on the database write lock; the stale path re-checks inside
/// an immediate transaction, so racing rebuilders serialize instead of
/// double-building.
fn refresh_index(
    connection: &mut rusqlite::Connection,
    store: &InMemoryStore,
) -> Result<(), rusqlite::Error> {
    if index_is_current(connection)? {
        return Ok(());
    }
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if index_is_current(&transaction)? {
        return Ok(());
    }
    let generation = read_metadata(&transaction, "generation")?.unwrap_or(0);
    rebuild_index(&transaction, store)?;
    transaction.execute(
        "INSERT INTO stumble_store_metadata (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        rusqlite::params![SEARCH_INDEXED_GENERATION_KEY, generation.to_string()],
    )?;
    transaction.commit()
}

/// A throwaway fully-schemed in-memory database for nodes without SQLite
/// persistence, so search runs the identical refresh-and-query path.
fn ephemeral_store_database() -> Result<rusqlite::Connection, rusqlite::Error> {
    let connection = rusqlite::Connection::open_in_memory()?;
    apply_sqlite_schema(&connection)?;
    Ok(connection)
}

fn index_is_current(connection: &rusqlite::Connection) -> Result<bool, rusqlite::Error> {
    let generation = read_metadata(connection, "generation")?.unwrap_or(0);
    Ok(read_metadata(connection, SEARCH_INDEXED_GENERATION_KEY)? == Some(generation))
}

fn read_metadata(
    connection: &rusqlite::Connection,
    key: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    use rusqlite::OptionalExtension;
    connection
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM stumble_store_metadata WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()
}

fn rebuild_index(
    transaction: &rusqlite::Transaction<'_>,
    store: &InMemoryStore,
) -> Result<(), rusqlite::Error> {
    transaction.execute("DELETE FROM stumble_search_index", [])?;
    let mut insert = transaction.prepare(
        "INSERT INTO stumble_search_index
           (submission_id, tenant_id, title, url, domain, description, summary,
            tags, notes, snapshot)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    let mut submissions: Vec<&Submission> = store.submissions.values().collect();
    submissions.sort_by_key(|submission| submission.id);
    for submission in submissions {
        insert.execute(rusqlite::params![
            submission.id.to_string(),
            tenant_column(submission.tenant_id),
            submission.title,
            // Both spellings of the source, so a query matching either the
            // shared URL or its canonical form finds the item.
            format!("{} {}", submission.url, submission.canonical_url),
            submission.domain,
            submission.description.as_deref().unwrap_or_default(),
            submission.summary.as_deref().unwrap_or_default(),
            submission.tags.join(" "),
            submission_notes(store, submission),
            snapshot_text(store, submission.id),
        ])?;
    }
    Ok(())
}

fn tenant_column(tenant_id: Option<TenantId>) -> String {
    tenant_id.map(|id| id.to_string()).unwrap_or_default()
}

/// Joins the submitter's note with every local User's Private Note on the
/// item. Notes stay node-local either way, and a Home Node has one Owner.
fn submission_notes(store: &InMemoryStore, submission: &Submission) -> String {
    let private_notes = store
        .private_notes
        .iter()
        .filter(|((_, submission_id), _)| *submission_id == submission.id)
        .map(|(_, body)| body.as_str());
    submission
        .submitter_note
        .as_deref()
        .into_iter()
        .chain(private_notes)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Reads the newest archived Readable Snapshot's text, capped at
/// [`MAX_INDEXED_SNAPSHOT_BYTES`]. A missing or unreadable file indexes as
/// empty text rather than failing the search.
fn snapshot_text(store: &InMemoryStore, submission_id: SubmissionId) -> String {
    let newest = store
        .submission_assets
        .values()
        .filter(|asset| {
            asset.submission_id == submission_id
                && asset.asset_type == SubmissionAssetType::ReadableSnapshot
        })
        .max_by_key(|asset| asset.created_at);
    let Some(path) = newest.and_then(|asset| asset.local_path.as_deref()) else {
        return String::new();
    };
    let Ok(mut bytes) = std::fs::read(path) else {
        return String::new();
    };
    bytes.truncate(MAX_INDEXED_SNAPSHOT_BYTES);
    String::from_utf8_lossy(&bytes).into_owned()
}

fn query_index(
    connection: &mut rusqlite::Connection,
    store: &InMemoryStore,
    ctx: &AuthContext,
    match_expression: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, rusqlite::Error> {
    // One weight per table column in creation order; unindexed columns get 0.
    let mut statement = connection.prepare(&format!(
        "SELECT submission_id, title, domain,
                snippet(stumble_search_index, -1, '[', ']', ' … ', {SNIPPET_TOKENS}),
                bm25(stumble_search_index, 0.0, 0.0, 8.0, 2.0, 3.0, 3.0, 5.0, 6.0, 4.0, 1.0)
         FROM stumble_search_index
         WHERE stumble_search_index MATCH ?1 AND tenant_id = ?2
         ORDER BY 5
         LIMIT ?3"
    ))?;
    let rows = statement.query_map(
        rusqlite::params![match_expression, tenant_column(ctx.tenant_id), limit as i64],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
            ))
        },
    )?;
    let mut hits = Vec::new();
    for row in rows {
        let (submission_id, title, domain, snippet, rank) = row?;
        let Ok(submission_id) = submission_id.parse::<SubmissionId>() else {
            continue;
        };
        // The index is derived; a row can outlive its record within one
        // generation only in ephemeral test stores, so a miss just skips.
        let Some(submission) = store.submissions.get(&submission_id) else {
            continue;
        };
        hits.push(SearchHit {
            content_item_id: ContentItemId::from(submission_id),
            title,
            url: submission.url.clone(),
            domain,
            pods: submission_pod_slugs(store, submission_id),
            snippet,
            score: -rank,
        });
    }
    Ok(hits)
}

fn submission_pod_slugs(store: &InMemoryStore, submission_id: SubmissionId) -> Vec<String> {
    let mut slugs: Vec<String> = store
        .submission_pods
        .iter()
        .filter(|link| link.submission_id == submission_id)
        .filter_map(|link| store.pods.get(&link.pod_id))
        .map(|pod| pod.slug.clone())
        .collect();
    slugs.sort();
    slugs.dedup();
    slugs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seeds::empty_home_node_store;

    fn tools_with_added(urls_and_titles: &[(&str, &str, &str)]) -> (AgentTools, AuthContext) {
        let tools = AgentTools::new(empty_home_node_store());
        let ctx = tools.local_owner_auth_context().unwrap();
        for (url, title, summary) in urls_and_titles {
            tools
                .add_reference(
                    &ctx,
                    AddReferenceRequest {
                        url: (*url).to_string(),
                        pod: None,
                        title: Some((*title).to_string()),
                        summary: Some((*summary).to_string()),
                        excerpt: None,
                        tags: vec!["reading".to_string()],
                        note: None,
                        images: Vec::new(),
                    },
                    Utc::now(),
                )
                .unwrap();
        }
        (tools, ctx)
    }

    #[test]
    fn quotes_every_term_so_fts_syntax_cannot_inject() {
        assert_eq!(
            fts_match_expression("attention economics"),
            Some("\"attention\" \"economics\"".to_string())
        );
        assert_eq!(
            fts_match_expression("  a AND (b OR \"c\")  "),
            Some("\"a\" \"AND\" \"(b\" \"OR\" \"\"\"c\"\")\"".to_string())
        );
        assert_eq!(fts_match_expression("   "), None);
    }

    #[test]
    fn searches_titles_and_summaries_in_memory() {
        let (tools, ctx) = tools_with_added(&[
            (
                "https://example.com/attention",
                "The attention economy",
                "How platforms monetize focus",
            ),
            (
                "https://example.com/rust",
                "Fearless concurrency",
                "Ownership makes data races impossible",
            ),
        ]);
        let results = tools
            .search_saved(
                &ctx,
                SearchRequest {
                    query: "attention".to_string(),
                    limit: None,
                },
            )
            .unwrap();
        assert_eq!(results.hits.len(), 1);
        let hit = &results.hits[0];
        assert_eq!(hit.title, "The attention economy");
        assert_eq!(hit.pods, vec!["saved".to_string()]);
        assert!(hit.score > 0.0);
        assert!(hit.snippet.contains("[attention]"), "{}", hit.snippet);
    }

    #[test]
    fn rejects_empty_queries_and_out_of_range_limits() {
        let (tools, ctx) = tools_with_added(&[]);
        for request in [
            SearchRequest {
                query: "  ".to_string(),
                limit: None,
            },
            SearchRequest {
                query: "fine".to_string(),
                limit: Some(0),
            },
            SearchRequest {
                query: "fine".to_string(),
                limit: Some(MAX_SEARCH_LIMIT + 1),
            },
        ] {
            assert!(matches!(
                tools.search_saved(&ctx, request),
                Err(AgentToolsError::Store(StoreError::Validation(_)))
            ));
        }
    }

    #[test]
    fn hostile_query_syntax_is_matched_literally_not_executed() {
        let (tools, ctx) =
            tools_with_added(&[("https://example.com/one", "Plain title", "Plain summary")]);
        let results = tools
            .search_saved(
                &ctx,
                SearchRequest {
                    query: "title NOT missing\" OR (".to_string(),
                    limit: None,
                },
            )
            .unwrap();
        assert!(results.hits.is_empty());
    }
}
