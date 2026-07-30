use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Federation and adapter contract understood by this first-release node.
pub const CURRENT_PROTOCOL_VERSION: &str = "stumble/1.0";

/// Machine-readable error returned when a pre-release contract is invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error(
    r#"{{"code":"legacy_contract_retired","contract":"{contract}","protocol_version":"{protocol_version}","replacement":"{replacement}"}}"#
)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct LegacyContractError {
    /// Stable compatibility error code.
    pub code: &'static str,
    /// Retired operation or contract family.
    pub contract: &'static str,
    /// Protocol version returning the error.
    pub protocol_version: &'static str,
    /// Canonical first-release operation replacing the retired contract.
    pub replacement: &'static str,
}

/// Retired pre-release contract families with canonical replacements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LegacyContract {
    /// Direct link submission bypassing Candidate review.
    LegacySubmission,
    /// Node-owned crawling or dedicated source connectors.
    CrawlerSourceConnector,
    /// In-node discovery, Stumble, or brief generation.
    LegacyFeedPresentation,
    /// Link-oriented feedback operations.
    LegacyFeedback,
    /// Pre-Pod-Package skill-pack naming.
    LegacySkillPack,
    /// Legacy development API-token revocation.
    LegacyApiToken,
    /// Peer-wide synchronization without a Pod scope.
    LegacyPeerSync,
}

impl LegacyContract {
    /// Returns the transport-neutral compatibility error for this contract.
    #[must_use]
    pub const fn error(self) -> LegacyContractError {
        let (contract, replacement) = match self {
            Self::LegacySubmission => ("legacy_submission", "submit_candidate"),
            Self::CrawlerSourceConnector => (
                "crawler_source_connector",
                "discovery_tasks+submit_candidate",
            ),
            Self::LegacyFeedPresentation => ("legacy_feed_presentation", "get_feed_batch"),
            Self::LegacyFeedback => ("legacy_feedback", "record_feed_feedback"),
            Self::LegacySkillPack => ("legacy_skill_pack", "pod_package"),
            Self::LegacyApiToken => ("legacy_api_token", "revoke_agent_harness"),
            Self::LegacyPeerSync => ("legacy_peer_sync", "sync_pod"),
        };
        LegacyContractError::new(contract, replacement)
    }
}

impl LegacyContractError {
    /// Creates a versioned compatibility error for a retired contract.
    #[must_use]
    pub const fn new(contract: &'static str, replacement: &'static str) -> Self {
        Self {
            code: "legacy_contract_retired",
            contract,
            protocol_version: CURRENT_PROTOCOL_VERSION,
            replacement,
        }
    }
}

pub type TenantId = Uuid;
pub type UserId = Uuid;
pub type PodId = Uuid;
pub type SubmissionId = Uuid;
pub type PeerId = Uuid;
pub type NodeIdentityId = Uuid;

/// Stable local identity of a User's [`Subscription`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubscriptionId(Uuid);

impl From<Uuid> for SubscriptionId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for SubscriptionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for SubscriptionId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

/// Stable canonical identity of a [`ContentItem`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentItemId(Uuid);

impl From<Uuid> for ContentItemId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<ContentItemId> for Uuid {
    fn from(value: ContentItemId) -> Self {
        value.0
    }
}

impl std::fmt::Display for ContentItemId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for ContentItemId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

/// Stable local identity of a private [`Candidate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CandidateId(Uuid);

impl From<Uuid> for CandidateId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for CandidateId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for CandidateId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

/// Stable local identity of one provenance-bearing Candidate Submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CandidateSubmissionId(Uuid);

impl From<Uuid> for CandidateSubmissionId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for CandidateSubmissionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for CandidateSubmissionId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}
/// Stable local identity of a [`DiscoveryTask`].
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct DiscoveryTaskId(Uuid);

impl From<Uuid> for DiscoveryTaskId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

/// Stable local identity of an immutable private Discovery Plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DiscoveryPlanId(Uuid);

impl From<Uuid> for DiscoveryPlanId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for DiscoveryPlanId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for DiscoveryPlanId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

impl std::fmt::Display for DiscoveryTaskId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for DiscoveryTaskId {
    type Err = uuid::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

/// Stable local identity of a private Discovery Result Batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DiscoveryResultBatchId(Uuid);

impl From<Uuid> for DiscoveryResultBatchId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for DiscoveryResultBatchId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for DiscoveryResultBatchId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

/// Stable local identity of a private Personal Discovery schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PersonalDiscoveryScheduleId(Uuid);

impl From<Uuid> for PersonalDiscoveryScheduleId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for PersonalDiscoveryScheduleId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for PersonalDiscoveryScheduleId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

/// Positive, bounded duration of a Discovery Task lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct DiscoveryLeaseSeconds(std::num::NonZeroU32);

impl DiscoveryLeaseSeconds {
    /// Maximum supported lease duration: seven days.
    pub const MAX: u32 = 604_800;

    /// Parses a lease duration of at most seven days.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is zero, exceeds seven days, or does not fit `u32`.
    pub fn new(value: u64) -> Result<Self, DiscoveryLeaseSecondsError> {
        let value = u32::try_from(value).map_err(|_| DiscoveryLeaseSecondsError(value))?;
        let value = std::num::NonZeroU32::new(value).ok_or(DiscoveryLeaseSecondsError(0))?;
        if value.get() > Self::MAX {
            return Err(DiscoveryLeaseSecondsError(u64::from(value.get())));
        }
        Ok(Self(value))
    }

    pub(crate) fn as_duration(self) -> chrono::Duration {
        chrono::Duration::seconds(i64::from(self.0.get()))
    }
}

impl<'de> Deserialize<'de> for DiscoveryLeaseSeconds {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(u64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for DiscoveryLeaseSeconds {
    type Err = DiscoveryLeaseSecondsError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<u64>()
            .map_err(|_| DiscoveryLeaseSecondsError(0))
            .and_then(Self::new)
    }
}

/// Error returned for a zero or overlong Discovery Task lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("lease duration must be between 1 and 604800 seconds, got {0}")]
pub struct DiscoveryLeaseSecondsError(u64);

/// Positive, immutable version number of a Pod Package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PackageVersion(i32);

impl<'de> Deserialize<'de> for PackageVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = i32::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl PackageVersion {
    /// Creates a positive Package Version.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is less than one.
    pub fn new(value: i32) -> Result<Self, PackageVersionError> {
        if value < 1 {
            return Err(PackageVersionError(value));
        }
        Ok(Self(value))
    }

    /// Returns the wire-compatible integer value.
    #[must_use]
    pub const fn value(self) -> i32 {
        self.0
    }
}

impl TryFrom<i32> for PackageVersion {
    type Error = PackageVersionError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Error returned for a non-positive Package Version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Package Version must be positive, got {0}")]
pub struct PackageVersionError(i32);
/// Stable local identity of an [`AgentHarness`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentHarnessId(Uuid);

impl From<Uuid> for AgentHarnessId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for AgentHarnessId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for AgentHarnessId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

/// Stable local identity of a sensitive-change proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PendingProposalId(Uuid);

impl From<Uuid> for PendingProposalId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for PendingProposalId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for PendingProposalId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMode {
    Local,
    Hosted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    InviteOnly,
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantRole {
    Owner,
    Admin,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PodRole {
    Owner,
    Curator,
}

/// Pod workflow actions allowed by current relationship, capability, and scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PodAllowedAction {
    VisibilitySet,
    Subscribe,
    Unsubscribe,
    SubscriptionSet,
    RoleList,
    RoleGrant,
    RoleRevoke,
}

/// Package material selected for a new Pod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PodCreationPackage {
    Default,
    Initial {
        package: PodPackageContents,
    },
    /// An immutable source package snapshot retained with its identity.
    Derived {
        source_package: PodSkillPack,
    },
}

/// Complete request for atomically creating a Pod and its first package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatePodLifecycleRequest {
    pub pod: CreatePodRequest,
    pub package: PodCreationPackage,
}

/// Outcome of a visibility transition under the sensitive-change policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "result", rename_all = "snake_case")]
pub enum PodVisibilityOutcome {
    Updated(Pod),
    PendingApproval(Box<PendingProposal>),
}

/// Outcome of a Trust Policy change under the sensitive-change policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "result", rename_all = "snake_case")]
pub enum TrustPolicyChangeOutcome {
    Applied(Box<TrustPolicy>),
    PendingApproval(Box<PendingProposal>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    ReadOnly,
    ReadWrite,
}

/// One public Pod identity excluded by a User's local Trust Policy.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BlockedPod {
    /// Origin whose Pod is excluded.
    pub origin_node_id: NodeIdentityId,
    /// Origin-local public Pod slug.
    pub pod_slug: String,
}

impl BlockedPod {
    /// Creates one local public Pod exclusion.
    #[must_use]
    pub fn new(origin_node_id: NodeIdentityId, pod_slug: impl Into<String>) -> Self {
        Self {
            origin_node_id,
            pod_slug: pod_slug.into(),
        }
    }
}

/// Replaceable optional Index Node selected by a User.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct IndexNode {
    /// Human-readable local label.
    pub label: String,
    /// Base address used for outbound announcement search.
    pub base_url: String,
}

/// User-controlled local rules governing public Pod discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct TrustPolicy {
    /// User whose discovery behavior this policy controls.
    pub user_id: UserId,
    /// Optional hosted tenant boundary.
    pub tenant_id: Option<TenantId>,
    /// Optional and replaceable announcement indexes.
    pub index_nodes: Vec<IndexNode>,
    /// Public Pods excluded from Explore.
    pub blocked_pods: std::collections::BTreeSet<BlockedPod>,
    /// Origin Nodes excluded from Explore.
    pub blocked_nodes: std::collections::BTreeSet<NodeIdentityId>,
    /// Content sources excluded from Explore samples.
    pub blocked_sources: std::collections::BTreeSet<String>,
    /// Topics excluded from Pod subjects and Explore samples.
    pub blocked_topics: std::collections::BTreeSet<String>,
}

impl TrustPolicy {
    /// Creates an empty local Trust Policy for one User.
    #[must_use]
    pub const fn new(user_id: UserId, tenant_id: Option<TenantId>) -> Self {
        Self {
            user_id,
            tenant_id,
            index_nodes: Vec::new(),
            blocked_pods: std::collections::BTreeSet::new(),
            blocked_nodes: std::collections::BTreeSet::new(),
            blocked_sources: std::collections::BTreeSet::new(),
            blocked_topics: std::collections::BTreeSet::new(),
        }
    }

    /// Whether announcements received only from this Index base URL remain eligible.
    #[must_use]
    pub fn retains_index_url(&self, source: &str) -> bool {
        self.index_nodes
            .iter()
            .any(|index| index.base_url == source)
    }

    /// Whether a public Pod Announcement is excluded by node, pod, or topic blocks.
    ///
    /// Topic matching lowercases announcement text and checks `contains` against the
    /// stored blocked topic string (Explore semantics — policy topics are not re-cased).
    #[must_use]
    pub fn blocks_announcement(&self, announcement: &PodAnnouncement) -> bool {
        self.blocked_nodes.contains(&announcement.origin_node_id)
            || self.blocked_pods.iter().any(|blocked| {
                blocked.origin_node_id == announcement.origin_node_id
                    && blocked
                        .pod_slug
                        .eq_ignore_ascii_case(&announcement.pod_slug)
            })
            || self.blocked_topics.iter().any(|topic| {
                announcement.subject.to_lowercase().contains(topic)
                    || announcement.pod_name.to_lowercase().contains(topic)
                    || announcement.pod_slug.to_lowercase().contains(topic)
            })
    }

    /// Whether a public Pod is excluded by node or pod blocks.
    #[must_use]
    pub fn blocks_pod(&self, origin_node_id: NodeIdentityId, pod_slug: &str) -> bool {
        self.blocked_nodes.contains(&origin_node_id)
            || self.blocked_pods.iter().any(|blocked| {
                blocked.origin_node_id == origin_node_id
                    && blocked.pod_slug.eq_ignore_ascii_case(pod_slug)
            })
    }

    /// Whether a Content Reference sample is excluded by source or topic blocks.
    ///
    /// Topic matching lowercases title/summary and checks `contains` against the
    /// stored blocked topic string (Explore semantics — policy topics are not re-cased).
    #[must_use]
    pub fn blocks_content_reference(&self, reference: &FeedContentReference) -> bool {
        self.blocks_source_and_topics(
            &reference.source,
            &reference.tags,
            &reference.title,
            reference.summary.as_deref(),
        )
    }

    /// Whether a source domain and topic-bearing fields are excluded by Trust Policy.
    #[must_use]
    pub fn blocks_source_and_topics(
        &self,
        source: &str,
        tags: &[String],
        title: &str,
        summary: Option<&str>,
    ) -> bool {
        self.blocked_sources
            .iter()
            .any(|blocked| blocked.eq_ignore_ascii_case(source))
            || self.blocked_topics.iter().any(|topic| {
                tags.iter().any(|tag| tag.eq_ignore_ascii_case(topic))
                    || title.to_lowercase().contains(topic)
                    || summary.is_some_and(|summary| summary.to_lowercase().contains(topic))
            })
    }
}

/// Shared discovery tokenization used by Explore routing and Personal Discovery matching.
///
/// Splits on non-alphanumeric characters, drops tokens of length ≤ 3 and a fixed stop
/// list, preserves input case, and caps output at 80 tokens.
#[must_use]
pub(crate) fn discovery_tokens(text: &str) -> Vec<String> {
    let stop = [
        "the",
        "and",
        "for",
        "with",
        "pod",
        "this",
        "that",
        "from",
        "into",
        "links",
        "link",
        "discovery",
        "personal",
        "public",
        "private",
        "use",
        "when",
        "brief",
        "style",
        "good",
        "bad",
        "stuff",
        "weird",
    ];
    let mut out = Vec::new();
    for token in text
        .split(|c: char| !c.is_alphanumeric())
        .map(str::trim)
        .filter(|token| token.len() > 3)
    {
        if !stop.contains(&token) && !out.iter().any(|existing| existing == token) {
            out.push(token.to_string());
        }
        if out.len() >= 80 {
            break;
        }
    }
    out
}

/// Sensitive local Trust Policy edit requiring independent approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TrustPolicyChange {
    /// Add a replaceable Index Node used for outbound discovery queries.
    AddIndexNode {
        /// Local operator label.
        label: String,
        /// HTTPS base address, with loopback HTTP allowed for local operation.
        base_url: String,
    },
    /// Remove one Index Node and stop considering results received only from it.
    RemoveIndexNode {
        /// Configured Index Node base address.
        base_url: String,
    },
    /// Exclude one public Pod from local discovery.
    BlockPod {
        /// Origin hosting the excluded Pod.
        origin_node_id: NodeIdentityId,
        /// Origin-local Pod slug.
        pod_slug: String,
    },
    /// Exclude every announcement from one Origin Node.
    BlockNode {
        /// Excluded Origin identity.
        node_id: NodeIdentityId,
    },
    /// Exclude Content Reference samples from one source domain.
    BlockSource {
        /// Case-insensitive source domain.
        source: String,
    },
    /// Exclude matching Pod subjects and Content Reference samples.
    BlockTopic {
        /// Case-insensitive topic phrase.
        topic: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMode {
    #[default]
    DeepMatch,
    Adjacent,
    OldGem,
    HumanPick,
    RabbitHole,
    Stumble,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrawlerSourceType {
    Rss,
    Atom,
    Sitemap,
    Webpage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackKind {
    #[serde(alias = "more_like_this")]
    Interesting,
    #[serde(alias = "less_like_this")]
    NotForMe,
    #[serde(alias = "dismiss")]
    Dismissed,
    #[serde(alias = "save")]
    Saved,
    BlockSource,
    BlockTopic,
}

impl std::str::FromStr for FeedbackKind {
    type Err = FeedbackKindParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.replace('-', "_").as_str() {
            "interesting" | "more_like_this" => Ok(Self::Interesting),
            "not_for_me" | "less_like_this" => Ok(Self::NotForMe),
            "dismissed" | "dismiss" => Ok(Self::Dismissed),
            "saved" | "save" => Ok(Self::Saved),
            "block_source" => Ok(Self::BlockSource),
            "block_topic" => Ok(Self::BlockTopic),
            _ => Err(FeedbackKindParseError(value.to_string())),
        }
    }
}

/// Error returned for an unknown Feedback Signal name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown Feedback Signal: {0}")]
pub struct FeedbackKindParseError(String);

/// Lifecycle of a finite Feed Batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FeedBatchState {
    /// One or more items are ready for this consumption session.
    Ready,
    /// No eligible item remains for the requested recurrence window.
    CaughtUp,
}

/// Permission-derived action an Agent Harness may offer for a Feed item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FeedAllowedAction {
    /// Preserve the item in the User's local saved set.
    Save,
    /// Record positive explicit feedback.
    MoreLikeThis,
    /// Record negative explicit feedback and suppress automatic resurfacing.
    LessLikeThis,
    /// Remove this item from automatic future delivery.
    Dismiss,
    /// Exclude this source from future delivery.
    BlockSource,
    /// Exclude this item's topics from future delivery.
    BlockTopic,
    /// Create an Accepted Placement in an authorized local Pod.
    AddToPod,
}

/// Private feedback already recorded for one Feed item.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedFeedbackState {
    /// Whether the User saved this item.
    pub saved: bool,
    /// Whether the User requested more like this item.
    pub more_like_this: bool,
    /// Whether the User requested less like this item.
    pub less_like_this: bool,
    /// Whether the User dismissed this item.
    pub dismissed: bool,
    /// Whether the User blocked this item's source.
    pub source_blocked: bool,
    /// Whether the User blocked one or more of this item's topics.
    pub topic_blocked: bool,
}

/// Configurable request for a finite Feed Batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct FeedBatchRequest {
    /// Maximum number of Content Items in the finite batch.
    #[serde(default = "default_feed_batch_size")]
    pub size: usize,
    /// Optional per-request recurrence override. Omission uses the User's
    /// explicit Taste Profile preference, which defaults to thirty days.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence_penalty_days: Option<RecurrencePenaltyDays>,
    /// Composition constraints for this Feed Batch.
    #[serde(default)]
    pub feed_mix: FeedMix,
    /// Temporary focus and avoidance instructions for this Feed Batch only.
    #[serde(default)]
    pub batch_intent: BatchIntent,
}

const fn default_feed_batch_size() -> usize {
    7
}

const fn default_recurrence_penalty_days() -> RecurrencePenaltyDays {
    RecurrencePenaltyDays(30)
}

impl FeedBatchRequest {
    /// Creates a request using the User's Taste Profile recurrence preference,
    /// which defaults to thirty days.
    ///
    /// # Errors
    ///
    /// Returns an error when `size` is zero or greater than 100.
    pub fn new(size: usize) -> Result<Self, FeedBatchRequestError> {
        if !(1..=100).contains(&size) {
            return Err(FeedBatchRequestError);
        }
        Ok(Self {
            size,
            recurrence_penalty_days: None,
            feed_mix: FeedMix::default(),
            batch_intent: BatchIntent::default(),
        })
    }

    /// Sets an exact per-request recurrence override.
    #[must_use]
    pub const fn with_recurrence_penalty_days(mut self, days: RecurrencePenaltyDays) -> Self {
        self.recurrence_penalty_days = Some(days);
        self
    }

    /// Replaces the composition constraints for this request.
    #[must_use]
    pub fn with_feed_mix(mut self, feed_mix: FeedMix) -> Self {
        self.feed_mix = feed_mix;
        self
    }

    /// Adds temporary focus and avoidance instructions to this request.
    #[must_use]
    pub fn with_batch_intent(mut self, batch_intent: BatchIntent) -> Self {
        self.batch_intent = batch_intent;
        self
    }
}

