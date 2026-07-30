use crate::domain::{
    AnnouncementStreamPage, BootstrapStreamRequest, DiscoveryPeerAdvertisementSample,
    DiscoveryPeerSampleRequest, DiscoveryPeerSyncFailure, DiscoveryPeerSyncFailureKind,
};
use std::collections::HashMap;

/// Transport port for fetching unranked peer-advertisement samples.
///
/// Production implementations perform HTTP GET against Bootstrap or peer sample
/// paths. Tests inject scripted samples. Requests must carry only
/// [`DiscoveryPeerSampleRequest`] fields.
pub trait PeerAdvertisementSampleClient: Send + Sync {
    /// Fetches one unranked peer-advertisement sample from `base_url`.
    ///
    /// # Errors
    ///
    /// Returns a typed [`DiscoveryPeerSyncFailure`] for transport or protocol errors.
    fn fetch_peer_advertisement_sample(
        &self,
        base_url: &str,
        request: &DiscoveryPeerSampleRequest,
    ) -> Result<DiscoveryPeerAdvertisementSample, DiscoveryPeerSyncFailure>;
}

/// Transport port for fetching Discovery Peer Announcement Stream pages.
///
/// Production implementations perform HTTP GET against
/// `{endpoint}/discovery/peer/announcements/stream`. Tests inject scripted pages.
/// Requests must carry only cursor pagination fields.
pub trait DiscoveryPeerStreamClient: Send + Sync {
    /// Fetches one stream page from a Discovery Peer `base_url`.
    ///
    /// # Errors
    ///
    /// Returns a typed [`DiscoveryPeerSyncFailure`] for transport or protocol errors.
    fn fetch_peer_announcement_stream(
        &self,
        base_url: &str,
        request: &BootstrapStreamRequest,
    ) -> Result<AnnouncementStreamPage, DiscoveryPeerSyncFailure>;
}

/// In-memory scripted peer sample client for tests.
#[derive(Debug, Default)]
pub struct ScriptedPeerAdvertisementSampleClient {
    /// Samples keyed by normalized base URL.
    pub samples: HashMap<String, DiscoveryPeerAdvertisementSample>,
    /// Forced failures keyed by base URL.
    pub failures: HashMap<String, DiscoveryPeerSyncFailure>,
    /// Captured outbound requests for privacy assertions.
    pub captured: std::sync::Mutex<Vec<(String, DiscoveryPeerSampleRequest)>>,
}

impl ScriptedPeerAdvertisementSampleClient {
    /// Creates an empty scripted sample client.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a successful sample for `base_url`.
    pub fn push_sample(&mut self, base_url: &str, sample: DiscoveryPeerAdvertisementSample) {
        self.samples.insert(base_url.to_string(), sample);
    }

    /// Registers a forced failure for every sample fetch against `base_url`.
    pub fn fail(&mut self, base_url: &str, failure: DiscoveryPeerSyncFailure) {
        self.failures.insert(base_url.to_string(), failure);
    }
}

impl PeerAdvertisementSampleClient for ScriptedPeerAdvertisementSampleClient {
    fn fetch_peer_advertisement_sample(
        &self,
        base_url: &str,
        request: &DiscoveryPeerSampleRequest,
    ) -> Result<DiscoveryPeerAdvertisementSample, DiscoveryPeerSyncFailure> {
        if let Ok(mut captured) = self.captured.lock() {
            captured.push((base_url.to_string(), request.clone()));
        }
        if let Some(failure) = self.failures.get(base_url) {
            return Err(failure.clone());
        }
        self.samples.get(base_url).cloned().ok_or_else(|| {
            DiscoveryPeerSyncFailure::new(
                DiscoveryPeerSyncFailureKind::Transport,
                format!("no scripted peer sample for {base_url}"),
            )
        })
    }
}

/// In-memory scripted Discovery Peer stream client for tests.
#[derive(Debug, Default)]
pub struct ScriptedDiscoveryPeerStreamClient {
    /// Pages keyed by base URL, then by request cursor (`""` for start).
    pub pages: HashMap<String, HashMap<String, AnnouncementStreamPage>>,
    /// Forced failures keyed by base URL.
    pub failures: HashMap<String, DiscoveryPeerSyncFailure>,
    /// Captured outbound requests for privacy assertions.
    pub captured: std::sync::Mutex<Vec<(String, BootstrapStreamRequest)>>,
}

impl ScriptedDiscoveryPeerStreamClient {
    /// Creates an empty scripted stream client.
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
    pub fn fail(&mut self, base_url: &str, failure: DiscoveryPeerSyncFailure) {
        self.failures.insert(base_url.to_string(), failure);
    }
}

impl DiscoveryPeerStreamClient for ScriptedDiscoveryPeerStreamClient {
    fn fetch_peer_announcement_stream(
        &self,
        base_url: &str,
        request: &BootstrapStreamRequest,
    ) -> Result<AnnouncementStreamPage, DiscoveryPeerSyncFailure> {
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
            .ok_or_else(|| {
                DiscoveryPeerSyncFailure::new(
                    DiscoveryPeerSyncFailureKind::Transport,
                    format!("no scripted peer stream page for {base_url} cursor {cursor_key:?}"),
                )
            })
    }
}
