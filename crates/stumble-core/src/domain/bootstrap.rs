use super::*;

/// Stable identity of one configured Bootstrap endpoint on a Home Node.
pub type BootstrapEndpointId = Uuid;

/// Sponsored Bootstrap base URL inserted into new Home Node config as an ordinary
/// removable default. Not a protocol constant and not an authority for Pods.
pub const DEFAULT_SPONSORED_BOOTSTRAP_URL: &str = "https://bootstrap.stumble.network";

/// User-controlled Bootstrap endpoint configuration entry.
///
/// Ordered list entries are ordinary config: addable, disableable, and removable.
/// The sponsored default is one such entry, never a network-wide singleton.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BootstrapEndpointConfig {
    /// Stable local identity of this endpoint entry.
    pub id: BootstrapEndpointId,
    /// Human-readable local label.
    pub label: String,
    /// HTTPS base address used for outbound Announcement Stream fetches.
    pub base_url: String,
    /// When false, the endpoint is skipped during sync and no longer provides
    /// eligibility provenance for sole-source announcements.
    pub enabled: bool,
    /// Ascending sync order among configured endpoints.
    pub order: u32,
    /// Whether this entry was seeded as the sponsored distribution default.
    pub is_sponsored_default: bool,
    /// Time at which this entry was added to local config.
    pub created_at: DateTime<Utc>,
}

/// Per-Bootstrap synchronization cursor and last-attempt bookkeeping.
///
/// Survives SQLite restart so refresh resumes without replaying history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BootstrapSyncState {
    /// Configured endpoint this progress belongs to.
    pub endpoint_id: BootstrapEndpointId,
    /// Opaque stream cursor last successfully consumed, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Time of the last fully successful page consumption.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<DateTime<Utc>>,
    /// Time of the most recent sync attempt (success or failure).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<DateTime<Utc>>,
    /// Typed failure from the most recent unsuccessful attempt, cleared on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<BootstrapSyncFailure>,
}

/// Typed outbound Bootstrap synchronization failure for operators.
///
/// Contains no Taste Profile, Subscription, feedback, or other private evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BootstrapSyncFailure {
    /// Stable machine-readable failure class.
    pub kind: BootstrapSyncFailureKind,
    /// Human-readable detail safe for operators (no private evidence).
    pub message: String,
}

impl BootstrapSyncFailure {
    /// Builds a typed operator-facing sync failure.
    #[must_use]
    pub fn new(kind: BootstrapSyncFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// Stable classes of outbound Bootstrap sync failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BootstrapSyncFailureKind {
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
}

impl BootstrapSyncFailureKind {
    /// Wire code returned on operator surfaces.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Transport => "transport",
            Self::Protocol => "protocol",
            Self::Malformed => "malformed",
            Self::InvalidSignature => "invalid_signature",
            Self::Validation => "validation",
        }
    }
}

impl std::fmt::Display for BootstrapSyncFailureKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Operator view of one Bootstrap endpoint plus its sync progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BootstrapEndpointStatus {
    /// Configured endpoint.
    pub endpoint: BootstrapEndpointConfig,
    /// Persisted cursor and last-attempt state for this endpoint.
    pub sync: BootstrapSyncState,
}

/// Summary of one outbound Bootstrap synchronization pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BootstrapSyncReport {
    /// Per-endpoint outcomes in configuration order.
    pub outcomes: Vec<BootstrapSyncEndpointOutcome>,
    /// Total announcements newly retained or refreshed this pass.
    pub retained_announcements: usize,
    /// Total withdrawals retained this pass.
    pub retained_withdrawals: usize,
}

