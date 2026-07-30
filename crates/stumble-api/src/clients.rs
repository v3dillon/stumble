//! Production HTTP clients for Stumble's synchronous core network seams.
//!
//! Each client wraps reqwest behind a Tokio handle so `stumble-core` stays free
//! of blocking HTTP dependencies. Shared by the CLI sync commands.

use stumble_core::*;

/// Production HTTP client for topic-neutral Announcement Stream pages.
pub struct ReqwestAnnouncementStreamClient {
    client: reqwest::Client,
    handle: tokio::runtime::Handle,
}

impl ReqwestAnnouncementStreamClient {
    /// Builds a client that drives HTTP on `handle`.
    #[must_use]
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            client: reqwest::Client::new(),
            handle,
        }
    }
}

impl AnnouncementStreamClient for ReqwestAnnouncementStreamClient {
    fn fetch_announcement_stream(
        &self,
        base_url: &str,
        request: &BootstrapStreamRequest,
    ) -> Result<AnnouncementStreamPage, BootstrapSyncFailure> {
        let base = base_url.trim_end_matches('/');
        let url = format!("{base}/bootstrap/announcements/stream");
        let client = self.client.clone();
        let cursor = request.cursor.clone();
        let limit = request.limit;
        self.handle.block_on(async move {
            let mut http = client.get(&url);
            if let Some(cursor) = &cursor {
                http = http.query(&[("cursor", cursor.as_str())]);
            }
            if let Some(limit) = limit {
                http = http.query(&[("limit", limit.to_string())]);
            }
            let response = http.send().await.map_err(|error| {
                BootstrapSyncFailure::new(BootstrapSyncFailureKind::Transport, error.to_string())
            })?;
            if !response.status().is_success() {
                return Err(BootstrapSyncFailure::new(
                    BootstrapSyncFailureKind::Protocol,
                    format!("bootstrap stream HTTP {}", response.status()),
                ));
            }
            response.json().await.map_err(|error| {
                BootstrapSyncFailure::new(BootstrapSyncFailureKind::Protocol, error.to_string())
            })
        })
    }
}

/// Production HTTP client for Bootstrap/peer advertisement samples.
pub struct ReqwestPeerAdvertisementSampleClient {
    client: reqwest::Client,
    handle: tokio::runtime::Handle,
}

impl ReqwestPeerAdvertisementSampleClient {
    /// Builds a client that drives HTTP on `handle`.
    #[must_use]
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            client: reqwest::Client::new(),
            handle,
        }
    }
}

impl PeerAdvertisementSampleClient for ReqwestPeerAdvertisementSampleClient {
    fn fetch_peer_advertisement_sample(
        &self,
        base_url: &str,
        request: &DiscoveryPeerSampleRequest,
    ) -> Result<DiscoveryPeerAdvertisementSample, DiscoveryPeerSyncFailure> {
        debug_assert!(peer_sample_request_is_public_only(request));
        let base = base_url.trim_end_matches('/');
        // Prefer Bootstrap open sample path; peer sample path is used for peer endpoints.
        let bootstrap_url = format!("{base}/bootstrap/peer-advertisements");
        let peer_url = format!("{base}/discovery/peer/advertisements");
        let client = self.client.clone();
        let limit = request.limit;
        self.handle.block_on(async move {
            for url in [bootstrap_url, peer_url] {
                let mut http = client.get(&url);
                if let Some(limit) = limit {
                    http = http.query(&[("limit", limit.to_string())]);
                }
                let response = match http.send().await {
                    Ok(response) => response,
                    Err(error) => {
                        return Err(DiscoveryPeerSyncFailure::new(
                            DiscoveryPeerSyncFailureKind::Transport,
                            error.to_string(),
                        ));
                    }
                };
                if response.status().is_success() {
                    return response.json().await.map_err(|error| {
                        DiscoveryPeerSyncFailure::new(
                            DiscoveryPeerSyncFailureKind::Protocol,
                            error.to_string(),
                        )
                    });
                }
                if response.status().as_u16() == 404 || response.status().as_u16() == 403 {
                    continue;
                }
                return Err(DiscoveryPeerSyncFailure::new(
                    DiscoveryPeerSyncFailureKind::Protocol,
                    format!("peer sample HTTP {}", response.status()),
                ));
            }
            Err(DiscoveryPeerSyncFailure::new(
                DiscoveryPeerSyncFailureKind::Transport,
                format!("no peer advertisement sample available at {base}"),
            ))
        })
    }
}

