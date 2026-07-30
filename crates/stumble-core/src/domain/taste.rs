use super::*;

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