/// Percentage from zero through one hundred used by Feed Mix targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct FeedPercentage(u8);

#[derive(Deserialize)]
#[serde(untagged)]
enum RawFeedPercentage {
    Number(u8),
    String(String),
}

impl<'de> Deserialize<'de> for FeedPercentage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match RawFeedPercentage::deserialize(deserializer)? {
            RawFeedPercentage::Number(value) => Self::new(value),
            RawFeedPercentage::String(value) => value.parse(),
        }
        .map_err(serde::de::Error::custom)
    }
}

impl FeedPercentage {
    /// Parses a percentage from zero through one hundred.
    ///
    /// # Errors
    ///
    /// Returns [`FeedMixError::Percentage`] when `value` exceeds one hundred.
    pub const fn new(value: u8) -> Result<Self, FeedMixError> {
        if value > 100 {
            Err(FeedMixError::Percentage(value))
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the validated primitive percentage.
    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for FeedPercentage {
    type Error = FeedMixError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl std::str::FromStr for FeedPercentage {
    type Err = FeedMixError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<u8>()
            .map_err(|_| FeedMixError::PercentageParse(value.into()))?
            .try_into()
    }
}

/// Positive maximum contribution attributed to one Pod or source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct FeedCap(std::num::NonZeroUsize);

#[derive(Deserialize)]
#[serde(untagged)]
enum RawFeedCap {
    Number(usize),
    String(String),
}

impl<'de> Deserialize<'de> for FeedCap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match RawFeedCap::deserialize(deserializer)? {
            RawFeedCap::Number(value) => Self::new(value),
            RawFeedCap::String(value) => value.parse(),
        }
        .map_err(serde::de::Error::custom)
    }
}

impl FeedCap {
    /// Parses a strictly positive cap.
    ///
    /// # Errors
    ///
    /// Returns [`FeedMixError::ZeroCap`] when `value` is zero.
    pub const fn new(value: usize) -> Result<Self, FeedMixError> {
        match std::num::NonZeroUsize::new(value) {
            Some(value) => Ok(Self(value)),
            None => Err(FeedMixError::ZeroCap),
        }
    }

    /// Returns the validated positive primitive cap.
    #[must_use]
    pub const fn value(self) -> usize {
        self.0.get()
    }
}

impl TryFrom<usize> for FeedCap {
    type Error = FeedMixError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl std::str::FromStr for FeedCap {
    type Err = FeedMixError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<usize>()
            .map_err(|_| FeedMixError::CapParse(value.into()))?
            .try_into()
    }
}

/// Error returned when Feed Mix constraints are not valid domain values.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FeedMixError {
    /// A percentage exceeded one hundred.
    #[error("Feed Mix percentage {0} exceeds 100")]
    Percentage(u8),
    /// A percentage transport value was not an unsigned integer.
    #[error("Feed Mix percentage must be an unsigned integer: {0}")]
    PercentageParse(String),
    /// A cap transport value was not an unsigned integer.
    #[error("Feed Mix cap must be an unsigned integer: {0}")]
    CapParse(String),
    /// A cap was zero.
    #[error("Feed Mix caps must be positive")]
    ZeroCap,
    /// Percentage targets exceeded one complete batch.
    #[error("Feed Mix percentage targets must total at most 100")]
    TargetTotal,
}

/// Configurable constraints used to compose one finite Feed Batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawFeedMix")]
#[non_exhaustive]
pub struct FeedMix {
    /// Target percentage of highest-value subscribed Content Items.
    high_value_percent: FeedPercentage,
    /// Maximum target percentage of Exploration Items when all categories exist.
    exploration_percent: FeedPercentage,
    /// Maximum target percentage of Old Gems when all categories exist.
    old_gem_percent: FeedPercentage,
    /// Maximum selected items attributed to one Pod.
    per_pod_cap: FeedCap,
    /// Maximum selected items from one source.
    per_source_cap: FeedCap,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawFeedMix {
    high_value_percent: u8,
    exploration_percent: u8,
    old_gem_percent: u8,
    per_pod_cap: usize,
    per_source_cap: usize,
}

impl Default for RawFeedMix {
    fn default() -> Self {
        Self {
            high_value_percent: 80,
            exploration_percent: 10,
            old_gem_percent: 10,
            per_pod_cap: 3,
            per_source_cap: 2,
        }
    }
}

impl TryFrom<RawFeedMix> for FeedMix {
    type Error = FeedMixError;

    fn try_from(raw: RawFeedMix) -> Result<Self, Self::Error> {
        Self::new(
            raw.high_value_percent,
            raw.exploration_percent,
            raw.old_gem_percent,
            raw.per_pod_cap,
            raw.per_source_cap,
        )
    }
}

impl Default for FeedMix {
    fn default() -> Self {
        Self {
            high_value_percent: FeedPercentage(80),
            exploration_percent: FeedPercentage(10),
            old_gem_percent: FeedPercentage(10),
            per_pod_cap: FeedCap(
                std::num::NonZeroUsize::new(3).unwrap_or(std::num::NonZeroUsize::MIN),
            ),
            per_source_cap: FeedCap(
                std::num::NonZeroUsize::new(2).unwrap_or(std::num::NonZeroUsize::MIN),
            ),
        }
    }
}

impl FeedMix {
    /// Creates validated Feed Mix constraints.
    ///
    /// # Errors
    ///
    /// Returns an error for percentages above one hundred, zero caps, or targets
    /// whose sum exceeds one complete batch.
    pub fn new(
        high_value_percent: u8,
        exploration_percent: u8,
        old_gem_percent: u8,
        per_pod_cap: usize,
        per_source_cap: usize,
    ) -> Result<Self, FeedMixError> {
        let high_value_percent = FeedPercentage::new(high_value_percent)?;
        let exploration_percent = FeedPercentage::new(exploration_percent)?;
        let old_gem_percent = FeedPercentage::new(old_gem_percent)?;
        if u16::from(high_value_percent.value())
            + u16::from(exploration_percent.value())
            + u16::from(old_gem_percent.value())
            > 100
        {
            return Err(FeedMixError::TargetTotal);
        }
        Ok(Self {
            high_value_percent,
            exploration_percent,
            old_gem_percent,
            per_pod_cap: FeedCap::new(per_pod_cap)?,
            per_source_cap: FeedCap::new(per_source_cap)?,
        })
    }

    /// Replaces the percentage targets used before unavailable-category backfill.
    ///
    /// # Errors
    ///
    /// Returns an error when a percentage or the combined target is invalid.
    pub fn with_targets(
        self,
        high_value_percent: u8,
        exploration_percent: u8,
        old_gem_percent: u8,
    ) -> Result<Self, FeedMixError> {
        Self::new(
            high_value_percent,
            exploration_percent,
            old_gem_percent,
            self.per_pod_cap.value(),
            self.per_source_cap.value(),
        )
    }

    /// Replaces the maximum contribution attributed to one Pod or source.
    ///
    /// # Errors
    ///
    /// Returns an error when either cap is zero.
    pub fn with_caps(
        self,
        per_pod_cap: usize,
        per_source_cap: usize,
    ) -> Result<Self, FeedMixError> {
        Self::new(
            self.high_value_percent.value(),
            self.exploration_percent.value(),
            self.old_gem_percent.value(),
            per_pod_cap,
            per_source_cap,
        )
    }

    /// Returns the highest-value subscribed target.
    #[must_use]
    pub const fn high_value_percent(self) -> FeedPercentage {
        self.high_value_percent
    }

    /// Returns the Exploration Item target.
    #[must_use]
    pub const fn exploration_percent(self) -> FeedPercentage {
        self.exploration_percent
    }

    /// Returns the Old Gem target.
    #[must_use]
    pub const fn old_gem_percent(self) -> FeedPercentage {
        self.old_gem_percent
    }

    /// Returns the per-Pod diversity cap.
    #[must_use]
    pub const fn per_pod_cap(self) -> FeedCap {
        self.per_pod_cap
    }

    /// Returns the per-source diversity cap.
    #[must_use]
    pub const fn per_source_cap(self) -> FeedCap {
        self.per_source_cap
    }
}

/// Optional transport-level overrides resolved against a complete Feed Mix.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct FeedMixOverrides {
    /// Optional highest-value subscribed target override.
    pub high_value_percent: Option<FeedPercentage>,
    /// Optional Exploration Item target override.
    pub exploration_percent: Option<FeedPercentage>,
    /// Optional Old Gem target override.
    pub old_gem_percent: Option<FeedPercentage>,
    /// Optional per-Pod diversity cap override.
    pub per_pod_cap: Option<FeedCap>,
    /// Optional per-source diversity cap override.
    pub per_source_cap: Option<FeedCap>,
}

impl FeedMixOverrides {
    /// Creates a partial Feed Mix override from adapter-provided values.
    #[must_use]
    pub const fn new(
        high_value_percent: Option<FeedPercentage>,
        exploration_percent: Option<FeedPercentage>,
        old_gem_percent: Option<FeedPercentage>,
        per_pod_cap: Option<FeedCap>,
        per_source_cap: Option<FeedCap>,
    ) -> Self {
        Self {
            high_value_percent,
            exploration_percent,
            old_gem_percent,
            per_pod_cap,
            per_source_cap,
        }
    }

    /// Resolves omitted values from `defaults` and validates the resulting mix.
    ///
    /// # Errors
    ///
    /// Returns an error when the combined percentage targets exceed one batch.
    pub fn resolve(self, defaults: FeedMix) -> Result<FeedMix, FeedMixError> {
        FeedMix::new(
            self.high_value_percent
                .unwrap_or(defaults.high_value_percent())
                .value(),
            self.exploration_percent
                .unwrap_or(defaults.exploration_percent())
                .value(),
            self.old_gem_percent
                .unwrap_or(defaults.old_gem_percent())
                .value(),
            self.per_pod_cap.unwrap_or(defaults.per_pod_cap()).value(),
            self.per_source_cap
                .unwrap_or(defaults.per_source_cap())
                .value(),
        )
    }
}

/// Temporary focus and avoidance instructions affecting only one Feed Batch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct BatchIntent {
    /// Topics whose matching Content References receive a temporary boost.
    pub focus_topics: Vec<String>,
    /// Topics excluded from this Feed Batch without changing the Taste Profile.
    pub avoid_topics: Vec<String>,
}

impl BatchIntent {
    /// Creates temporary focus and avoidance instructions for one request.
    #[must_use]
    pub const fn new(focus_topics: Vec<String>, avoid_topics: Vec<String>) -> Self {
        Self {
            focus_topics,
            avoid_topics,
        }
    }
}

/// Composition role under which a Content Item was selected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FeedItemKind {
    /// Highest-value unseen content from a subscribed Pod.
    #[default]
    Subscribed,
    /// Clearly labeled content from an unsubscribed public Pod.
    Exploration,
    /// Previously Delivered content deliberately resurfaced after eligibility returned.
    OldGem,
}

/// Validated recurrence suppression window in days.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct RecurrencePenaltyDays(u32);

impl RecurrencePenaltyDays {
    /// Longest supported recurrence window (100 years).
    pub const MAX: u32 = 36_500;

    /// Parses a bounded recurrence window.
    ///
    /// # Errors
    ///
    /// Returns an error for values greater than [`Self::MAX`].
    pub const fn new(days: u32) -> Result<Self, RecurrencePenaltyDaysError> {
        if days > Self::MAX {
            return Err(RecurrencePenaltyDaysError(days));
        }
        Ok(Self(days))
    }

    /// Returns the validated number of days.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Default for RecurrencePenaltyDays {
    fn default() -> Self {
        default_recurrence_penalty_days()
    }
}

impl std::str::FromStr for RecurrencePenaltyDays {
    type Err = RecurrencePenaltyDaysParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let days = value
            .parse()
            .map_err(RecurrencePenaltyDaysParseError::InvalidInteger)?;
        Self::new(days).map_err(RecurrencePenaltyDaysParseError::OutOfRange)
    }
}

impl<'de> Deserialize<'de> for RecurrencePenaltyDays {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Error returned for an out-of-range recurrence window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("recurrence penalty days {0} exceeds the maximum of 36500")]
pub struct RecurrencePenaltyDaysError(u32);

/// Error returned while parsing recurrence days from a transport string.
#[derive(Debug, thiserror::Error)]
pub enum RecurrencePenaltyDaysParseError {
    /// Input was not an unsigned integer.
    #[error("recurrence penalty days must be an unsigned integer")]
    InvalidInteger(#[source] std::num::ParseIntError),
    /// Parsed input exceeded the supported range.
    #[error(transparent)]
    OutOfRange(#[from] RecurrencePenaltyDaysError),
}

/// Error returned for a Feed Batch size outside the supported range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Feed Batch size must be between 1 and 100")]
pub struct FeedBatchRequestError;

/// Source reference returned without mirroring third-party content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedContentReference {
    /// Stable canonical Content Item identity.
    pub content_item_id: ContentItemId,
    /// Original durable source location.
    pub source_url: String,
    /// Normalized identity used for deduplication.
    pub canonical_url: String,
    /// Permitted source title.
    pub title: String,
    /// Optional permitted source description or excerpt.
    pub permitted_description: Option<String>,
    /// Generated local understanding of the reference.
    pub summary: Option<String>,
    /// Permitted attached-media URL references; no media bytes are retained.
    #[serde(default)]
    pub media_references: Vec<MediaReference>,
    /// Source domain used by source-block feedback.
    pub source: String,
    /// Subject tags used by topic-block feedback.
    pub tags: Vec<String>,
}

/// Evidence explaining the local Attention Value used for initial Feed ordering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedRankingEvidence {
    /// Initial local Attention Value used for ordering.
    pub attention_value: f32,
    /// Human-inspectable reasons supporting selection.
    pub reasons: Vec<String>,
    /// Whether recurrence reduced this item's score in this batch.
    pub recurrence_penalty_applied: bool,
}

/// One canonical Content Item delivered once with all accepted placement evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedBatchItem {
    /// Reference-first representation of the selected item.
    pub content_reference: FeedContentReference,
    /// All Accepted Placements contributing eligibility and context.
    pub placements: Vec<AcceptedPlacementProjection>,
    /// Discovery provenance retained from Candidate Submissions.
    pub provenance: Vec<CandidateProvenance>,
    /// Inspectable evidence for initial Feed ordering.
    pub ranking_evidence: FeedRankingEvidence,
    /// Explicit label for unsubscribed public-Pod exploration.
    pub is_exploration: bool,
    /// Composition role under which this item was selected.
    #[serde(default)]
    pub kind: FeedItemKind,
    /// Current private explicit feedback for this item.
    pub feedback_state: FeedFeedbackState,
    /// Operations allowed by the current Harness Grant.
    pub allowed_actions: Vec<FeedAllowedAction>,
}

/// Stable, finite set of locally ranked Content Items for one consumption session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedBatch {
    /// Stable identity returned by repeated retrieval.
    pub id: Uuid,
    /// User whose private projection owns the batch.
    pub user_id: UserId,
    /// Harness Grant scope under which this stable batch was created.
    #[serde(default)]
    pub harness_id: Option<AgentHarnessId>,
    /// Optional hosted tenant boundary.
    pub tenant_id: Option<TenantId>,
    /// Configured maximum number of items.
    pub requested_size: usize,
    /// Recurrence suppression window used during selection.
    pub recurrence_penalty_days: u32,
    /// Composition constraints used to select this stable batch.
    #[serde(default)]
    pub feed_mix: FeedMix,
    /// Temporary request instructions recorded with this stable batch.
    #[serde(default)]
    pub batch_intent: BatchIntent,
    /// Ready or explicit Caught Up state.
    pub state: FeedBatchState,
    /// Stable finite item sequence.
    pub items: Vec<FeedBatchItem>,
    /// Time at which inclusion marked items Delivered.
    pub created_at: DateTime<Utc>,
    /// Time at which the User deliberately finished this batch.
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionAssetType {
    RepresentativeImage,
    /// Reader-mode text copy of the source page, strictly local (ADR-0052).
    ReadableSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionAssetSource {
    PageImage,
    PageText,
    AiGenerated,
    UserProvided,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    pub id: TenantId,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantUser {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub role: TenantRole,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: Uuid,
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub token_hash: String,
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    /// Harness authenticated by this token; absent only for local owner contexts.
    #[serde(default)]
    pub harness_id: Option<AgentHarnessId>,
}

/// Whether a harness operates with a User present or as unattended automation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHarnessKind {
    /// A User-facing harness that can participate in interactive flows.
    Interactive,
    /// A background harness intended to receive least authority.
    Unattended,
}

impl std::str::FromStr for AgentHarnessKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "interactive" => Ok(Self::Interactive),
            "unattended" => Ok(Self::Unattended),
            _ => Err(format!("unknown Agent Harness kind: {value}")),
        }
    }
}

/// Independently grantable Home Node operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessCapability {
    /// Retrieve finite Feed-oriented content.
    FeedRead,
    /// Record private User feedback and preference changes.
    Feedback,
    /// Manage due discovery work and Source Rules.
    DiscoveryTasks,
    /// Request and inspect private Personal Discovery plans with a User present.
    PersonalDiscoveryManagement,
    /// Execute task-scoped Personal Discovery without reading the Taste Profile.
    PersonalDiscoveryExecution,
    /// Submit discovered Candidates and their assets.
    CandidateSubmission,
    /// Create Pods and change accepted Pod content.
    PodCuration,
    /// Change Pod Packages.
    PackageManagement,
    /// Change the User's Subscriptions.
    SubscriptionManagement,
    /// Manage node-local authority and administration.
    Administration,
    /// Independently approve or reject sensitive changes.
    Approval,
    /// Submit local agent semantic evidence that enriches Pod Similarity only.
    ///
    /// Evidence stays on the Home Node, adjusts inspectable local ordering under
    /// Core policy, and never creates trust, Subscriptions, Accepted Placements,
    /// or Feed eligibility by itself.
    PodSimilarityEvidence,
}

impl std::fmt::Display for HarnessCapability {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FeedRead => "feed_read",
            Self::Feedback => "feedback",
            Self::DiscoveryTasks => "discovery_tasks",
            Self::PersonalDiscoveryManagement => "personal_discovery_management",
            Self::PersonalDiscoveryExecution => "personal_discovery_execution",
            Self::CandidateSubmission => "candidate_submission",
            Self::PodCuration => "pod_curation",
            Self::PackageManagement => "package_management",
            Self::SubscriptionManagement => "subscription_management",
            Self::Administration => "administration",
            Self::Approval => "approval",
            Self::PodSimilarityEvidence => "pod_similarity_evidence",
        })
    }
}

impl std::str::FromStr for HarnessCapability {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.replace('-', "_").as_str() {
            "feed_read" => Ok(Self::FeedRead),
            "feedback" => Ok(Self::Feedback),
            "discovery_tasks" => Ok(Self::DiscoveryTasks),
            "personal_discovery_management" => Ok(Self::PersonalDiscoveryManagement),
            "personal_discovery_execution" => Ok(Self::PersonalDiscoveryExecution),
            "candidate_submission" => Ok(Self::CandidateSubmission),
            "pod_curation" => Ok(Self::PodCuration),
            "package_management" => Ok(Self::PackageManagement),
            "subscription_management" => Ok(Self::SubscriptionManagement),
            "administration" => Ok(Self::Administration),
            "approval" => Ok(Self::Approval),
            "pod_similarity_evidence" => Ok(Self::PodSimilarityEvidence),
            _ => Err(format!("unknown harness capability: {value}")),
        }
    }
}

