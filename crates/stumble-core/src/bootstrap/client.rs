//! Outbound Home Node Bootstrap configuration and Announcement Stream sync.
//!
//! A fresh Home Node receives the sponsored Bootstrap endpoint as an ordinary
//! removable default. Synchronization is outbound-only, topic-neutral, and never
//! attaches private discovery evidence to remote requests.
//!
//! Network I/O is separated from store mutation: pages are fetched into memory,
//! then applied under a short write critical section so callers can avoid
//! holding store locks across HTTP.

use crate::domain::{
    AnnouncementStreamEventKind, AnnouncementStreamPage, BootstrapEndpointConfig,
    BootstrapEndpointId, BootstrapEndpointStatus, BootstrapStreamRequest,
    BootstrapSyncEndpointOutcome, BootstrapSyncFailure, BootstrapSyncFailureKind,
    BootstrapSyncReport, BootstrapSyncState, DEFAULT_SPONSORED_BOOTSTRAP_URL,
};
use crate::pod_announcement::{
    retain_verified_pod_announcement, retain_verified_pod_withdrawal, DeliveryProvenance,
};
use crate::store::{InMemoryStore, StoreError};
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Maximum pages fetched from one Bootstrap endpoint during a single sync pass.
const MAX_PAGES_PER_ENDPOINT: usize = 32;

/// Default page size requested by outbound Bootstrap stream sync.
const DEFAULT_SYNC_PAGE_LIMIT: usize = 50;

/// Transport port for fetching topic-neutral Announcement Stream pages.
///
/// Production implementations perform HTTP GET against
/// `{base_url}/bootstrap/announcements/stream`. Tests inject scripted pages.
/// Requests must carry only [`BootstrapStreamRequest`] fields.
pub trait AnnouncementStreamClient: Send + Sync {
    /// Fetches one stream page from `base_url` using the given request.
    ///
    /// # Errors
    ///
    /// Returns a typed [`BootstrapSyncFailure`] for transport or protocol errors.
    fn fetch_announcement_stream(
        &self,
        base_url: &str,
        request: &BootstrapStreamRequest,
    ) -> Result<AnnouncementStreamPage, BootstrapSyncFailure>;
}

/// Seeds the sponsored Bootstrap endpoint when the Home Node has none configured.
///
/// The sponsored URL is ordinary removable config, not a protocol constant.
pub fn ensure_default_bootstrap_endpoint(store: &mut InMemoryStore, now: DateTime<Utc>) {
    if !store.bootstrap_endpoints.is_empty() {
        return;
    }
    let id = Uuid::now_v7();
    store.bootstrap_endpoints.insert(
        id,
        BootstrapEndpointConfig {
            id,
            label: "Sponsored Bootstrap".into(),
            base_url: DEFAULT_SPONSORED_BOOTSTRAP_URL.to_string(),
            enabled: true,
            order: 0,
            is_sponsored_default: true,
            created_at: now,
        },
    );
    store.bootstrap_sync_states.insert(
        id,
        BootstrapSyncState {
            endpoint_id: id,
            cursor: None,
            last_success_at: None,
            last_attempt_at: None,
            last_error: None,
        },
    );
}