/// Outcome of attempting sync against one Bootstrap endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BootstrapSyncEndpointOutcome {
    /// Endpoint identity.
    pub endpoint_id: BootstrapEndpointId,
    /// Endpoint base URL (public network address only).
    pub base_url: String,
    /// Whether this endpoint completed successfully.
    pub ok: bool,
    /// Pages successfully consumed from this endpoint.
    pub pages_fetched: usize,
    /// Announcements retained from this endpoint during the pass.
    pub retained_announcements: usize,
    /// Withdrawals retained from this endpoint during the pass.
    pub retained_withdrawals: usize,
    /// Cursor after this attempt, when advanced or already present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Typed failure when `ok` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<BootstrapSyncFailure>,
}

/// Outbound Announcement Stream fetch request.
///
/// Intentionally carries only cursor pagination fields. Home Nodes must never
/// attach Taste Profile, Subscriptions, feedback, Source Affinity, or
/// interest-derived queries to Bootstrap synchronization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BootstrapStreamRequest {
    /// Opaque resume cursor; absent starts at the beginning of the stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Optional page size bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// One query match returned by a replaceable Index Node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PodAnnouncementSearchResult {
    /// Verified origin-authored announcement.
    pub announcement: PodAnnouncement,
    /// Query relevance computed by this Index Node, not a quality score.
    pub relevance: f32,
    /// Inspectable local reasons for the query match.
    pub reasons: Vec<String>,
}

impl PodAnnouncementSearchResult {
    /// Builds one Index search hit. Relevance is retrieval evidence only.
    #[must_use]
    pub fn new(
        announcement: PodAnnouncement,
        relevance: f32,
        reasons: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            announcement,
            relevance,
            reasons: reasons.into_iter().collect(),
        }
    }
}

/// Non-authoritative Pod Announcement search response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PodAnnouncementSearchResponse {
    /// Normalized query used by this Index Node.
    pub query: String,
    /// Replaceable results backed by verified signed announcements.
    pub results: Vec<PodAnnouncementSearchResult>,
}

impl PodAnnouncementSearchResponse {
    /// Builds a non-authoritative Index search response.
    #[must_use]
    pub fn new(query: impl Into<String>, results: Vec<PodAnnouncementSearchResult>) -> Self {
        Self {
            query: query.into(),
            results,
        }
    }
}

/// Explicit Index search request. Contains only the User-authored query and a
/// bound; never User identity, Taste Profile, or other private evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct IndexSearchRequest {
    /// Explicit query string authored by the User (may be empty for catalog listing).
    pub query: String,
    /// Optional page size bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

impl IndexSearchRequest {
    /// Builds a public-only Index search request (query + optional limit).
    #[must_use]
    pub fn new(query: impl Into<String>, limit: Option<usize>) -> Self {
        Self {
            query: query.into(),
            limit,
        }
    }
}

/// Stable machine-readable Index search failure class.
///
/// Public Index processing requires no User account. Failures do not encode
/// quality, trust, popularity, or personalized authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum IndexSearchFailureKind {
    /// Query or limit fields failed structural validation.
    Malformed,
    /// Query string exceeded the bounded size accepted by Index search.
    QueryTooLarge,
    /// Per-network Index search rate limit was exceeded.
    RateLimited,
    /// Remote Index advertises an incompatible protocol version.
    IncompatibleProtocol,
    /// This node does not currently enable the Index capability.
    IndexDisabled,
    /// Transport failure talking to a remote Index Node.
    Transport,
    /// Remote response could not be decoded as a valid search payload.
    Protocol,
}

impl IndexSearchFailureKind {
    /// Wire code returned on public Index search failure responses.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::QueryTooLarge => "query_too_large",
            Self::RateLimited => "rate_limited",
            Self::IncompatibleProtocol => "incompatible_protocol",
            Self::IndexDisabled => "index_disabled",
            Self::Transport => "transport",
            Self::Protocol => "protocol",
        }
    }
}

impl std::fmt::Display for IndexSearchFailureKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Bounded typed Index search failure for Home Node and Index operators.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(deny_unknown_fields)]
#[error("index search failed ({kind}): {message}")]
#[non_exhaustive]
pub struct IndexSearchFailure {
    /// Stable machine-readable failure class.
    pub kind: IndexSearchFailureKind,
    /// Human-readable operator detail (never contains User identifiers).
    pub message: String,
}