/// A sensitive change that cannot take effect when it is proposed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SensitiveChange {
    /// Create a new Pod with public exposure from its first accepted state.
    CreatePublicPod { request: CreatePodRequest },
    /// Create a public Pod and its selected first package atomically.
    CreatePublicPodLifecycle { request: CreatePodLifecycleRequest },
    /// Expose an existing private Pod through public federation surfaces.
    PublishPod { pod_id: PodId },
    /// Expand an existing Pod's visibility after approval.
    ExpandPodVisibility {
        pod_id: PodId,
        visibility: Visibility,
    },
    /// Expand the capabilities or Pod scope of an existing Harness Grant.
    ExpandHarnessGrant {
        harness_id: AgentHarnessId,
        capabilities: Vec<HarnessCapability>,
        pod_ids: Option<Vec<PodId>>,
    },
    /// Add a node to the local Trust Policy.
    AddTrustedPeer {
        #[serde(default)]
        node_id: NodeIdentityId,
        display_name: String,
        base_url: String,
        public_key: String,
    },
    /// Disable a peer in the local Trust Policy while retaining its audit state.
    RemoveTrustedPeer {
        /// Locally configured peer identity.
        peer_id: PeerId,
    },
    /// Change public Pod discovery rules local to one User.
    ChangeTrustPolicy {
        /// Validated local policy edit.
        change: TrustPolicyChange,
    },
    /// Apply a validated Package Revision to a public Pod.
    RevisePublicPodPackage {
        pod_id: PodId,
        base_version: PackageVersion,
        patch: SkillPackPatch,
    },
    /// Remove an accepted public Pod association for one Content Item.
    RemovePublicSubmissionFromPod {
        pod_id: PodId,
        submission_id: SubmissionId,
    },
    /// Enable Autonomous Curation for a local Pod.
    EnableAutonomousCuration {
        pod_id: PodId,
        confidence_threshold: CandidateConfidence,
    },
    /// Assign one of the two canonical Pod Roles to a User.
    GrantPodRole {
        pod_id: PodId,
        user_id: UserId,
        role: PodRole,
    },
    /// Remove one canonical Pod Role from a User.
    RevokePodRole {
        pod_id: PodId,
        user_id: UserId,
        role: PodRole,
    },
}

/// Auditable lifecycle state of a [`PendingProposal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    /// Awaiting an independent decision before expiry.
    Pending,
    /// Applied after independent approval.
    Accepted,
    /// Declined without applying the requested change.
    Rejected,
    /// Reached its expiry without applying the requested change.
    Expired,
}

/// Expiring, auditable request for one sensitive authoritative change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingProposal {
    /// Stable local proposal identity.
    pub id: PendingProposalId,
    /// Typed requested authoritative change.
    pub requested_change: SensitiveChange,
    /// Stable resource references affected by the change.
    pub affected_resources: Vec<ProposalResource>,
    /// User-facing consequences expected if approved.
    pub expected_consequences: Vec<String>,
    /// Structured before/after values shown to an approver.
    pub structured_diff: Vec<ProposalResourceDiff>,
    /// Harness that requested the change.
    pub proposer: AgentHarnessId,
    /// User for whom the proposer requested the change.
    pub user_id: UserId,
    /// Hosted tenant boundary of the requested change.
    pub tenant_id: Option<TenantId>,
    /// Time at which the proposal was created.
    pub created_at: DateTime<Utc>,
    /// Time after which the proposal cannot be approved.
    pub expires_at: DateTime<Utc>,
    /// Current auditable lifecycle state.
    pub status: ProposalStatus,
    /// Independent harness that decided the proposal, when applicable.
    pub decided_by: Option<ProposalDecisionActor>,
    /// Decision or expiry time, when terminal.
    pub decided_at: Option<DateTime<Utc>>,
    /// Optional reason supplied when rejecting the proposal.
    pub rejection_reason: Option<String>,
}

/// Identity that independently approved or rejected a Pending Proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProposalDecisionActor {
    /// Backward-compatible Harness identity representation.
    Harness(AgentHarnessId),
    /// Automatically authenticated local Home Node Owner.
    Owner { owner_user_id: UserId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalAllowedAction {
    Approve,
    Reject,
}

/// One affected resource's inspectable state transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalResourceDiff {
    /// Stable resource reference matching an entry in `affected_resources`.
    pub resource: ProposalResource,
    /// Current value, or `null` when the resource does not yet exist.
    pub before: Value,
    /// Requested value if the proposal is accepted.
    pub after: Value,
}

/// Type-safe identity of a resource affected by a proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "identity", rename_all = "snake_case")]
pub enum ProposalResource {
    Pod(PodId),
    PodSlug(String),
    AgentHarness(AgentHarnessId),
    TrustedPeerUrl(String),
    /// Local discovery rules owned by one User.
    TrustPolicy(UserId),
    PodPackage(PodId),
    PodCurationPolicy(PodId),
    PodRoles(PodId),
    SubmissionPlacement {
        pod_id: PodId,
        submission_id: SubmissionId,
    },
}

/// Outcome of requesting Pod creation through the sensitive-change policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "result", rename_all = "snake_case")]
pub enum CreatePodOutcome {
    Created(Pod),
    PendingApproval(Box<PendingProposal>),
}

/// Outcome of requesting a Pod Placement removal through approval policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "result", rename_all = "snake_case")]
pub enum RemoveSubmissionOutcome {
    Removed { submission_purged: bool },
    PendingApproval(Box<PendingProposal>),
}

/// Outcome of removing one canonical Content Item placement from a Pod.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RemoveContentItemOutcome {
    /// A private placement was reversed immediately while retaining the Content Item.
    Removed { placement: Box<PodPlacement> },
    /// A public placement awaits independent approval and a Placement Tombstone.
    PendingApproval { proposal: Box<PendingProposal> },
}

/// Transport-neutral request to create a Pending Proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePendingProposalRequest {
    /// Typed sensitive change to request.
    pub requested_change: SensitiveChange,
    /// Positive lifetime in seconds, capped at seven days by [`AgentTools`](crate::AgentTools).
    pub expires_in_seconds: u64,
}

/// Transport-neutral request to reject a Pending Proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectPendingProposalRequest {
    /// Inspectable reason for declining the change.
    pub reason: String,
}

/// Capabilities and optional Pod boundary assigned to an Agent Harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessGrant {
    /// Operations the harness may perform.
    pub capabilities: Vec<HarnessCapability>,
    /// `None` authorizes all local Pods; an empty list authorizes none.
    pub pod_ids: Option<Vec<PodId>>,
}

/// Registered external environment through which a User operates Stumble.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentHarness {
    /// Local harness identity.
    pub id: AgentHarnessId,
    /// User on whose behalf the harness operates.
    pub user_id: UserId,
    /// Optional hosted tenant boundary.
    pub tenant_id: Option<TenantId>,
    /// Human-readable operator label.
    pub label: String,
    /// Interactive or unattended operating mode.
    pub kind: AgentHarnessKind,
    /// Current least-authority grant.
    pub grant: HarnessGrant,
    /// Registration timestamp.
    pub created_at: DateTime<Utc>,
    /// Revocation timestamp, if revoked.
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Inspectable Agent Harness metadata that never includes credential material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentHarnessView {
    #[serde(flatten)]
    pub harness: AgentHarness,
    /// Stable, non-secret identifier derived from the stored credential hash.
    pub credential_fingerprint: String,
    /// Current lifecycle state, derived from revocation metadata.
    pub status: AgentHarnessStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHarnessStatus {
    Active,
    Revoked,
}

/// Type-safe operation recorded in a harness write audit entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessWriteOperation {
    RegisterAgentHarness,
    RevokeAgentHarness,
    CreateTenant,
    CreateDevToken,
    AddTrustedPeer,
    /// Retain a verified announcement delivered by a trusted peer.
    ReceivePodAnnouncement,
    /// Retain a verified Pod Withdrawal delivered by a trusted peer.
    ReceivePodWithdrawal,
    /// Publish a signed Pod Withdrawal for a formerly public Pod.
    WithdrawPublicPod,
    /// Publish a signed optional recommendation from a public Pod.
    EndorsePublicPod,
    ImportPodEvents,
    CreatePod,
    JoinPod,
    SetPrioritySubscription,
    SubscribePublicPod,
    SynchronizeSubscription,
    SubmitLinkToPod,
    SubmitCandidate,
    SetPodCurationPolicy,
    CurateCandidate,
    ReviewCandidatePlacement,
    RouteCandidatePlacement,
    AddContentItemToPod,
    RemoveSubmissionFromPod,
    AddSubmissionAsset,
    GenerateBrief,
    CreateFeedBatch,
    CompleteFeedBatch,
    PatchSkillPack,
    ImportSkillPack,
    ForkSkillPack,
    AddSourceToPod,
    SaveLink,
    RecordFeedFeedback,
    BlockSource,
    BlockTopic,
    UpdatePreferences,
    ResetLearnedTaste,
    RetractInterestSeed,
    CreateDiscoveryTask,
    RequestPersonalDiscovery,
    CreatePersonalDiscoverySchedule,
    UpdatePersonalDiscoverySchedule,
    DisablePersonalDiscoverySchedule,
    RemovePersonalDiscoverySchedule,
    ClaimDiscoveryTask,
    RenewDiscoveryTaskLease,
    CompleteDiscoveryTask,
    FailDiscoveryTask,
    CompleteDiscoveryResultBatch,
    ReportDiscoverySourceAvailability,
    DismissDiscoveryResultBatch,
    MarkDiscoveryResultBatchReviewed,
    ReviewDiscoveryResultItem,
    AttemptDiscoveryResultsReadyNotification,
    /// Submit local agent semantic evidence for Pod Similarity ranking.
    SubmitPodSimilarityAgentEvidence,
}

/// Current lifecycle state with state-specific lease data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "lease", rename_all = "snake_case")]
pub enum DiscoveryTaskState {
    /// Available to an authorized harness.
    Pending,
    /// Exclusively owned until the embedded lease expires.
    Leased(DiscoveryTaskLease),
    /// Successfully completed and immutable.
    Completed,
    /// Exhausted the permitted attempts.
    TerminalFailure,
}

/// Provenance and instructions that created a Discovery Task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiscoveryTaskOrigin {
    /// Due work derived from one versioned Source Rule.
    Scheduled {
        /// Zero-based position in the Pod Package Source Rules.
        source_rule_index: usize,
    },
    /// Immediate work requested during a conversation.
    Immediate {
        /// Discovery intent that a later claiming harness must follow.
        instructions: String,
        /// Retry-safe key unique to the requesting harness.
        idempotency_key: String,
        /// Harness that supplied the intent.
        requested_by: AgentHarnessId,
    },
    /// On-demand User-scoped work governed only by its pinned Discovery Plan.
    PersonalRequest {
        /// Retry-safe key unique to the requesting interactive Harness.
        idempotency_key: String,
        /// Interactive Harness that requested the plan.
        requested_by: Option<AgentHarnessId>,
    },
    /// Due work derived from one named private Personal Discovery schedule.
    ///
    /// Identity for a schedule period is `(schedule_id, due_at)`; materialization is
    /// idempotent across retries, restarts, concurrent wakeups, and scheduler changes.
    PersonalScheduled {
        /// Schedule that produced this period's task.
        schedule_id: PersonalDiscoveryScheduleId,
    },
}

/// Evidence basis that makes generic Personal Discovery ready.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum DiscoveryPlanBasis {
    ExplicitTopic(String),
    CorroboratedTopic(String),
    CorroboratedSource(SourceAffinitySignal),
}

/// Private readiness summary without raw evidence history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalDiscoveryReadiness {
    pub ready: bool,
    pub basis: Vec<DiscoveryPlanBasis>,
}

/// Temporary intent that applies only to one Personal Discovery run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PersonalDiscoveryIntent {
    Topic(String),
    SimilarToUrl(String),
}

/// Request for an immutable, retry-safe Personal Discovery Plan and task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestPersonalDiscovery {
    #[serde(default)]
    pub intent: Option<PersonalDiscoveryIntent>,
    #[serde(default)]
    pub result_count: Option<u16>,
    pub idempotency_key: String,
    /// Optional Browser Grant eligibility that restricts planned source neighborhoods.
    ///
    /// When present, only these generic source locators may be selected. Taste Profile
    /// evidence, Pod Packages, and Discovery Leads never broaden this set.
    #[serde(default)]
    pub browser_grant_eligible_sources: Option<Vec<String>>,
}

/// One selected topic with an inspectable, aggregate rationale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryPlanTopic {
    pub value: String,
    pub rationale: String,
    pub temporary: bool,
}

/// Whether a selected source neighborhood fills proven or adjacent allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryPlanSourceRole {
    /// Drawn from explicit preferences or corroborated User evidence.
    #[default]
    Proven,
    /// Adjacent exploration, including network-matched Discovery Leads.
    Adjacent,
}

/// One selected source neighborhood with aggregate evidence only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryPlanSourceNeighborhood {
    pub signal: SourceAffinitySignal,
    pub rationale: String,
    pub temporary: bool,
    /// Proven vs adjacent allocation role for this neighborhood.
    #[serde(default)]
    pub role: DiscoveryPlanSourceRole,
}

/// Provenance of a private Discovery Lead from verified public Stumble metadata.
///
/// Leads and their matching inputs remain Home Node private state and must never
/// appear in federation, Explore, announcement, or Index serialization.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryLeadProvenance {
    /// Compact signed public Pod advertisement retained locally.
    PodAnnouncement {
        announcement_id: Uuid,
        origin_node_id: NodeIdentityId,
        pod_slug: String,
    },
    /// Bounded Origin-signed Explore sample Content Reference.
    ExploreSample {
        announcement_id: Uuid,
        sample_artifact_id: Uuid,
        source: String,
    },
    /// Signed optional endorsement of a currently known public Pod.
    Endorsement {
        endorsement_id: Uuid,
        endorsed_node_id: NodeIdentityId,
        endorsed_pod_slug: String,
    },
    /// Locally available accepted Content Reference on a public Pod.
    PublicContentReference {
        content_item_id: ContentItemId,
        pod_id: PodId,
        source: String,
    },
}

/// Private potential source neighborhood before plan selection.
///
/// Produced only from verified, currently trusted, non-blocked public metadata
/// already local to the Home Node. Relevance is always recomputed locally.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DiscoveryLead {
    /// Generic source neighborhood the lead proposes for exploration.
    pub signal: SourceAffinitySignal,
    /// Public subject tokens used for local matching (not private profile terms).
    pub public_topics: Vec<String>,
    /// Inspectable origin of the lead within the local reservoir.
    pub provenance: DiscoveryLeadProvenance,
    /// Locally recomputed relevance; remote Index scores are never authoritative.
    pub local_relevance: f32,
}

/// Finite proven-neighborhood and adjacent-exploration quotas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryPlanAllocation {
    pub proven: u16,
    pub adjacent: u16,
}

/// Enforceable selection constraints supplied to the executing worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryPlanConstraints {
    pub max_per_domain: u16,
    pub max_per_author_or_account: u16,
    pub max_per_publisher: u16,
    pub max_per_community: u16,
    pub canonical_deduplication: bool,
    pub suppress_recently_reviewed: bool,
    pub blocked_topics: Vec<String>,
    pub blocked_sources: Vec<String>,
    pub blocked_source_affinities: Vec<SourceAffinitySignal>,
}

/// Immutable minimized worker contract for one Personal Discovery run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryPlan {
    pub id: DiscoveryPlanId,
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub result_count: u16,
    pub topics: Vec<DiscoveryPlanTopic>,
    pub source_neighborhoods: Vec<DiscoveryPlanSourceNeighborhood>,
    pub allocation: DiscoveryPlanAllocation,
    pub constraints: DiscoveryPlanConstraints,
    pub intent: Option<PersonalDiscoveryIntent>,
    pub created_at: DateTime<Utc>,
}

/// Atomic result of requesting Personal Discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedPersonalDiscovery {
    pub plan: DiscoveryPlan,
    pub task: DiscoveryTask,
}

/// Recurrence for a private Personal Discovery schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PersonalDiscoveryCadence {
    Hourly,
    Daily,
    Weekly,
    Monthly,
}

impl PersonalDiscoveryCadence {
    /// Deterministic period start used as the task due time for materialization.
    #[must_use]
    pub fn period_start(self, now: DateTime<Utc>) -> DateTime<Utc> {
        use chrono::{Datelike, Timelike};
        match self {
            Self::Hourly => now
                .with_minute(0)
                .and_then(|value| value.with_second(0))
                .and_then(|value| value.with_nanosecond(0))
                .expect("BUG: zero is a valid time component"),
            Self::Daily => now
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .expect("BUG: midnight is valid")
                .and_utc(),
            Self::Weekly => {
                let monday = now.date_naive()
                    - chrono::Duration::days(i64::from(now.weekday().num_days_from_monday()));
                monday
                    .and_hms_opt(0, 0, 0)
                    .expect("BUG: midnight is valid")
                    .and_utc()
            }
            Self::Monthly => chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
                .and_then(|date| date.and_hms_opt(0, 0, 0))
                .expect("BUG: first day of a valid month is valid")
                .and_utc(),
        }
    }
}

/// How a completed scheduled batch may be delivered to the User.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PersonalDiscoveryDeliveryMode {
    /// Emit at most one results-ready notification attempt when the harness supports delivery.
    NotifyWhenSupported,
    /// Retain the batch silently for later retrieval without a notification attempt.
    QueueOnly,
}

/// Optional temporary focus and avoidance for one schedule's runs.
///
/// Does not change durable Taste Profile preferences.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct PersonalDiscoveryScheduleIntent {
    /// Temporary focus topics applied when a period's plan materializes.
    pub focus_topics: Vec<String>,
    /// Temporary avoidance topics applied only to this schedule's plans.
    pub avoid_topics: Vec<String>,
}

impl PersonalDiscoveryScheduleIntent {
    /// Creates temporary schedule focus and avoidance instructions.
    #[must_use]
    pub fn new(focus_topics: Vec<String>, avoid_topics: Vec<String>) -> Self {
        Self {
            focus_topics,
            avoid_topics,
        }
    }
}

/// Request to create a named private Personal Discovery schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CreatePersonalDiscoveryScheduleRequest {
    /// User-visible unique name within the User's private schedules.
    pub name: String,
    pub cadence: PersonalDiscoveryCadence,
    #[serde(default)]
    pub intent: PersonalDiscoveryScheduleIntent,
    /// Finite batch size for each materialized run (1..=100).
    #[serde(default)]
    pub result_count: Option<u16>,
    pub delivery_mode: PersonalDiscoveryDeliveryMode,
}

/// Partial update for an existing private Personal Discovery schedule.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "snake_case")]
pub struct UpdatePersonalDiscoveryScheduleRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cadence: Option<PersonalDiscoveryCadence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<PersonalDiscoveryScheduleIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_count: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_mode: Option<PersonalDiscoveryDeliveryMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// Named private opt-in Personal Discovery schedule.
///
/// Local private state only; never federated and independent of Pod Source Rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalDiscoverySchedule {
    pub id: PersonalDiscoveryScheduleId,
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub name: String,
    pub cadence: PersonalDiscoveryCadence,
    pub intent: PersonalDiscoveryScheduleIntent,
    pub result_count: u16,
    pub delivery_mode: PersonalDiscoveryDeliveryMode,
    /// Disabled schedules retain configuration but do not materialize due work.
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Inspectable reason a schedule is not materializing due work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum PersonalDiscoveryScheduleBackpressure {
    /// No backpressure; the schedule may materialize when due and ready.
    None,
    /// A prior completed batch is still Ready for review or dismissal.
    UnreviewedBatch {
        batch_id: DiscoveryResultBatchId,
        task_id: DiscoveryTaskId,
    },
    /// A prior period's task is still pending or leased.
    InFlightTask { task_id: DiscoveryTaskId },
}

