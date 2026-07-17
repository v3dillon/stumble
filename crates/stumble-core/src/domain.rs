use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DiscoveryTaskId(Uuid);

impl From<Uuid> for DiscoveryTaskId {
    fn from(value: Uuid) -> Self {
        Self(value)
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
    Moderator,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMode {
    DeepMatch,
    Adjacent,
    OldGem,
    HumanPick,
    RabbitHole,
    Stumble,
}

impl Default for DiscoveryMode {
    fn default() -> Self {
        Self::DeepMatch
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrawlerSourceType {
    Rss,
    Atom,
    Sitemap,
    Webpage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrawlCandidateStatus {
    Pending,
    Promoted,
    Rejected,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
        })
    }

    /// Sets an exact per-request recurrence override.
    #[must_use]
    pub const fn with_recurrence_penalty_days(mut self, days: RecurrencePenaltyDays) -> Self {
        self.recurrence_penalty_days = Some(days);
        self
    }
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionAssetSource {
    PageImage,
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
}

impl std::fmt::Display for HarnessCapability {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FeedRead => "feed_read",
            Self::Feedback => "feedback",
            Self::DiscoveryTasks => "discovery_tasks",
            Self::CandidateSubmission => "candidate_submission",
            Self::PodCuration => "pod_curation",
            Self::PackageManagement => "package_management",
            Self::SubscriptionManagement => "subscription_management",
            Self::Administration => "administration",
            Self::Approval => "approval",
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
            "candidate_submission" => Ok(Self::CandidateSubmission),
            "pod_curation" => Ok(Self::PodCuration),
            "package_management" => Ok(Self::PackageManagement),
            "subscription_management" => Ok(Self::SubscriptionManagement),
            "administration" => Ok(Self::Administration),
            "approval" => Ok(Self::Approval),
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
    /// Expose an existing private Pod through public federation surfaces.
    PublishPod { pod_id: PodId },
    /// Expand the capabilities or Pod scope of an existing Harness Grant.
    ExpandHarnessGrant {
        harness_id: AgentHarnessId,
        capabilities: Vec<HarnessCapability>,
        pod_ids: Option<Vec<PodId>>,
    },
    /// Add a node to the local Trust Policy.
    AddTrustedPeer {
        display_name: String,
        base_url: String,
        public_key: String,
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
    pub decided_by: Option<AgentHarnessId>,
    /// Decision or expiry time, when terminal.
    pub decided_at: Option<DateTime<Utc>>,
    /// Optional reason supplied when rejecting the proposal.
    pub rejection_reason: Option<String>,
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
    PodPackage(PodId),
    PodCurationPolicy(PodId),
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

/// Type-safe operation recorded in a harness write audit entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessWriteOperation {
    RegisterAgentHarness,
    RevokeAgentHarness,
    CreateTenant,
    CreateDevToken,
    AddTrustedPeer,
    ImportPodEvents,
    IndexPublicPods,
    CreatePod,
    JoinPod,
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
    CreateCrawlCandidate,
    PromoteCrawlCandidate,
    SaveLink,
    RecordFeedFeedback,
    BlockSource,
    BlockTopic,
    UpdatePreferences,
    ResetLearnedTaste,
    CreateDiscoveryTask,
    ClaimDiscoveryTask,
    RenewDiscoveryTaskLease,
    CompleteDiscoveryTask,
    FailDiscoveryTask,
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

/// Leaseable discovery work derived from a Source Rule or immediate request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryTask {
    /// Stable task identity.
    pub id: DiscoveryTaskId,
    /// Pod whose Package governs this work.
    pub pod_id: PodId,
    /// Immutable Package version used by the worker.
    pub package_version: PackageVersion,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodMembership {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FederatedPodEventType {
    PodCreated,
    PodPublished,
    PodSkillPackUpdated,
    PodPackageImported,
    PodPackageForked,
    ContentItemPlaced,
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
    pub description: Option<String>,
    pub domain: String,
    pub submitted_by: Option<UserId>,
    pub discovered_by_crawler: bool,
    pub submitter_note: Option<String>,
    pub summary: Option<String>,
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
    permitted_description: Option<String>,
    domain: String,
    summary: Option<String>,
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
            permitted_description: self.legacy_record.description.clone(),
            domain: self.legacy_record.domain.clone(),
            summary: self.legacy_record.summary.clone(),
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
                description: wire.permitted_description,
                domain: wire.domain,
                submitted_by: None,
                discovered_by_crawler: false,
                submitter_note: None,
                summary: wire.summary,
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

/// Harness confidence retained as bounded evidence, never authority.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CandidateConfidence(f32);

// Construction and deserialization reject NaN, making equality reflexive.
impl Eq for CandidateConfidence {}

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
    /// Source URL exactly as first submitted.
    pub source_url: String,
    /// Stumble-normalized identity used for deduplication.
    pub canonical_url: String,
    /// Non-authoritative review lifecycle.
    pub review_state: CandidateReviewState,
    /// Time at which Stumble first encountered this canonical identity.
    pub created_at: DateTime<Utc>,
}

/// Source metadata known to the submitting Agent Harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CandidateSourceMetadata {
    /// Known source title, when supplied.
    pub title: Option<String>,
    /// Known source author or publisher, when supplied.
    pub author: Option<String>,
    /// Known source publication time, when supplied.
    pub published_at: Option<DateTime<Utc>>,
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
    /// Harness-proposed descriptive tags.
    pub tags: Vec<String>,
    /// Evidence describing how the harness found the source.
    pub provenance: CandidateProvenance,
    /// One or more separately evidenced authorized local Pods.
    pub proposed_placements: Vec<ProposedCandidatePlacement>,
    /// Claimed task and Package version for task-driven discovery.
    pub task_context: Option<CandidateTaskContext>,
    /// Retry-safe key assigned by the executing harness workflow.
    pub harness_idempotency_key: String,
    /// Retry-safe key assigned by the harness's calling client.
    pub client_idempotency_key: String,
}

/// Strict structured input through which an Agent Harness proposes a Candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CandidateSubmissionRequest {
    /// Validated evidence serialized directly as the request object.
    pub evidence: CandidateSubmissionEvidence,
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
    /// Complete immutable evidence, flattened for wire compatibility.
    #[serde(flatten)]
    pub evidence: CandidateSubmissionEvidence,
    /// Time at which Stumble committed this evidence.
    pub created_at: DateTime<Utc>,
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
    /// Independent submissions retained for this canonical identity.
    pub submissions: Vec<CandidateSubmission>,
    /// Permission-derived operations the harness can perform next.
    pub allowed_actions: Vec<CandidateAllowedAction>,
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
pub struct CrawlCandidate {
    pub id: Uuid,
    pub tenant_id: Option<TenantId>,
    pub pod_id: PodId,
    pub crawler_source_id: Uuid,
    pub url: String,
    pub canonical_url: String,
    pub title: String,
    pub description: Option<String>,
    pub domain: String,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub status: CrawlCandidateStatus,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UserPreferences {
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub interests: Vec<String>,
    pub blocked_topics: Vec<String>,
    pub blocked_sources: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Local-only relationship making one remote public Pod Feed-eligible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Subscription {
    /// Stable Home Node identity for the relationship.
    pub id: SubscriptionId,
    /// User whose Feed may use synchronized accepted content.
    pub user_id: UserId,
    /// Optional hosted tenant boundary.
    pub tenant_id: Option<TenantId>,
    /// Canonical direct address supplied by the User.
    pub public_pod_url: String,
    /// Authoritative Origin Node identity.
    pub origin_node_id: NodeIdentityId,
    /// Origin Node verification key pinned at subscription time.
    pub origin_public_key: String,
    /// Public Pod slug at the direct address.
    pub pod_slug: String,
    /// Local projected Pod identity.
    pub local_pod_id: PodId,
    /// Last contiguous signed event projected by the Home Node.
    pub last_event_hash: Option<String>,
    /// Time at which the User subscribed.
    pub created_at: DateTime<Utc>,
    /// Time of the latest successful synchronization attempt.
    pub synchronized_at: DateTime<Utc>,
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
pub struct HubRegisteredNode {
    pub node_id: NodeIdentityId,
    pub display_name: String,
    pub base_url: String,
    pub public_key: String,
    pub protocol_version: String,
    pub registered_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubRegisteredPod {
    pub id: Uuid,
    pub node_id: NodeIdentityId,
    pub node_base_url: String,
    pub pod_slug: String,
    pub pod_name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub skill_pack_version: i32,
    pub latest_event_hash: Option<String>,
    pub manifest_url: String,
    pub events_url: String,
    pub registered_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubRegisterNodeRequest {
    pub node_id: NodeIdentityId,
    pub display_name: String,
    pub base_url: String,
    pub public_key: String,
    pub protocol_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubRegisterPodRequest {
    pub node_id: NodeIdentityId,
    pub node_base_url: String,
    pub pod_slug: String,
    pub pod_name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub skill_pack_version: i32,
    pub latest_event_hash: Option<String>,
    pub manifest_url: String,
    pub events_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubSearchPodResult {
    pub pod: HubRegisteredPod,
    pub score: f32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubSearchPodsResponse {
    pub query: String,
    pub results: Vec<HubSearchPodResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PodDiscoveryScope {
    Local,
    Global,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodDiscoveryFeedItem {
    pub pod: HubRegisteredPod,
    pub scope: PodDiscoveryScope,
    pub score: f32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodDiscoveryFeedResponse {
    pub query: String,
    pub local_public_pods: Vec<PodDiscoveryFeedItem>,
    pub global_public_pods: Vec<PodDiscoveryFeedItem>,
    pub private_interests_exported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomePublicPodDiscoveryResponse {
    pub topics: Vec<String>,
    pub local_public_pods: Vec<PodRouteCandidate>,
    pub hub_results: Vec<HubSearchPodResult>,
    pub private_interests_exported: bool,
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
