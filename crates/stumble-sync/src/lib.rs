use serde::{Deserialize, Serialize};
use stumble_core::*;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncReport {
    pub peer_id: Uuid,
    pub pod_slug: String,
    pub fetched_events: usize,
    pub imported_events: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum DirectSubscriptionError {
    #[error("direct subscription core task failed")]
    CoreTask(#[source] tokio::task::JoinError),
    #[error("invalid public Pod URL {url}")]
    InvalidUrl {
        url: String,
        #[source]
        source: url::ParseError,
    },
    #[error("invalid public Pod address")]
    InvalidAddress(#[source] AgentToolsError),
    #[error("failed to fetch public Pod artifacts from {url}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("origin node no longer recognizes the stored synchronization cursor")]
    UnknownCursor,
    #[error(transparent)]
    Core(#[from] AgentToolsError),
}

/// Failure while synchronizing signed Pod Events from a trusted peer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PeerSyncError {
    /// A trusted peer artifact could not be fetched or decoded.
    #[error("failed to fetch trusted peer artifacts from {url}")]
    Request {
        /// Artifact URL that failed.
        url: String,
        /// Underlying HTTP transport or response-decoding failure.
        #[source]
        source: reqwest::Error,
    },
    /// The peer advertises an event contract this node cannot interpret.
    #[error("incompatible protocol version {received}; this node supports {supported}")]
    IncompatibleProtocol {
        /// Version advertised by the peer.
        received: String,
        /// Version supported by this node.
        supported: &'static str,
    },
    /// The peer presented a key different from the trusted record.
    #[error("remote public key does not match the trusted peer")]
    PublicKeyMismatch,
    /// The peer presented a canonical Node ID different from the trusted record.
    #[error("remote Node identity does not match the trusted peer")]
    NodeIdentityMismatch,
    /// The selected peer is not the Origin Node pinned by the Subscription.
    #[error("selected peer does not match the Subscription Origin Node")]
    SubscriptionPeerMismatch,
    /// The blocking event-import task failed before returning a result.
    #[error("trusted peer import task failed")]
    ImportTask(#[source] tokio::task::JoinError),
    /// Core authorization, verification, projection, or persistence failed.
    #[error(transparent)]
    Core(#[from] AgentToolsError),
    /// Direct-address synchronization failed after the selected peer was verified.
    #[error(transparent)]
    DirectSubscription(#[from] DirectSubscriptionError),
}

trait PeerSyncTransport {
    async fn node_info(&self, base_url: &str) -> Result<NodeInfo, PeerSyncError>;
    async fn events(&self, url: &str) -> Result<Vec<EventLog>, PeerSyncError>;
}

struct HttpPeerSyncTransport {
    client: reqwest::Client,
}

impl PeerSyncTransport for HttpPeerSyncTransport {
    async fn node_info(&self, base_url: &str) -> Result<NodeInfo, PeerSyncError> {
        let well_known_url = format!("{base_url}/.well-known/stumble-node");
        let response = self.client.get(&well_known_url).send().await;
        if let Ok(response) = response.and_then(reqwest::Response::error_for_status) {
            return response
                .json::<WellKnownNode>()
                .await
                .map(|well_known| well_known.node)
                .map_err(|source| PeerSyncError::Request {
                    url: well_known_url,
                    source,
                });
        }
        let node_url = format!("{base_url}/federation/node");
        self.client
            .get(&node_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|source| PeerSyncError::Request {
                url: node_url.clone(),
                source,
            })?
            .json::<NodeInfo>()
            .await
            .map_err(|source| PeerSyncError::Request {
                url: node_url,
                source,
            })
    }

    async fn events(&self, url: &str) -> Result<Vec<EventLog>, PeerSyncError> {
        self.client
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|source| PeerSyncError::Request {
                url: url.to_string(),
                source,
            })?
            .json::<Vec<EventLog>>()
            .await
            .map_err(|source| PeerSyncError::Request {
                url: url.to_string(),
                source,
            })
    }
}

/// Fetches a directly addressed public Pod and creates a local Subscription.
///
/// # Errors
///
/// Returns an error when the URL is invalid, the Origin Node is unavailable or
/// returns invalid JSON, signed artifacts fail validation, or persistence fails.
pub async fn subscribe_pod_from_url(
    tools: &AgentTools,
    ctx: &AuthContext,
    public_pod_url: &str,
) -> Result<SynchronizationResult, DirectSubscriptionError> {
    let address = public_pod_url.to_owned();
    let public_pod_url = tokio::task::spawn_blocking(move || canonical_public_pod_url(&address))
        .await
        .map_err(DirectSubscriptionError::CoreTask)?
        .map_err(DirectSubscriptionError::InvalidAddress)?;
    let client = origin_client(&public_pod_url)?;
    let snapshot = fetch_pod_snapshot(&client, &public_pod_url, None).await?;
    let tools = tools.clone();
    let ctx = ctx.clone();
    tokio::task::spawn_blocking(move || {
        tools.subscribe_public_pod(
            &ctx,
            SubscribePublicPodRequest::new(public_pod_url, snapshot),
            chrono::Utc::now(),
        )
    })
    .await
    .map_err(DirectSubscriptionError::CoreTask)?
    .map_err(DirectSubscriptionError::Core)
}

/// Fetches and applies only events after a Subscription's stored cursor.
///
/// # Errors
///
/// Returns an error when the Subscription is inaccessible, the Origin Node is
/// unavailable, its cursor is unknown, artifact validation fails, or persistence fails.
pub async fn synchronize_subscription_from_origin(
    tools: &AgentTools,
    ctx: &AuthContext,
    subscription_id: SubscriptionId,
) -> Result<SynchronizationResult, DirectSubscriptionError> {
    let read_tools = tools.clone();
    let read_ctx = ctx.clone();
    let (public_pod_url, cursor) = tokio::task::spawn_blocking(move || {
        let subscription = read_tools
            .subscription(&read_ctx, subscription_id)
            .map_err(DirectSubscriptionError::Core)?;
        let public_pod_url = canonical_public_pod_url(&subscription.public_pod_url)
            .map_err(DirectSubscriptionError::InvalidAddress)?;
        Ok::<_, DirectSubscriptionError>((public_pod_url, subscription.last_event_hash))
    })
    .await
    .map_err(DirectSubscriptionError::CoreTask)??;
    let client = origin_client(&public_pod_url)?;
    let snapshot = fetch_pod_snapshot(&client, &public_pod_url, cursor.as_deref()).await?;
    let tools = tools.clone();
    let ctx = ctx.clone();
    tokio::task::spawn_blocking(move || {
        tools.synchronize_subscription(&ctx, subscription_id, snapshot)
    })
    .await
    .map_err(DirectSubscriptionError::CoreTask)?
    .map_err(DirectSubscriptionError::Core)
}

fn origin_client(public_pod_url: &str) -> Result<reqwest::Client, DirectSubscriptionError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|source| DirectSubscriptionError::Request {
            url: public_pod_url.to_string(),
            source,
        })
}

async fn fetch_pod_snapshot(
    client: &reqwest::Client,
    public_pod_url: &str,
    after_event_hash: Option<&str>,
) -> Result<FederationPodSnapshot, DirectSubscriptionError> {
    let pod_url = reqwest::Url::parse(public_pod_url).map_err(|source| {
        DirectSubscriptionError::InvalidUrl {
            url: public_pod_url.to_string(),
            source,
        }
    })?;
    // A Relay-shaped address serves the whole Origin-signed snapshot in one
    // response. The snapshot's own `node` is the Origin; the Relay host's
    // well-known identity is the Relay and must never be pinned as Origin.
    if relay_public_pod_url_parts(pod_url.path()).is_some() {
        let snapshot = fetch_json::<FederationPodSnapshot>(client, public_pod_url).await?;
        let events = filter_events_after_cursor(snapshot.events, after_event_hash)?;
        return Ok(FederationPodSnapshot::new(
            snapshot.node,
            snapshot.manifest,
            events,
        ));
    }
    let mut origin_url = pod_url.clone();
    origin_url.set_path("");
    origin_url.set_query(None);
    origin_url.set_fragment(None);
    let node_url = format!(
        "{}/.well-known/stumble-node",
        origin_url.as_str().trim_end_matches('/')
    );
    let node = client
        .get(&node_url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|source| DirectSubscriptionError::Request {
            url: node_url.clone(),
            source,
        })?
        .json::<WellKnownNode>()
        .await
        .map_err(|source| DirectSubscriptionError::Request {
            url: node_url,
            source,
        })?
        .node;
    let manifest_url = format!("{}/manifest", public_pod_url.trim_end_matches('/'));
    let manifest = fetch_json(client, &manifest_url).await?;
    let events_url = format!("{}/events", public_pod_url.trim_end_matches('/'));
    let all_events = fetch_json::<Vec<EventLog>>(client, &events_url).await?;
    let events = filter_events_after_cursor(all_events, after_event_hash)?;
    Ok(FederationPodSnapshot::new(node, manifest, events))
}

fn filter_events_after_cursor(
    all_events: Vec<EventLog>,
    after_event_hash: Option<&str>,
) -> Result<Vec<EventLog>, DirectSubscriptionError> {
    match after_event_hash {
        Some(cursor) => {
            let index = all_events
                .iter()
                .position(|event| event.content_hash == cursor)
                .ok_or(DirectSubscriptionError::UnknownCursor)?;
            Ok(all_events.into_iter().skip(index + 1).collect())
        }
        None => Ok(all_events),
    }
}

async fn fetch_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
) -> Result<T, DirectSubscriptionError> {
    client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|source| DirectSubscriptionError::Request {
            url: url.to_string(),
            source,
        })?
        .json::<T>()
        .await
        .map_err(|source| DirectSubscriptionError::Request {
            url: url.to_string(),
            source,
        })
}

/// Fetches and imports signed events for one Pod from a trusted peer.
///
/// # Errors
///
/// Returns an error before import when the peer cannot be reached, advertises
/// an incompatible protocol, presents a different key, or event import fails.
pub async fn sync_pod_from_peer(
    tools: &AgentTools,
    ctx: &AuthContext,
    peer: &TrustedPeer,
    pod_slug: &str,
) -> Result<SyncReport, PeerSyncError> {
    let transport = HttpPeerSyncTransport {
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|source| PeerSyncError::Request {
                url: peer.base_url.clone(),
                source,
            })?,
    };
    sync_pod_from_peer_with_transport(tools, ctx, peer, pod_slug, &transport).await
}

