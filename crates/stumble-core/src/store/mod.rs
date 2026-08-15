//! The Home Node store: an in-memory domain state with authoritative SQLite
//! persistence and a legacy JSON snapshot format.
//!
//! - [`registry`] declares every persisted collection once (wire struct,
//!   collection names, and record keys are generated together).
//! - [`sqlite`] owns schema application and change persistence.
//! - [`migrations`] owns forward migrations of persisted values.
//! - [`snapshot`] reads and writes the legacy JSON format.
//! - [`queries`] holds domain queries over [`InMemoryStore`].

mod migrations;
mod queries;
mod registry;
mod snapshot;
mod sqlite;

pub(crate) use registry::{store_records, StoreRecords};
pub use snapshot::{load_store_snapshot, save_store_snapshot};
pub(crate) use sqlite::{apply_sqlite_schema, open_sqlite_store};
pub use sqlite::{
    load_or_initialize_sqlite_store, load_sqlite_store, persist_sqlite_store_changes,
    read_store_generation, sqlite_home_node_is_initialized,
};

use crate::domain::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("duplicate: {0}")]
    Duplicate(String),
    #[error("tenant boundary violation")]
    TenantBoundary,
    #[error("untrusted peer")]
    UntrustedPeer,
    #[error("invalid event signature")]
    InvalidSignature,
    #[error("pod announcement lease expired")]
    AnnouncementExpired,
    #[error("pod has been withdrawn from discovery")]
    AnnouncementWithdrawn,
    #[error("pod announcement is stale")]
    AnnouncementStale,
    #[error("pod withdrawal is stale")]
    WithdrawalStale,
    #[error("validation failed: {0}")]
    Validation(String),
}

