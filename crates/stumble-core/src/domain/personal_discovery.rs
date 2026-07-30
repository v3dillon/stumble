use super::*;

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