impl IndexSearchFailure {
    /// Builds a typed failure with a stable kind and message.
    #[must_use]
    pub fn new(kind: IndexSearchFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// Rate-limit bookkeeping for public Index search.
///
/// Persists only short-lived request timestamps—never query text, User ids, or
/// product analytics. Survives restart so abuse bounds remain continuous.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[non_exhaustive]
pub struct IndexRuntimeState {
    /// Timestamps of recent search attempts used for per-network rate limits.
    pub recent_search_attempts: Vec<DateTime<Utc>>,
}

impl Default for IndexRuntimeState {
    fn default() -> Self {
        Self {
            recent_search_attempts: Vec::new(),
        }
    }
}

/// Summary of importing explicit Explore results from configured Index Nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct IndexExploreImportReport {
    /// Explicit query string that was sent (never inferred interests).
    pub query: String,
    /// Per-Index outcomes in Trust Policy configuration order.
    pub outcomes: Vec<IndexExploreImportOutcome>,
    /// Total announcements retained after local verification.
    pub retained_announcements: usize,
}

/// Outcome of querying one configured Index Node during explicit Explore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct IndexExploreImportOutcome {
    /// Configured Index base URL.
    pub index_base_url: String,
    /// Whether the Index returned a usable response that was applied.
    pub ok: bool,
    /// Number of search hits returned before local verification.
    pub result_count: usize,
    /// Number of announcements retained after local verification.
    pub retained: usize,
    /// Typed failure when the Index could not be used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<IndexSearchFailure>,
}

/// Stable machine-readable reason a Bootstrap Node rejected open admission.
///
/// Reasons are inspectable by Origins and operators. They do not encode quality,
/// trust, rank, or personalized policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BootstrapAdmissionRejectionReason {
    /// Payload failed structural or canonical-field validation.
    Malformed,
    /// Origin identity fields are inconsistent or unusable.
    InvalidIdentity,
    /// Signature verification failed.
    InvalidSignature,
    /// Canonical Origin endpoint was not reachable (transport/DNS).
    UnreachableOrigin,
    /// Origin responded but did not yield a usable public manifest.
    ManifestUnavailable,
    /// Advertised protocol is not compatible with this Bootstrap Node.
    IncompatibleProtocol,
    /// Announcement Lease is expired or otherwise not current.
    StaleLease,
    /// A covering Pod Withdrawal already ends discovery for this Pod.
    AnnouncementWithdrawn,
    /// Per-network or per-Origin admission rate limit was exceeded.
    RateLimited,
    /// Signed payload exceeded the bounded size accepted by open admission.
    PayloadTooLarge,
    /// Reachable manifest does not match the signed announcement.
    ManifestMismatch,
    /// Origin already holds the maximum number of active admitted Pods.
    OriginQuotaExceeded,
    /// This node does not currently enable Bootstrap admission.
    BootstrapDisabled,
}

impl BootstrapAdmissionRejectionReason {
    /// Wire code returned on public Bootstrap rejection responses.
    #[must_use]
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::InvalidIdentity => "invalid_identity",
            Self::InvalidSignature => "invalid_signature",
            Self::UnreachableOrigin => "unreachable_origin",
            Self::ManifestUnavailable => "manifest_unavailable",
            Self::IncompatibleProtocol => "incompatible_protocol",
            Self::StaleLease => "stale_lease",
            Self::AnnouncementWithdrawn => "announcement_withdrawn",
            Self::RateLimited => "rate_limited",
            Self::PayloadTooLarge => "payload_too_large",
            Self::ManifestMismatch => "manifest_mismatch",
            Self::OriginQuotaExceeded => "origin_quota_exceeded",
            Self::BootstrapDisabled => "bootstrap_disabled",
        }
    }
}

impl std::fmt::Display for BootstrapAdmissionRejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_code())
    }
}