#[derive(Debug, Error)]
pub enum StorePersistenceError {
    #[error("storage io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("storage json failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("storage sqlite failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("unsupported store version: {0}")]
    UnsupportedVersion(u32),
    #[error(transparent)]
    InvalidPackageVersion(#[from] PackageVersionError),
    #[error("concurrent write conflict in {collection} record {record_key}")]
    ConcurrentWriteConflict {
        collection: String,
        record_key: String,
    },
    #[error("record in {collection} is missing key field {field}")]
    MissingRecordKeyField { collection: String, field: String },
    #[error("refusing to initialize a populated SQLite database without migration metadata")]
    PopulatedUninitializedDatabase,
}

/// Rebuilds a store from a persisted record snapshot; the recovery path when
/// the database itself cannot be reloaded after a failed persist.
pub(crate) fn store_from_records(
    records: &StoreRecords,
) -> Result<InMemoryStore, StorePersistenceError> {
    let rows = records
        .iter()
        .map(|((collection, _), value_json)| {
            Ok((collection.as_str(), serde_json::from_str(value_json)?))
        })
        .collect::<Result<Vec<_>, StorePersistenceError>>()?;
    registry::store_from_rows(rows)
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryStore {
    pub tenants: HashMap<TenantId, Tenant>,
    pub users: HashMap<UserId, User>,
    pub tenant_users: Vec<TenantUser>,
    pub api_tokens: HashMap<Uuid, ApiToken>,
    pub agent_harnesses: HashMap<AgentHarnessId, AgentHarness>,
    pub pending_proposals: HashMap<PendingProposalId, PendingProposal>,
    pub harness_write_audit: Vec<HarnessWriteAudit>,
    pub discovery_tasks: HashMap<DiscoveryTaskId, DiscoveryTask>,
    pub discovery_plans: HashMap<DiscoveryPlanId, DiscoveryPlan>,
    pub discovery_result_batches: HashMap<DiscoveryResultBatchId, DiscoveryResultBatch>,
    /// Replaceable learning evidence produced by deliberate Discovery Result item actions.
    pub(crate) discovery_result_item_learning_links: Vec<DiscoveryResultItemLearningLink>,
    /// Named private Personal Discovery schedules (never federated).
    pub personal_discovery_schedules:
        HashMap<PersonalDiscoveryScheduleId, PersonalDiscoverySchedule>,
    /// One-shot private Discovery-results-ready Events keyed by batch id.
    pub discovery_results_ready_events: HashMap<DiscoveryResultBatchId, DiscoveryResultsReadyEvent>,
    /// Lease-scoped private planned source availability facts (never auth material).
    pub discovery_task_source_availability:
        HashMap<DiscoveryTaskId, DiscoveryTaskSourceAvailability>,
    /// Private one-shot authentication-needed notices keyed by emission identity.
    pub authentication_needed_notices: Vec<AuthenticationNeededNotice>,
    pub candidates: HashMap<CandidateId, Candidate>,
    pub candidate_submissions: HashMap<CandidateSubmissionId, CandidateSubmission>,
    pub(crate) interest_seeds: HashMap<(UserId, CandidateId), InterestSeed>,
    pub pod_curation_policies: HashMap<PodId, CurationPolicy>,
    pub pod_placements: HashMap<(CandidateId, PodId), PodPlacement>,
    pub accepted_placement_projections:
        HashMap<(ContentItemId, PodId), AcceptedPlacementProjection>,
    pub(crate) placement_tombstones: Vec<PlacementTombstone>,
    pub(crate) federated_content_item_ids: HashMap<FederatedContentItemKey, ContentItemId>,
    pub node_identities: HashMap<NodeIdentityId, NodeIdentity>,
    pub trusted_peers: HashMap<PeerId, TrustedPeer>,
    pub known_pod_announcements: HashMap<(NodeIdentityId, String), KnownPodAnnouncement>,
    pub known_pod_withdrawals: HashMap<(NodeIdentityId, String), KnownPodWithdrawal>,
    /// Topic-neutral Announcement Stream log keyed by monotonic sequence.
    pub announcement_stream_entries: BTreeMap<u64, AnnouncementStreamEntry>,
    /// Peer-local Announcement Stream log (separate sequence from Bootstrap).
    pub discovery_peer_stream_entries: BTreeMap<u64, AnnouncementStreamEntry>,
    /// Minimal operator audit of Bootstrap admission rejections.
    pub bootstrap_rejection_audits: Vec<BootstrapRejectionAudit>,
    /// Bootstrap rate-limit and stream sequence bookkeeping.
    pub bootstrap_runtime: Option<BootstrapRuntimeState>,
    /// Index search rate-limit bookkeeping (timestamps only; no query analytics).
    pub index_runtime: Option<IndexRuntimeState>,
    /// Opt-in Discovery Peer service state (disabled by default; outbound-only).
    pub discovery_peer_service: Option<DiscoveryPeerServiceState>,
    /// Verified Discovery Peer Advertisements retained for unranked sampling.
    pub known_discovery_peer_advertisements:
        HashMap<NodeIdentityId, KnownDiscoveryPeerAdvertisement>,
    /// Ordered User-controlled Bootstrap endpoints for outbound stream sync.
    pub bootstrap_endpoints: HashMap<BootstrapEndpointId, BootstrapEndpointConfig>,
    /// Per-endpoint Announcement Stream cursor and last-attempt state.
    pub bootstrap_sync_states: HashMap<BootstrapEndpointId, BootstrapSyncState>,
    /// Home Node automatic Discovery Peer gossip preference (enabled by default).
    pub discovery_peer_gossip_config: Option<DiscoveryPeerGossipConfig>,
    /// Bounded rotating outbound Discovery Peer set (not Trusted Peers).
    pub outbound_discovery_peers: HashMap<NodeIdentityId, OutboundDiscoveryPeer>,
    /// Per-peer stream cursor, health, and last-success state.
    pub discovery_peer_sync_states: HashMap<NodeIdentityId, DiscoveryPeerSyncState>,
    pub trust_policies: HashMap<(UserId, Option<TenantId>), TrustPolicy>,
    pub pod_endorsements: HashMap<Uuid, PodEndorsement>,
    /// Private local agent semantic evidence for Pod Similarity (never federated).
    pub pod_similarity_agent_evidence: HashMap<Uuid, PodSimilarityAgentEvidence>,
    pub pod_explore_sample_sets: HashMap<Uuid, PodExploreSamples>,
    pub subscriptions: HashMap<SubscriptionId, Subscription>,
    pub pods: HashMap<PodId, Pod>,
    pub pod_roles: Vec<PodRoleAssignment>,
    pub pod_rules: HashMap<PodId, PodRules>,
    pub pod_skill_packs: HashMap<PodId, PodSkillPack>,
    /// Immutable historical Pod Package versions.
    pub(crate) pod_package_versions: HashMap<(PodId, PackageVersion), PodPackage>,
    pub event_log: Vec<EventLog>,
    pub submissions: HashMap<SubmissionId, Submission>,
    pub submission_pods: Vec<SubmissionPod>,
    pub submission_assets: HashMap<Uuid, SubmissionAsset>,
    pub crawler_sources: HashMap<Uuid, CrawlerSource>,
    pub user_preferences: HashMap<(UserId, Option<TenantId>), UserPreferences>,
    pub feedback_events: Vec<FeedbackEvent>,
    pub(crate) taste_learning_evidence: Vec<TasteLearningEvidence>,
    pub feed_batches: HashMap<Uuid, FeedBatch>,
    pub briefs: HashMap<Uuid, Brief>,
    /// Private per-User Context prose (never federated).
    pub user_contexts: HashMap<(UserId, Option<TenantId>), UserContext>,
    /// Private User-scoped watches (not Pod Source Rules; never federated).
    pub user_watches: HashMap<UserWatchId, UserWatch>,
    pub saves: HashSet<(UserId, SubmissionId)>,
    pub private_notes: BTreeMap<(UserId, SubmissionId), String>,
    pub reading_history: HashSet<(UserId, SubmissionId)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct FederatedContentItemKey {
    tenant_id: Option<TenantId>,
    origin_node_id: NodeIdentityId,
    origin_content_item_id: ContentItemId,
}

impl FederatedContentItemKey {
    pub(crate) const fn new(
        tenant_id: Option<TenantId>,
        origin_node_id: NodeIdentityId,
        origin_content_item_id: ContentItemId,
    ) -> Self {
        Self {
            tenant_id,
            origin_node_id,
            origin_content_item_id,
        }
    }
}

#[cfg(test)]
mod tests;
