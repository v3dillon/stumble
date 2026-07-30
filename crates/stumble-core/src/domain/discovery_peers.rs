use super::*;

/// Renewable Discovery Peer Advertisement lease duration in whole days.
pub const PEER_ADVERTISEMENT_LEASE_DURATION_DAYS: i64 = 7;

/// Returns the renewable validity period carried by every Discovery Peer Advertisement.
#[must_use]
pub fn peer_advertisement_lease_duration() -> chrono::Duration {
    chrono::Duration::days(PEER_ADVERTISEMENT_LEASE_DURATION_DAYS)
}

/// Narrow public capability a Discovery Peer may advertise.
///
/// Does not grant Pod Event, Subscription, Taste Profile, or administrative access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryPeerCapability {
    /// Serves bounded Announcement Stream pages and peer-advertisement samples.
    AnnouncementServing,
}

impl DiscoveryPeerCapability {
    /// Wire code for this capability.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AnnouncementServing => "announcement_serving",
        }
    }
}

impl std::fmt::Display for DiscoveryPeerCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Signed, renewable statement that an opted-in node serves public discovery artifacts.
///
/// Contains only identity, endpoint, protocol version, announcement-serving capability,
/// and lease expiry. It never carries private state or rank assertions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DiscoveryPeerAdvertisement {
    /// Stable identity of this signed advertisement.
    pub id: Uuid,
    /// Advertising node identity.
    pub node_id: NodeIdentityId,
    /// Node identity and verification key.
    pub signer: NodeInfo,
    /// Declared public base endpoint that serves discovery peer contracts.
    pub public_endpoint: String,
    /// Protocol version this peer serves.
    pub protocol_version: String,
    /// Narrow public capability (announcement serving only).
    pub capability: DiscoveryPeerCapability,
    /// Time at which the node signed this advertisement.
    pub issued_at: DateTime<Utc>,
    /// Exclusive end of the renewable peer advertisement lease.
    pub expires_at: DateTime<Utc>,
    /// Ed25519 signature over every preceding field.
    pub signature: String,
}

impl DiscoveryPeerAdvertisement {
    /// Returns whether this advertisement's lease is still active at `now`.
    #[must_use]
    pub fn lease_is_active(&self, now: DateTime<Utc>) -> bool {
        self.expires_at > now
    }
}

/// Locally retained verified Discovery Peer Advertisement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct KnownDiscoveryPeerAdvertisement {
    /// Peer-authored signed advertisement, unchanged by relays.
    pub advertisement: DiscoveryPeerAdvertisement,
    /// Time at which this node verified and retained it.
    pub received_at: DateTime<Utc>,
    /// Bootstrap base URLs and peer endpoints that delivered this advertisement.
    ///
    /// Accumulated across multi-source samples and copied into
    /// [`OutboundDiscoveryPeer::learned_from`] at selection time.
    #[serde(default)]
    pub learned_from: BTreeSet<String>,
}

/// User opt-in state for serving as a Discovery Peer.
///
/// Default is disabled: ordinary Home Nodes remain outbound-only for discovery.
/// Survives SQLite restart together with the current advertisement lease and
/// peer serving stream cursor high-water.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[non_exhaustive]
pub struct DiscoveryPeerServiceState {
    /// Whether announcement serving is currently enabled by the User.
    pub enabled: bool,
    /// Declared public base endpoint required while enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_endpoint: Option<String>,
    /// Current signed advertisement lease when enabled and verified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_advertisement: Option<DiscoveryPeerAdvertisement>,
    /// Time of the last successful enable/renew verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<DateTime<Utc>>,
    /// Next monotonic sequence for peer-local Announcement Stream serving.
    pub next_stream_sequence: u64,
}

impl Default for DiscoveryPeerServiceState {
    fn default() -> Self {
        Self {
            enabled: false,
            public_endpoint: None,
            current_advertisement: None,
            verified_at: None,
            // Sequence positions are 1-based so empty cursors resume from start.
            next_stream_sequence: 1,
        }
    }
}