/// Schedule configuration plus inspectable dormancy and backpressure state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalDiscoveryScheduleStatus {
    pub schedule: PersonalDiscoverySchedule,
    /// True when cold-start readiness is below threshold (schedule remains dormant).
    pub readiness_dormant: bool,
    pub backpressure: PersonalDiscoveryScheduleBackpressure,
    /// Period start that would materialize if not dormant, disabled, or backpressured.
    pub current_period_start: DateTime<Utc>,
    /// Task already materialized for the current period, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_period_task_id: Option<DiscoveryTaskId>,
}

/// Private one-shot notice that a scheduled Discovery Result Batch is ready.
///
/// Distinct from batch review state and from notification delivery attempts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryResultsReadyEvent {
    pub id: Uuid,
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub schedule_id: PersonalDiscoveryScheduleId,
    pub batch_id: DiscoveryResultBatchId,
    pub task_id: DiscoveryTaskId,
    pub delivery_mode: PersonalDiscoveryDeliveryMode,
    pub created_at: DateTime<Utc>,
    /// Set after the single allowed notification attempt (notify-when-supported only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_attempted_at: Option<DateTime<Utc>>,
}

/// Outcome of attempting one-shot results-ready notification delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryResultsReadyNotificationOutcome {
    /// First notify-when-supported attempt; batch remains Ready / unreviewed.
    ShouldNotify {
        event: DiscoveryResultsReadyEvent,
        batch: DiscoveryResultBatch,
    },
    /// A prior attempt already consumed the one-shot allowance.
    AlreadyAttempted {
        event: DiscoveryResultsReadyEvent,
        batch: DiscoveryResultBatch,
    },
    /// Queue-only delivery retains the batch silently without notification.
    QueueOnly {
        event: DiscoveryResultsReadyEvent,
        batch: DiscoveryResultBatch,
    },
}

/// Lifecycle of a private Discovery Result Batch, distinct from task completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryResultBatchState {
    /// Completed run awaiting User review.
    Ready,
    /// User finished reviewing the batch without whole-batch dismissal.
    Reviewed,
    /// User dismissed the entire batch without item-level learning evidence.
    Dismissed,
}

/// Whether a results-ready notice has been delivered for this batch.
///
/// Independent of batch review state and of Discovery Task completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryResultNotificationState {
    /// On-demand / queue-only runs do not emit a results-ready notice.
    #[default]
    NotApplicable,
    /// Scheduled completion may notify once when the harness supports delivery.
    Pending,
    /// One-shot notice was delivered; does not mark the batch reviewed.
    Delivered,
}

/// One ordered Candidate reference retained by a Discovery Result Batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryResultItem {
    /// Zero-based position within the finite shortlist.
    pub position: u16,
    /// Canonical private Candidate identity.
    pub candidate_id: CandidateId,
    /// Provenance-bearing submission that produced this result.
    pub submission_id: CandidateSubmissionId,
    /// Canonical URL identity retained for inspection and suppression.
    pub canonical_url: String,
    /// Allocation role under which the item was selected into the batch.
    pub allocation_role: DiscoveryPlanSourceRole,
    /// Private per-item review decision; distinct from batch completion and placement.
    #[serde(default)]
    pub review: DiscoveryResultItemReview,
}

/// Deliberate User action recorded against one Discovery Result Batch item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryResultItemAction {
    /// Create an Accepted Placement in the User's private Inbox.
    Save,
    /// Create an Accepted Placement in an authorized Pod.
    AddToPod,
    /// Explicit supporting learning evidence for eligible topics and Source Affinities.
    MoreLikeThis,
    /// Explicit opposing learning evidence; suppresses immediate rediscovery.
    NotForMe,
    /// Acknowledge the item without learning or placement.
    Ignore,
}

/// Permission-derived actions an interactive harness may offer for a result item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryResultAllowedAction {
    /// Save into the private Inbox.
    Save,
    /// Place into an authorized Pod (requires Pod Role + Harness Grant).
    AddToPod,
    /// Reinforce eligible topics and Source Affinities.
    MoreLikeThis,
    /// Reject the result and record opposing evidence.
    NotForMe,
    /// Leave the item without learning.
    Ignore,
}

/// Private per-item review state, independent of batch Ready/Reviewed/Dismissed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryResultItemReview {
    /// No deliberate item action yet.
    #[default]
    Unreviewed,
    /// User recorded one deliberate action (may replace a prior action).
    Reviewed {
        /// Current deliberate action.
        action: DiscoveryResultItemAction,
        /// When the current action was recorded.
        reviewed_at: DateTime<Utc>,
        /// Prior action when the User replaced an earlier decision (inspectable).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replaced_action: Option<DiscoveryResultItemAction>,
        /// Pod that received an Accepted Placement for Save or Add to Pod.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placement_pod_id: Option<PodId>,
        /// Content Item identity for placement-bearing actions.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_item_id: Option<ContentItemId>,
    },
}

/// Requested deliberate action for one Discovery Result Batch item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryResultItemActionRequest {
    /// Save into the User's private Inbox.
    Save,
    /// Place into the selected authorized Pod.
    AddToPod {
        /// Target Pod for explicit curation.
        pod_id: PodId,
        /// Optional public Pod-fit note retained on the placement.
        #[serde(default)]
        curation_note: Option<CurationRationale>,
    },
    /// Create supporting learning evidence.
    MoreLikeThis,
    /// Create opposing learning evidence and reject rediscovery.
    NotForMe,
    /// Leave the item without learning or placement.
    Ignore,
}

impl DiscoveryResultItemActionRequest {
    /// Maps the request to the durable review action discriminant.
    #[must_use]
    pub const fn action(&self) -> DiscoveryResultItemAction {
        match self {
            Self::Save => DiscoveryResultItemAction::Save,
            Self::AddToPod { .. } => DiscoveryResultItemAction::AddToPod,
            Self::MoreLikeThis => DiscoveryResultItemAction::MoreLikeThis,
            Self::NotForMe => DiscoveryResultItemAction::NotForMe,
            Self::Ignore => DiscoveryResultItemAction::Ignore,
        }
    }
}

/// Request to review one item inside a private Discovery Result Batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ReviewDiscoveryResultItemRequest {
    /// Batch owning the item.
    pub batch_id: DiscoveryResultBatchId,
    /// Candidate identity of the shortlist item.
    pub candidate_id: CandidateId,
    /// Deliberate action to apply (idempotent when repeated).
    pub action: DiscoveryResultItemActionRequest,
}

/// Outcome of one private Discovery Result item review.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DiscoveryResultItemReviewOutcome {
    /// Batch after the atomic review mutation (state may remain Ready).
    pub batch: DiscoveryResultBatch,
    /// Item after review mutation.
    pub item: DiscoveryResultItem,
    /// Accepted Placement when Save or Add to Pod produced one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<PodPlacement>,
    /// Whether this call replaced a different prior item action.
    pub action_replaced: bool,
    /// Actions currently allowed for this caller on this item.
    pub allowed_actions: Vec<DiscoveryResultAllowedAction>,
    /// Updated aggregate Taste Profile evidence after the action.
    pub taste_profile: TasteProfile,
}

/// Private linkage from a reviewed result item to replaceable taste evidence rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DiscoveryResultItemLearningLink {
    pub batch_id: DiscoveryResultBatchId,
    pub candidate_id: CandidateId,
    pub evidence_ids: Vec<Uuid>,
}

/// Inspectable reason a quota could not be filled or was reallocated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiscoveryResultAvailabilityReason {
    /// Fewer proven-neighborhood results than the plan allocation requested.
    InsufficientProven { requested: u16, filled: u16 },
    /// Fewer adjacent-exploration results than the plan allocation requested.
    InsufficientAdjacent { requested: u16, filled: u16 },
    /// Domain diversity cap excluded further candidates.
    DomainCap { domain: String, rejected_count: u16 },
    /// Author or account diversity cap excluded further candidates.
    AuthorOrAccountCap {
        identity: String,
        rejected_count: u16,
    },
    /// Publisher diversity cap excluded further candidates.
    PublisherCap {
        identity: String,
        rejected_count: u16,
    },
    /// Community diversity cap excluded further candidates.
    CommunityCap {
        identity: String,
        rejected_count: u16,
    },
    /// Explicit block excluded a candidate.
    Blocked { detail: String },
    /// Canonical URL already selected into this batch.
    CanonicalDuplicate { canonical_url: String },
    /// Canonical URL appeared in a recent prior result batch for this User.
    RecentlyReviewed { canonical_url: String },
    /// Worker-reported source neighborhood unavailability.
    SourceUnavailable { source: String, reason: String },
    /// Remaining slots moved between proven and adjacent without weakening policy.
    Reallocated {
        from: DiscoveryPlanSourceRole,
        to: DiscoveryPlanSourceRole,
        count: u16,
    },
    /// Overall shortfall after policy enforcement (no invented results).
    Underfilled { requested: u16, filled: u16 },
    /// Scheduled run skipped an authenticated source without waiting or logging in.
    AuthenticationSkippedScheduled { source: String, reason: String },
    /// On-demand run continued after requesting User-assisted login for a source.
    AuthenticationAssistanceRequested { source: String, reason: String },
    /// Planned source was outside the harness-reported Browser Grant eligibility set.
    BrowserGrantIneligible { source: String, reason: String },
}

/// Structured availability fact for a planned source neighborhood.
///
/// Facts only: never credentials, cookies, tokens, or raw browser state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SourceAvailabilityState {
    /// Source is reachable under the harness Browser Connector session.
    Available,
    /// Source needs User-assisted login; harness owns the session outside Stumble.
    AuthenticationRequired,
    /// Prior session expired; restore requires User assistance outside Stumble.
    SessionExpired,
    /// Source cannot be reached for a non-auth reason (network, outage, etc.).
    Inaccessible,
    /// Browser Grant does not permit this planned source for the harness.
    BrowserGrantIneligible,
}

impl SourceAvailabilityState {
    /// Whether this state indicates authentication assistance may be valuable.
    #[must_use]
    pub const fn authentication_required(self) -> bool {
        matches!(self, Self::AuthenticationRequired | Self::SessionExpired)
    }

    /// Whether the source is usable for discovery work right now.
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }

    /// Stable fingerprint component so notice eligibility reopens after state changes.
    #[must_use]
    pub const fn fingerprint_label(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::AuthenticationRequired => "authentication_required",
            Self::SessionExpired => "session_expired",
            Self::Inaccessible => "inaccessible",
            Self::BrowserGrantIneligible => "browser_grant_ineligible",
        }
    }
}

/// Worker-reported availability for a planned source neighborhood.
///
/// Rejects unknown fields so workers cannot smuggle credentials or browser state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ReportedSourceAvailability {
    /// Generic source locator (domain or affinity key), never a credential.
    pub source: String,
    /// Structured availability without auth material.
    pub state: SourceAvailabilityState,
    /// Inspectable harness reason (for example session expired or grant missing).
    #[serde(default)]
    pub reason: String,
}

impl ReportedSourceAvailability {
    /// Whether authentication assistance is indicated for this report.
    #[must_use]
    pub const fn authentication_required(&self) -> bool {
        self.state.authentication_required()
    }

    /// Stable fingerprint for one-shot authentication-needed notice suppression.
    #[must_use]
    pub fn state_fingerprint(&self) -> String {
        format!(
            "{}:{}",
            self.source.trim().to_ascii_lowercase(),
            self.state.fingerprint_label()
        )
    }
}

/// Lease-scoped private snapshot of planned source availability for one task.
///
/// Stores availability facts only. Never credentials, cookies, tokens, or browser state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryTaskSourceAvailability {
    pub task_id: DiscoveryTaskId,
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub reported_by: AgentHarnessId,
    /// Latest availability reports keyed by normalized source locator.
    pub reports: Vec<ReportedSourceAvailability>,
    /// When set, only these sources are Browser-Grant-eligible for this task.
    ///
    /// Never broadened by Taste Profile, Pod Package, Discovery Lead, or remote metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_grant_eligible_sources: Option<Vec<String>>,
    pub updated_at: DateTime<Utc>,
}

/// Request for a leased worker to report planned source availability facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ReportDiscoverySourceAvailabilityRequest {
    /// Claimed Personal Discovery Task these facts apply to.
    ///
    /// HTTP adapters may supply this from the path and leave the body field defaulted.
    #[serde(default)]
    pub task_id: DiscoveryTaskId,
    /// Availability facts for planned source neighborhoods (no auth material).
    pub reports: Vec<ReportedSourceAvailability>,
    /// Optional Browser Grant eligibility set that restricts — never broadens — access.
    #[serde(default)]
    pub browser_grant_eligible_sources: Option<Vec<String>>,
}

/// Private one-shot authentication-needed notice for an unavailable source state.
///
/// Emitted at most once per `(user, source, state fingerprint)` until availability changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticationNeededNotice {
    pub id: Uuid,
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    /// Generic source locator needing User-assisted login outside Stumble.
    pub source: String,
    /// Fingerprint of the unavailable authentication state.
    pub state_fingerprint: String,
    /// Task that first recorded this unavailable state.
    pub task_id: DiscoveryTaskId,
    pub first_emitted_at: DateTime<Utc>,
    /// Whether an interactive harness should still present this notice.
    pub delivery_pending: bool,
}

/// Outcome of evaluating authentication-needed notice emission for one source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AuthenticationNeededNoticeOutcome {
    /// First notice for this unavailable source state; present to the User once.
    ShouldNotify { notice: AuthenticationNeededNotice },
    /// Prior notice still covers this unavailable source state.
    Suppressed { notice: AuthenticationNeededNotice },
    /// Scheduled runs never wait for authentication.
    ScheduledSkip { source: String },
    /// Source is available or does not require authentication assistance.
    NotApplicable { source: String },
}

/// Result of reporting planned source availability on a leased task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedDiscoverySourceAvailability {
    /// Lease-scoped private availability snapshot after this report.
    pub availability: DiscoveryTaskSourceAvailability,
    /// On-demand authentication-needed notice outcomes evaluated from this report.
    pub authentication_notices: Vec<AuthenticationNeededNoticeOutcome>,
}

/// Request to atomically finish a leased Personal Discovery Task into a batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CompleteDiscoveryResultBatchRequest {
    /// Claimed Personal Discovery Task producing this batch.
    pub task_id: DiscoveryTaskId,
    /// Ordered shortlist of prior task-bound submissions (finite, provenance-bearing).
    pub submission_ids: Vec<CandidateSubmissionId>,
    /// Optional worker-reported source availability for inspectable shortfalls.
    #[serde(default)]
    pub source_availability: Vec<ReportedSourceAvailability>,
    /// Optional Browser Grant eligibility set applied at completion when not already reported.
    #[serde(default)]
    pub browser_grant_eligible_sources: Option<Vec<String>>,
}

/// Private finite shortlist returned from one Personal Discovery Task.
///
/// Never federated. Retains task and plan identity for explainability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryResultBatch {
    pub id: DiscoveryResultBatchId,
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub task_id: DiscoveryTaskId,
    pub plan_id: DiscoveryPlanId,
    /// Ready / reviewed / dismissed lifecycle, independent of task state.
    pub state: DiscoveryResultBatchState,
    /// One-shot results-ready notice state, independent of review.
    pub notification_state: DiscoveryResultNotificationState,
    /// Plan-requested finite size.
    pub requested_size: u16,
    /// Plan allocation quotas at completion time.
    pub allocation: DiscoveryPlanAllocation,
    /// How many selected items filled each allocation role after policy.
    pub allocation_filled: DiscoveryPlanAllocation,
    /// Ordered finite Candidate references with provenance.
    pub items: Vec<DiscoveryResultItem>,
    /// Inspectable underfill, reallocation, cap, block, and availability reasons.
    pub source_availability: Vec<DiscoveryResultAvailabilityReason>,
    pub created_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub dismissed_at: Option<DateTime<Utc>>,
}

/// Exclusive, expiring ownership of a Discovery Task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryTaskLease {
    /// Harness with exclusive execution authority.
    pub harness_id: AgentHarnessId,
    /// Time at which this attempt began.
    pub claimed_at: DateTime<Utc>,
    /// Time after which another harness may safely claim the task.
    pub expires_at: DateTime<Utc>,
}

/// Inspectable outcome of one claimed task attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryTaskAttemptOutcome {
    /// Harness completed the task successfully.
    Completed,
    /// Harness explicitly failed the attempt with an inspectable reason.
    Failed {
        /// Harness-supplied explanation.
        reason: String,
    },
    /// Harness abandoned the task until its lease expired.
    LeaseExpired,
}

/// Immutable history entry for a completed or failed lease attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryTaskAttempt {
    /// Harness responsible for this attempt.
    pub harness_id: AgentHarnessId,
    /// Lease claim time.
    pub started_at: DateTime<Utc>,
    /// Completion, failure, or expiry time.
    pub finished_at: DateTime<Utc>,
    /// Inspectable terminal result of this attempt.
    pub outcome: DiscoveryTaskAttemptOutcome,
}

/// Immutable contract governing one Discovery Task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiscoveryTaskTarget {
    /// Pod discovery pinned to the Package version the worker must follow.
    Pod {
        /// Pod whose Package governs this work.
        pod_id: PodId,
        /// Immutable Package version used by the worker.
        package_version: PackageVersion,
    },
    /// Personal Discovery pinned to a private immutable Discovery Plan.
    Personal {
        /// Plan minimized for and pinned to this task.
        discovery_plan_id: DiscoveryPlanId,
    },
}

impl DiscoveryTaskTarget {
    /// Returns the Pod contract when this is Pod discovery.
    #[must_use]
    pub const fn pod(&self) -> Option<(PodId, PackageVersion)> {
        match self {
            Self::Pod {
                pod_id,
                package_version,
            } => Some((*pod_id, *package_version)),
            Self::Personal { .. } => None,
        }
    }

    /// Returns the pinned plan identity when this is Personal Discovery.
    #[must_use]
    pub const fn discovery_plan_id(&self) -> Option<DiscoveryPlanId> {
        match self {
            Self::Personal { discovery_plan_id } => Some(*discovery_plan_id),
            Self::Pod { .. } => None,
        }
    }
}

/// Leaseable discovery work derived from a Source Rule or immediate request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryTask {
    /// Stable task identity.
    pub id: DiscoveryTaskId,
    /// Immutable Pod or Personal Discovery contract.
    pub target: DiscoveryTaskTarget,
    /// Scheduled or conversational provenance.
    pub origin: DiscoveryTaskOrigin,
    /// Earliest claim time.
    pub due_at: DateTime<Utc>,
    /// Current lifecycle state.
    pub state: DiscoveryTaskState,
    /// Completed, failed, and expired attempt history.
    pub attempts: Vec<DiscoveryTaskAttempt>,
    /// Time at which Stumble created the task.
    pub created_at: DateTime<Utc>,
}

impl Serialize for DiscoveryTask {
    fn serialize<Serializer>(
        &self,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error>
    where
        Serializer: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let field_count = if self.target.pod().is_some() { 9 } else { 7 };
        let mut task = serializer.serialize_struct("DiscoveryTask", field_count)?;
        task.serialize_field("id", &self.id)?;
        task.serialize_field("target", &self.target)?;
        if let Some((pod_id, package_version)) = self.target.pod() {
            task.serialize_field("pod_id", &pod_id)?;
            task.serialize_field("package_version", &package_version)?;
        }
        task.serialize_field("origin", &self.origin)?;
        task.serialize_field("due_at", &self.due_at)?;
        task.serialize_field("state", &self.state)?;
        task.serialize_field("attempts", &self.attempts)?;
        task.serialize_field("created_at", &self.created_at)?;
        task.end()
    }
}