/// Returns configured Bootstrap endpoints in ascending order.
#[must_use]
pub fn list_bootstrap_endpoints(store: &InMemoryStore) -> Vec<BootstrapEndpointConfig> {
    let mut endpoints: Vec<_> = store.bootstrap_endpoints.values().cloned().collect();
    endpoints.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.created_at.cmp(&right.created_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    endpoints
}

/// Returns endpoint config joined with per-endpoint sync progress.
#[must_use]
pub fn bootstrap_endpoint_statuses(store: &InMemoryStore) -> Vec<BootstrapEndpointStatus> {
    list_bootstrap_endpoints(store)
        .into_iter()
        .map(|endpoint| {
            let sync = store
                .bootstrap_sync_states
                .get(&endpoint.id)
                .cloned()
                .unwrap_or_else(|| empty_sync_state(endpoint.id));
            BootstrapEndpointStatus { endpoint, sync }
        })
        .collect()
}

/// Adds a Bootstrap endpoint to the ordered User-controlled list.
///
/// # Errors
///
/// Returns validation errors for empty labels or invalid base URLs, and
/// duplicate when the normalized base URL is already configured.
pub fn add_bootstrap_endpoint(
    store: &mut InMemoryStore,
    label: &str,
    base_url: &str,
    now: DateTime<Utc>,
) -> Result<BootstrapEndpointConfig, StoreError> {
    let label = label.trim();
    if label.is_empty() {
        return Err(StoreError::Validation(
            "Bootstrap endpoint label must not be empty".into(),
        ));
    }
    let base_url = normalize_bootstrap_base_url(base_url)?;
    if store
        .bootstrap_endpoints
        .values()
        .any(|endpoint| endpoint.base_url == base_url)
    {
        return Err(StoreError::Duplicate(format!(
            "Bootstrap endpoint {base_url}"
        )));
    }
    let order = store
        .bootstrap_endpoints
        .values()
        .map(|endpoint| endpoint.order)
        .max()
        .map_or(0, |max| max.saturating_add(1));
    let id = Uuid::now_v7();
    let endpoint = BootstrapEndpointConfig {
        id,
        label: label.to_string(),
        base_url,
        enabled: true,
        order,
        is_sponsored_default: false,
        created_at: now,
    };
    store.bootstrap_endpoints.insert(id, endpoint.clone());
    store.bootstrap_sync_states.insert(id, empty_sync_state(id));
    Ok(endpoint)
}

/// Enables or disables a configured Bootstrap endpoint without deleting audit state.
///
/// Disabling stops sync and eligibility provenance from this endpoint while
/// preserving the config row, cursor, and independently learned announcements.
///
/// # Errors
///
/// Returns not-found when the endpoint id is unknown.
pub fn set_bootstrap_endpoint_enabled(
    store: &mut InMemoryStore,
    endpoint_id: BootstrapEndpointId,
    enabled: bool,
) -> Result<BootstrapEndpointConfig, StoreError> {
    let endpoint = store
        .bootstrap_endpoints
        .get_mut(&endpoint_id)
        .ok_or_else(|| StoreError::NotFound(format!("Bootstrap endpoint {endpoint_id}")))?;
    endpoint.enabled = enabled;
    Ok(endpoint.clone())
}

/// Removes a Bootstrap endpoint from configuration.
///
/// Sync state for the endpoint is dropped. Announcements remain in the local
/// audit store; those whose only delivery source was this endpoint become
/// ineligible for new discovery while independently learned copies stay usable.
///
/// # Errors
///
/// Returns not-found when the endpoint id is unknown.
pub fn remove_bootstrap_endpoint(
    store: &mut InMemoryStore,
    endpoint_id: BootstrapEndpointId,
) -> Result<BootstrapEndpointConfig, StoreError> {
    let endpoint = store
        .bootstrap_endpoints
        .remove(&endpoint_id)
        .ok_or_else(|| StoreError::NotFound(format!("Bootstrap endpoint {endpoint_id}")))?;
    store.bootstrap_sync_states.remove(&endpoint_id);
    Ok(endpoint)
}

/// Snapshot of one enabled endpoint for a lock-free fetch plan.
#[derive(Debug, Clone)]
pub struct BootstrapEndpointSyncPlan {
    /// Configured endpoint.
    pub endpoint: BootstrapEndpointConfig,
    /// Cursor to resume from, when previously persisted.
    pub cursor: Option<String>,
}

/// Pages fetched from one endpoint before any store mutation.
#[derive(Debug, Clone)]
pub struct FetchedBootstrapStream {
    /// Successfully fetched pages in order (may be empty).
    pub pages: Vec<AnnouncementStreamPage>,
    /// Cursor used for the first page request.
    pub start_cursor: Option<String>,
    /// Transport/protocol failure after the last successful page, if any.
    pub fetch_error: Option<BootstrapSyncFailure>,
}

/// Builds the ordered plan of enabled endpoints and their cursors (read-only).
#[must_use]
pub fn plan_bootstrap_sync(store: &InMemoryStore) -> Vec<BootstrapEndpointSyncPlan> {
    list_bootstrap_endpoints(store)
        .into_iter()
        .filter(|endpoint| endpoint.enabled)
        .map(|endpoint| {
            let cursor = store
                .bootstrap_sync_states
                .get(&endpoint.id)
                .and_then(|state| state.cursor.clone());
            BootstrapEndpointSyncPlan { endpoint, cursor }
        })
        .collect()
}

/// Fetches stream pages for one endpoint without touching the store.
///
/// Stops at end-of-stream, the per-endpoint page cap, or the first transport
/// failure. Already-fetched pages are returned for apply so partial progress
/// can still be retained.
#[must_use]
pub fn fetch_bootstrap_stream_pages(
    client: &dyn AnnouncementStreamClient,
    base_url: &str,
    start_cursor: Option<String>,
) -> FetchedBootstrapStream {
    let mut pages = Vec::new();
    let mut cursor = start_cursor.clone();

    for _ in 0..MAX_PAGES_PER_ENDPOINT {
        let request = BootstrapStreamRequest {
            cursor: cursor.clone(),
            limit: Some(DEFAULT_SYNC_PAGE_LIMIT),
        };
        // Privacy invariant: request serializes only cursor + limit.
        debug_assert!(request_is_public_only(&request));

        let page = match client.fetch_announcement_stream(base_url, &request) {
            Ok(page) => page,
            Err(error) => {
                return FetchedBootstrapStream {
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
            _ => {
                return FetchedBootstrapStream {
                    pages,
                    start_cursor,
                    fetch_error: None,
                };
            }
        }
    }

    FetchedBootstrapStream {
        pages,
        start_cursor,
        fetch_error: None,
    }
}

/// Applies previously fetched stream pages and updates cursor / error state.
///
/// Hard failures roll back the current page (no partial page retain) and do not
/// advance the cursor past that page. Soft skip entries (stale/expired/withdrawn)
/// do not fail the page.
pub fn apply_bootstrap_stream_pages(
    store: &mut InMemoryStore,
    endpoint: &BootstrapEndpointConfig,
    fetched: FetchedBootstrapStream,
    now: DateTime<Utc>,
) -> BootstrapSyncEndpointOutcome {
    let mut state = store
        .bootstrap_sync_states
        .get(&endpoint.id)
        .cloned()
        .unwrap_or_else(|| empty_sync_state(endpoint.id));
    state.last_attempt_at = Some(now);

    let mut cursor = fetched.start_cursor;
    let mut pages_fetched = 0usize;
    let mut retained_announcements = 0usize;
    let mut retained_withdrawals = 0usize;

    for page in &fetched.pages {
        match apply_stream_page_staged(store, &endpoint.base_url, page, now) {
            Ok((announcements, withdrawals)) => {
                retained_announcements = retained_announcements.saturating_add(announcements);
                retained_withdrawals = retained_withdrawals.saturating_add(withdrawals);
                pages_fetched = pages_fetched.saturating_add(1);
                match &page.next_cursor {
                    Some(next) if next != &cursor.clone().unwrap_or_default() => {
                        cursor = Some(next.clone());
                        state.cursor = cursor.clone();
                    }
                    _ => {
                        state.cursor = cursor.clone();
                        state.last_success_at = Some(now);
                        state.last_error = None;
                        store.bootstrap_sync_states.insert(endpoint.id, state);
                        return endpoint_outcome(
                            endpoint,
                            true,
                            pages_fetched,
                            retained_announcements,
                            retained_withdrawals,
                            cursor,
                            None,
                        );
                    }
                }
            }
            Err(error) => {
                // Cursor stays at the request cursor for this page; partial page not applied.
                state.last_error = Some(error.clone());
                store.bootstrap_sync_states.insert(endpoint.id, state);
                return endpoint_outcome(
                    endpoint,
                    false,
                    pages_fetched,
                    retained_announcements,
                    retained_withdrawals,
                    cursor,
                    Some(error),
                );
            }
        }
    }

    if let Some(error) = fetched.fetch_error {
        state.cursor = cursor.clone();
        state.last_error = Some(error.clone());
        store.bootstrap_sync_states.insert(endpoint.id, state);
        return endpoint_outcome(
            endpoint,
            false,
            pages_fetched,
            retained_announcements,
            retained_withdrawals,
            cursor,
            Some(error),
        );
    }

    // Hit page cap with more data available, or zero-page success with no error.
    state.cursor = cursor.clone();
    state.last_success_at = Some(now);
    state.last_error = None;
    store.bootstrap_sync_states.insert(endpoint.id, state);
    endpoint_outcome(
        endpoint,
        true,
        pages_fetched,
        retained_announcements,
        retained_withdrawals,
        cursor,
        None,
    )
}

/// Synchronizes Announcement Streams from each enabled Bootstrap in order.
///
/// Transport or protocol failure on one endpoint records a typed error and
/// falls through to the next without discarding previously verified announcements.
///
/// Fetches pages before mutating the store so callers that wrap this function
/// can instead call [`fetch_bootstrap_stream_pages`] outside a lock and
/// [`apply_bootstrap_stream_pages`] under a short write section.
pub fn sync_bootstrap_endpoints(
    store: &mut InMemoryStore,
    client: &dyn AnnouncementStreamClient,
    now: DateTime<Utc>,
) -> BootstrapSyncReport {
    let plans = plan_bootstrap_sync(store);
    let mut outcomes = Vec::with_capacity(plans.len());
    let mut retained_announcements = 0usize;
    let mut retained_withdrawals = 0usize;

    for plan in plans {
        let fetched = fetch_bootstrap_stream_pages(client, &plan.endpoint.base_url, plan.cursor);
        let outcome = apply_bootstrap_stream_pages(store, &plan.endpoint, fetched, now);
        retained_announcements =
            retained_announcements.saturating_add(outcome.retained_announcements);
        retained_withdrawals = retained_withdrawals.saturating_add(outcome.retained_withdrawals);
        outcomes.push(outcome);
    }

    BootstrapSyncReport {
        outcomes,
        retained_announcements,
        retained_withdrawals,
    }
}

fn endpoint_outcome(
    endpoint: &BootstrapEndpointConfig,
    ok: bool,
    pages_fetched: usize,
    retained_announcements: usize,
    retained_withdrawals: usize,
    cursor: Option<String>,
    error: Option<BootstrapSyncFailure>,
) -> BootstrapSyncEndpointOutcome {
    BootstrapSyncEndpointOutcome {
        endpoint_id: endpoint.id,
        base_url: endpoint.base_url.clone(),
        ok,
        pages_fetched,
        retained_announcements,
        retained_withdrawals,
        cursor,
        error,
    }
}

/// Maps store retain errors into skip-vs-hard-failure control flow.
fn map_retain_error(error: StoreError, subject: &str) -> Result<(), BootstrapSyncFailure> {
    match error {
        StoreError::AnnouncementStale
        | StoreError::AnnouncementExpired
        | StoreError::AnnouncementWithdrawn
        | StoreError::WithdrawalStale => Ok(()),
        StoreError::InvalidSignature => Err(BootstrapSyncFailure::new(
            BootstrapSyncFailureKind::InvalidSignature,
            format!("{subject} signature verification failed"),
        )),
        StoreError::Validation(message) => Err(BootstrapSyncFailure::new(
            BootstrapSyncFailureKind::Validation,
            message,
        )),
        error => Err(BootstrapSyncFailure::new(
            BootstrapSyncFailureKind::Protocol,
            error.to_string(),
        )),
    }
}

/// Applies one page atomically: hard failure restores pre-page announcement state.
fn apply_stream_page_staged(
    store: &mut InMemoryStore,
    bootstrap_base_url: &str,
    page: &AnnouncementStreamPage,
    now: DateTime<Utc>,
) -> Result<(usize, usize), BootstrapSyncFailure> {
    let before_announcements = store.known_pod_announcements.clone();
    let before_withdrawals = store.known_pod_withdrawals.clone();
    match apply_stream_page(store, bootstrap_base_url, page, now) {
        Ok(counts) => Ok(counts),
        Err(error) => {
            store.known_pod_announcements = before_announcements;
            store.known_pod_withdrawals = before_withdrawals;
            Err(error)
        }
    }
}

fn apply_stream_page(
    store: &mut InMemoryStore,
    bootstrap_base_url: &str,
    page: &AnnouncementStreamPage,
    now: DateTime<Utc>,
) -> Result<(usize, usize), BootstrapSyncFailure> {
    let mut retained_announcements = 0usize;
    let mut retained_withdrawals = 0usize;
    for entry in &page.entries {
        match entry.kind {
            AnnouncementStreamEventKind::Admitted | AnnouncementStreamEventKind::Renewed => {
                let Some(announcement) = entry.payload.as_announcement().cloned() else {
                    return Err(BootstrapSyncFailure::new(
                        BootstrapSyncFailureKind::Malformed,
                        "stream entry missing announcement payload",
                    ));
                };
                match retain_verified_pod_announcement(
                    store,
                    announcement,
                    DeliveryProvenance::bootstrap(bootstrap_base_url),
                    now,
                ) {
                    Ok(_) => retained_announcements = retained_announcements.saturating_add(1),
                    Err(error) => {
                        map_retain_error(error, "announcement")?;
                    }
                }
            }
            AnnouncementStreamEventKind::Withdrawn => {
                let Some(withdrawal) = entry.payload.as_withdrawal().cloned() else {
                    return Err(BootstrapSyncFailure::new(
                        BootstrapSyncFailureKind::Malformed,
                        "stream entry missing withdrawal payload",
                    ));
                };
                match retain_verified_pod_withdrawal(store, withdrawal, None, now) {
                    Ok(_) => retained_withdrawals = retained_withdrawals.saturating_add(1),
                    Err(error) => {
                        map_retain_error(error, "withdrawal")?;
                    }
                }
            }
            AnnouncementStreamEventKind::Expired => {
                // Lease expiry is evaluated locally; stream notice needs no private state.
            }
        }
    }
    Ok((retained_announcements, retained_withdrawals))
}

/// Normalizes a Bootstrap base URL (HTTPS, or loopback HTTP).
///
/// # Errors
///
/// Returns validation when the URL is empty, uses a disallowed scheme, or has no host.
pub fn normalize_bootstrap_base_url(value: &str) -> Result<String, StoreError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(StoreError::Validation(
            "Bootstrap base URL must not be empty".into(),
        ));
    }
    let mut url = url::Url::parse(trimmed)
        .map_err(|error| StoreError::Validation(format!("bad Bootstrap base URL: {error}")))?;
    let host = url
        .host_str()
        .ok_or_else(|| StoreError::Validation("Bootstrap base URL must include a host".into()))?;
    let is_loopback_http = url.scheme() == "http"
        && (host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback()));
    if url.scheme() != "https" && !is_loopback_http {
        return Err(StoreError::Validation(
            "Bootstrap base URL must use HTTPS except on loopback".into(),
        ));
    }
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    let mut normalized = url.to_string();
    while normalized.ends_with('/') {
        normalized.pop();
    }
    Ok(normalized)
}