/// Stable machine-readable reason a peer advertisement was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryPeerAdmissionRejectionReason {
    /// Payload failed structural validation.
    Malformed,
    /// Node identity fields are inconsistent or unusable.
    InvalidIdentity,
    /// Signature verification failed (forged or corrupted).
    InvalidSignature,
    /// Declared public endpoint was not reachable.
    UnreachableEndpoint,
    /// Advertised protocol is not compatible.
    IncompatibleProtocol,
    /// Advertisement lease is expired or otherwise not current.
    StaleLease,
    /// Endpoint uses a private, reserved, or non-public address policy violation.
    PrivateEndpoint,
    /// Endpoint violates HTTPS-outside-loopback policy.
    InsecureEndpoint,
    /// Signed payload exceeded the bounded size accepted for admission.
    PayloadTooLarge,
    /// Capability is not a permitted discovery peer capability.
    UnsupportedCapability,
    /// This node does not currently enable Bootstrap peer admission.
    BootstrapDisabled,
    /// Local discovery peer service is not enabled for inbound serving.
    PeerServiceDisabled,
    /// Enable/renew was denied because verification preconditions failed.
    VerificationFailed,
    /// Reachable endpoint identity did not match the signed advertisement or local node.
    IdentityMismatch,
    /// Per-network or per-node peer-advertisement admission rate limit was exceeded.
    RateLimited,
}

impl DiscoveryPeerAdmissionRejectionReason {
    /// Wire code returned on public peer admission/serve rejection responses.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::InvalidIdentity => "invalid_identity",
            Self::InvalidSignature => "invalid_signature",
            Self::UnreachableEndpoint => "unreachable_endpoint",
            Self::IncompatibleProtocol => "incompatible_protocol",
            Self::StaleLease => "stale_lease",
            Self::PrivateEndpoint => "private_endpoint",
            Self::InsecureEndpoint => "insecure_endpoint",
            Self::PayloadTooLarge => "payload_too_large",
            Self::UnsupportedCapability => "unsupported_capability",
            Self::BootstrapDisabled => "bootstrap_disabled",
            Self::PeerServiceDisabled => "peer_service_disabled",
            Self::VerificationFailed => "verification_failed",
            Self::IdentityMismatch => "identity_mismatch",
            Self::RateLimited => "rate_limited",
        }
    }
}

impl std::fmt::Display for DiscoveryPeerAdmissionRejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Successful open Bootstrap admission of a Discovery Peer Advertisement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DiscoveryPeerAdmissionAcceptance {
    /// Whether admission was newly applied or an idempotent replay.
    pub outcome: BootstrapAdmissionOutcomeKind,
    /// Verified peer advertisement retained by this Bootstrap Node.
    pub known: KnownDiscoveryPeerAdvertisement,
}

/// Reachable identity facts returned by a Discovery Peer endpoint probe.
///
/// Used to bind enablement and Bootstrap peer-ad admission to the live node
/// behind a public endpoint (node id, verification key, protocol).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DiscoveryPeerIdentityView {
    /// Node identity observed at the endpoint.
    pub node_id: NodeIdentityId,
    /// Verification public key observed at the endpoint.
    pub public_key: String,
    /// Protocol version observed at the endpoint.
    pub protocol_version: String,
}

impl DiscoveryPeerIdentityView {
    /// Builds the live identity view a probe observed at a peer endpoint.
    #[must_use]
    pub const fn new(
        node_id: NodeIdentityId,
        public_key: String,
        protocol_version: String,
    ) -> Self {
        Self {
            node_id,
            public_key,
            protocol_version,
        }
    }
}

/// Bounded randomized sample of current peer advertisements (unranked).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DiscoveryPeerAdvertisementSample {
    /// Small unranked sample of current valid peer advertisements.
    pub advertisements: Vec<DiscoveryPeerAdvertisement>,
    /// Effective sample bound applied by the server.
    pub limit: usize,
}

impl DiscoveryPeerAdvertisementSample {
    /// Builds one peer sample response.
    #[must_use]
    pub fn new(advertisements: Vec<DiscoveryPeerAdvertisement>, limit: usize) -> Self {
        Self {
            advertisements,
            limit,
        }
    }
}

/// Default maximum size of the automatic outbound Discovery Peer set.
pub const DEFAULT_MAX_OUTBOUND_DISCOVERY_PEERS: usize = 4;

