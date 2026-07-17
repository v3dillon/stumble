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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackKind {
    Interesting,
    NotForMe,
    Dismissed,
    Saved,
    BlockSource,
    BlockTopic,
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
    SubmitLinkToPod,
    SubmitCandidate,
    RemoveSubmissionFromPod,
    AddSubmissionAsset,
    GenerateBrief,
    PatchSkillPack,
    ImportSkillPack,
    ForkSkillPack,
    AddSourceToPod,
    CreateCrawlCandidate,
    PromoteCrawlCandidate,
    SaveLink,
    BlockSource,
    BlockTopic,
    UpdatePreferences,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

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
pub struct UserPreferences {
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub interests: Vec<String>,
    pub blocked_topics: Vec<String>,
    pub blocked_sources: Vec<String>,
    pub preferred_brief_length: usize,
    pub preferred_discovery_mode: DiscoveryMode,
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