/// Verifies a selected trusted peer and refreshes one Subscription from its Origin Node.
///
/// # Errors
///
/// Returns an error when the peer does not match the pinned Subscription identity,
/// peer verification fails, or signed incremental synchronization fails.
pub async fn synchronize_subscription_from_peer(
    tools: &AgentTools,
    ctx: &AuthContext,
    peer: &TrustedPeer,
    subscription_id: SubscriptionId,
) -> Result<SynchronizationResult, PeerSyncError> {
    let read_tools = tools.clone();
    let read_ctx = ctx.clone();
    let selected_peer = peer.clone();
    let (pod_slug, cursor) = tokio::task::spawn_blocking(move || {
        let subscription = read_tools.subscription(&read_ctx, subscription_id)?;
        if subscription.origin_node_id != selected_peer.node_id
            || subscription.origin_public_key != selected_peer.public_key
        {
            return Err(PeerSyncError::SubscriptionPeerMismatch);
        }
        Ok((subscription.pod_slug, subscription.last_event_hash))
    })
    .await
    .map_err(DirectSubscriptionError::CoreTask)??;
    let public_pod_url = format!(
        "{}/federation/pods/{}",
        peer.base_url.trim_end_matches('/'),
        pod_slug
    );
    let client = origin_client(&public_pod_url)?;
    let snapshot = fetch_pod_snapshot(&client, &public_pod_url, cursor.as_deref()).await?;
    validate_peer_identity(peer, &snapshot.node)?;
    let tools = tools.clone();
    let ctx = ctx.clone();
    tokio::task::spawn_blocking(move || {
        tools.synchronize_subscription(&ctx, subscription_id, snapshot)
    })
    .await
    .map_err(DirectSubscriptionError::CoreTask)?
    .map_err(PeerSyncError::Core)
}