fn empty_sync_state(endpoint_id: BootstrapEndpointId) -> BootstrapSyncState {
    BootstrapSyncState {
        endpoint_id,
        cursor: None,
        last_success_at: None,
        last_attempt_at: None,
        last_error: None,
    }
}

/// Asserts an outbound stream request carries only public pagination fields.
#[must_use]
pub fn request_is_public_only(request: &BootstrapStreamRequest) -> bool {
    let Ok(value) = serde_json::to_value(request) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    let allowed: BTreeSet<&str> = ["cursor", "limit"].into_iter().collect();
    object.keys().all(|key| allowed.contains(key.as_str()))
        && !object.contains_key("taste_profile")
        && !object.contains_key("subscriptions")
        && !object.contains_key("feedback")
        && !object.contains_key("source_affinity")
        && !object.contains_key("query")
        && !object.contains_key("interests")
}

/// In-memory scripted stream client for tests.
#[derive(Debug, Default)]
pub struct ScriptedAnnouncementStreamClient {
    /// Pages keyed by normalized base URL, then by request cursor (`""` for start).
    pub pages: std::collections::HashMap<
        String,
        std::collections::HashMap<String, AnnouncementStreamPage>,
    >,
    /// Forced failures keyed by base URL.
    pub failures: std::collections::HashMap<String, BootstrapSyncFailure>,
    /// Captured outbound requests for privacy assertions.
    pub captured: std::sync::Mutex<Vec<(String, BootstrapStreamRequest)>>,
}