/// Lifecycle kind recorded in a topic-neutral Announcement Stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AnnouncementStreamEventKind {
    /// Newly admitted public Pod Announcement.
    Admitted,
    /// Lease or public-metadata renewal of an already admitted Pod.
    Renewed,
    /// Origin-signed withdrawal ending new discovery for the Pod.
    Withdrawn,
    /// Lease expiry transition emitted by the Bootstrap/Index node.
    Expired,
}

/// Typed public artifact carried by an Announcement Stream entry.
///
/// Exactly one variant is present; admission and withdrawal are not dual
/// optional fields on the same record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AnnouncementStreamPayload {
    /// Origin-signed announcement for admitted, renewed, or expired transitions.
    Announcement(PodAnnouncement),
    /// Origin-signed withdrawal for withdrawn transitions.
    Withdrawal(PodWithdrawal),
}

impl AnnouncementStreamPayload {
    /// Returns the announcement body when this payload carries one.
    #[must_use]
    pub fn as_announcement(&self) -> Option<&PodAnnouncement> {
        match self {
            Self::Announcement(announcement) => Some(announcement),
            Self::Withdrawal(_) => None,
        }
    }

    /// Returns the withdrawal body when this payload carries one.
    #[must_use]
    pub fn as_withdrawal(&self) -> Option<&PodWithdrawal> {
        match self {
            Self::Withdrawal(withdrawal) => Some(withdrawal),
            Self::Announcement(_) => None,
        }
    }
}

/// One ordered lifecycle record in an Announcement Stream.
///
/// Entries carry only public Origin-signed discovery artifacts. They never
/// include Taste Profiles, Subscriptions, feedback, or personalized ranking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct AnnouncementStreamEntry {
    /// Monotonic stream position used as a resume cursor.
    pub sequence: u64,
    /// Time at which this Bootstrap/Index node recorded the transition.
    pub recorded_at: DateTime<Utc>,
    /// Lifecycle transition kind.
    pub kind: AnnouncementStreamEventKind,
    /// Origin Node of the affected public Pod.
    pub origin_node_id: NodeIdentityId,
    /// Public Pod slug affected by the transition.
    pub pod_slug: String,
    /// Typed Origin-signed artifact for this lifecycle transition.
    pub payload: AnnouncementStreamPayload,
}

impl AnnouncementStreamEntry {
    /// Builds one stream lifecycle entry.
    #[must_use]
    pub fn new(
        sequence: u64,
        recorded_at: DateTime<Utc>,
        kind: AnnouncementStreamEventKind,
        origin_node_id: NodeIdentityId,
        pod_slug: impl Into<String>,
        payload: AnnouncementStreamPayload,
    ) -> Self {
        Self {
            sequence,
            recorded_at,
            kind,
            origin_node_id,
            pod_slug: pod_slug.into(),
            payload,
        }
    }
}

/// Cursor-paginated page of a topic-neutral Announcement Stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct AnnouncementStreamPage {
    /// Stream entries strictly after the request cursor, in sequence order.
    pub entries: Vec<AnnouncementStreamEntry>,
    /// Opaque cursor to resume after the last returned entry, if more may exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Effective page bound applied by the server.
    pub limit: usize,
}

impl AnnouncementStreamPage {
    /// Builds one stream page for tests and scripted transports.
    #[must_use]
    pub fn new(
        entries: Vec<AnnouncementStreamEntry>,
        next_cursor: Option<String>,
        limit: usize,
    ) -> Self {
        Self {
            entries,
            next_cursor,
            limit,
        }
    }
}

/// Successful open Bootstrap admission outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BootstrapAdmissionAcceptance {
    /// Whether admission was newly applied or an idempotent replay.
    pub outcome: BootstrapAdmissionOutcomeKind,
    /// Verified announcement retained by this Bootstrap Node.
    pub known: KnownPodAnnouncement,
    /// Stream sequence assigned when a lifecycle entry was appended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_sequence: Option<u64>,
}

