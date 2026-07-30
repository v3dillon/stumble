use super::*;

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

pub(super) const fn default_recurrence_penalty_days() -> RecurrencePenaltyDays {
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
