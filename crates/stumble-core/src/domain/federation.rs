use super::*;

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