#[derive(Deserialize)]
struct DiscoveryTaskWire {
    id: DiscoveryTaskId,
    #[serde(default)]
    target: Option<DiscoveryTaskTarget>,
    #[serde(default)]
    pod_id: Option<PodId>,
    #[serde(default)]
    package_version: Option<PackageVersion>,
    origin: DiscoveryTaskOrigin,
    due_at: DateTime<Utc>,
    state: DiscoveryTaskState,
    attempts: Vec<DiscoveryTaskAttempt>,
    created_at: DateTime<Utc>,
}

impl<'de> Deserialize<'de> for DiscoveryTask {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let wire = DiscoveryTaskWire::deserialize(deserializer)?;
        let target = match (wire.target, wire.pod_id, wire.package_version) {
            (Some(target), None, None) => target,
            (
                Some(
                    target @ DiscoveryTaskTarget::Pod {
                        pod_id: target_pod_id,
                        package_version: target_package_version,
                    },
                ),
                Some(pod_id),
                Some(package_version),
            ) if target_pod_id == pod_id && target_package_version == package_version => target,
            (Some(_), Some(_), Some(_)) => {
                return Err(Deserializer::Error::custom(
                    "typed and legacy Discovery Task targets must agree",
                ))
            }
            (Some(_), _, _) => {
                return Err(Deserializer::Error::custom(
                    "legacy Discovery Task target fields must be complete",
                ))
            }
            (None, Some(pod_id), Some(package_version)) => DiscoveryTaskTarget::Pod {
                pod_id,
                package_version,
            },
            (None, None, _) => return Err(Deserializer::Error::missing_field("target")),
            (None, Some(_), None) => {
                return Err(Deserializer::Error::missing_field("package_version"))
            }
        };
        Ok(Self {
            id: wire.id,
            target,
            origin: wire.origin,
            due_at: wire.due_at,
            state: wire.state,
            attempts: wire.attempts,
            created_at: wire.created_at,
        })
    }
}

/// Request for immediate conversational discovery with retry-safe identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateImmediateDiscoveryTaskRequest {
    /// Pod to discover for.
    pub pod_id: PodId,
    /// Conversation-derived discovery intent.
    pub instructions: String,
    /// Retry-safe caller key.
    pub idempotency_key: String,
}

/// Local-only attribution record for a successful harness write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessWriteAudit {
    /// Stable local audit entry identity.
    pub id: Uuid,
    /// Harness responsible for the write.
    pub harness_id: AgentHarnessId,
    /// Typed operation that changed local state.
    pub operation: HarnessWriteOperation,
    /// Affected Pod, when the write is Pod-specific.
    pub pod_id: Option<PodId>,
    /// Timestamp at which the write committed.
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub id: NodeIdentityId,
    pub tenant_id: Option<TenantId>,
    pub display_name: String,
    pub public_key: String,
    pub private_key_encrypted_or_local: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedPeer {
    pub id: PeerId,
    /// Canonical identity advertised and signed by the remote Node.
    #[serde(default)]
    pub node_id: NodeIdentityId,
    pub tenant_id: Option<TenantId>,
    pub display_name: String,
    pub base_url: String,
    pub public_key: String,
    pub trust_level: TrustLevel,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pod {
    pub id: PodId,
    pub tenant_id: Option<TenantId>,
    pub name: String,
    pub slug: String,
    pub description: String,
    pub visibility: Visibility,
    pub created_by: Option<UserId>,
    pub created_at: DateTime<Utc>,
    pub origin_node_id: Option<NodeIdentityId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodRoleAssignment {
    pub user_id: UserId,
    pub pod_id: PodId,
    pub role: PodRole,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodRules {
    pub pod_id: PodId,
    pub blocked_topics: Vec<String>,
    pub blocked_domains: Vec<String>,
    pub auto_promote_crawler_candidates: bool,
    pub federate_sources: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PodSkillPack {
    /// Stable identity shared by the versions of this package.
    pub id: Uuid,
    /// Pod governed by this package.
    pub pod_id: PodId,
    /// Legacy wire-compatible numeric version. New storage APIs use [`PackageVersion`].
    pub version: i32,
    /// Subject language, scope, and boundaries. This is deliberately separate
    /// from the operational instructions in `skill_md`.
    #[serde(default)]
    pub context_md: String,
    /// Legacy Pod metadata retained for compatibility.
    pub pod_yaml: String,
    /// Scoped, untrusted discovery and curation instructions.
    pub skill_md: String,
    /// Declarative Source Rule suggestions.
    pub sources_yaml: String,
    /// Pod-owned filtering suggestions.
    pub filters_yaml: String,
    /// Positive calibration examples.
    pub examples_good_md: String,
    /// Negative calibration examples.
    pub examples_bad_md: String,
    /// User who owns the authoritative package version.
    #[serde(default)]
    pub owner_id: Option<UserId>,
    /// Harness that proposed this package version, if any.
    #[serde(default)]
    pub proposer_harness_id: Option<AgentHarnessId>,
    /// Timestamp at which this immutable version was created.
    pub created_at: DateTime<Utc>,
    /// Legacy alias of `created_at` retained for wire compatibility.
    pub updated_at: DateTime<Utc>,
}

/// Canonical name for the signed, versioned bundle historically exposed as a
/// `PodSkillPack`. The legacy name remains for wire and source compatibility.
pub type PodPackage = PodSkillPack;

/// Result of requesting one version-aware Pod Package Revision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "result", rename_all = "snake_case")]
pub enum PodPackageRevisionOutcome {
    /// A non-public origin package was revised immediately.
    Revised(Box<PodPackage>),
    /// A public origin package is unchanged until this proposal is approved.
    PendingApproval(Box<PendingProposal>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FederatedPodEventType {
    PodCreated,
    PodPublished,
    PodSkillPackUpdated,
    PodPackageImported,
    PodPackageForked,
    ContentItemPlaced,
    ContentItemMetadataUpdated,
    PlacementTombstoned,
    LegacyLinkRemoved,
    LegacyLinkSubmitted,
}

impl FederatedPodEventType {
    pub(crate) fn from_wire(value: &str) -> Option<Self> {
        match value {
            "pod_created" => Some(Self::PodCreated),
            "pod_published" => Some(Self::PodPublished),
            "pod_skill_pack_updated" => Some(Self::PodSkillPackUpdated),
            "pod_package_imported" => Some(Self::PodPackageImported),
            "pod_package_forked" => Some(Self::PodPackageForked),
            "content_item_placed" => Some(Self::ContentItemPlaced),
            "content_item_metadata_updated" => Some(Self::ContentItemMetadataUpdated),
            "placement_tombstoned" => Some(Self::PlacementTombstoned),
            "link_removed" => Some(Self::LegacyLinkRemoved),
            "link_submitted" => Some(Self::LegacyLinkSubmitted),
            _ => None,
        }
    }

    pub(crate) const fn as_wire(self) -> &'static str {
        match self {
            Self::PodCreated => "pod_created",
            Self::PodPublished => "pod_published",
            Self::PodSkillPackUpdated => "pod_skill_pack_updated",
            Self::PodPackageImported => "pod_package_imported",
            Self::PodPackageForked => "pod_package_forked",
            Self::ContentItemPlaced => "content_item_placed",
            Self::ContentItemMetadataUpdated => "content_item_metadata_updated",
            Self::PlacementTombstoned => "placement_tombstoned",
            Self::LegacyLinkRemoved => "link_removed",
            Self::LegacyLinkSubmitted => "link_submitted",
        }
    }

    pub(crate) const fn is_federated(self) -> bool {
        match self {
            Self::PodCreated
            | Self::PodPublished
            | Self::PodSkillPackUpdated
            | Self::PodPackageImported
            | Self::PodPackageForked
            | Self::ContentItemPlaced
            | Self::ContentItemMetadataUpdated
            | Self::PlacementTombstoned
            | Self::LegacyLinkRemoved => true,
            Self::LegacyLinkSubmitted => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLog {
    pub event_id: Uuid,
    pub tenant_id: Option<TenantId>,
    pub event_type: String,
    pub pod_slug: String,
    pub author_node_id: NodeIdentityId,
    pub author_display_name: Option<String>,
    pub payload_json: Value,
    pub created_at: DateTime<Utc>,
    pub previous_event_hash: Option<String>,
    pub content_hash: String,
    pub signature: String,
    pub imported_from_peer_id: Option<PeerId>,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Submission {
    pub id: SubmissionId,
    pub tenant_id: Option<TenantId>,
    pub url: String,
    pub canonical_url: String,
    pub title: String,
    #[serde(default)]
    pub source_metadata: CandidateSourceMetadata,
    pub description: Option<String>,
    pub domain: String,
    pub submitted_by: Option<UserId>,
    pub discovered_by_crawler: bool,
    pub submitter_note: Option<String>,
    pub summary: Option<String>,
    #[serde(default)]
    pub provenance: Vec<CandidateProvenance>,
    #[serde(default)]
    pub media_references: Vec<MediaReference>,
    pub tags: Vec<String>,
    pub embedding: Option<Vec<f32>>,
    pub created_at: DateTime<Utc>,
    pub origin_event_id: Option<Uuid>,
}

/// Review lifecycle of a private Candidate before curation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CandidateReviewState {
    /// No authoritative Pod Placement has been created.
    Pending,
    /// At least one proposed Pod Placement became authoritative.
    Accepted,
}

/// Pod-owned autonomy mode for turning Candidate evidence into authoritative placements.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CurationPolicy {
    /// Every proposed placement waits for authorized review.
    Manual,
    /// Trusted task evidence may be accepted at or above the configured threshold.
    Assisted {
        /// Inclusive confidence floor for automatic acceptance.
        confidence_threshold: CandidateConfidence,
    },
    /// Any proposal at or above the configured threshold may be accepted automatically.
    Autonomous {
        /// Inclusive confidence floor for automatic acceptance.
        confidence_threshold: CandidateConfidence,
    },
}

impl Default for CurationPolicy {
    fn default() -> Self {
        Self::Assisted {
            confidence_threshold: CandidateConfidence(0.8),
        }
    }
}

/// Authoritative lifecycle of one Candidate-to-Pod association.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PodPlacementStatus {
    /// Waiting for an authorized decision.
    Pending,
    /// Authoritative and eligible for synchronization and Feeds.
    Accepted,
    /// Declined and suppressed from identical future local routing.
    Rejected,
    /// Formerly accepted but withdrawn and suppressed from identical routing.
    Reversed,
}

/// Path by which a placement reached its current authoritative state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CurationPath {
    /// Initial gated Candidate evidence.
    CandidateProposal,
    /// Explicit authorized review.
    ManualReview,
    /// Trusted high-confidence acceptance under Assisted Curation.
    AssistedAutomatic,
    /// Threshold acceptance under approved Autonomous Curation.
    AutonomousAutomatic,
    /// Additional local Pod proposed by the Routing Agent.
    RoutingAgent,
    /// Explicit authorized User curation that bypassed Candidate review.
    AddToPod,
}

/// Authenticated actor responsible for a curation transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CurationActor {
    /// Authenticated Agent Harness.
    Harness(AgentHarnessId),
    /// Directly authenticated User.
    User(UserId),
    /// Deterministic local automation without a harness identity.
    NodeAgent,
}

/// Immutable audit entry for a Pod Placement transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PlacementAuditEntry {
    /// State produced by this transition.
    pub status: PodPlacementStatus,
    /// Curation path responsible for this transition.
    pub curation_path: CurationPath,
    /// Attributable actor responsible for this transition.
    pub actor: CurationActor,
    /// Optional review or reversal rationale.
    pub note: Option<CurationRationale>,
    /// Time at which the transition committed.
    pub occurred_at: DateTime<Utc>,
}

/// Evidence-backed association between one canonical Content Item and one Pod.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PodPlacement {
    /// Private Candidate from which this association originated.
    pub candidate_id: CandidateId,
    /// Independently governed Pod receiving the association.
    pub pod_id: PodId,
    /// Canonical Content Item once the placement has been accepted.
    pub content_item_id: Option<ContentItemId>,
    /// Strongest retained explanation for this Pod association.
    pub reason: CurationRationale,
    /// Strongest retained confidence evidence, never authority by itself.
    pub confidence: CandidateConfidence,
    /// Immutable Candidate Submissions supporting this association.
    pub source_submission_ids: Vec<CandidateSubmissionId>,
    /// Origin placements visible when an explicit Add to Pod action preserved this item.
    #[serde(default)]
    pub origin_placements: Vec<AcceptedPlacementProjection>,
    /// Later signed withdrawals affecting preserved origin placements.
    #[serde(default)]
    pub origin_withdrawals: Vec<PlacementTombstone>,
    /// Current authoritative lifecycle state.
    pub status: PodPlacementStatus,
    /// Path that produced the current state.
    pub curation_path: CurationPath,
    /// Actor responsible for the current state.
    pub actor: CurationActor,
    /// Append-only state transition history.
    pub audit_history: Vec<PlacementAuditEntry>,
    /// Time at which the route was first proposed.
    pub created_at: DateTime<Utc>,
    /// Time at which the latest transition committed.
    pub updated_at: DateTime<Utc>,
}

/// Canonical unit of accepted content, independent of its Pod Placements.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ContentItem {
    legacy_record: Submission,
}

impl ContentItem {
    /// Returns the stable canonical identity shared by all Pod Placements.
    #[must_use]
    pub fn id(&self) -> ContentItemId {
        self.legacy_record.id.into()
    }

    /// Returns the original source reference retained for provenance.
    #[must_use]
    pub fn source_url(&self) -> &str {
        &self.legacy_record.url
    }

    /// Returns the normalized source identity used for deduplication.
    #[must_use]
    pub fn canonical_url(&self) -> &str {
        &self.legacy_record.canonical_url
    }

    /// Returns the source title or canonical-URL fallback.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.legacy_record.title
    }

    /// Returns the generated understanding retained independently of the source.
    #[must_use]
    pub fn summary(&self) -> Option<&str> {
        self.legacy_record.summary.as_deref()
    }

    /// Returns the excerpt that source policy permits Stumble to retain.
    #[must_use]
    pub fn permitted_description(&self) -> Option<&str> {
        self.legacy_record.description.as_deref()
    }

    /// Returns descriptive tags retained with this Content Reference.
    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.legacy_record.tags
    }

    /// Returns source title, author, and publication time retained at acceptance.
    #[must_use]
    pub const fn source_metadata(&self) -> &CandidateSourceMetadata {
        &self.legacy_record.source_metadata
    }

    /// Returns the discovery evidence retained with this Content Reference.
    #[must_use]
    pub fn provenance(&self) -> &[CandidateProvenance] {
        &self.legacy_record.provenance
    }

    /// Returns permitted attached-media URLs without implying byte archival.
    #[must_use]
    pub fn media_references(&self) -> &[MediaReference] {
        &self.legacy_record.media_references
    }

    pub(crate) fn into_legacy_record(self) -> Submission {
        self.legacy_record
    }
}

#[derive(Serialize, Deserialize)]
struct ContentItemWire {
    id: ContentItemId,
    source_url: String,
    canonical_url: String,
    title: String,
    #[serde(default)]
    source_metadata: CandidateSourceMetadata,
    permitted_description: Option<String>,
    domain: String,
    summary: Option<String>,
    #[serde(default)]
    provenance: Vec<CandidateProvenance>,
    #[serde(default)]
    media_references: Vec<MediaReference>,
    tags: Vec<String>,
    created_at: DateTime<Utc>,
    origin_event_id: Option<Uuid>,
}

impl Serialize for ContentItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ContentItemWire {
            id: self.id(),
            source_url: self.legacy_record.url.clone(),
            canonical_url: self.legacy_record.canonical_url.clone(),
            title: self.legacy_record.title.clone(),
            source_metadata: self.legacy_record.source_metadata.clone(),
            permitted_description: self.legacy_record.description.clone(),
            domain: self.legacy_record.domain.clone(),
            summary: self.legacy_record.summary.clone(),
            provenance: self.legacy_record.provenance.clone(),
            media_references: self.legacy_record.media_references.clone(),
            tags: self.legacy_record.tags.clone(),
            created_at: self.legacy_record.created_at,
            origin_event_id: self.legacy_record.origin_event_id,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ContentItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ContentItemWire::deserialize(deserializer)?;
        Ok(Self {
            legacy_record: Submission {
                id: wire.id.into(),
                tenant_id: None,
                url: wire.source_url,
                canonical_url: wire.canonical_url,
                title: wire.title,
                source_metadata: wire.source_metadata,
                description: wire.permitted_description,
                domain: wire.domain,
                submitted_by: None,
                discovered_by_crawler: false,
                submitter_note: None,
                summary: wire.summary,
                provenance: wire.provenance,
                media_references: wire.media_references,
                tags: wire.tags,
                embedding: None,
                created_at: wire.created_at,
                origin_event_id: wire.origin_event_id,
            },
        })
    }
}

impl From<&Submission> for ContentItem {
    fn from(value: &Submission) -> Self {
        Self {
            legacy_record: value.clone(),
        }
    }
}

/// Public, synchronization-safe evidence for one Accepted Placement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AcceptedPlacementProjection {
    /// Canonical Content Item placed in the Pod.
    pub content_item_id: ContentItemId,
    /// Local or origin Pod identity, remapped by Pod slug on import.
    pub pod_id: PodId,
    /// Public evidence explaining why the item belongs in the Pod.
    pub reason: CurationRationale,
    /// Curation path that produced the Accepted Placement.
    pub curation_path: CurationPath,
    /// Origin Node responsible for the authoritative acceptance.
    pub origin_node_id: NodeIdentityId,
    /// Time at which the placement became accepted.
    pub accepted_at: DateTime<Utc>,
}

/// Cumulative signed metadata retained for one accepted Content Reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContentItemMetadataUpdate {
    pub(crate) content_item_id: ContentItemId,
    #[serde(default)]
    pub(crate) source_metadata: CandidateSourceMetadata,
    #[serde(default)]
    pub(crate) permitted_excerpt: Option<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) provenance: Vec<CandidateProvenance>,
    #[serde(default)]
    pub(crate) media_references: Vec<MediaReference>,
}

/// Typed signed-event body for a Content Reference metadata update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContentItemMetadataUpdatedPayload {
    pub(crate) metadata_update: ContentItemMetadataUpdate,
}

/// One entry in a Pod's complete accepted stream, independent of Feed selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PodContentItem {
    /// Canonical item shared across every independent Pod Placement.
    pub content_item: ContentItem,
    /// Synchronization-safe evidence for this Pod's Accepted Placement.
    pub accepted_placement: AcceptedPlacementProjection,
}

/// Signed withdrawal of one Origin Pod's previously accepted placement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PlacementTombstone {
    /// Reference-first content snapshot retained for required withdrawal audit.
    pub content_reference: FeedContentReference,
    /// Immutable placement evidence that existed before withdrawal.
    pub origin_placement: AcceptedPlacementProjection,
    /// Time at which approval committed the withdrawal.
    pub withdrawn_at: DateTime<Utc>,
}

/// One private Save together with any signed origin withdrawals recorded for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SavedContentReference {
    /// Locally retained reference-first content representation.
    pub content_reference: FeedContentReference,
    /// Signed origin withdrawals retained without cancelling the Save.
    pub origin_withdrawals: Vec<PlacementTombstone>,
}

/// Result of evaluating all proposed placements for a Candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CandidateCurationResult {
    /// Candidate evaluated by this operation.
    pub candidate: Candidate,
    /// Canonical Content Item when any placement was accepted.
    pub content_item: Option<ContentItem>,
    /// Independently evaluated Pod Placements.
    pub placements: Vec<PodPlacement>,
}

