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
            client: http_client(),
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
            client: http_client(),
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
            client: http_client(),
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
            client: http_client(),
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

/// Shared outbound HTTP client with a bounded timeout so unreachable nodes
/// fail fast instead of hanging CLI commands and daemon ticks.
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default()
}

/// Runs a small isolated runtime on a fresh OS thread so probes stay safe to
/// call from both async request handlers and synchronous CLI paths.
fn probe_on_own_thread<T: Send + 'static>(
    work: impl std::future::Future<Output = T> + Send + 'static,
) -> Option<T> {
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()
            .map(|runtime| runtime.block_on(work))
    })
    .join()
    .ok()
    .flatten()
}

/// Production HTTP [`OriginProbe`]: verifies a live public manifest at the
/// announced canonical Pod URL before Bootstrap admission.
#[derive(Debug, Default)]
pub struct ReqwestOriginProbe;

impl OriginProbe for ReqwestOriginProbe {
    fn probe_public_manifest(
        &self,
        public_pod_url: &str,
        pod_slug: &str,
    ) -> Result<OriginPublicManifestView, OriginProbeError> {
        let manifest_url = format!("{}/manifest", public_pod_url.trim_end_matches('/'));
        let node_url = public_pod_url
            .split("/federation/pods/")
            .next()
            .map(|base| format!("{base}/federation/node"))
            .ok_or(OriginProbeError::Unreachable)?;
        let expected_slug = pod_slug.to_string();
        probe_on_own_thread(async move {
            let client = http_client();
            let manifest: PodManifest = client
                .get(&manifest_url)
                .send()
                .await
                .map_err(|_| OriginProbeError::Unreachable)?
                .error_for_status()
                .map_err(|_| OriginProbeError::ManifestUnavailable)?
                .json()
                .await
                .map_err(|_| OriginProbeError::ManifestUnavailable)?;
            let node: NodeInfo = client
                .get(&node_url)
                .send()
                .await
                .map_err(|_| OriginProbeError::Unreachable)?
                .error_for_status()
                .map_err(|_| OriginProbeError::ManifestUnavailable)?
                .json()
                .await
                .map_err(|_| OriginProbeError::ManifestUnavailable)?;
            if manifest.pod.slug != expected_slug {
                return Err(OriginProbeError::ManifestUnavailable);
            }
            Ok(OriginPublicManifestView {
                protocol_version: node.supported_protocol_version,
                pod_slug: manifest.pod.slug.clone(),
                pod_name: manifest.pod.name.clone(),
                subject: manifest.pod.description.clone(),
                package_version: manifest.skill_pack_version,
                latest_event_hash: manifest.latest_known_event_hash,
                visibility_public: manifest.pod.visibility == Visibility::Public,
                origin_node_id: manifest.pod.origin_node_id,
            })
        })
        .unwrap_or(Err(OriginProbeError::Unreachable))
    }
}

/// Production HTTP [`DiscoveryPeerProbe`]: reads the peer's well-known identity.
#[derive(Debug, Default)]
pub struct ReqwestDiscoveryPeerProbe;

impl DiscoveryPeerProbe for ReqwestDiscoveryPeerProbe {
    fn probe_peer_endpoint(
        &self,
        public_endpoint: &str,
    ) -> Result<DiscoveryPeerIdentityView, DiscoveryPeerProbeError> {
        let url = format!(
            "{}/.well-known/stumble-node",
            public_endpoint.trim_end_matches('/')
        );
        probe_on_own_thread(async move {
            let well_known: WellKnownNode = http_client()
                .get(&url)
                .send()
                .await
                .map_err(|_| DiscoveryPeerProbeError::Unreachable)?
                .error_for_status()
                .map_err(|_| DiscoveryPeerProbeError::Unreachable)?
                .json()
                .await
                .map_err(|_| DiscoveryPeerProbeError::Unreachable)?;
            Ok(DiscoveryPeerIdentityView::new(
                well_known.node.node_id,
                well_known.node.public_key,
                well_known.protocol,
            ))
        })
        .unwrap_or(Err(DiscoveryPeerProbeError::Unreachable))
    }
}

