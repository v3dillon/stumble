//! Shared cursor-paged Announcement Stream sync machinery.
//!
//! The Bootstrap client and the outbound Discovery Peer client synchronize the
//! same wire artifacts (Origin-signed announcements and withdrawals) over the
//! same paged stream shape; only the transport trait, the typed failure, and
//! the per-endpoint health policy differ. The fetch loop, the atomic page
//! stage, the retain-error classification, and the page-drain skeleton live
//! here once, parameterized by the engine's failure type.

use crate::domain::{AnnouncementStreamPage, BootstrapStreamRequest};
use crate::store::{InMemoryStore, StoreError};

/// Maximum pages fetched from one stream endpoint during a single sync pass.
pub(crate) const MAX_PAGES_PER_ENDPOINT: usize = 32;

/// Default page size requested by outbound stream sync.
pub(crate) const DEFAULT_SYNC_PAGE_LIMIT: usize = 50;

/// Pages fetched from one stream endpoint before any store mutation.
#[derive(Debug, Clone)]
pub struct FetchedStream<E> {
    /// Successfully fetched pages in order (may be empty).
    pub pages: Vec<AnnouncementStreamPage>,
    /// Cursor used for the first page request.
    pub start_cursor: Option<String>,
    /// Transport/protocol failure after the last successful page, if any.
    pub fetch_error: Option<E>,
}

/// Fetches stream pages without touching the store.
///
/// Stops at end-of-stream (repeated or absent next cursor), the page cap, or
/// the first transport failure. Already-fetched pages are returned so partial
/// progress can still be applied.
pub(crate) fn fetch_stream_pages<E>(
    start_cursor: Option<String>,
    mut fetch: impl FnMut(&BootstrapStreamRequest) -> Result<AnnouncementStreamPage, E>,
) -> FetchedStream<E> {
    let mut pages = Vec::new();
    let mut cursor = start_cursor.clone();

    for _ in 0..MAX_PAGES_PER_ENDPOINT {
        let request = BootstrapStreamRequest {
            cursor: cursor.clone(),
            limit: Some(DEFAULT_SYNC_PAGE_LIMIT),
        };
        let page = match fetch(&request) {
            Ok(page) => page,
            Err(error) => {
                return FetchedStream {
                    pages,
                    start_cursor,
                    fetch_error: Some(error),
                };
            }
        };

        let next_cursor = page.next_cursor.clone();
        pages.push(page);
        match next_cursor {
            Some(next) if next != cursor.clone().unwrap_or_default() => {
                cursor = Some(next);
            }
            _ => break,
        }
    }

    FetchedStream {
        pages,
        start_cursor,
        fetch_error: None,
    }
}

/// Retain failure classification shared by both engines' failure types.
pub(crate) enum RetainFailure {
    InvalidSignature(String),
    Validation(String),
    Protocol(String),
}

/// Maps a retain error to soft-skip (stale/expired/withdrawn artifacts) or a
/// classified hard failure.
pub(crate) fn map_retain_error(error: StoreError, subject: &str) -> Result<(), RetainFailure> {
    match error {
        StoreError::AnnouncementStale
        | StoreError::AnnouncementExpired
        | StoreError::AnnouncementWithdrawn
        | StoreError::WithdrawalStale => Ok(()),
        StoreError::InvalidSignature => Err(RetainFailure::InvalidSignature(format!(
            "{subject} signature verification failed"
        ))),
        StoreError::Validation(message) => Err(RetainFailure::Validation(message)),
        error => Err(RetainFailure::Protocol(error.to_string())),
    }
}

/// Applies one page atomically: a hard failure restores pre-page state.
///
/// Rollback snapshots exactly the announcement and withdrawal maps because the
/// retain functions (`retain_verified_pod_announcement` / `_withdrawal`)
/// mutate only those two collections. If a retain path ever grows a third
/// mutation, this staging must snapshot it too — page atomicity silently
/// breaks otherwise.
pub(crate) fn apply_page_staged<T, E>(
    store: &mut InMemoryStore,
    apply: impl FnOnce(&mut InMemoryStore) -> Result<T, E>,
) -> Result<T, E> {
    let before_announcements = store.known_pod_announcements.clone();
    let before_withdrawals = store.known_pod_withdrawals.clone();
    match apply(store) {
        Ok(value) => Ok(value),
        Err(error) => {
            store.known_pod_announcements = before_announcements;
            store.known_pod_withdrawals = before_withdrawals;
            Err(error)
        }
    }
}

/// Result of draining fetched pages into the store.
pub(crate) struct StreamDrain<E> {
    pub pages_applied: usize,
    pub retained_announcements: usize,
    pub retained_withdrawals: usize,
    pub cursor: Option<String>,
    pub failure: Option<E>,
}

/// Applies fetched pages in order, advancing the cursor per applied page.
///
/// A repeated (or absent) next cursor means end-of-stream; a page-apply
/// failure stops the drain with the cursor still pointing at the failed page.
/// The transport `fetch_error` surfaces only when every fetched page applied
/// and the stream did not cleanly end.
pub(crate) fn drain_stream_pages<E>(
    store: &mut InMemoryStore,
    fetched: FetchedStream<E>,
    mut record_cursor: impl FnMut(&Option<String>),
    mut apply_page: impl FnMut(&mut InMemoryStore, &AnnouncementStreamPage) -> Result<(usize, usize), E>,
) -> StreamDrain<E> {
    let mut cursor = fetched.start_cursor;
    let mut pages_applied = 0usize;
    let mut retained_announcements = 0usize;
    let mut retained_withdrawals = 0usize;
    let mut failure = None;
    let mut end_of_stream = false;

    for page in &fetched.pages {
        match apply_page(store, page) {
            Ok((announcements, withdrawals)) => {
                retained_announcements = retained_announcements.saturating_add(announcements);
                retained_withdrawals = retained_withdrawals.saturating_add(withdrawals);
                pages_applied = pages_applied.saturating_add(1);
                match &page.next_cursor {
                    Some(next) if next != &cursor.clone().unwrap_or_default() => {
                        cursor = Some(next.clone());
                    }
                    _ => {
                        cursor = page.next_cursor.clone().or(cursor);
                        end_of_stream = true;
                    }
                }
                record_cursor(&cursor);
                if end_of_stream {
                    break;
                }
            }
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    if failure.is_none() && !end_of_stream {
        failure = fetched.fetch_error;
    }

    StreamDrain {
        pages_applied,
        retained_announcements,
        retained_withdrawals,
        cursor,
        failure,
    }
}
