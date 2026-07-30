use super::*;

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