async fn sync_pod_from_peer_with_transport(
    tools: &AgentTools,
    ctx: &AuthContext,
    peer: &TrustedPeer,
    pod_slug: &str,
    transport: &impl PeerSyncTransport,
) -> Result<SyncReport, PeerSyncError> {
    let base = peer.base_url.trim_end_matches('/');
    let remote = transport.node_info(base).await?;
    validate_peer_identity(peer, &remote)?;
    let url = format!("{}/federation/pods/{}/events", base, pod_slug);
    let events = transport.events(&url).await?;
    let fetched_events = events.len();
    let imported_events =
        import_peer_events_on_blocking(tools.clone(), ctx.clone(), peer.id, events).await?;
    Ok(SyncReport {
        peer_id: peer.id,
        pod_slug: pod_slug.to_string(),
        fetched_events,
        imported_events,
    })
}

fn validate_peer_identity(peer: &TrustedPeer, remote: &NodeInfo) -> Result<(), PeerSyncError> {
    validate_peer_protocol(&remote.supported_protocol_version)?;
    if !peer.node_id.is_nil() && remote.node_id != peer.node_id {
        return Err(PeerSyncError::NodeIdentityMismatch);
    }
    if remote.public_key != peer.public_key {
        return Err(PeerSyncError::PublicKeyMismatch);
    }
    Ok(())
}