impl ScriptedAnnouncementStreamClient {
    /// Creates an empty scripted client.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a successful page for `base_url` at the given cursor.
    pub fn push_page(
        &mut self,
        base_url: &str,
        cursor: Option<&str>,
        page: AnnouncementStreamPage,
    ) {
        let key = cursor.unwrap_or("").to_string();
        self.pages
            .entry(base_url.to_string())
            .or_default()
            .insert(key, page);
    }

    /// Registers a forced failure for every fetch against `base_url`.
    pub fn fail(&mut self, base_url: &str, failure: BootstrapSyncFailure) {
        self.failures.insert(base_url.to_string(), failure);
    }
}

impl AnnouncementStreamClient for ScriptedAnnouncementStreamClient {
    fn fetch_announcement_stream(
        &self,
        base_url: &str,
        request: &BootstrapStreamRequest,
    ) -> Result<AnnouncementStreamPage, BootstrapSyncFailure> {
        if let Ok(mut captured) = self.captured.lock() {
            captured.push((base_url.to_string(), request.clone()));
        }
        if let Some(failure) = self.failures.get(base_url) {
            return Err(failure.clone());
        }
        let cursor_key = request.cursor.clone().unwrap_or_default();
        self.pages
            .get(base_url)
            .and_then(|pages| pages.get(&cursor_key))
            .cloned()
            .ok_or_else(|| BootstrapSyncFailure {
                kind: BootstrapSyncFailureKind::Transport,
                message: format!("no scripted page for {base_url} cursor {cursor_key:?}"),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        announcement_lease_duration, AnnouncementStreamEntry, AnnouncementStreamEventKind,
        AnnouncementStreamPayload, NodeInfo, PackageVersion, PodAnnouncement,
        CURRENT_PROTOCOL_VERSION,
    };
    use crate::pod_announcement::announcement_delivery_is_active;
    use crate::signing::{create_node_identity, sign_pod_announcement};
    use chrono::TimeZone;

    fn sample_announcement(
        node: &crate::domain::NodeIdentity,
        announced_at: DateTime<Utc>,
        slug: &str,
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
    fn default_endpoint_is_ordinary_removable_entry() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        ensure_default_bootstrap_endpoint(&mut store, now);
        let endpoints = list_bootstrap_endpoints(&store);
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].base_url, DEFAULT_SPONSORED_BOOTSTRAP_URL);
        assert!(endpoints[0].enabled);
        assert!(endpoints[0].is_sponsored_default);
        let id = endpoints[0].id;
        remove_bootstrap_endpoint(&mut store, id).unwrap();
        assert!(list_bootstrap_endpoints(&store).is_empty());
    }