/// Distinguishes first-time admission, renewals, and pure idempotent replays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BootstrapAdmissionOutcomeKind {
    /// First acceptance of this public Pod into Bootstrap state.
    Admitted,
    /// Preferable renewal of an already admitted public Pod.
    Renewed,
    /// Canonically identical submission already present; no new stream effect.
    Idempotent,
}

/// Distinguishes first-time withdrawal admission from pure idempotent replays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BootstrapWithdrawalOutcomeKind {
    /// Withdrawal newly applied to Bootstrap-admitted state.
    Admitted,
    /// Canonically identical withdrawal already present; no new stream effect.
    Idempotent,
}

/// Minimal operator audit record for a rejected open admission attempt.
///
/// Contains no User identifiers, Taste Profile data, or product analytics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BootstrapRejectionAudit {
    /// Stable audit identity.
    pub id: Uuid,
    /// Origin Node when identity could be parsed from the submission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_node_id: Option<NodeIdentityId>,
    /// Origin public key when present on the submission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_public_key: Option<String>,
    /// Announced Pod slug when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod_slug: Option<String>,
    /// Stable machine-readable rejection reason.
    pub reason: BootstrapAdmissionRejectionReason,
    /// Time at which rejection was recorded.
    pub rejected_at: DateTime<Utc>,
}

/// Origin Pod identity key used by Bootstrap-admitted bookkeeping.
pub type BootstrapAdmittedKey = (NodeIdentityId, String);

/// Rate-limit and stream bookkeeping for Bootstrap open admission.
///
/// Persisted so limits and cursors survive process restart. Only keys in
/// [`Self::admitted_keys`] are treated as Bootstrap-admitted for quota,
/// expiry transitions, and stream lifecycle effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[non_exhaustive]
pub struct BootstrapRuntimeState {
    /// Next monotonic Announcement Stream sequence number (starts at 1).
    pub next_stream_sequence: u64,
    /// Timestamps of recent admission attempts used for per-network limits.
    pub recent_network_admissions: Vec<DateTime<Utc>>,
    /// Recent per-Origin admission attempt timestamps.
    pub recent_origin_admissions: BTreeMap<NodeIdentityId, Vec<DateTime<Utc>>>,
    /// Public Pod keys admitted through open Bootstrap (not all known announcements).
    ///
    /// Expiry and withdrawal are terminal for this set: keys leave after one
    /// lifecycle effect so the set remains bounded to currently active admissions.
    pub admitted_keys: BTreeSet<BootstrapAdmittedKey>,
    /// Timestamps of recent Discovery Peer advertisement admissions (network-wide).
    #[serde(default)]
    pub recent_peer_admissions: Vec<DateTime<Utc>>,
    /// Recent per-node Discovery Peer advertisement admission timestamps.
    #[serde(default)]
    pub recent_peer_admissions_by_node: BTreeMap<NodeIdentityId, Vec<DateTime<Utc>>>,
}

impl Default for BootstrapRuntimeState {
    fn default() -> Self {
        Self {
            // Sequence positions are 1-based so an empty cursor (`after = 0`)
            // resumes from the beginning without skipping the first entry.
            next_stream_sequence: 1,
            recent_network_admissions: Vec::new(),
            recent_origin_admissions: BTreeMap::new(),
            admitted_keys: BTreeSet::new(),
            recent_peer_admissions: Vec::new(),
            recent_peer_admissions_by_node: BTreeMap::new(),
        }
    }
}

/// Successful open Bootstrap withdrawal admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BootstrapWithdrawalAcceptance {
    /// Whether withdrawal was newly applied or an idempotent replay.
    pub outcome: BootstrapWithdrawalOutcomeKind,
    /// Verified withdrawal retained by this Bootstrap Node.
    pub known: KnownPodWithdrawal,
    /// Stream sequence assigned when a lifecycle entry was appended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_sequence: Option<u64>,
}