fn validate_peer_protocol(version: &str) -> Result<(), PeerSyncError> {
    if version == CURRENT_PROTOCOL_VERSION {
        return Ok(());
    }
    Err(PeerSyncError::IncompatibleProtocol {
        received: version.to_string(),
        supported: CURRENT_PROTOCOL_VERSION,
    })
}

async fn import_peer_events_on_blocking(
    tools: AgentTools,
    ctx: AuthContext,
    peer_id: PeerId,
    events: Vec<EventLog>,
) -> Result<usize, PeerSyncError> {
    tokio::task::spawn_blocking(move || tools.import_pod_events(&ctx, peer_id, events))
        .await
        .map_err(PeerSyncError::ImportTask)?
        .map_err(PeerSyncError::Core)
}

pub fn export_events_jsonl(events: &[EventLog]) -> anyhow::Result<String> {
    Ok(events
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n"))
}

pub fn import_events_jsonl(text: &str) -> anyhow::Result<Vec<EventLog>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakePeerTransport {
        node: NodeInfo,
    }

    impl PeerSyncTransport for FakePeerTransport {
        async fn node_info(&self, _base_url: &str) -> Result<NodeInfo, PeerSyncError> {
            Ok(self.node.clone())
        }

        async fn events(&self, _url: &str) -> Result<Vec<EventLog>, PeerSyncError> {
            panic!("events must not be fetched before peer negotiation succeeds")
        }
    }

    fn peer(public_key: &str) -> TrustedPeer {
        TrustedPeer {
            id: Uuid::now_v7(),
            node_id: Uuid::now_v7(),
            tenant_id: None,
            display_name: "peer".into(),
            base_url: "https://peer.example".into(),
            public_key: public_key.into(),
            trust_level: TrustLevel::ReadOnly,
            enabled: true,
            created_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn incompatible_peer_protocol_stops_before_event_fetch() {
        let tools = AgentTools::new(seed_store());
        let ctx = tools.default_auth_context().unwrap();
        let peer = peer("trusted-key");
        let transport = FakePeerTransport {
            node: NodeInfo {
                node_id: Uuid::now_v7(),
                display_name: "old peer".into(),
                public_key: "trusted-key".into(),
                supported_protocol_version: "stumble/0.1".into(),
            },
        };

        let result =
            sync_pod_from_peer_with_transport(&tools, &ctx, &peer, "example-pod", &transport).await;

        assert!(matches!(
            result,
            Err(PeerSyncError::IncompatibleProtocol { received, supported })
                if received == "stumble/0.1" && supported == CURRENT_PROTOCOL_VERSION
        ));
    }

    #[tokio::test]
    async fn peer_key_mismatch_stops_before_event_fetch() {
        let tools = AgentTools::new(seed_store());
        let ctx = tools.default_auth_context().unwrap();
        let peer = peer("trusted-key");
        let transport = FakePeerTransport {
            node: NodeInfo {
                node_id: peer.node_id,
                display_name: "impostor".into(),
                public_key: "different-key".into(),
                supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
            },
        };

        let result =
            sync_pod_from_peer_with_transport(&tools, &ctx, &peer, "example-pod", &transport).await;

        assert!(matches!(result, Err(PeerSyncError::PublicKeyMismatch)));
    }

    #[tokio::test]
    async fn peer_node_identity_mismatch_stops_before_event_fetch() {
        let tools = AgentTools::new(seed_store());
        let ctx = tools.default_auth_context().unwrap();
        let peer = peer("trusted-key");
        let transport = FakePeerTransport {
            node: NodeInfo {
                node_id: Uuid::now_v7(),
                display_name: "different node".into(),
                public_key: "trusted-key".into(),
                supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
            },
        };

        let result =
            sync_pod_from_peer_with_transport(&tools, &ctx, &peer, "example-pod", &transport).await;

        assert!(matches!(result, Err(PeerSyncError::NodeIdentityMismatch)));
    }
}
