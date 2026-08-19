use super::*;

/// Private markdown prose the interactive User keeps about themself.
///
/// Like a Pod `CONTEXT.md` for the person. Local Home Node state only: it is
/// not a skill, never federates, and never appears on minimized worker plans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserContext {
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    /// User-authored markdown prose; only the interactive User (or a draft
    /// they accepted) writes it. Names durable interests and refusals; not a
    /// recap of the collection. Agent finds never train it.
    pub context_md: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Replaces the private User Context prose.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct SetUserContextRequest {
    /// Replacement markdown prose (an empty string clears the context).
    pub context_md: String,
}

/// Operation currently available through the User Context briefing packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UserContextAllowedAction {
    /// Replace the User Context prose (interactive User only).
    Set,
}

/// One briefing packet returned by `stumble context show`.
///
/// Private and interactive-only. Unattended personal discovery workers never
/// read this packet; they see only the minimized Discovery Plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UserContextPacket {
    /// User Context prose; empty string when unset.
    pub context_md: String,
    /// Same payload as `stumble feed taste show`.
    pub taste: TasteProfile,
    /// User-scoped watches; empty list when none exist.
    pub watches: Vec<UserWatch>,
    /// Same payload as `stumble discover personal readiness`.
    pub readiness: PersonalDiscoveryReadiness,
    /// Permission-derived operations for this caller.
    pub allowed_actions: Vec<UserContextAllowedAction>,
}

/// What kind of place a User watch points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UserWatchKind {
    /// A logged-in home timeline (for example an X timeline).
    Timeline,
    /// One account's public or followed feed.
    Account,
    /// A plain website or section page.
    Site,
}

/// How often a watch becomes due for the Personal Discovery plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UserWatchCadence {
    Hourly,
    #[default]
    Daily,
    Weekly,
}

impl UserWatchCadence {
    /// Deterministic period start shared with Personal Discovery cadences.
    #[must_use]
    pub fn period_start(self, now: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            Self::Hourly => PersonalDiscoveryCadence::Hourly.period_start(now),
            Self::Daily => PersonalDiscoveryCadence::Daily.period_start(now),
            Self::Weekly => PersonalDiscoveryCadence::Weekly.period_start(now),
        }
    }
}

/// One User-scoped standing watch over a source the User already trusts.
///
/// Watches live on the User, not on a Pod: they are not Pod Source Rules, are
/// not owned by the Inbox, and never federate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserWatch {
    pub id: UserWatchId,
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    /// Source locator the harness reads with its own tools.
    pub url: String,
    pub kind: UserWatchKind,
    #[serde(default)]
    pub cadence: UserWatchCadence,
    /// Optional harness-local skill name, stored only when the caller sets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    /// Latest worker-reported availability fact; never auth material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_availability: Option<ReportedSourceAvailability>,
    /// Period start of the last plan that carried this watch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_planned_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl UserWatch {
    /// Whether the watch belongs in the next Personal Discovery plan.
    #[must_use]
    pub fn is_due(&self, now: DateTime<Utc>) -> bool {
        match self.last_planned_at {
            None => true,
            Some(last_planned_at) => self.cadence.period_start(now) > last_planned_at,
        }
    }
}

/// Adds a User-scoped watch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddUserWatchRequest {
    pub url: String,
    pub kind: UserWatchKind,
    #[serde(default)]
    pub cadence: Option<UserWatchCadence>,
    #[serde(default)]
    pub skill: Option<String>,
}

/// One composed morning brief. Every section is always present.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MorningBrief {
    pub user: MorningBriefUser,
    pub outside: MorningBriefOutside,
    pub network: MorningBriefNetwork,
    pub gaps: Vec<MorningBriefGap>,
}

/// User Context prose plus a short taste line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MorningBriefUser {
    pub context_md: String,
    pub taste_summary: String,
}

/// Latest ready Discovery Result Batch, or an empty section with a reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MorningBriefOutside {
    pub batch_id: Option<DiscoveryResultBatchId>,
    pub items: Vec<DiscoveryResultItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_availability: Vec<DiscoveryResultAvailabilityReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Network half: current Feed Batch items and at most one Explore Pod.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MorningBriefNetwork {
    pub feed: Vec<FeedBatchItem>,
    pub explore: Vec<ExplorePodResult>,
}

/// One inspectable shortfall in the brief (login, bootstrap, missing grant).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MorningBriefGap {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch_id: Option<UserWatchId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}
