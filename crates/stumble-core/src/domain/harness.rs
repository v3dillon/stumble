use super::*;

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
    /// Delete a locally owned public Pod after independent approval.
    DeletePod { pod_id: PodId },
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

/// Local result of deleting an owned Pod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeletedPod {
    /// Removed local Pod identity.
    pub pod_id: PodId,
    /// Origin-local slug of the removed Pod.
    pub slug: String,
    /// Whether an Origin-signed Pod Withdrawal was issued.
    pub withdrawn: bool,
}

/// Outcome of requesting Pod deletion through the sensitive-change policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "result", rename_all = "snake_case")]
pub enum DeletePodOutcome {
    Deleted(DeletedPod),
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
    /// Remove a locally owned Pod from this Home Node.
    DeletePod,
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
    /// Replace the private User Context prose.
    SetUserContext,
    /// Add a private User-scoped watch.
    AddUserWatch,
    /// Remove a private User-scoped watch.
    RemoveUserWatch,
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