/// Hard upper bound on the automatic outbound Discovery Peer set.
pub const MAX_OUTBOUND_DISCOVERY_PEERS: usize = 8;

/// Consecutive hard failures before automatic local eviction of a Discovery Peer.
pub const PEER_FAILURES_BEFORE_EVICTION: u32 = 3;

/// Invalid lifecycle entries on one stream page that count as flooding.
pub const MAX_PEER_INVALID_ENTRIES_PER_PAGE: usize = 3;

/// Home Node preference for automatic Discovery Peer gossip.
///
/// Disabling stops automatic selection and peer stream sync without deleting
/// cached peer advertisements, outbound audit rows, cursors, or Bootstrap config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[non_exhaustive]
pub struct DiscoveryPeerGossipConfig {
    /// When false, automatic peer learning/sync is skipped.
    pub automatic_gossip_enabled: bool,
    /// Bounded size of the rotating outbound peer set.
    pub max_outbound_peers: usize,
    /// Time of the last config mutation, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

impl Default for DiscoveryPeerGossipConfig {
    fn default() -> Self {
        Self {
            automatic_gossip_enabled: true,
            max_outbound_peers: DEFAULT_MAX_OUTBOUND_DISCOVERY_PEERS,
            updated_at: None,
        }
    }
}

/// One peer in the Home Node's bounded rotating outbound Discovery Peer set.
///
/// Selection is automatic and capability-limited. It never creates a Trusted Peer
/// relationship or grants Pod Event / private / administrative access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct OutboundDiscoveryPeer {
    /// Advertising node identity (also the local outbound set key).
    pub node_id: NodeIdentityId,
    /// Verified public base endpoint for peer stream and sample contracts.
    pub public_endpoint: String,
    /// Identity of the currently retained signed advertisement.
    pub advertisement_id: Uuid,
    /// Protocol version declared by the peer.
    pub protocol_version: String,
    /// Time at which this peer entered (or re-entered) the outbound set.
    pub selected_at: DateTime<Utc>,
    /// Bootstrap base URLs and peer endpoints that delivered the advertisement.
    #[serde(default)]
    pub learned_from: BTreeSet<String>,
}

/// Health of one outbound Discovery Peer relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryPeerHealth {
    /// Recent sync succeeded or the peer has not yet failed.
    Healthy,
    /// Backed off after failures but still retained for possible resume.
    BackedOff,
    /// Automatically removed from the active outbound set.
    Evicted,
}

impl DiscoveryPeerHealth {
    /// Wire code for operator surfaces.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::BackedOff => "backed_off",
            Self::Evicted => "evicted",
        }
    }
}

impl std::fmt::Display for DiscoveryPeerHealth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Per-Discovery-Peer stream cursor and health bookkeeping.
///
/// Survives SQLite restart so rotation, eviction, and cursor resume continue
/// without replaying history. Keyed by peer node identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DiscoveryPeerSyncState {
    /// Peer node identity this progress belongs to.
    pub node_id: NodeIdentityId,
    /// Opaque stream cursor last successfully consumed, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Time of the last fully successful page consumption.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<DateTime<Utc>>,
    /// Time of the most recent sync attempt (success or failure).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<DateTime<Utc>>,
    /// Consecutive hard failures since the last success.
    #[serde(default)]
    pub consecutive_failures: u32,
    /// Earliest time another attempt is allowed after backoff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff_until: Option<DateTime<Utc>>,
    /// Current local health classification.
    #[serde(default = "default_discovery_peer_health")]
    pub health: DiscoveryPeerHealth,
    /// Typed failure from the most recent unsuccessful attempt, cleared on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<DiscoveryPeerSyncFailure>,
}

fn default_discovery_peer_health() -> DiscoveryPeerHealth {
    DiscoveryPeerHealth::Healthy
}

/// Typed outbound Discovery Peer synchronization failure for operators.
///
/// Contains no Taste Profile, Subscription, feedback, or other private evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DiscoveryPeerSyncFailure {
    /// Stable machine-readable failure class.
    pub kind: DiscoveryPeerSyncFailureKind,
    /// Human-readable detail safe for operators (no private evidence).
    pub message: String,
}