/// Authorized review decision for one pending placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PlacementReviewDecision {
    /// Create an Accepted Placement.
    Accept,
    /// Retain a rejected route for audit and suppression.
    Reject,
}

/// Routing Agent request to propose another authorized local Pod Placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct RouteCandidatePlacementRequest {
    /// Authorized local Pod proposed by the Routing Agent.
    pub pod_id: PodId,
    /// Evidence explaining the additional subject match.
    pub reason: CurationRationale,
    /// Bounded routing confidence retained as evidence.
    pub confidence: CandidateConfidence,
}

impl RouteCandidatePlacementRequest {
    /// Creates a validated Routing Agent proposal.
    ///
    /// # Errors
    ///
    /// Returns an error when `reason` is empty or only whitespace.
    pub fn new(
        pod_id: PodId,
        reason: impl Into<String>,
        confidence: CandidateConfidence,
    ) -> Result<Self, CurationRationaleError> {
        Ok(Self {
            pod_id,
            reason: CurationRationale::new(reason)?,
            confidence,
        })
    }
}

/// Explicit User curation request that bypasses Candidate review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct AddContentItemToPodRequest {
    /// Existing canonical Content Item to place.
    pub content_item_id: ContentItemId,
    /// Authorized local Pod receiving the item.
    pub pod_id: PodId,
    /// Optional User-authored curation rationale.
    pub curation_note: Option<CurationRationale>,
}

impl AddContentItemToPodRequest {
    /// Creates an explicit Add to Pod request with an optional validated note.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied note is empty or only whitespace.
    pub fn new(
        content_item_id: ContentItemId,
        pod_id: PodId,
        curation_note: Option<String>,
    ) -> Result<Self, CurationRationaleError> {
        Ok(Self {
            content_item_id,
            pod_id,
            curation_note: curation_note.map(CurationRationale::new).transpose()?,
        })
    }
}

/// Slug of the default private Pod that receives `stumble add` references.
pub const DEFAULT_SAVED_POD_SLUG: &str = "saved";

/// One-shot request that turns a shared URL into Feed-eligible content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct AddReferenceRequest {
    /// External source URL being shared.
    pub url: String,
    /// Target Pod slug; the default private `saved` Pod is used when omitted.
    #[serde(default)]
    pub pod: Option<String>,
    /// Source title when known; the canonical URL is used otherwise.
    #[serde(default)]
    pub title: Option<String>,
    /// Understanding of the source generated by the sharer or their harness.
    #[serde(default)]
    pub summary: Option<String>,
    /// Excerpt that source policy permits Stumble to retain.
    #[serde(default)]
    pub excerpt: Option<String>,
    /// Descriptive subject tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Optional curation rationale recorded on the Accepted Placement.
    #[serde(default)]
    pub note: Option<String>,
    /// Harness-selected illustrative image URLs from the source page,
    /// reference-first (bytes are not archived).
    #[serde(default)]
    pub images: Vec<String>,
}

/// Outcome of a one-shot `stumble add`, including any setup it performed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AddedReference {
    /// Canonical Content Item now placed in the Pod.
    pub content_item: ContentItem,
    /// Pod that received the Accepted Placement.
    pub pod_id: PodId,
    /// Slug of the Pod that received the Accepted Placement.
    pub pod_slug: String,
    /// Whether the default `saved` Pod was created by this call.
    pub pod_created: bool,
    /// Whether the caller's User is subscribed so the item is Feed-eligible.
    pub subscribed: bool,
    /// The Accepted Placement created or confirmed by this call.
    pub placement: PodPlacement,
}

/// Validated non-empty evidence or rationale retained in a curation audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CurationRationale(String);

impl CurationRationale {
    /// Parses a non-empty rationale, trimming surrounding whitespace.
    ///
    /// # Errors
    ///
    /// Returns an error when the rationale is empty or only whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, CurationRationaleError> {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(CurationRationaleError);
        }
        Ok(Self(trimmed.to_string()))
    }

    /// Returns the validated rationale text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CurationRationale {
    type Error = CurationRationaleError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl std::str::FromStr for CurationRationale {
    type Err = CurationRationaleError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl std::fmt::Display for CurationRationale {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CurationRationale {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Error returned for an empty curation rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("curation rationale must not be empty")]
pub struct CurationRationaleError;

/// Coarse external media type supplied by an Agent Harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CandidateContentType {
    Article,
    Video,
    Audio,
    Image,
    Podcast,
    Repository,
    Dataset,
    Other,
}

/// Permitted attached-media category supplied by an Agent Harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MediaReferenceType {
    /// An image available at the referenced source URL.
    Image,
    /// A video available at the referenced source URL.
    Video,
}

/// Reference-first attached media retained without downloading its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaReference {
    /// Typed media category for presentation and policy decisions.
    media_type: MediaReferenceType,
    /// Canonical permitted HTTP(S) location; Stumble does not archive the target bytes.
    url: String,
}

/// Error returned when a URL cannot cross Stumble's canonical URL boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid or unsupported URL: {0}")]
pub struct CanonicalUrlError(String);

impl MediaReference {
    /// Validates and canonicalizes an attached-media reference at its domain boundary.
    pub fn new(
        media_type: MediaReferenceType,
        url: impl AsRef<str>,
    ) -> Result<Self, CanonicalUrlError> {
        Ok(Self {
            media_type,
            url: canonicalize_web_url(url.as_ref())?,
        })
    }

    /// Returns the presentation category supplied for this canonical media identity.
    #[must_use]
    pub const fn media_type(&self) -> MediaReferenceType {
        self.media_type
    }

    /// Returns the canonical permitted HTTP(S) location.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct MediaReferenceWire {
    media_type: MediaReferenceType,
    url: String,
}

impl Serialize for MediaReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        MediaReferenceWire {
            media_type: self.media_type,
            url: self.url.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MediaReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MediaReferenceWire::deserialize(deserializer)?;
        Self::new(wire.media_type, wire.url).map_err(serde::de::Error::custom)
    }
}

/// Applies Stumble's canonical URL spelling policy to a permitted web URL.
pub(crate) fn canonicalize_web_url(value: &str) -> Result<String, CanonicalUrlError> {
    let canonical = canonicalize_url_spelling(value)?;
    let url = url::Url::parse(&canonical).map_err(|error| CanonicalUrlError(error.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(CanonicalUrlError(value.to_string()));
    }
    Ok(canonical)
}

/// Applies Stumble's shared canonical spelling policy without restricting URL schemes.
pub(crate) fn canonicalize_url_spelling(value: &str) -> Result<String, CanonicalUrlError> {
    let mut url = url::Url::parse(value).map_err(|error| CanonicalUrlError(error.to_string()))?;
    url.set_fragment(None);
    if (url.scheme() == "https" && url.port() == Some(443))
        || (url.scheme() == "http" && url.port() == Some(80))
    {
        let _ = url.set_port(None);
    }
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| {
            !matches!(
                key.as_ref(),
                "utm_source" | "utm_medium" | "utm_campaign" | "utm_term" | "utm_content"
            )
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    pairs.sort();
    url.set_query(None);
    if !pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(pairs);
    }
    Ok(url.to_string())
}

/// Error returned when one canonical media identity has incompatible type evidence.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("canonical media URL {url} has conflicting media types")]
pub(crate) struct MediaEvidenceConflictError {
    url: String,
}

/// Resolves media evidence into a canonical, deduplicated, URL-sorted union.
pub(crate) fn resolve_media_evidence<'a>(
    references: impl IntoIterator<Item = &'a MediaReference>,
) -> Result<Vec<MediaReference>, MediaEvidenceConflictError> {
    let mut resolved = BTreeMap::new();
    for reference in references {
        if resolved
            .insert(reference.url(), reference.clone())
            .is_some_and(|existing: MediaReference| existing.media_type() != reference.media_type())
        {
            return Err(MediaEvidenceConflictError {
                url: reference.url().into(),
            });
        }
    }
    Ok(resolved.into_values().collect())
}

/// Harness confidence retained as bounded evidence, never authority.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CandidateConfidence(f32);

// Construction and deserialization reject NaN, making equality reflexive.
impl Eq for CandidateConfidence {}

impl Serialize for CandidateConfidence {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Canonicalize through the shortest decimal that round-trips to this f32.
        // This keeps nested proposal JSON stable across SQLite reloads while
        // retaining a numeric wire value and the exact domain value.
        let canonical = self
            .0
            .to_string()
            .parse::<f64>()
            .expect("a finite f32 always has a valid decimal representation");
        serializer.serialize_f64(canonical)
    }
}

impl CandidateConfidence {
    /// Creates finite confidence evidence in the inclusive range `0.0..=1.0`.
    ///
    /// # Errors
    ///
    /// Returns an error for non-finite or out-of-range values.
    pub fn new(value: f32) -> Result<Self, CandidateConfidenceError> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(CandidateConfidenceError(value))
        }
    }

    /// Returns the wire-compatible confidence value.
    #[must_use]
    pub const fn value(self) -> f32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for CandidateConfidence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(f32::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Error returned for invalid Candidate confidence evidence.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error("candidate confidence must be finite and between 0 and 1, got {0}")]
pub struct CandidateConfidenceError(f32);

/// Canonical private discovery identity shared by independent submissions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// Stable local identity.
    pub id: CandidateId,
    /// Optional hosted tenant boundary.
    pub tenant_id: Option<TenantId>,
    /// Target-neutral canonical source URL; exact URLs remain in scoped evidence.
    pub source_url: String,
    /// Stumble-normalized identity used for deduplication.
    pub canonical_url: String,
    /// Non-authoritative review lifecycle.
    pub review_state: CandidateReviewState,
    /// Time at which Stumble first encountered this canonical identity.
    pub created_at: DateTime<Utc>,
}

/// Source metadata known to the submitting Agent Harness.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CandidateSourceMetadata {
    /// Known source title, when supplied.
    pub title: Option<String>,
    /// Known source author or publisher, when supplied.
    pub author: Option<String>,
    /// Known source publication time, when supplied.
    pub published_at: Option<DateTime<Utc>>,
}

/// Optional permitted source-neighborhood facts for Interest Seed enrichment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
#[non_exhaustive]
pub struct CandidateInterestSeedMetadata {
    /// Publisher distinct from the source author or account.
    pub publisher: Option<String>,
    /// Community in which the reference appeared.
    pub community: Option<String>,
}

impl CandidateInterestSeedMetadata {
    /// Creates optional source-neighborhood metadata for private learning.
    #[must_use]
    pub const fn new(publisher: Option<String>, community: Option<String>) -> Self {
        Self {
            publisher,
            community,
        }
    }
}

/// Inspectable evidence describing how an Agent Harness found a Candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CandidateProvenance {
    /// Time at which the harness discovered the source.
    pub discovered_at: DateTime<Utc>,
    /// Harness-defined method such as `browser_search` or `api_query`.
    pub discovery_method: String,
    /// Page or result from which the source was discovered, when applicable.
    pub referrer_url: Option<String>,
}

/// Evidence proposing that a Candidate belongs in one authorized local Pod.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ProposedCandidatePlacement {
    /// Authorized local Pod proposed by the harness.
    pub pod_id: PodId,
    /// Evidence explaining why the Candidate belongs in this Pod.
    pub reason: String,
    /// Bounded harness confidence retained only as evidence.
    pub confidence: CandidateConfidence,
}

/// Discovery Task and immutable Pod Package version used by a worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CandidateTaskContext {
    /// Claimed Discovery Task used for this submission.
    pub task_id: DiscoveryTaskId,
    /// Immutable Pod Package version used during discovery.
    pub package_version: PackageVersion,
}

/// Complete provenance and placement evidence supplied by an Agent Harness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CandidateSubmissionEvidence {
    /// External source reference proposed by the harness.
    pub source_url: String,
    /// Metadata already known without Stumble fetching the source.
    pub source_metadata: CandidateSourceMetadata,
    /// Excerpt that source policy permits Stumble to retain.
    pub permitted_excerpt: Option<String>,
    /// Harness-generated understanding of the source.
    pub summary: Option<String>,
    /// Coarse external media type.
    pub content_type: CandidateContentType,
    /// Permitted attached-media URL references; no media bytes are retained.
    #[serde(default)]
    pub media_references: Vec<MediaReference>,
    /// Harness-proposed descriptive tags.
    pub tags: Vec<String>,
    /// Evidence describing how the harness found the source.
    pub provenance: CandidateProvenance,
    /// Retry-safe key assigned by the executing harness workflow.
    pub harness_idempotency_key: String,
    /// Retry-safe key assigned by the harness's calling client.
    pub client_idempotency_key: String,
}

/// Strict structured input through which an Agent Harness proposes a Candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CandidateSubmissionRequest {
    /// Explicit operation target, authorized by core against the caller.
    pub target: CandidateSubmissionRequestTarget,
    /// Validated evidence serialized alongside the target.
    #[serde(flatten)]
    pub evidence: CandidateSubmissionEvidence,
}

/// Caller-selected Candidate Submission operation target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CandidateSubmissionRequestTarget {
    /// A direct User action with its private-learning controls.
    User {
        /// Whether this explicit User action contributes private learning evidence.
        #[serde(default = "default_candidate_learning")]
        learn: bool,
        /// Optional source-neighborhood facts permitted for private learning.
        #[serde(default)]
        interest_seed_metadata: CandidateInterestSeedMetadata,
    },
    /// Evidence proposing one or more authorized Pod placements.
    PodPlacements {
        /// Separately evidenced authorized local Pods; validated as non-empty.
        placements: Vec<ProposedCandidatePlacement>,
        /// Owning discovery task and pinned package version, when task-driven.
        task_context: Option<CandidateTaskContext>,
    },
    /// Agent-discovered shortlist item bound to a claimed Personal Discovery Task.
    ///
    /// Never creates Interest Seeds or other learning evidence by itself.
    PersonalDiscovery {
        /// Claimed Personal Discovery Task authorizing this submission.
        task_id: DiscoveryTaskId,
        /// Allocation role under which the worker presents this result.
        allocation_role: DiscoveryPlanSourceRole,
        /// Optional permitted source-neighborhood facts for diversity caps.
        #[serde(default)]
        source_facts: CandidateInterestSeedMetadata,
    },
}

impl CandidateSubmissionRequestTarget {
    /// Returns proposed Pod placements, or an empty slice for non-Pod targets.
    #[must_use]
    pub fn placements(&self) -> &[ProposedCandidatePlacement] {
        match self {
            Self::User { .. } | Self::PersonalDiscovery { .. } => &[],
            Self::PodPlacements { placements, .. } => placements,
        }
    }

    /// Returns the discovery-task context carried by a Pod target, when present.
    #[must_use]
    pub const fn task_context(&self) -> Option<CandidateTaskContext> {
        match self {
            Self::User { .. } | Self::PersonalDiscovery { .. } => None,
            Self::PodPlacements { task_context, .. } => *task_context,
        }
    }

    /// Returns the Personal Discovery task identity, when present.
    #[must_use]
    pub const fn personal_discovery_task_id(&self) -> Option<DiscoveryTaskId> {
        match self {
            Self::PersonalDiscovery { task_id, .. } => Some(*task_id),
            Self::User { .. } | Self::PodPlacements { .. } => None,
        }
    }
}

/// Immutable private evidence retained for one Candidate Submission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateSubmission {
    /// Stable identity of this evidence record.
    pub id: CandidateSubmissionId,
    /// Canonical private Candidate proposed by this record.
    pub candidate_id: CandidateId,
    /// Optional hosted tenant boundary.
    pub tenant_id: Option<TenantId>,
    /// Authenticated harness responsible for the submission.
    pub submitted_by: AgentHarnessId,
    /// Core-authorized target for this evidence record.
    pub target: CandidateSubmissionTarget,
    /// Complete immutable evidence, flattened for wire compatibility.
    #[serde(flatten)]
    pub evidence: CandidateSubmissionEvidence,
    /// Time at which Stumble committed this evidence.
    pub created_at: DateTime<Utc>,
}

/// Summary-rich merged projection used by public Candidate read surfaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateReference {
    /// Exact source location retained by the selected evidence record.
    pub source_url: String,
    /// Source-provided title, author, and publication time when known.
    pub source_metadata: CandidateSourceMetadata,
    /// Source text that policy permits Stumble to retain.
    pub permitted_excerpt: Option<String>,
    /// Harness-generated understanding that survives source deletion.
    pub summary: Option<String>,
    /// Coarse source content type.
    pub content_type: CandidateContentType,
    /// Reference-first media attachments; bytes are not archived.
    pub media_references: Vec<MediaReference>,
    /// Descriptive subject tags.
    pub tags: Vec<String>,
    /// Evidence describing how the source was discovered.
    pub provenance: CandidateProvenance,
}

impl From<&CandidateSubmission> for CandidateReference {
    fn from(submission: &CandidateSubmission) -> Self {
        Self {
            source_url: submission.evidence.source_url.clone(),
            source_metadata: submission.evidence.source_metadata.clone(),
            permitted_excerpt: submission.evidence.permitted_excerpt.clone(),
            summary: submission.evidence.summary.clone(),
            content_type: submission.evidence.content_type,
            media_references: submission.evidence.media_references.clone(),
            tags: submission.evidence.tags.clone(),
            provenance: submission.evidence.provenance.clone(),
        }
    }
}

impl CandidateReference {
    /// Merges visible submissions without allowing sparse later evidence to erase retained facts.
    #[must_use]
    pub fn from_submissions<'a>(
        submissions: impl IntoIterator<Item = &'a CandidateSubmission>,
    ) -> Option<Self> {
        let mut submissions = submissions.into_iter().collect::<Vec<_>>();
        submissions.sort_by_key(|submission| (submission.created_at, submission.id));
        let latest = *submissions.last()?;
        let mut reference = Self::from(latest);

        for submission in submissions.iter().rev().copied() {
            let evidence = &submission.evidence;
            reference.source_metadata.title = reference
                .source_metadata
                .title
                .or_else(|| evidence.source_metadata.title.clone());
            reference.source_metadata.author = reference
                .source_metadata
                .author
                .or_else(|| evidence.source_metadata.author.clone());
            reference.source_metadata.published_at = reference
                .source_metadata
                .published_at
                .or(evidence.source_metadata.published_at);
            reference.permitted_excerpt = reference
                .permitted_excerpt
                .or_else(|| evidence.permitted_excerpt.clone());
            reference.summary = reference.summary.or_else(|| evidence.summary.clone());
            for media in &evidence.media_references {
                if !reference.media_references.contains(media) {
                    reference.media_references.push(media.clone());
                }
            }
            for tag in &evidence.tags {
                if !reference.tags.contains(tag) {
                    reference.tags.push(tag.clone());
                }
            }
        }
        Some(reference)
    }
}

/// Scope governing Candidate Submission authorization and visibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CandidateSubmissionTarget {
    /// Evidence proposed to one or more Pods owned by this target.
    PodPlacements {
        /// Independently evidenced Pod destinations for this submission.
        placements: Vec<ProposedCandidatePlacement>,
        /// Owning discovery task and pinned package version, when task-driven.
        task_context: Option<CandidateTaskContext>,
    },
    /// Private reference submitted directly by this User.
    User {
        /// User whose explicit action created this private evidence.
        user_id: UserId,
        /// Whether this action contributes private learning evidence.
        learn: bool,
        /// Optional source-neighborhood facts permitted for private learning.
        interest_seed_metadata: CandidateInterestSeedMetadata,
    },
    /// Agent-discovered Personal Discovery shortlist item; never User evidence.
    PersonalDiscovery {
        /// User who owns the Personal Discovery Task and plan.
        user_id: UserId,
        /// Claimed Personal Discovery Task.
        task_id: DiscoveryTaskId,
        /// Immutable plan pinned to the task.
        discovery_plan_id: DiscoveryPlanId,
        /// Allocation role under which the worker presented this result.
        allocation_role: DiscoveryPlanSourceRole,
        /// Optional permitted source-neighborhood facts for diversity caps.
        source_facts: CandidateInterestSeedMetadata,
    },
}

