use super::*;

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