impl DiscoveryPeerSyncFailure {
    /// Builds a typed operator-facing peer sync failure.
    #[must_use]
    pub fn new(kind: DiscoveryPeerSyncFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// Stable classes of outbound Discovery Peer sync failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryPeerSyncFailureKind {
    /// Transport-level failure (DNS, connection, timeout, HTTP transport).
    Transport,
    /// Remote protocol response was unusable (status, shape, or version).
    Protocol,
    /// Stream page or cursor was malformed.
    Malformed,
    /// A delivered announcement failed local signature verification.
    InvalidSignature,
    /// A delivered announcement failed local validation (URL, lease, identity).
    Validation,
    /// Peer advertisement or stream declared an incompatible protocol.
    IncompatibleProtocol,
    /// Peer advertisement lease expired.
    ExpiredAdvertisement,
    /// Peer delivered an abusive volume of invalid lifecycle artifacts.
    Flooding,
    /// Peer endpoint was not reachable under local probe policy.
    Unreachable,
}

impl DiscoveryPeerSyncFailureKind {
    /// Wire code returned on operator surfaces.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Transport => "transport",
            Self::Protocol => "protocol",
            Self::Malformed => "malformed",
            Self::InvalidSignature => "invalid_signature",
            Self::Validation => "validation",
            Self::IncompatibleProtocol => "incompatible_protocol",
            Self::ExpiredAdvertisement => "expired_advertisement",
            Self::Flooding => "flooding",
            Self::Unreachable => "unreachable",
        }
    }
}

impl std::fmt::Display for DiscoveryPeerSyncFailureKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Operator view of one outbound Discovery Peer plus its sync progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct OutboundDiscoveryPeerStatus {
    /// Selected outbound peer.
    pub peer: OutboundDiscoveryPeer,
    /// Persisted cursor and health for this peer.
    pub sync: DiscoveryPeerSyncState,
}

/// Summary of one outbound Discovery Peer synchronization pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DiscoveryPeerSyncReport {
    /// Per-peer outcomes in selection order.
    pub outcomes: Vec<DiscoveryPeerSyncOutcome>,
    /// Total announcements newly retained or refreshed this pass.
    pub retained_announcements: usize,
    /// Total withdrawals retained this pass.
    pub retained_withdrawals: usize,
    /// Peers automatically evicted during this pass.
    pub evicted: Vec<NodeIdentityId>,
}

/// Outcome of attempting sync against one outbound Discovery Peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DiscoveryPeerSyncOutcome {
    /// Peer node identity.
    pub node_id: NodeIdentityId,
    /// Peer public endpoint (network address only).
    pub public_endpoint: String,
    /// Whether this peer completed successfully.
    pub ok: bool,
    /// Pages successfully consumed from this peer.
    pub pages_fetched: usize,
    /// Announcements retained from this peer during the pass.
    pub retained_announcements: usize,
    /// Withdrawals retained from this peer during the pass.
    pub retained_withdrawals: usize,
    /// Cursor after this attempt, when advanced or already present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Health after this attempt.
    pub health: DiscoveryPeerHealth,
    /// Typed failure when `ok` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<DiscoveryPeerSyncFailure>,
}

/// Home Node discovery readiness for operators and degraded-mode messaging.
///
/// Degraded discovery means automatic Bootstrap/peer network discovery is
/// impaired; direct Pod URL subscription and local audit state remain available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DiscoveryStatus {
    /// Whether automatic peer gossip is currently enabled.
    pub automatic_gossip_enabled: bool,
    /// Number of enabled Bootstrap endpoints in local configuration.
    pub enabled_bootstrap_count: usize,
    /// Number of currently selected healthy/backed-off outbound peers.
    pub active_outbound_peer_count: usize,
    /// True when no viable Bootstrap remains and automatic network discovery is limited.
    pub degraded: bool,
    /// Stable reason code when `degraded` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    /// Operator-facing summary (no private evidence).
    pub message: String,
}

/// Outbound request for a peer-advertisement sample (pagination-free public only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DiscoveryPeerSampleRequest {
    /// Optional sample size bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}