/// Production HTTP client for Discovery Peer Announcement Stream pages.
pub struct ReqwestDiscoveryPeerStreamClient {
    client: reqwest::Client,
    handle: tokio::runtime::Handle,
}

impl ReqwestDiscoveryPeerStreamClient {
    /// Builds a client that drives HTTP on `handle`.
    #[must_use]
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            client: reqwest::Client::new(),
            handle,
        }
    }
}

impl DiscoveryPeerStreamClient for ReqwestDiscoveryPeerStreamClient {
    fn fetch_peer_announcement_stream(
        &self,
        base_url: &str,
        request: &BootstrapStreamRequest,
    ) -> Result<AnnouncementStreamPage, DiscoveryPeerSyncFailure> {
        debug_assert!(peer_stream_request_is_public_only(request));
        let base = base_url.trim_end_matches('/');
        let url = format!("{base}/discovery/peer/announcements/stream");
        let client = self.client.clone();
        let cursor = request.cursor.clone();
        let limit = request.limit;
        self.handle.block_on(async move {
            let mut http = client.get(&url);
            if let Some(cursor) = &cursor {
                http = http.query(&[("cursor", cursor.as_str())]);
            }
            if let Some(limit) = limit {
                http = http.query(&[("limit", limit.to_string())]);
            }
            let response = http.send().await.map_err(|error| {
                DiscoveryPeerSyncFailure::new(
                    DiscoveryPeerSyncFailureKind::Transport,
                    error.to_string(),
                )
            })?;
            if !response.status().is_success() {
                return Err(DiscoveryPeerSyncFailure::new(
                    DiscoveryPeerSyncFailureKind::Protocol,
                    format!("discovery peer stream HTTP {}", response.status()),
                ));
            }
            response.json().await.map_err(|error| {
                DiscoveryPeerSyncFailure::new(
                    DiscoveryPeerSyncFailureKind::Protocol,
                    error.to_string(),
                )
            })
        })
    }
}

/// Production HTTP client for replaceable Index Node search.
///
/// Uses a Tokio handle so the Core inject seam can stay synchronous without
/// adding a blocking HTTP dependency to `stumble-core`. Shared by intentional
/// Explore paths (CLI `stumble pod explore` when Indexes are configured).
pub struct ReqwestIndexSearchClient {
    client: reqwest::Client,
    handle: tokio::runtime::Handle,
}

impl ReqwestIndexSearchClient {
    /// Builds a client that drives HTTP on `handle`.
    #[must_use]
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            client: reqwest::Client::new(),
            handle,
        }
    }
}

impl IndexSearchClient for ReqwestIndexSearchClient {
    fn search_index(
        &self,
        base_url: &str,
        request: &IndexSearchRequest,
    ) -> Result<PodAnnouncementSearchResponse, IndexSearchFailure> {
        debug_assert!(index_request_is_public_only(request));
        let base = base_url.trim().trim_end_matches('/');
        let url = format!("{base}/discovery/announcements");
        let client = self.client.clone();
        let query = request.query.clone();
        let limit = request.limit;
        self.handle.block_on(async move {
            let mut http = client.get(&url).query(&[("q", query.as_str())]);
            if let Some(limit) = limit {
                http = http.query(&[("limit", limit.to_string())]);
            }
            let response = http.send().await.map_err(|error| {
                IndexSearchFailure::new(IndexSearchFailureKind::Transport, error.to_string())
            })?;
            let status = response.status();
            if !status.is_success() {
                // Prefer structured error bodies when present.
                if let Ok(body) = response.json::<serde_json::Value>().await {
                    if let Some(code) = body.get("code").and_then(|v| v.as_str()) {
                        let kind = match code {
                            "malformed" => IndexSearchFailureKind::Malformed,
                            "query_too_large" => IndexSearchFailureKind::QueryTooLarge,
                            "rate_limited" => IndexSearchFailureKind::RateLimited,
                            "incompatible_protocol" => IndexSearchFailureKind::IncompatibleProtocol,
                            "index_disabled" => IndexSearchFailureKind::IndexDisabled,
                            _ => IndexSearchFailureKind::Protocol,
                        };
                        let message = body
                            .get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or(code)
                            .to_string();
                        return Err(IndexSearchFailure::new(kind, message));
                    }
                }
                return Err(IndexSearchFailure::new(
                    IndexSearchFailureKind::Protocol,
                    format!("index search HTTP {status}"),
                ));
            }
            response.json().await.map_err(|error| {
                IndexSearchFailure::new(IndexSearchFailureKind::Protocol, error.to_string())
            })
        })
    }
}