impl CandidateSubmissionTarget {
    /// Returns proposed Pod placements, or an empty slice for non-Pod targets.
    #[must_use]
    pub fn placements(&self) -> &[ProposedCandidatePlacement] {
        match self {
            Self::User { .. } | Self::PersonalDiscovery { .. } => &[],
            Self::PodPlacements { placements, .. } => placements,
        }
    }

    /// Returns the acquisition origin derived from the authorized target.
    #[must_use]
    pub const fn acquisition_origin(&self) -> CandidateAcquisitionOrigin {
        match self {
            Self::User { .. } => CandidateAcquisitionOrigin::InteractiveUser,
            Self::PodPlacements { .. } | Self::PersonalDiscovery { .. } => {
                CandidateAcquisitionOrigin::AgentDiscovery
            }
        }
    }

    /// Reports whether this target contributes private learning evidence.
    #[must_use]
    pub const fn learning_enabled(&self) -> bool {
        matches!(self, Self::User { learn: true, .. })
    }

    /// Returns the discovery-task context carried by a Pod target, when present.
    #[must_use]
    pub const fn task_context(&self) -> Option<CandidateTaskContext> {
        match self {
            Self::User { .. } | Self::PersonalDiscovery { .. } => None,
            Self::PodPlacements { task_context, .. } => *task_context,
        }
    }

    /// Returns the Personal Discovery task identity, when present.
    #[must_use]
    pub const fn personal_discovery_task_id(&self) -> Option<DiscoveryTaskId> {
        match self {
            Self::PersonalDiscovery { task_id, .. } => Some(*task_id),
            Self::User { .. } | Self::PodPlacements { .. } => None,
        }
    }

    /// Returns private Interest Seed metadata for a User target.
    #[must_use]
    pub fn interest_seed_metadata(&self) -> Option<&CandidateInterestSeedMetadata> {
        match self {
            Self::User {
                interest_seed_metadata,
                ..
            } => Some(interest_seed_metadata),
            Self::PodPlacements { .. } | Self::PersonalDiscovery { .. } => None,
        }
    }

    /// Returns source facts used for Personal Discovery diversity caps.
    #[must_use]
    pub fn personal_source_facts(&self) -> Option<&CandidateInterestSeedMetadata> {
        match self {
            Self::PersonalDiscovery { source_facts, .. } => Some(source_facts),
            Self::User { .. } | Self::PodPlacements { .. } => None,
        }
    }
}

const fn default_candidate_learning() -> bool {
    true
}

/// Trusted origin of a Candidate Submission, never accepted from caller metadata.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CandidateAcquisitionOrigin {
    /// Conservative migration/default for autonomous or historical submissions.
    #[default]
    AgentDiscovery,
    /// Explicit, core-authorized direct User submission operation.
    InteractiveUser,
}

/// Operation the authenticated harness may perform after receiving Candidate data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CandidateAllowedAction {
    /// Inspect the canonical Candidate and all in-scope evidence.
    InspectCandidate,
    /// Submit another independently provenance-bearing evidence record.
    SubmitCandidateEvidence,
    /// Evaluate every proposed placement under its current Pod policy.
    EvaluateCandidate,
    /// Propose another evidence-backed placement within local Pod scope.
    RouteCandidatePlacement,
    /// Decide one pending placement without changing other placements.
    ReviewCandidatePlacement,
}

/// Result of an idempotent Candidate Submission operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubmittedCandidate {
    /// Canonical private Candidate, reused on canonical deduplication.
    pub candidate: Candidate,
    /// New or idempotently reused evidence record.
    pub submission: CandidateSubmission,
    /// Permission-derived operations the harness can perform next.
    pub allowed_actions: Vec<CandidateAllowedAction>,
}

/// Private Candidate plus every independent provenance-bearing submission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateInspection {
    /// Canonical private Candidate and review state.
    pub candidate: Candidate,
    /// Merged visible summary-rich source reference for list and digest rendering.
    pub reference: CandidateReference,
    /// Independent submissions retained for this canonical identity.
    pub submissions: Vec<CandidateSubmission>,
    /// Independently governed placement states and retained evidence.
    pub placements: Vec<PodPlacement>,
    /// Permission-derived operations the harness can perform next.
    pub allowed_actions: Vec<CandidateAllowedAction>,
}

/// Compact Candidate list item that retains the merged visible source understanding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateListItem {
    /// Canonical private Candidate and review state.
    #[serde(flatten)]
    pub candidate: Candidate,
    /// Merged visible summary-rich source reference.
    pub reference: CandidateReference,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionPod {
    pub submission_id: SubmissionId,
    pub pod_id: PodId,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionAsset {
    pub id: Uuid,
    pub tenant_id: Option<TenantId>,
    pub submission_id: SubmissionId,
    pub asset_type: SubmissionAssetType,
    pub source: SubmissionAssetSource,
    pub url: Option<String>,
    pub local_path: Option<String>,
    pub mime_type: Option<String>,
    pub alt_text: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerSource {
    pub id: Uuid,
    pub tenant_id: Option<TenantId>,
    pub pod_id: PodId,
    pub source_type: CrawlerSourceType,
    pub url: String,
    pub enabled: bool,
    pub crawl_interval_minutes: i32,
    pub last_crawled_at: Option<DateTime<Utc>>,
    pub origin_event_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UserPreferences {
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub interests: Vec<String>,
    pub blocked_topics: Vec<String>,
    pub blocked_sources: Vec<String>,
    #[serde(default)]
    pub blocked_source_affinities: Vec<SourceAffinitySignal>,
    pub preferred_brief_length: usize,
    pub preferred_discovery_mode: DiscoveryMode,
    #[serde(default = "default_recurrence_penalty_days")]
    pub recurrence_penalty_days: RecurrencePenaltyDays,
}

/// User-controlled settings within a private Taste Profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ExplicitTastePreferences {
    /// Topics the User explicitly wants to prioritize.
    pub interests: Vec<String>,
    /// Topics the User explicitly excludes.
    pub blocked_topics: Vec<String>,
    /// Sources the User explicitly excludes.
    pub blocked_sources: Vec<String>,
    /// Typed publisher, author/account, community, referrer, or source exclusions.
    #[serde(default)]
    pub blocked_source_affinities: Vec<SourceAffinitySignal>,
    /// Default recurrence suppression window for Feed Batches.
    pub recurrence_penalty_days: u32,
}

/// Inspectable private personalization state for one User.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TasteProfile {
    /// User who owns this private profile.
    pub user_id: UserId,
    /// Optional hosted tenant boundary.
    pub tenant_id: Option<TenantId>,
    /// User-authored preferences that override inference.
    pub explicit: ExplicitTastePreferences,
    /// Inspectable locally learned weights.
    pub learned: Vec<LearnedTasteWeight>,
    /// Aggregate Interest Seed state without raw URL history.
    pub interest_seed_evidence: InterestSeedEvidenceSummary,
    /// Aggregate topic and source-neighborhood evidence.
    pub source_affinities: Vec<SourceAffinity>,
    /// Permission- and state-derived profile operations for this caller.
    pub allowed_actions: Vec<TasteProfileAllowedAction>,
}

/// Operation currently available through an inspected Taste Profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TasteProfileAllowedAction {
    /// Replace explicit Taste Profile preferences.
    Set,
    /// Reset all or selected learned evidence.
    Reset,
    /// Retract an active Interest Seed contribution.
    Retract,
}

/// Aggregate lifecycle counts for private Interest Seeds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct InterestSeedEvidenceSummary {
    /// Number of currently active private Interest Seeds.
    pub active_seed_count: u32,
    /// Number of retained but retracted private Interest Seeds.
    pub retracted_seed_count: u32,
}

/// Inspectable aggregate affinity learned from User evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SourceAffinity {
    /// Typed source-neighborhood subject of the aggregate affinity.
    pub signal: SourceAffinitySignal,
    /// Bounded ranking adjustment after explicit-preference precedence.
    pub weight: f32,
    /// Number of active Interest Seeds supporting this affinity.
    pub supporting_seeds: u32,
    /// Number of positive feedback events supporting this affinity.
    pub supporting_feedback: u32,
    /// Number of negative feedback events opposing this affinity.
    pub opposing_feedback: u32,
    /// Whether the User explicitly blocks this exact typed affinity.
    pub explicitly_blocked: bool,
}

/// Typed source-neighborhood signal, distinct from topic learning.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SourceAffinitySignal {
    /// Canonical source domain.
    Source(String),
    /// Publisher distinct from an author or account.
    Publisher(String),
    /// Authorship identity or social account.
    AuthorOrAccount(String),
    /// Community in which a reference appeared.
    Community(String),
    /// Canonical domain of the discovery referrer.
    ReferrerContext(String),
}

impl SourceAffinitySignal {
    pub(crate) fn key(&self) -> (&'static str, &str) {
        match self {
            Self::Source(value) => ("source", value),
            Self::Publisher(value) => ("publisher", value),
            Self::AuthorOrAccount(value) => ("author_or_account", value),
            Self::Community(value) => ("community", value),
            Self::ReferrerContext(value) => ("referrer_context", value),
        }
    }

    pub(crate) fn eq_ignore_ascii_case(&self, other: &Self) -> bool {
        let (kind, value) = self.key();
        let (other_kind, other_value) = other.key();
        kind == other_kind && value.eq_ignore_ascii_case(other_value)
    }

    pub(crate) fn normalized(self) -> Option<Self> {
        let mut signal = self;
        let value = match &mut signal {
            Self::Source(value)
            | Self::Publisher(value)
            | Self::AuthorOrAccount(value)
            | Self::Community(value)
            | Self::ReferrerContext(value) => value,
        };
        let normalized = value.trim().to_string();
        if normalized.is_empty() {
            return None;
        }
        *value = normalized;
        Some(signal)
    }
}

/// One explainable learned preference. Evidence is aggregated to avoid exposing raw history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LearnedTasteWeight {
    /// Topic or source represented by this weight.
    pub signal: LearnedTasteSignal,
    /// Bounded ranking adjustment; zero until weak evidence is corroborated.
    pub weight: f32,
    /// Number of aggregate positive actions.
    pub supporting_signals: u32,
    /// Number of aggregate negative actions.
    pub opposing_signals: u32,
    /// Evidence categories and counts without raw history identifiers.
    pub evidence_summary: Vec<LearnedTasteEvidenceSummary>,
}

/// Subject of a learned preference.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[non_exhaustive]
pub enum LearnedTasteSignal {
    /// Normalized subject tag.
    Topic(String),
    /// Normalized source domain.
    Source(String),
    /// Normalized publisher.
    Publisher(String),
    /// Normalized author or account.
    AuthorOrAccount(String),
    /// Normalized community.
    Community(String),
    /// Normalized referring source context.
    ReferrerContext(String),
}

impl LearnedTasteSignal {
    pub(crate) fn key(&self) -> (&'static str, &str) {
        match self {
            Self::Topic(value) => ("topic", value),
            Self::Source(value) => ("source", value),
            Self::Publisher(value) => ("publisher", value),
            Self::AuthorOrAccount(value) => ("author_or_account", value),
            Self::Community(value) => ("community", value),
            Self::ReferrerContext(value) => ("referrer_context", value),
        }
    }

    pub(crate) fn source_affinity(&self) -> Option<SourceAffinitySignal> {
        match self {
            Self::Topic(_) => None,
            Self::Source(value) => Some(SourceAffinitySignal::Source(value.clone())),
            Self::Publisher(value) => Some(SourceAffinitySignal::Publisher(value.clone())),
            Self::AuthorOrAccount(value) => {
                Some(SourceAffinitySignal::AuthorOrAccount(value.clone()))
            }
            Self::Community(value) => Some(SourceAffinitySignal::Community(value.clone())),
            Self::ReferrerContext(value) => {
                Some(SourceAffinitySignal::ReferrerContext(value.clone()))
            }
        }
    }
}

/// Retractable private evidence derived from one canonical User submission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct InterestSeed {
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub candidate_id: CandidateId,
    pub evidence: Vec<InterestSeedSignalEvidence>,
    pub created_at: DateTime<Utc>,
    pub retracted_at: Option<DateTime<Utc>>,
}

/// One enriched Interest Seed signal with its establishing provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct InterestSeedSignalEvidence {
    pub signal: LearnedTasteSignal,
    pub provenance: CandidateProvenance,
}

/// Aggregate evidence kind and count, without Content Item or history identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LearnedTasteEvidenceSummary {
    /// Category of explicit User action.
    pub kind: LearnedTasteEvidenceKind,
    /// Number of actions in this category.
    pub count: u32,
}

/// Private action category contributing to a learned weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LearnedTasteEvidenceKind {
    /// Explicitly learning-enabled User link submission.
    UserSubmission,
    /// Save action.
    Save,
    /// More like this action.
    MoreLikeThis,
    /// Less like this action.
    LessLikeThis,
    /// Dismiss action.
    Dismiss,
    /// Add to Pod action.
    AddToPod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TasteLearningEvidence {
    pub id: Uuid,
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub signal: LearnedTasteSignal,
    pub kind: LearnedTasteEvidenceKind,
    pub direction: TasteEvidenceDirection,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TasteEvidenceDirection {
    Supporting,
    Opposing,
}

/// Edits the explicit layer of a Taste Profile.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct UpdateTasteProfileRequest {
    /// Replacement explicit interests when supplied.
    pub interests: Option<Vec<String>>,
    /// Replacement explicit topic blocks when supplied.
    pub blocked_topics: Option<Vec<String>>,
    /// Replacement explicit source blocks when supplied.
    pub blocked_sources: Option<Vec<String>>,
    /// Replacement typed source-neighborhood blocks when supplied.
    pub blocked_source_affinities: Option<Vec<SourceAffinitySignal>>,
    /// Replacement default Feed recurrence window when supplied.
    pub recurrence_penalty_days: Option<RecurrencePenaltyDays>,
}

/// Selects one learned preference to reset, or all preferences when omitted.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ResetLearnedTasteRequest {
    /// Learned signal to reset, or `None` to reset the complete learned layer.
    pub signal: Option<LearnedTasteSignal>,
}

impl ResetLearnedTasteRequest {
    /// Selects the complete learned layer for reset.
    #[must_use]
    pub const fn all() -> Self {
        Self { signal: None }
    }

    /// Selects one topic or source weight for reset.
    #[must_use]
    pub const fn for_signal(signal: LearnedTasteSignal) -> Self {
        Self {
            signal: Some(signal),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackEvent {
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub submission_id: SubmissionId,
    pub event_type: FeedbackKind,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub local_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Brief {
    pub id: Uuid,
    pub tenant_id: Option<TenantId>,
    pub user_id: Option<UserId>,
    pub title: String,
    pub query: Option<String>,
    pub created_at: DateTime<Utc>,
    pub private: bool,
    pub items: Vec<BriefItem>,
    pub reflection: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefItem {
    pub submission_id: SubmissionId,
    pub role: String,
    pub title: String,
    pub url: String,
    pub summary: String,
    pub why_it_matters: String,
    pub why_user_may_care: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecommendationExplanation {
    pub matched_interests: Vec<String>,
    pub matched_pod_signals: Vec<String>,
    pub blocked_or_downranked_signals_avoided: Vec<String>,
    pub source_reason: String,
    pub novelty_reason: String,
    pub human_or_crawler_origin: String,
    pub final_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryItem {
    pub title: String,
    pub url: String,
    pub short_summary: String,
    pub why_matches_request: String,
    pub why_belongs_in_pod: String,
    pub source: String,
    pub origin: String,
    pub recommendation_explanation: RecommendationExplanation,
    pub submission_id: SubmissionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeInfo {
    pub node_id: NodeIdentityId,
    pub display_name: String,
    pub public_key: String,
    pub supported_protocol_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodManifest {
    pub pod: Pod,
    pub latest_known_event_hash: Option<String>,
    pub skill_pack_version: i32,
    pub public_source_summary: Vec<String>,
}

/// Renewable Announcement Lease duration in whole days.
pub const ANNOUNCEMENT_LEASE_DURATION_DAYS: i64 = 30;

/// Returns the renewable validity period carried by every signed Pod Announcement.
#[must_use]
pub fn announcement_lease_duration() -> chrono::Duration {
    chrono::Duration::days(ANNOUNCEMENT_LEASE_DURATION_DAYS)
}

/// Compact signed advertisement for one public Pod on the Stumble Substrate.
///
/// Announcements identify where authoritative artifacts can be fetched without
/// carrying the Pod Package, Pod Events, or Content Items themselves. Each
/// announcement carries a renewable 30-day Announcement Lease in `expires_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PodAnnouncement {
    /// Stable identity of this signed advertisement.
    pub id: Uuid,
    /// Authoritative Origin Node.
    pub origin_node_id: NodeIdentityId,
    /// Origin identity and verification key.
    pub signer: NodeInfo,
    /// Public Pod identity at the Origin Node.
    pub pod_slug: String,
    /// Human-readable Pod name.
    pub pod_name: String,
    /// Compact subject description used for discovery.
    pub subject: String,
    /// Canonical direct address, independent of any Index Node.
    pub public_pod_url: String,
    /// Current signed Pod Package version.
    pub package_version: PackageVersion,
    /// Latest authoritative Pod Event pointer.
    pub latest_event_hash: Option<String>,
    /// Time at which the Origin Node signed this advertisement.
    pub announced_at: DateTime<Utc>,
    /// Exclusive end of the renewable Announcement Lease (`announced_at` + 30 days).
    /// The lease is active while `expires_at > now`.
    pub expires_at: DateTime<Utc>,
    /// Ed25519 signature over every preceding field.
    pub signature: String,
}

impl PodAnnouncement {
    /// Returns whether this announcement's Announcement Lease is still active at `now`.
    ///
    /// The lease end is exclusive: active only while `expires_at > now`.
    #[must_use]
    pub fn lease_is_active(&self, now: DateTime<Utc>) -> bool {
        self.expires_at > now
    }
}

/// Origin-signed statement that a formerly public Pod leaves new discovery.
///
/// A withdrawal ends announcement relaying and Explore eligibility for the Pod
/// without deleting Subscriptions or previously synchronized content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PodWithdrawal {
    /// Stable identity of this signed withdrawal.
    pub id: Uuid,
    /// Authoritative Origin Node.
    pub origin_node_id: NodeIdentityId,
    /// Origin identity and verification key.
    pub signer: NodeInfo,
    /// Public Pod identity withdrawn from discovery.
    pub pod_slug: String,
    /// Optional canonical direct address covered by the withdrawal.
    pub public_pod_url: Option<String>,
    /// Optional exact announcement identity this withdrawal supersedes.
    pub covers_announcement_id: Option<Uuid>,
    /// Time at which the Origin Node signed the withdrawal.
    pub withdrawn_at: DateTime<Utc>,
    /// Ed25519 signature over every preceding field.
    pub signature: String,
}

/// Locally retained verified Pod Withdrawal and delivery provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct KnownPodWithdrawal {
    /// Origin-authored signed withdrawal, unchanged by relays.
    pub withdrawal: PodWithdrawal,
    /// Trusted peer that delivered it, absent when indexed directly.
    pub received_from_peer_id: Option<PeerId>,
    /// Time at which this node verified and retained it.
    pub received_at: DateTime<Utc>,
}

/// Locally retained verified announcement and its immediate delivery provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct KnownPodAnnouncement {
    /// Origin-authored signed advertisement, unchanged by relays.
    pub announcement: PodAnnouncement,
    /// Trusted peer that delivered it, absent when indexed directly.
    pub received_from_peer_id: Option<PeerId>,
    /// Configured Index Node base URLs that returned this announcement (multi-source).
    ///
    /// Multiple Indexes accumulate across retains of the same signed announcement
    /// identity. Removing an Index excludes announcements whose *only* remaining
    /// delivery source was that Index from current eligibility while preserving
    /// this audit row. Accepts legacy singular `received_from_index_url` on load.
    #[serde(
        default,
        alias = "received_from_index_url",
        deserialize_with = "deserialize_index_provenance_urls"
    )]
    pub received_from_index_urls: BTreeSet<String>,
    /// Bootstrap base URLs that delivered this announcement (multi-source).
    ///
    /// Removing a configured Bootstrap excludes announcements whose *only*
    /// remaining delivery source was that endpoint from current eligibility
    /// while preserving this audit row.
    #[serde(default)]
    pub received_from_bootstrap_urls: BTreeSet<String>,
    /// Discovery Peer public endpoints that delivered this announcement (multi-source).
    ///
    /// Evicting or losing a Discovery Peer excludes announcements whose *only*
    /// remaining delivery source was that peer endpoint from current eligibility
    /// while preserving this audit row. Independent Bootstrap/Index/peer sources
    /// keep the announcement eligible.
    #[serde(default)]
    pub received_from_discovery_peer_endpoints: BTreeSet<String>,
    /// Time at which this node verified and retained it.
    pub received_at: DateTime<Utc>,
}

/// Deserializes multi-Index provenance, migrating legacy singular URL strings.
fn deserialize_index_provenance_urls<'de, D>(deserializer: D) -> Result<BTreeSet<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        Many(Vec<String>),
        One(String),
    }

    match Option::<OneOrMany>::deserialize(deserializer)? {
        None => Ok(BTreeSet::new()),
        Some(OneOrMany::One(url)) => {
            let mut set = BTreeSet::new();
            if !url.is_empty() {
                set.insert(url);
            }
            Ok(set)
        }
        Some(OneOrMany::Many(urls)) => Ok(urls.into_iter().filter(|url| !url.is_empty()).collect()),
    }
}

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

/// Signed recommendation of one public Pod by another public Pod.
///
/// An endorsement is portable evidence only. Each Home Node decides whether
/// and how much it affects local discovery ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PodEndorsement {
    /// Stable signed endorsement identity.
    pub id: Uuid,
    /// Origin Node of the endorsing Pod.
    pub endorsing_node_id: NodeIdentityId,
    /// Identity and key verifying the endorsement.
    pub signer: NodeInfo,
    /// Public Pod making the recommendation.
    pub endorsing_pod_slug: String,
    /// Exact signed announcement establishing the endorsing public Pod.
    pub endorsing_announcement_id: Uuid,
    /// Origin Node of the recommended Pod.
    pub endorsed_node_id: NodeIdentityId,
    /// Recommended public Pod slug.
    pub endorsed_pod_slug: String,
    /// Exact signed announcement considered by the endorser.
    pub endorsed_announcement_id: Uuid,
    /// Human-inspectable reason supplied by the curator.
    pub reason: String,
    /// Time at which the recommendation was signed.
    pub endorsed_at: DateTime<Utc>,
    /// Ed25519 signature over every preceding field.
    pub signature: String,
}