    #[test]
    fn multi_bootstrap_fallthrough_preserves_verified_announcements() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let first =
            add_bootstrap_endpoint(&mut store, "primary", "https://boot-a.example", now).unwrap();
        let second =
            add_bootstrap_endpoint(&mut store, "backup", "https://boot-b.example", now).unwrap();
        let node = create_node_identity("origin", None);
        let announcement = sample_announcement(&node, now, "systems");
        let page = AnnouncementStreamPage {
            entries: vec![AnnouncementStreamEntry {
                sequence: 1,
                recorded_at: now,
                kind: AnnouncementStreamEventKind::Admitted,
                origin_node_id: announcement.origin_node_id,
                pod_slug: announcement.pod_slug.clone(),
                payload: AnnouncementStreamPayload::Announcement(announcement.clone()),
            }],
            next_cursor: None,
            limit: 50,
        };
        let mut client = ScriptedAnnouncementStreamClient::new();
        client.fail(
            &first.base_url,
            BootstrapSyncFailure {
                kind: BootstrapSyncFailureKind::Transport,
                message: "connection refused".into(),
            },
        );
        client.push_page(&second.base_url, None, page);

        let report = sync_bootstrap_endpoints(&mut store, &client, now);
        assert!(!report.outcomes[0].ok);
        assert_eq!(
            report.outcomes[0].error.as_ref().unwrap().kind,
            BootstrapSyncFailureKind::Transport
        );
        assert!(report.outcomes[1].ok);
        assert_eq!(report.retained_announcements, 1);
        let known = store
            .known_pod_announcements
            .get(&(announcement.origin_node_id, announcement.pod_slug.clone()))
            .unwrap();
        assert!(known
            .received_from_bootstrap_urls
            .contains(&second.base_url));
        // First endpoint cursor not advanced; second success recorded.
        assert!(store.bootstrap_sync_states[&first.id].last_error.is_some());
        assert!(store.bootstrap_sync_states[&second.id]
            .last_success_at
            .is_some());
    }

    #[test]
    fn remove_excludes_sole_source_keeps_audit_and_independent_copy() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let a = add_bootstrap_endpoint(&mut store, "a", "https://boot-a.example", now).unwrap();
        let b = add_bootstrap_endpoint(&mut store, "b", "https://boot-b.example", now).unwrap();
        let node = create_node_identity("origin", None);
        let sole = sample_announcement(&node, now, "sole-source");
        let shared = sample_announcement(&node, now, "shared-source");
        retain_verified_pod_announcement(
            &mut store,
            sole.clone(),
            DeliveryProvenance::bootstrap(a.base_url.clone()),
            now,
        )
        .unwrap();
        retain_verified_pod_announcement(
            &mut store,
            shared.clone(),
            DeliveryProvenance::bootstrap(a.base_url.clone()),
            now,
        )
        .unwrap();
        retain_verified_pod_announcement(
            &mut store,
            shared.clone(),
            DeliveryProvenance::bootstrap(b.base_url.clone()),
            now,
        )
        .unwrap();

        remove_bootstrap_endpoint(&mut store, a.id).unwrap();

        let sole_known = store
            .known_pod_announcements
            .get(&(sole.origin_node_id, sole.pod_slug.clone()))
            .unwrap();
        assert!(
            !announcement_delivery_is_active(&store, sole_known, None),
            "sole-source must leave eligibility"
        );
        // Audit row preserved.
        assert!(store
            .known_pod_announcements
            .contains_key(&(sole.origin_node_id, sole.pod_slug.clone())));

        let shared_known = store
            .known_pod_announcements
            .get(&(shared.origin_node_id, shared.pod_slug.clone()))
            .unwrap();
        assert!(announcement_delivery_is_active(&store, shared_known, None));
    }

    #[test]
    fn outbound_request_shape_excludes_private_evidence() {
        let request = BootstrapStreamRequest {
            cursor: Some("7".into()),
            limit: Some(25),
        };
        assert!(request_is_public_only(&request));
        let wire = serde_json::to_value(&request).unwrap();
        let object = wire.as_object().unwrap();
        for forbidden in [
            "taste_profile",
            "subscriptions",
            "feedback",
            "source_affinity",
            "interests",
            "query",
            "user_id",
        ] {
            assert!(!object.contains_key(forbidden));
        }
    }

    #[test]
    fn config_and_cursor_survive_round_trip_fields() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let endpoint =
            add_bootstrap_endpoint(&mut store, "primary", "https://boot.example", now).unwrap();
        store.bootstrap_sync_states.insert(
            endpoint.id,
            BootstrapSyncState {
                endpoint_id: endpoint.id,
                cursor: Some("42".into()),
                last_success_at: Some(now),
                last_attempt_at: Some(now),
                last_error: None,
            },
        );
        let statuses = bootstrap_endpoint_statuses(&store);
        assert_eq!(statuses[0].sync.cursor.as_deref(), Some("42"));
        set_bootstrap_endpoint_enabled(&mut store, endpoint.id, false).unwrap();
        assert!(!list_bootstrap_endpoints(&store)[0].enabled);
    }

    #[test]
    fn hard_page_failure_does_not_leave_partial_page_applied() {
        let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
        let mut store = InMemoryStore::default();
        let endpoint =
            add_bootstrap_endpoint(&mut store, "primary", "https://boot.example", now).unwrap();
        let good_node = create_node_identity("good", None);
        let bad_node = create_node_identity("bad", None);
        let good = sample_announcement(&good_node, now, "good-pod");
        let mut bad = sample_announcement(&bad_node, now, "bad-pod");
        bad.signature = "not-a-valid-signature".into();

        let page = AnnouncementStreamPage {
            entries: vec![
                AnnouncementStreamEntry {
                    sequence: 1,
                    recorded_at: now,
                    kind: AnnouncementStreamEventKind::Admitted,
                    origin_node_id: good.origin_node_id,
                    pod_slug: good.pod_slug.clone(),
                    payload: AnnouncementStreamPayload::Announcement(good.clone()),
                },
                AnnouncementStreamEntry {
                    sequence: 2,
                    recorded_at: now,
                    kind: AnnouncementStreamEventKind::Admitted,
                    origin_node_id: bad.origin_node_id,
                    pod_slug: bad.pod_slug.clone(),
                    payload: AnnouncementStreamPayload::Announcement(bad),
                },
            ],
            next_cursor: Some("1".into()),
            limit: 50,
        };
        let mut client = ScriptedAnnouncementStreamClient::new();
        client.push_page(&endpoint.base_url, None, page);

        let report = sync_bootstrap_endpoints(&mut store, &client, now);
        assert!(!report.outcomes[0].ok);
        assert_eq!(
            report.outcomes[0].error.as_ref().unwrap().kind,
            BootstrapSyncFailureKind::InvalidSignature
        );
        assert!(
            store.known_pod_announcements.is_empty(),
            "partial page must roll back on hard failure"
        );
        assert_eq!(store.bootstrap_sync_states[&endpoint.id].cursor, None);
    }
}