/// Submits a signed Pod Announcement to one Bootstrap endpoint's open
/// admission route. Used by `stumble pod publish` to push announcements out.
pub async fn submit_pod_announcement_to_bootstrap(
    base_url: &str,
    announcement: &PodAnnouncement,
) -> Result<serde_json::Value, String> {
    let url = format!(
        "{}/bootstrap/announcements",
        base_url.trim_end_matches('/')
    );
    let response = http_client()
        .post(&url)
        .json(announcement)
        .send()
        .await
        .map_err(|error| format!("bootstrap unreachable: {error}"))?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .unwrap_or_else(|_| serde_json::json!({"status": status.as_u16()}));
    if status.is_success() {
        Ok(body)
    } else {
        Err(body
            .get("code")
            .and_then(|code| code.as_str())
            .map_or_else(
                || format!("bootstrap admission HTTP {status}"),
                ToString::to_string,
            ))
    }
}

/// Production HTTP [`OriginExploreSampleClient`]: fetches bounded signed
/// samples from the Origin named in a verified announcement.
pub struct ReqwestOriginExploreSampleClient {
    client: reqwest::Client,
    handle: tokio::runtime::Handle,
}

impl ReqwestOriginExploreSampleClient {
    /// Builds a client that drives HTTP on `handle`.
    #[must_use]
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            client: http_client(),
            handle,
        }
    }
}

impl OriginExploreSampleClient for ReqwestOriginExploreSampleClient {
    fn fetch_explore_samples(
        &self,
        announcement: &PodAnnouncement,
        limit: usize,
    ) -> Result<PodExploreSamples, SampleFetchError> {
        let url = format!(
            "{}/explore-samples",
            announcement.public_pod_url.trim_end_matches('/')
        );
        let body = serde_json::json!({ "announcement": announcement, "limit": limit });
        let client = self.client.clone();
        self.handle.block_on(async move {
            let response = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|error| SampleFetchError::Transport(error.to_string()))?;
            if !response.status().is_success() {
                return Err(SampleFetchError::Transport(format!(
                    "origin explore samples HTTP {}",
                    response.status()
                )));
            }
            response
                .json()
                .await
                .map_err(|error| SampleFetchError::Verification(error.to_string()))
        })
    }
}

/// Submits a signed Pod Endorsement to one Bootstrap endpoint.
pub async fn submit_pod_endorsement_to_bootstrap(
    base_url: &str,
    endorsement: &PodEndorsement,
) -> Result<serde_json::Value, String> {
    let url = format!("{}/bootstrap/endorsements", base_url.trim_end_matches('/'));
    let response = http_client()
        .post(&url)
        .json(endorsement)
        .send()
        .await
        .map_err(|error| format!("bootstrap unreachable: {error}"))?;
    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .unwrap_or_else(|_| serde_json::json!({"status": status.as_u16()}));
    if status.is_success() {
        Ok(body)
    } else {
        Err(body
            .get("code")
            .and_then(|code| code.as_str())
            .map_or_else(
                || format!("bootstrap endorsement HTTP {status}"),
                ToString::to_string,
            ))
    }
}

/// Fetches valid endorsements of one Pod from a Bootstrap endpoint.
pub async fn fetch_pod_endorsements_from_bootstrap(
    base_url: &str,
    endorsed_node_id: NodeIdentityId,
    endorsed_pod_slug: &str,
) -> Result<Vec<PodEndorsement>, String> {
    let url = format!("{}/bootstrap/endorsements", base_url.trim_end_matches('/'));
    http_client()
        .get(&url)
        .query(&[
            ("endorsed_node_id", endorsed_node_id.to_string()),
            ("endorsed_pod_slug", endorsed_pod_slug.to_string()),
        ])
        .send()
        .await
        .map_err(|error| format!("bootstrap unreachable: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())
}