/// Public announcement identity referenced by local agent Pod Similarity evidence.
///
/// Identifies the exact current Pod Announcement the agent used as an input.
/// Never federated and never treated as an Endorsement or global score.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct PodSimilarityAgentEvidenceAnnouncementRef {
    /// Exact signed announcement identity.
    pub announcement_id: Uuid,
    /// Origin Node of the referenced public Pod.
    pub origin_node_id: NodeIdentityId,
    /// Public Pod slug at the Origin.
    pub pod_slug: String,
}

/// Strict structured input through which an authorized harness submits local
/// semantic relationship evidence between two exact current Pod Announcements.
///
/// Evidence remains Home Node private state: it is never exported as an
/// Endorsement, announcement field, global score, or remote interest query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct SubmitPodSimilarityAgentEvidenceRequest {
    /// First exact current Pod Announcement in the semantic relationship.
    pub left_announcement_id: Uuid,
    /// Second exact current Pod Announcement in the semantic relationship.
    pub right_announcement_id: Uuid,
    /// Bounded harness confidence retained only as local ranking evidence.
    pub confidence: CandidateConfidence,
    /// Human-inspectable explanation of the claimed relationship.
    pub explanation: String,
    /// Public inputs the agent used; must include both relationship announcements.
    pub public_inputs: Vec<PodSimilarityAgentEvidenceAnnouncementRef>,
    /// Model or harness provenance used for idempotency and audit.
    pub model_provenance: String,
    /// Retry-safe key assigned by the executing harness workflow.
    pub harness_idempotency_key: String,
    /// Optional requested freshness in hours; Core clamps to policy bounds.
    #[serde(default)]
    pub freshness_hours: Option<u32>,
}

/// Private Home Node record of agent-enriched Pod Similarity evidence.
///
/// Survives SQLite restart with audit provenance. Never creates trust,
/// Subscription, Accepted Placement, or Feed eligibility by itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct PodSimilarityAgentEvidence {
    /// Stable local evidence identity.
    pub id: Uuid,
    /// User whose Home Node ranking may consider this evidence.
    pub user_id: UserId,
    /// Optional hosted tenant boundary.
    pub tenant_id: Option<TenantId>,
    /// Authenticated harness that submitted the evidence.
    pub submitted_by: AgentHarnessId,
    /// First announcement bound by the semantic relationship.
    pub left: PodSimilarityAgentEvidenceAnnouncementRef,
    /// Second announcement bound by the semantic relationship.
    pub right: PodSimilarityAgentEvidenceAnnouncementRef,
    /// Bounded confidence retained only as local ranking evidence.
    pub confidence: CandidateConfidence,
    /// Human-inspectable explanation.
    pub explanation: String,
    /// Public inputs identified at submission time.
    pub public_inputs: Vec<PodSimilarityAgentEvidenceAnnouncementRef>,
    /// Model or harness provenance for idempotency and audit.
    pub model_provenance: String,
    /// Retry-safe key assigned by the executing harness workflow.
    pub harness_idempotency_key: String,
    /// Time at which Core accepted the evidence.
    pub submitted_at: DateTime<Utc>,
    /// Time after which the evidence is no longer active for ranking.
    pub expires_at: DateTime<Utc>,
}

/// Origin-signed permitted Content Reference samples for one announcement.
///
/// This bounded Explore artifact is separate from the compact announcement and
/// does not synchronize the Pod or create a Subscription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PodExploreSamples {
    /// Stable signed sample artifact identity.
    pub id: Uuid,
    /// Exact current announcement these samples describe.
    pub announcement_id: Uuid,
    /// Authoritative Origin Node.
    pub origin_node_id: NodeIdentityId,
    /// Origin identity and verification key.
    pub signer: NodeInfo,
    /// Public Pod identity at the Origin Node.
    pub pod_slug: String,
    /// Bounded reference-first public samples.
    pub samples: Vec<FeedContentReference>,
    /// Time at which the Origin Node selected the samples.
    pub sampled_at: DateTime<Utc>,
    /// Ed25519 signature over every preceding field.
    pub signature: String,
}

/// Bounded request for intentional public Pod discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ExploreRequest {
    /// Explicit subject query; an empty query returns the available public set.
    pub query: String,
    /// Maximum public Pods to return.
    pub limit: usize,
    /// Maximum permitted Content References sampled from each locally known Pod.
    pub sample_size: usize,
}

impl ExploreRequest {
    /// Creates a bounded Explore request.
    ///
    /// # Errors
    ///
    /// Returns an error unless `limit` is `1..=50` and `sample_size` is `0..=10`.
    pub fn new(
        query: impl Into<String>,
        limit: usize,
        sample_size: usize,
    ) -> Result<Self, ExploreRequestError> {
        if !(1..=50).contains(&limit) || sample_size > 10 {
            return Err(ExploreRequestError);
        }
        Ok(Self {
            query: query.into(),
            limit,
            sample_size,
        })
    }
}

/// Error returned for an out-of-range Explore request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Explore limit must be 1 to 50 and sample size must be at most 10")]
#[non_exhaustive]
pub struct ExploreRequestError;

/// One public Pod returned through intentional Explore.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ExplorePodResult {
    /// Verified public Pod advertisement.
    pub announcement: PodAnnouncement,
    /// Query and local-evidence relevance, never universal reputation.
    pub relevance: f32,
    /// Inspectable reasons for this Home Node's ordering.
    pub reasons: Vec<String>,
    /// Optional signed evidence considered locally.
    pub endorsements: Vec<PodEndorsement>,
    /// Permitted local samples, filtered by the User's Trust Policy.
    pub sample_content_references: Vec<FeedContentReference>,
    /// Whether the User already has a Subscription to this public Pod.
    pub is_subscribed: bool,
    /// Limited labeled trial exposure for a strongly similar unendorsed Pod.
    #[serde(default)]
    pub trial_exposure: bool,
}

/// Structured response from intentional public Pod discovery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ExploreResponse {
    /// Normalized explicit query.
    pub query: String,
    /// Locally filtered, non-authoritative public Pod results.
    pub results: Vec<ExplorePodResult>,
}

/// Signed public artifacts exported by an Origin Node for one Pod.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FederationPodSnapshot {
    /// Origin identity whose public key verifies every returned Pod Event.
    pub node: NodeInfo,
    /// Current public Pod metadata and latest signed-event pointer.
    pub manifest: PodManifest,
    /// Events after the requested cursor, in append order.
    pub events: Vec<EventLog>,
}

impl FederationPodSnapshot {
    /// Creates an ordered snapshot fetched from an Origin Node.
    #[must_use]
    pub const fn new(node: NodeInfo, manifest: PodManifest, events: Vec<EventLog>) -> Self {
        Self {
            node,
            manifest,
            events,
        }
    }
}

/// Local-only relationship making one Pod Feed-eligible for one User.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Subscription {
    /// Stable Home Node identity for the relationship.
    pub id: SubscriptionId,
    /// User whose Feed may use synchronized accepted content.
    pub user_id: UserId,
    /// Optional hosted tenant boundary.
    pub tenant_id: Option<TenantId>,
    /// Canonical direct address; local Pods use the `stumble://local/pods/` scheme.
    pub public_pod_url: String,
    /// Authoritative Origin Node identity.
    pub origin_node_id: NodeIdentityId,
    /// Origin Node verification key pinned at subscription time.
    pub origin_public_key: String,
    /// Public Pod slug at the direct address.
    pub pod_slug: String,
    /// Local projected Pod identity.
    pub local_pod_id: PodId,
    /// Whether this Subscription receives bounded Feed representation.
    #[serde(default)]
    pub is_priority: bool,
    /// Last contiguous signed event projected by the Home Node.
    pub last_event_hash: Option<String>,
    /// Time at which the User subscribed.
    pub created_at: DateTime<Utc>,
    /// Time of the latest successful synchronization attempt.
    pub synchronized_at: DateTime<Utc>,
    /// Most recent failed refresh, cleared by the next successful synchronization.
    #[serde(default)]
    pub last_sync_failure: Option<SynchronizationFailure>,
}

impl Subscription {
    /// Creates Feed eligibility for a Pod hosted on this Home Node.
    #[must_use]
    pub fn new_local(
        id: SubscriptionId,
        user_id: UserId,
        pod: &Pod,
        node: &NodeIdentity,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            user_id,
            tenant_id: pod.tenant_id,
            public_pod_url: format!("stumble://local/pods/{}", pod.id),
            origin_node_id: node.id,
            origin_public_key: node.public_key.clone(),
            pod_slug: pod.slug.clone(),
            local_pod_id: pod.id,
            is_priority: false,
            last_event_hash: None,
            created_at,
            synchronized_at: created_at,
            last_sync_failure: None,
        }
    }
}

/// Persisted operator-facing failure from the latest Subscription refresh attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SynchronizationFailure {
    /// Stable failure category suitable for recovery routing.
    pub code: String,
    /// Human-readable diagnostic without secret material.
    pub message: String,
    /// Whether retrying the high-level synchronization workflow may recover.
    pub retryable: bool,
    /// Time the failed attempt completed.
    pub occurred_at: DateTime<Utc>,
}

/// Request to enable or disable one User's Priority Subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct SetPrioritySubscriptionRequest {
    /// Subscribed Pod whose bounded representation changes.
    pub pod_id: PodId,
    /// Whether future Feed Batches should guarantee bounded representation.
    pub is_priority: bool,
}

impl SetPrioritySubscriptionRequest {
    /// Creates a Priority Subscription update.
    #[must_use]
    pub const fn new(pod_id: PodId, is_priority: bool) -> Self {
        Self {
            pod_id,
            is_priority,
        }
    }
}

/// Direct-address request containing artifacts fetched outbound from an Origin Node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct SubscribePublicPodRequest {
    /// Canonical public Pod URL used for future outbound synchronization.
    pub public_pod_url: String,
    /// Origin snapshot fetched from that address.
    pub snapshot: FederationPodSnapshot,
}

impl SubscribePublicPodRequest {
    /// Creates a direct-address Subscription request.
    #[must_use]
    pub fn new(public_pod_url: impl Into<String>, snapshot: FederationPodSnapshot) -> Self {
        Self {
            public_pod_url: public_pod_url.into(),
            snapshot,
        }
    }
}

/// Observable result of creating or incrementally refreshing a Subscription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SynchronizationResult {
    /// Updated local Subscription and cursor.
    pub subscription: Subscription,
    /// Number of newly projected signed events.
    pub imported_events: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WellKnownNode {
    pub protocol: String,
    pub node: NodeInfo,
    pub endpoints: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedSkillPack {
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    pub user_id: Option<UserId>,
    pub tenant_id: Option<TenantId>,
    pub node_id: NodeIdentityId,
    /// Harness whose scoped grant applies to this request.
    #[serde(default)]
    pub harness_id: Option<AgentHarnessId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatePodRequest {
    pub name: String,
    pub slug: String,
    pub description: String,
    pub visibility: Visibility,
}

/// Complete portable contents of one Pod Package version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    try_from = "RawPodPackageContents"
)]
pub struct PodPackageContents {
    /// Subject language, scope, and boundaries.
    pub context_md: String,
    /// Scoped, untrusted harness instructions.
    pub skill_md: String,
    /// Declarative Source Rules.
    pub sources_yaml: String,
    /// Pod-owned filters.
    pub filters_yaml: String,
    /// Positive calibration examples.
    pub examples_good_md: String,
    /// Negative calibration examples.
    pub examples_bad_md: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct RawPodPackageContents {
    pub context_md: String,
    pub skill_md: String,
    pub sources_yaml: String,
    pub filters_yaml: String,
    pub examples_good_md: String,
    pub examples_bad_md: String,
}

/// Atomic request to create a private Pod and its first package version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CreatePrivatePodWithPackageRequest {
    /// Display name for the private Pod.
    pub name: String,
    /// Stable local URL slug.
    pub slug: String,
    /// Human-readable Pod summary.
    pub description: String,
    /// Complete validated initial package contents.
    pub package: PodPackageContents,
}

/// Result of atomically creating a private Pod and package.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CreatedPodPackage {
    /// Newly created private Pod.
    pub pod: Pod,
    /// Immutable initial package version.
    pub package: PodPackage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitLinkRequest {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub note: Option<String>,
    pub tags: Vec<String>,
    pub discovered_by_crawler: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionWithAssets {
    pub submission: Submission,
    pub assets: Vec<SubmissionAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkIntakeRequest {
    pub url: String,
    pub note: Option<String>,
    pub tags: Vec<String>,
    pub representative_image: Option<RepresentativeImageRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoRouteIntakeRequest {
    pub url: String,
    pub note: Option<String>,
    pub tags: Vec<String>,
    pub representative_image: Option<RepresentativeImageRequest>,
    pub min_confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepresentativeImageRequest {
    pub source: SubmissionAssetSource,
    pub url: Option<String>,
    pub local_path: Option<String>,
    pub mime_type: Option<String>,
    pub alt_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadableSnapshotRequest {
    pub source: ReadableSnapshotSource,
    pub local_path: String,
    pub mime_type: Option<String>,
}

/// Origin of a Readable Snapshot's text. A snapshot is an archive of what
/// the page said, so an AI-generated source does not exist here (ADR-0052).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadableSnapshotSource {
    PageText,
    UserProvided,
}

impl From<ReadableSnapshotSource> for SubmissionAssetSource {
    fn from(source: ReadableSnapshotSource) -> Self {
        match source {
            ReadableSnapshotSource::PageText => Self::PageText,
            ReadableSnapshotSource::UserProvided => Self::UserProvided,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkIntakeResponse {
    pub submission: Submission,
    pub assets: Vec<SubmissionAsset>,
    pub fetched_title: Option<String>,
    pub fetched_summary: Option<String>,
    pub representative_image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodRouteCandidate {
    pub pod_slug: String,
    pub pod_name: String,
    pub score: f32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteLinkRequest {
    pub url: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteLinkResponse {
    pub candidates: Vec<PodRouteCandidate>,
    pub selected: Option<PodRouteCandidate>,
    pub needs_confirmation: bool,
    pub confidence_threshold: f32,
    pub suggested_new_pod: Option<CreatePodRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoRouteIntakeResponse {
    pub routing: RouteLinkResponse,
    pub intake: Option<LinkIntakeResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverRequest {
    pub query: String,
    pub avoid: Vec<String>,
    pub limit: usize,
    pub mode: DiscoveryMode,
    pub user_id: Option<UserId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateBriefRequest {
    pub pod_slugs: Vec<String>,
    pub query: Option<String>,
    pub user_id: Option<UserId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPackPatch {
    #[serde(default)]
    pub context_md: Option<String>,
    pub pod_yaml: Option<String>,
    pub skill_md: Option<String>,
    pub sources_yaml: Option<String>,
    pub filters_yaml: Option<String>,
    pub examples_good_md: Option<String>,
    pub examples_bad_md: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTenantRequest {
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevTokenRequest {
    pub user_id: Option<UserId>,
    pub tenant_slug: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevTokenResponse {
    pub token: String,
    pub token_hash: String,
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
}

/// Strict registration payload for a new Agent Harness and Harness Grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterAgentHarnessRequest {
    /// Human-readable operator label.
    pub label: String,
    /// Interactive or unattended operating mode.
    pub kind: AgentHarnessKind,
    /// Independently allowed operations.
    pub capabilities: Vec<HarnessCapability>,
    /// Optional allowlist of local Pods.
    pub pod_ids: Option<Vec<PodId>>,
}

/// One-time bearer token whose diagnostic formatting is always redacted.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HarnessToken(String);

impl HarnessToken {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    /// Borrows the token for authentication without copying it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for HarnessToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("HarnessToken([redacted])")
    }
}

/// Result of registration. The plaintext token is never returned again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterAgentHarnessResponse {
    /// Persisted harness and normalized grant.
    pub harness: AgentHarness,
    /// Returned only at registration. Only its hash is retained by the Home Node.
    pub token: HarnessToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePreferencesRequest {
    pub interests: Option<Vec<String>>,
    pub blocked_topics: Option<Vec<String>>,
    pub blocked_sources: Option<Vec<String>>,
    pub preferred_brief_length: Option<usize>,
    pub preferred_discovery_mode: Option<DiscoveryMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodAgentContext {
    pub pod_slug: String,
    pub pod_name: String,
    pub skill_pack_version: i32,
    pub skill_md: String,
    pub pod_yaml: String,
    pub filters_yaml: String,
    pub validation: ValidationReport,
}
