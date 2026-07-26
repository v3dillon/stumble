use crate::domain::*;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::Duration;
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
    #[error("refusing to initialize a populated SQLite database without migration metadata")]
    PopulatedUninitializedDatabase,
}

/// Reports whether a SQLite path contains an initialized Stumble store without
/// creating the file when it is absent.
pub fn sqlite_home_node_is_initialized(
    database_path: &Path,
) -> Result<bool, StorePersistenceError> {
    if !database_path.is_file() {
        return Ok(false);
    }
    let connection = rusqlite::Connection::open_with_flags(
        database_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let has_schema: bool = connection.query_row(
        "SELECT
           EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'stumble_store_metadata')
           AND EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'stumble_store_records')",
        [],
        |row| row.get(0),
    )?;
    if !has_schema {
        return Ok(false);
    }
    Ok(sqlite_store_state(&connection)? == SqliteStoreState::Initialized)
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
    pub crawl_candidates: HashMap<Uuid, CrawlCandidate>,
    pub user_preferences: HashMap<(UserId, Option<TenantId>), UserPreferences>,
    pub feedback_events: Vec<FeedbackEvent>,
    pub(crate) taste_learning_evidence: Vec<TasteLearningEvidence>,
    pub feed_batches: HashMap<Uuid, FeedBatch>,
    pub briefs: HashMap<Uuid, Brief>,
    pub saves: HashSet<(UserId, SubmissionId)>,
    pub private_notes: BTreeMap<(UserId, SubmissionId), String>,
    pub reading_history: HashSet<(UserId, SubmissionId)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedStore {
    version: u32,
    tenants: Vec<Tenant>,
    users: Vec<User>,
    tenant_users: Vec<TenantUser>,
    api_tokens: Vec<ApiToken>,
    #[serde(default)]
    agent_harnesses: Vec<AgentHarness>,
    #[serde(default)]
    pending_proposals: Vec<PendingProposal>,
    #[serde(default)]
    harness_write_audit: Vec<HarnessWriteAudit>,
    #[serde(default)]
    discovery_tasks: Vec<DiscoveryTask>,
    #[serde(default)]
    discovery_plans: Vec<DiscoveryPlan>,
    #[serde(default)]
    discovery_result_batches: Vec<DiscoveryResultBatch>,
    #[serde(default)]
    discovery_result_item_learning_links: Vec<DiscoveryResultItemLearningLink>,
    #[serde(default)]
    personal_discovery_schedules: Vec<PersonalDiscoverySchedule>,
    #[serde(default)]
    discovery_results_ready_events: Vec<DiscoveryResultsReadyEvent>,
    #[serde(default)]
    discovery_task_source_availability: Vec<DiscoveryTaskSourceAvailability>,
    #[serde(default)]
    authentication_needed_notices: Vec<AuthenticationNeededNotice>,
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(default)]
    candidate_submissions: Vec<CandidateSubmission>,
    #[serde(default)]
    interest_seeds: Vec<InterestSeed>,
    #[serde(default)]
    pod_curation_policies: Vec<PersistedPodCurationPolicy>,
    #[serde(default)]
    pod_placements: Vec<PodPlacement>,
    #[serde(default)]
    accepted_placement_projections: Vec<AcceptedPlacementProjection>,
    #[serde(default)]
    placement_tombstones: Vec<PlacementTombstone>,
    #[serde(default)]
    federated_content_item_ids: Vec<PersistedFederatedContentItemId>,
    node_identities: Vec<NodeIdentity>,
    trusted_peers: Vec<TrustedPeer>,
    #[serde(default)]
    known_pod_announcements: Vec<KnownPodAnnouncement>,
    #[serde(default)]
    known_pod_withdrawals: Vec<KnownPodWithdrawal>,
    #[serde(default)]
    announcement_stream_entries: Vec<AnnouncementStreamEntry>,
    #[serde(default)]
    discovery_peer_stream_entries: Vec<AnnouncementStreamEntry>,
    #[serde(default)]
    bootstrap_rejection_audits: Vec<BootstrapRejectionAudit>,
    /// Zero or one Bootstrap runtime bookkeeping record.
    #[serde(default)]
    bootstrap_runtime: Vec<BootstrapRuntimeState>,
    /// Zero or one Index runtime bookkeeping record.
    #[serde(default)]
    index_runtime: Vec<IndexRuntimeState>,
    /// Zero or one Discovery Peer service opt-in record.
    #[serde(default)]
    discovery_peer_service: Vec<DiscoveryPeerServiceState>,
    #[serde(default)]
    known_discovery_peer_advertisements: Vec<KnownDiscoveryPeerAdvertisement>,
    #[serde(default)]
    bootstrap_endpoints: Vec<BootstrapEndpointConfig>,
    #[serde(default)]
    bootstrap_sync_states: Vec<BootstrapSyncState>,
    /// Zero or one Discovery Peer gossip config record.
    #[serde(default)]
    discovery_peer_gossip_config: Vec<DiscoveryPeerGossipConfig>,
    #[serde(default)]
    outbound_discovery_peers: Vec<OutboundDiscoveryPeer>,
    #[serde(default)]
    discovery_peer_sync_states: Vec<DiscoveryPeerSyncState>,
    #[serde(default)]
    trust_policies: Vec<TrustPolicy>,
    #[serde(default)]
    pod_endorsements: Vec<PodEndorsement>,
    #[serde(default)]
    pod_similarity_agent_evidence: Vec<PodSimilarityAgentEvidence>,
    #[serde(default)]
    pod_explore_sample_sets: Vec<PodExploreSamples>,
    #[serde(default)]
    subscriptions: Vec<Subscription>,
    pods: Vec<Pod>,
    #[serde(default)]
    pod_roles: Vec<PodRoleAssignment>,
    #[serde(default)]
    pod_memberships: Vec<LegacyPodMembership>,
    pod_rules: Vec<PodRules>,
    pod_skill_packs: Vec<PodSkillPack>,
    #[serde(default, alias = "pod_skill_pack_versions")]
    pod_package_versions: Vec<PodSkillPack>,
    event_log: Vec<EventLog>,
    submissions: Vec<Submission>,
    submission_pods: Vec<SubmissionPod>,
    submission_assets: Vec<SubmissionAsset>,
    crawler_sources: Vec<CrawlerSource>,
    crawl_candidates: Vec<CrawlCandidate>,
    user_preferences: Vec<UserPreferences>,
    feedback_events: Vec<FeedbackEvent>,
    #[serde(default)]
    taste_learning_evidence: Vec<TasteLearningEvidence>,
    #[serde(default)]
    feed_batches: Vec<FeedBatch>,
    briefs: Vec<Brief>,
    saves: Vec<PersistedUserSubmission>,
    private_notes: Vec<PersistedPrivateNote>,
    reading_history: Vec<PersistedUserSubmission>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedUserSubmission {
    user_id: UserId,
    submission_id: SubmissionId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedPrivateNote {
    user_id: UserId,
    submission_id: SubmissionId,
    body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedPodCurationPolicy {
    pod_id: PodId,
    policy: CurationPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyPodMembership {
    user_id: UserId,
    pod_id: PodId,
    role: LegacyPodRole,
    #[serde(default)]
    is_priority: bool,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyPodRole {
    Owner,
    Moderator,
    Admin,
    Member,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedFederatedContentItemId {
    #[serde(default)]
    tenant_id: Option<TenantId>,
    origin_node_id: NodeIdentityId,
    origin_content_item_id: ContentItemId,
    local_content_item_id: ContentItemId,
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

fn migrate_legacy_pod_memberships(
    legacy_memberships: &[LegacyPodMembership],
    pods: &[Pod],
    node_identities: &[NodeIdentity],
    subscriptions: &mut Vec<Subscription>,
    pod_roles: &mut Vec<PodRoleAssignment>,
) {
    for membership in legacy_memberships {
        let Some(pod) = pods.iter().find(|pod| pod.id == membership.pod_id) else {
            continue;
        };
        if let Some(subscription) = subscriptions.iter_mut().find(|subscription| {
            subscription.user_id == membership.user_id
                && subscription.local_pod_id == membership.pod_id
        }) {
            subscription.is_priority |= membership.is_priority;
        } else {
            let origin = pod
                .origin_node_id
                .and_then(|node_id| node_identities.iter().find(|node| node.id == node_id))
                .or_else(|| {
                    node_identities
                        .iter()
                        .find(|node| node.tenant_id == pod.tenant_id)
                });
            if let Some(origin) = origin {
                let mut subscription = Subscription::new_local(
                    legacy_subscription_id(membership.user_id, membership.pod_id),
                    membership.user_id,
                    pod,
                    origin,
                    membership.created_at,
                );
                subscription.is_priority = membership.is_priority;
                subscriptions.push(subscription);
            }
        }

        let role = match membership.role {
            LegacyPodRole::Owner => Some(PodRole::Owner),
            LegacyPodRole::Moderator | LegacyPodRole::Admin => Some(PodRole::Curator),
            LegacyPodRole::Member => None,
        };
        if let Some(role) = role {
            if let Some(assignment) = pod_roles.iter_mut().find(|assignment| {
                assignment.user_id == membership.user_id && assignment.pod_id == membership.pod_id
            }) {
                assignment.role = role;
            } else {
                pod_roles.push(PodRoleAssignment {
                    user_id: membership.user_id,
                    pod_id: membership.pod_id,
                    role,
                    created_at: membership.created_at,
                });
            }
        }
    }
}

fn legacy_subscription_id(user_id: UserId, pod_id: PodId) -> SubscriptionId {
    let mut hasher = Sha256::new();
    hasher.update(b"stumble legacy Subscription\0");
    hasher.update(user_id.as_bytes());
    hasher.update(pod_id.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes).into()
}

impl From<&InMemoryStore> for PersistedStore {
    fn from(store: &InMemoryStore) -> Self {
        Self {
            version: 1,
            tenants: store.tenants.values().cloned().collect(),
            users: store.users.values().cloned().collect(),
            tenant_users: store.tenant_users.clone(),
            api_tokens: store.api_tokens.values().cloned().collect(),
            agent_harnesses: store.agent_harnesses.values().cloned().collect(),
            pending_proposals: store.pending_proposals.values().cloned().collect(),
            harness_write_audit: store.harness_write_audit.clone(),
            discovery_tasks: store.discovery_tasks.values().cloned().collect(),
            discovery_plans: store.discovery_plans.values().cloned().collect(),
            discovery_result_batches: store.discovery_result_batches.values().cloned().collect(),
            discovery_result_item_learning_links: store
                .discovery_result_item_learning_links
                .clone(),
            personal_discovery_schedules: store
                .personal_discovery_schedules
                .values()
                .cloned()
                .collect(),
            discovery_results_ready_events: store
                .discovery_results_ready_events
                .values()
                .cloned()
                .collect(),
            discovery_task_source_availability: store
                .discovery_task_source_availability
                .values()
                .cloned()
                .collect(),
            authentication_needed_notices: store.authentication_needed_notices.clone(),
            candidates: store.candidates.values().cloned().collect(),
            candidate_submissions: store.candidate_submissions.values().cloned().collect(),
            interest_seeds: store.interest_seeds.values().cloned().collect(),
            pod_curation_policies: store
                .pod_curation_policies
                .iter()
                .map(|(pod_id, policy)| PersistedPodCurationPolicy {
                    pod_id: *pod_id,
                    policy: *policy,
                })
                .collect(),
            pod_placements: store.pod_placements.values().cloned().collect(),
            accepted_placement_projections: store
                .accepted_placement_projections
                .values()
                .cloned()
                .collect(),
            placement_tombstones: store.placement_tombstones.clone(),
            federated_content_item_ids: store
                .federated_content_item_ids
                .iter()
                .map(
                    |(key, local_content_item_id)| PersistedFederatedContentItemId {
                        tenant_id: key.tenant_id,
                        origin_node_id: key.origin_node_id,
                        origin_content_item_id: key.origin_content_item_id,
                        local_content_item_id: *local_content_item_id,
                    },
                )
                .collect(),
            node_identities: store.node_identities.values().cloned().collect(),
            trusted_peers: store.trusted_peers.values().cloned().collect(),
            known_pod_announcements: store.known_pod_announcements.values().cloned().collect(),
            known_pod_withdrawals: store.known_pod_withdrawals.values().cloned().collect(),
            announcement_stream_entries: store
                .announcement_stream_entries
                .values()
                .cloned()
                .collect(),
            discovery_peer_stream_entries: store
                .discovery_peer_stream_entries
                .values()
                .cloned()
                .collect(),
            bootstrap_rejection_audits: store.bootstrap_rejection_audits.clone(),
            bootstrap_runtime: store.bootstrap_runtime.clone().into_iter().collect(),
            index_runtime: store.index_runtime.clone().into_iter().collect(),
            discovery_peer_service: store.discovery_peer_service.clone().into_iter().collect(),
            known_discovery_peer_advertisements: store
                .known_discovery_peer_advertisements
                .values()
                .cloned()
                .collect(),
            bootstrap_endpoints: store.bootstrap_endpoints.values().cloned().collect(),
            bootstrap_sync_states: store.bootstrap_sync_states.values().cloned().collect(),
            discovery_peer_gossip_config: store
                .discovery_peer_gossip_config
                .clone()
                .into_iter()
                .collect(),
            outbound_discovery_peers: store.outbound_discovery_peers.values().cloned().collect(),
            discovery_peer_sync_states: store
                .discovery_peer_sync_states
                .values()
                .cloned()
                .collect(),
            trust_policies: store.trust_policies.values().cloned().collect(),
            pod_endorsements: store.pod_endorsements.values().cloned().collect(),
            pod_similarity_agent_evidence: store
                .pod_similarity_agent_evidence
                .values()
                .cloned()
                .collect(),
            pod_explore_sample_sets: store.pod_explore_sample_sets.values().cloned().collect(),
            subscriptions: store.subscriptions.values().cloned().collect(),
            pods: store.pods.values().cloned().collect(),
            pod_roles: store.pod_roles.clone(),
            pod_memberships: Vec::new(),
            pod_rules: store.pod_rules.values().cloned().collect(),
            pod_skill_packs: store.pod_skill_packs.values().cloned().collect(),
            pod_package_versions: store.pod_package_versions.values().cloned().collect(),
            event_log: store.event_log.clone(),
            submissions: store.submissions.values().cloned().collect(),
            submission_pods: store.submission_pods.clone(),
            submission_assets: store.submission_assets.values().cloned().collect(),
            crawler_sources: store.crawler_sources.values().cloned().collect(),
            crawl_candidates: store.crawl_candidates.values().cloned().collect(),
            user_preferences: store.user_preferences.values().cloned().collect(),
            feedback_events: store.feedback_events.clone(),
            taste_learning_evidence: store.taste_learning_evidence.clone(),
            feed_batches: store.feed_batches.values().cloned().collect(),
            briefs: store.briefs.values().cloned().collect(),
            saves: store
                .saves
                .iter()
                .map(|(user_id, submission_id)| PersistedUserSubmission {
                    user_id: *user_id,
                    submission_id: *submission_id,
                })
                .collect(),
            private_notes: store
                .private_notes
                .iter()
                .map(|((user_id, submission_id), body)| PersistedPrivateNote {
                    user_id: *user_id,
                    submission_id: *submission_id,
                    body: body.clone(),
                })
                .collect(),
            reading_history: store
                .reading_history
                .iter()
                .map(|(user_id, submission_id)| PersistedUserSubmission {
                    user_id: *user_id,
                    submission_id: *submission_id,
                })
                .collect(),
        }
    }
}

impl TryFrom<PersistedStore> for InMemoryStore {
    type Error = StorePersistenceError;

    fn try_from(snapshot: PersistedStore) -> Result<Self, Self::Error> {
        let mut subscriptions = snapshot.subscriptions;
        let mut pod_roles = snapshot.pod_roles;
        migrate_legacy_pod_memberships(
            &snapshot.pod_memberships,
            &snapshot.pods,
            &snapshot.node_identities,
            &mut subscriptions,
            &mut pod_roles,
        );
        let current_skill_packs = snapshot.pod_skill_packs;
        let mut historical_skill_packs = snapshot
            .pod_package_versions
            .into_iter()
            .map(|pack| {
                PackageVersion::new(pack.version).map(|version| ((pack.pod_id, version), pack))
            })
            .collect::<Result<HashMap<_, _>, _>>()?;
        for pack in &current_skill_packs {
            let version = PackageVersion::new(pack.version)?;
            historical_skill_packs
                .entry((pack.pod_id, version))
                .or_insert_with(|| pack.clone());
        }
        Ok(Self {
            tenants: snapshot
                .tenants
                .into_iter()
                .map(|tenant| (tenant.id, tenant))
                .collect(),
            users: snapshot
                .users
                .into_iter()
                .map(|user| (user.id, user))
                .collect(),
            tenant_users: snapshot.tenant_users,
            api_tokens: snapshot
                .api_tokens
                .into_iter()
                .map(|token| (token.id, token))
                .collect(),
            agent_harnesses: snapshot
                .agent_harnesses
                .into_iter()
                .map(|harness| (harness.id, harness))
                .collect(),
            pending_proposals: snapshot
                .pending_proposals
                .into_iter()
                .map(|proposal| (proposal.id, proposal))
                .collect(),
            harness_write_audit: snapshot.harness_write_audit,
            discovery_tasks: snapshot
                .discovery_tasks
                .into_iter()
                .map(|task| (task.id, task))
                .collect(),
            discovery_plans: snapshot
                .discovery_plans
                .into_iter()
                .map(|plan| (plan.id, plan))
                .collect(),
            discovery_result_batches: snapshot
                .discovery_result_batches
                .into_iter()
                .map(|batch| (batch.id, batch))
                .collect(),
            discovery_result_item_learning_links: snapshot.discovery_result_item_learning_links,
            personal_discovery_schedules: snapshot
                .personal_discovery_schedules
                .into_iter()
                .map(|schedule| (schedule.id, schedule))
                .collect(),
            discovery_results_ready_events: snapshot
                .discovery_results_ready_events
                .into_iter()
                .map(|event| (event.batch_id, event))
                .collect(),
            discovery_task_source_availability: snapshot
                .discovery_task_source_availability
                .into_iter()
                .map(|entry| (entry.task_id, entry))
                .collect(),
            authentication_needed_notices: snapshot.authentication_needed_notices,
            candidates: snapshot
                .candidates
                .into_iter()
                .map(|candidate| (candidate.id, candidate))
                .collect(),
            candidate_submissions: snapshot
                .candidate_submissions
                .into_iter()
                .map(|submission| (submission.id, submission))
                .collect(),
            interest_seeds: snapshot
                .interest_seeds
                .into_iter()
                .map(|seed| ((seed.user_id, seed.candidate_id), seed))
                .collect(),
            pod_curation_policies: snapshot
                .pod_curation_policies
                .into_iter()
                .map(|entry| (entry.pod_id, entry.policy))
                .collect(),
            pod_placements: snapshot
                .pod_placements
                .into_iter()
                .map(|placement| ((placement.candidate_id, placement.pod_id), placement))
                .collect(),
            accepted_placement_projections: snapshot
                .accepted_placement_projections
                .into_iter()
                .map(|projection| ((projection.content_item_id, projection.pod_id), projection))
                .collect(),
            placement_tombstones: snapshot.placement_tombstones,
            federated_content_item_ids: snapshot
                .federated_content_item_ids
                .into_iter()
                .map(|entry| {
                    (
                        FederatedContentItemKey::new(
                            entry.tenant_id,
                            entry.origin_node_id,
                            entry.origin_content_item_id,
                        ),
                        entry.local_content_item_id,
                    )
                })
                .collect(),
            node_identities: snapshot
                .node_identities
                .into_iter()
                .map(|node| (node.id, node))
                .collect(),
            trusted_peers: snapshot
                .trusted_peers
                .into_iter()
                .map(|peer| (peer.id, peer))
                .collect(),
            known_pod_announcements: snapshot
                .known_pod_announcements
                .into_iter()
                .map(|known| {
                    (
                        (
                            known.announcement.origin_node_id,
                            known.announcement.pod_slug.clone(),
                        ),
                        known,
                    )
                })
                .collect(),
            known_pod_withdrawals: snapshot
                .known_pod_withdrawals
                .into_iter()
                .map(|known| {
                    (
                        (
                            known.withdrawal.origin_node_id,
                            known.withdrawal.pod_slug.clone(),
                        ),
                        known,
                    )
                })
                .collect(),
            announcement_stream_entries: snapshot
                .announcement_stream_entries
                .into_iter()
                .map(|entry| (entry.sequence, entry))
                .collect(),
            discovery_peer_stream_entries: snapshot
                .discovery_peer_stream_entries
                .into_iter()
                .map(|entry| (entry.sequence, entry))
                .collect(),
            bootstrap_rejection_audits: snapshot.bootstrap_rejection_audits,
            bootstrap_runtime: snapshot.bootstrap_runtime.into_iter().next(),
            index_runtime: snapshot.index_runtime.into_iter().next(),
            discovery_peer_service: snapshot.discovery_peer_service.into_iter().next(),
            known_discovery_peer_advertisements: snapshot
                .known_discovery_peer_advertisements
                .into_iter()
                .map(|known| (known.advertisement.node_id, known))
                .collect(),
            bootstrap_endpoints: snapshot
                .bootstrap_endpoints
                .into_iter()
                .map(|endpoint| (endpoint.id, endpoint))
                .collect(),
            bootstrap_sync_states: snapshot
                .bootstrap_sync_states
                .into_iter()
                .map(|state| (state.endpoint_id, state))
                .collect(),
            discovery_peer_gossip_config: snapshot.discovery_peer_gossip_config.into_iter().next(),
            outbound_discovery_peers: snapshot
                .outbound_discovery_peers
                .into_iter()
                .map(|peer| (peer.node_id, peer))
                .collect(),
            discovery_peer_sync_states: snapshot
                .discovery_peer_sync_states
                .into_iter()
                .map(|state| (state.node_id, state))
                .collect(),
            trust_policies: snapshot
                .trust_policies
                .into_iter()
                .map(|policy| ((policy.user_id, policy.tenant_id), policy))
                .collect(),
            pod_endorsements: snapshot
                .pod_endorsements
                .into_iter()
                .map(|endorsement| (endorsement.id, endorsement))
                .collect(),
            pod_similarity_agent_evidence: snapshot
                .pod_similarity_agent_evidence
                .into_iter()
                .map(|evidence| (evidence.id, evidence))
                .collect(),
            pod_explore_sample_sets: snapshot
                .pod_explore_sample_sets
                .into_iter()
                .map(|samples| (samples.announcement_id, samples))
                .collect(),
            subscriptions: subscriptions
                .into_iter()
                .map(|subscription| (subscription.id, subscription))
                .collect(),
            pods: snapshot.pods.into_iter().map(|pod| (pod.id, pod)).collect(),
            pod_roles,
            pod_rules: snapshot
                .pod_rules
                .into_iter()
                .map(|rules| (rules.pod_id, rules))
                .collect(),
            pod_skill_packs: current_skill_packs
                .into_iter()
                .map(|pack| (pack.pod_id, pack))
                .collect(),
            pod_package_versions: historical_skill_packs,
            event_log: snapshot.event_log,
            submissions: snapshot
                .submissions
                .into_iter()
                .map(|submission| (submission.id, submission))
                .collect(),
            submission_pods: snapshot.submission_pods,
            submission_assets: snapshot
                .submission_assets
                .into_iter()
                .map(|asset| (asset.id, asset))
                .collect(),
            crawler_sources: snapshot
                .crawler_sources
                .into_iter()
                .map(|source| (source.id, source))
                .collect(),
            crawl_candidates: snapshot
                .crawl_candidates
                .into_iter()
                .map(|candidate| (candidate.id, candidate))
                .collect(),
            user_preferences: snapshot
                .user_preferences
                .into_iter()
                .map(|prefs| ((prefs.user_id, prefs.tenant_id), prefs))
                .collect(),
            feedback_events: snapshot.feedback_events,
            taste_learning_evidence: snapshot.taste_learning_evidence,
            feed_batches: snapshot
                .feed_batches
                .into_iter()
                .map(|batch| (batch.id, batch))
                .collect(),
            briefs: snapshot
                .briefs
                .into_iter()
                .map(|brief| (brief.id, brief))
                .collect(),
            saves: snapshot
                .saves
                .into_iter()
                .map(|save| (save.user_id, save.submission_id))
                .collect(),
            private_notes: snapshot
                .private_notes
                .into_iter()
                .map(|note| ((note.user_id, note.submission_id), note.body))
                .collect(),
            reading_history: snapshot
                .reading_history
                .into_iter()
                .map(|history| (history.user_id, history.submission_id))
                .collect(),
        })
    }
}

pub fn save_store_snapshot(
    store: &InMemoryStore,
    path: &Path,
) -> Result<(), StorePersistenceError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let snapshot = PersistedStore::from(store);
    let bytes = serde_json::to_vec_pretty(&snapshot)?;
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, bytes)?;
    std::fs::rename(tmp_path, path)?;
    Ok(())
}

pub fn load_store_snapshot(path: &Path) -> Result<InMemoryStore, StorePersistenceError> {
    let bytes = std::fs::read(path)?;
    let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if let Some(candidates) = value
        .get_mut("candidates")
        .and_then(serde_json::Value::as_array_mut)
    {
        for candidate in candidates {
            migrate_candidate_value(candidate)?;
        }
    }
    if let Some(submissions) = value
        .get_mut("candidate_submissions")
        .and_then(serde_json::Value::as_array_mut)
    {
        for submission in submissions {
            migrate_candidate_submission_value(submission)?;
        }
    }
    if let Some(preferences) = value
        .get_mut("user_preferences")
        .and_then(serde_json::Value::as_array_mut)
    {
        for preference in preferences {
            migrate_user_preferences_value(preference)?;
        }
    }
    let snapshot: PersistedStore = serde_json::from_value(value)?;
    if snapshot.version != 1 {
        return Err(StorePersistenceError::UnsupportedVersion(snapshot.version));
    }
    snapshot.try_into()
}

pub fn load_or_seed_store_snapshot(
    path: &Path,
    seed: impl FnOnce() -> InMemoryStore,
) -> Result<InMemoryStore, StorePersistenceError> {
    if path.exists() {
        load_store_snapshot(path)
    } else {
        let store = seed();
        save_store_snapshot(&store, path)?;
        Ok(store)
    }
}

const SQLITE_STORE_SCHEMA: &str =
    include_str!("../../../migrations/sqlite/0002_authoritative_store.sql");
const SQLITE_DROP_LEGACY_HUB: &str =
    include_str!("../../../migrations/sqlite/0003_drop_legacy_hub.sql");
const STORE_COLLECTIONS: &[&str] = &[
    "tenants",
    "users",
    "tenant_users",
    "api_tokens",
    "agent_harnesses",
    "pending_proposals",
    "harness_write_audit",
    "discovery_tasks",
    "discovery_plans",
    "discovery_result_batches",
    "discovery_result_item_learning_links",
    "personal_discovery_schedules",
    "discovery_results_ready_events",
    "discovery_task_source_availability",
    "authentication_needed_notices",
    "candidates",
    "candidate_submissions",
    "interest_seeds",
    "pod_curation_policies",
    "pod_placements",
    "accepted_placement_projections",
    "placement_tombstones",
    "federated_content_item_ids",
    "node_identities",
    "trusted_peers",
    "known_pod_announcements",
    "known_pod_withdrawals",
    "announcement_stream_entries",
    "discovery_peer_stream_entries",
    "bootstrap_rejection_audits",
    "bootstrap_runtime",
    "index_runtime",
    "discovery_peer_service",
    "known_discovery_peer_advertisements",
    "bootstrap_endpoints",
    "bootstrap_sync_states",
    "discovery_peer_gossip_config",
    "outbound_discovery_peers",
    "discovery_peer_sync_states",
    "trust_policies",
    "pod_endorsements",
    "pod_similarity_agent_evidence",
    "pod_explore_sample_sets",
    "subscriptions",
    "pods",
    "pod_roles",
    "pod_memberships",
    "pod_rules",
    "pod_skill_packs",
    "pod_package_versions",
    "event_log",
    "submissions",
    "submission_pods",
    "submission_assets",
    "crawler_sources",
    "crawl_candidates",
    "user_preferences",
    "feedback_events",
    "taste_learning_evidence",
    "feed_batches",
    "briefs",
    "saves",
    "private_notes",
    "reading_history",
];

type StoreRecords = BTreeMap<(String, String), String>;

/// Opens the authoritative SQLite store, importing a legacy JSON snapshot only
/// when the database has never been initialized.
pub fn load_or_initialize_sqlite_store(
    database_path: &Path,
    legacy_path: &Path,
    seed: impl FnOnce() -> InMemoryStore,
) -> Result<InMemoryStore, StorePersistenceError> {
    if let Some(parent) = database_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut connection = open_sqlite_store(database_path)?;
    match sqlite_store_state(&connection)? {
        SqliteStoreState::Initialized => return load_sqlite_store_from_connection(&mut connection),
        SqliteStoreState::PopulatedWithoutMetadata => {
            return Err(StorePersistenceError::PopulatedUninitializedDatabase)
        }
        SqliteStoreState::Empty => {}
    }

    let store = if legacy_path.exists() {
        let store = load_store_snapshot(legacy_path)?;
        let backup_path = legacy_path.with_extension("json.migrated.bak");
        if !backup_path.exists() {
            std::fs::copy(legacy_path, backup_path)?;
        }
        store
    } else {
        seed()
    };
    initialize_sqlite_store(&mut connection, &store)?;
    Ok(store)
}

pub fn load_sqlite_store(database_path: &Path) -> Result<InMemoryStore, StorePersistenceError> {
    let mut connection = open_sqlite_store(database_path)?;
    load_sqlite_store_from_connection(&mut connection)
}

/// Applies only changed domain records in one SQLite transaction.
pub fn persist_sqlite_store_changes(
    database_path: &Path,
    previous: &InMemoryStore,
    current: &InMemoryStore,
) -> Result<(), StorePersistenceError> {
    let mut connection = open_sqlite_store(database_path)?;
    let previous_records = store_records(previous)?;
    let current_records = store_records(current)?;
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

    for (collection_and_key, previous_value) in previous_records.iter().filter(|(key, value)| {
        current_records
            .get(*key)
            .is_none_or(|current| current != *value)
    }) {
        ensure_record_unchanged(&transaction, collection_and_key, Some(previous_value))?;
    }
    for collection_and_key in current_records
        .keys()
        .filter(|key| !previous_records.contains_key(*key))
    {
        ensure_record_unchanged(&transaction, collection_and_key, None)?;
    }

    for (collection_and_key, _) in previous_records
        .iter()
        .filter(|(collection_and_key, _)| !current_records.contains_key(*collection_and_key))
    {
        transaction.execute(
            "DELETE FROM stumble_store_records WHERE collection = ?1 AND record_key = ?2",
            rusqlite::params![collection_and_key.0, collection_and_key.1],
        )?;
    }
    for ((collection, record_key), value_json) in
        current_records
            .iter()
            .filter(|(collection_and_key, value)| {
                previous_records.get(*collection_and_key) != Some(*value)
            })
    {
        transaction.execute(
            "INSERT INTO stumble_store_records (collection, record_key, value_json) VALUES (?1, ?2, ?3)
             ON CONFLICT (collection, record_key) DO UPDATE SET value_json = excluded.value_json",
            rusqlite::params![collection, record_key, value_json],
        )?;
    }
    transaction.execute(
        "INSERT INTO stumble_store_metadata (key, value) VALUES ('initialized', '1')
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

fn open_sqlite_store(path: &Path) -> Result<rusqlite::Connection, StorePersistenceError> {
    let connection = rusqlite::Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(SQLITE_STORE_SCHEMA)?;
    // Forward migration: drop non-authoritative legacy Hub caches without
    // transforming their contents. Idempotent for new and existing databases.
    connection.execute_batch(SQLITE_DROP_LEGACY_HUB)?;
    Ok(connection)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SqliteStoreState {
    Empty,
    Initialized,
    PopulatedWithoutMetadata,
}

fn sqlite_store_state(
    connection: &rusqlite::Connection,
) -> Result<SqliteStoreState, StorePersistenceError> {
    let (initialized, populated): (bool, bool) = connection.query_row(
        "SELECT
           EXISTS(SELECT 1 FROM stumble_store_metadata WHERE key = 'initialized'),
           EXISTS(SELECT 1 FROM stumble_store_records)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(match (initialized, populated) {
        (true, _) => SqliteStoreState::Initialized,
        (false, true) => SqliteStoreState::PopulatedWithoutMetadata,
        (false, false) => SqliteStoreState::Empty,
    })
}

fn ensure_record_unchanged(
    transaction: &rusqlite::Transaction<'_>,
    collection_and_key: &(String, String),
    expected: Option<&String>,
) -> Result<(), StorePersistenceError> {
    let actual = transaction
        .query_row(
            "SELECT value_json FROM stumble_store_records WHERE collection = ?1 AND record_key = ?2",
            rusqlite::params![collection_and_key.0, collection_and_key.1],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if actual.as_ref() != expected {
        return Err(StorePersistenceError::ConcurrentWriteConflict {
            collection: collection_and_key.0.clone(),
            record_key: collection_and_key.1.clone(),
        });
    }
    Ok(())
}

fn initialize_sqlite_store(
    connection: &mut rusqlite::Connection,
    store: &InMemoryStore,
) -> Result<(), StorePersistenceError> {
    let records = store_records(store)?;
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for ((collection, record_key), value_json) in records {
        transaction.execute(
            "INSERT INTO stumble_store_records (collection, record_key, value_json) VALUES (?1, ?2, ?3)",
            rusqlite::params![collection, record_key, value_json],
        )?;
    }
    transaction.execute(
        "INSERT INTO stumble_store_metadata (key, value) VALUES ('initialized', '1')",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

fn load_sqlite_store_from_connection(
    connection: &mut rusqlite::Connection,
) -> Result<InMemoryStore, StorePersistenceError> {
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mut legacy_discovery_task_rows = Vec::new();
    let mut legacy_candidate_rows = Vec::new();
    let mut legacy_candidate_submission_rows = Vec::new();
    let mut legacy_user_preferences_rows = Vec::new();
    let mut collections = serde_json::Map::new();
    for collection in STORE_COLLECTIONS {
        collections.insert(
            (*collection).to_string(),
            serde_json::Value::Array(Vec::new()),
        );
    }
    collections.insert("version".to_string(), serde_json::json!(1));

    let mut statement = transaction.prepare(
        "SELECT collection, record_key, value_json FROM stumble_store_records
         ORDER BY collection, record_key",
    )?;
    let records = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for record in records {
        let (collection, record_key, value_json) = record?;
        let mut value: serde_json::Value = serde_json::from_str(&value_json)?;
        if collection == "discovery_tasks" && value.get("target").is_none() {
            legacy_discovery_task_rows.push(record_key.clone());
        }
        if collection == "candidates" && migrate_candidate_value(&mut value)? {
            legacy_candidate_rows.push(record_key.clone());
        }
        if collection == "candidate_submissions" && migrate_candidate_submission_value(&mut value)?
        {
            legacy_candidate_submission_rows.push(record_key.clone());
        }
        if collection == "user_preferences" && migrate_user_preferences_value(&mut value)? {
            legacy_user_preferences_rows.push(record_key.clone());
        }
        if let Some(serde_json::Value::Array(values)) = collections.get_mut(&collection) {
            values.push(value);
        }
    }
    drop(statement);
    let snapshot: PersistedStore = serde_json::from_value(serde_json::Value::Object(collections))?;
    if snapshot.version != 1 {
        return Err(StorePersistenceError::UnsupportedVersion(snapshot.version));
    }
    let had_legacy_pod_memberships = !snapshot.pod_memberships.is_empty();
    let store = snapshot.try_into()?;
    persist_migrated_records(
        &transaction,
        &store,
        "discovery_tasks",
        &legacy_discovery_task_rows,
    )?;
    persist_migrated_records(&transaction, &store, "candidates", &legacy_candidate_rows)?;
    persist_migrated_records(
        &transaction,
        &store,
        "candidate_submissions",
        &legacy_candidate_submission_rows,
    )?;
    persist_migrated_records(
        &transaction,
        &store,
        "user_preferences",
        &legacy_user_preferences_rows,
    )?;
    transaction.commit()?;
    if had_legacy_pod_memberships {
        persist_migrated_pod_relationships(connection, &store)?;
    }
    Ok(store)
}

fn migrate_candidate_value(value: &mut serde_json::Value) -> Result<bool, StorePersistenceError> {
    let record = value.as_object_mut().ok_or_else(|| {
        StorePersistenceError::Json(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Candidate row must be an object",
        )))
    })?;
    let Some(canonical_url) = record
        .get("canonical_url")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
    else {
        return Ok(false);
    };
    if record.get("source_url").and_then(serde_json::Value::as_str) == Some(canonical_url.as_str())
    {
        return Ok(false);
    }
    record.insert(
        "source_url".into(),
        serde_json::Value::String(canonical_url),
    );
    Ok(true)
}

fn migrate_candidate_submission_value(
    value: &mut serde_json::Value,
) -> Result<bool, StorePersistenceError> {
    if value.get("target").is_some() {
        return Ok(false);
    }
    let record = value.as_object_mut().ok_or_else(|| {
        StorePersistenceError::Json(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Candidate Submission row must be an object",
        )))
    })?;
    let placements = record
        .remove("proposed_placements")
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    let task_context = record
        .remove("task_context")
        .unwrap_or(serde_json::Value::Null);
    record.insert(
        "target".into(),
        serde_json::json!({
            "kind": "pod_placements",
            "placements": placements,
            "task_context": task_context,
        }),
    );
    Ok(true)
}

fn migrate_user_preferences_value(
    value: &mut serde_json::Value,
) -> Result<bool, StorePersistenceError> {
    let record = value.as_object_mut().ok_or_else(|| {
        StorePersistenceError::Json(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "User Preferences row must be an object",
        )))
    })?;
    if record.contains_key("blocked_source_affinities") {
        return Ok(false);
    }
    record.insert(
        "blocked_source_affinities".into(),
        serde_json::Value::Array(Vec::new()),
    );
    Ok(true)
}

fn persist_migrated_records(
    transaction: &rusqlite::Transaction<'_>,
    store: &InMemoryStore,
    collection: &str,
    legacy_record_keys: &[String],
) -> Result<(), StorePersistenceError> {
    if legacy_record_keys.is_empty() {
        return Ok(());
    }
    let records = store_records(store)?;
    for record_key in legacy_record_keys {
        let collection_and_key = (collection.to_string(), record_key.clone());
        let value_json = records
            .get(&collection_and_key)
            .expect("loaded migrated value has a canonical store record");
        let updated = transaction.execute(
            "UPDATE stumble_store_records SET value_json = ?1
             WHERE collection = ?2 AND record_key = ?3",
            rusqlite::params![value_json, collection, record_key],
        )?;
        debug_assert_eq!(updated, 1, "loaded migrated row still exists");
    }
    Ok(())
}

fn persist_migrated_pod_relationships(
    connection: &mut rusqlite::Connection,
    store: &InMemoryStore,
) -> Result<(), StorePersistenceError> {
    let records = store_records(store)?;
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    transaction.execute(
        "DELETE FROM stumble_store_records WHERE collection = 'pod_memberships'",
        [],
    )?;
    for ((collection, record_key), value_json) in records
        .into_iter()
        .filter(|((collection, _), _)| collection == "subscriptions" || collection == "pod_roles")
    {
        transaction.execute(
            "INSERT INTO stumble_store_records (collection, record_key, value_json) VALUES (?1, ?2, ?3)
             ON CONFLICT (collection, record_key) DO UPDATE SET value_json = excluded.value_json",
            rusqlite::params![collection, record_key, value_json],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn store_records(store: &InMemoryStore) -> Result<StoreRecords, StorePersistenceError> {
    let snapshot_value = serde_json::to_value(PersistedStore::from(store))?;
    let snapshot = snapshot_value
        .as_object()
        .expect("PersistedStore serializes as an object");
    let mut records = BTreeMap::new();
    for collection in STORE_COLLECTIONS {
        let values = snapshot
            .get(*collection)
            .and_then(serde_json::Value::as_array)
            .expect("PersistedStore collections serialize as arrays");
        for value in values {
            let record_key = record_key(collection, value)?;
            records.insert(
                ((*collection).to_string(), record_key),
                serde_json::to_string(value)?,
            );
        }
    }
    Ok(records)
}

fn record_key(
    collection: &str,
    value: &serde_json::Value,
) -> Result<String, StorePersistenceError> {
    let fields: &[&str] = match collection {
        "tenant_users" => &["tenant_id", "user_id"],
        "pod_roles" | "pod_memberships" => &["user_id", "pod_id"],
        "submission_pods" => &["submission_id", "pod_id"],
        "user_preferences" => &["user_id", "tenant_id"],
        "interest_seeds" => &["user_id", "candidate_id"],
        "trust_policies" => &["user_id", "tenant_id"],
        "saves" | "private_notes" | "reading_history" => &["user_id", "submission_id"],
        "known_pod_announcements" => {
            let announcement = value
                .get("announcement")
                .unwrap_or(&serde_json::Value::Null);
            return Ok(serde_json::to_string(&[
                announcement
                    .get("origin_node_id")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                announcement
                    .get("pod_slug")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            ])?);
        }
        "known_pod_withdrawals" => {
            let withdrawal = value.get("withdrawal").unwrap_or(&serde_json::Value::Null);
            return Ok(serde_json::to_string(&[
                withdrawal
                    .get("origin_node_id")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                withdrawal
                    .get("pod_slug")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            ])?);
        }
        "announcement_stream_entries" => &["sequence"],
        "discovery_peer_stream_entries" => &["sequence"],
        "bootstrap_rejection_audits" => &["id"],
        // Singleton bookkeeping record; fixed key so upserts replace in place.
        "bootstrap_runtime" => return Ok("bootstrap".to_string()),
        "index_runtime" => return Ok("index".to_string()),
        "discovery_peer_service" => return Ok("discovery_peer".to_string()),
        "known_discovery_peer_advertisements" => {
            let advertisement = value
                .get("advertisement")
                .unwrap_or(&serde_json::Value::Null);
            return Ok(serde_json::to_string(
                advertisement
                    .get("node_id")
                    .unwrap_or(&serde_json::Value::Null),
            )?);
        }
        "bootstrap_endpoints" => &["id"],
        "bootstrap_sync_states" => &["endpoint_id"],
        "discovery_peer_gossip_config" => return Ok("discovery_peer_gossip".to_string()),
        "outbound_discovery_peers" => &["node_id"],
        "discovery_peer_sync_states" => &["node_id"],
        "pod_rules" | "pod_skill_packs" => &["pod_id"],
        "pod_curation_policies" => &["pod_id"],
        "pod_placements" => &["candidate_id", "pod_id"],
        "accepted_placement_projections" => &["content_item_id", "pod_id"],
        "placement_tombstones" => return Ok(serde_json::to_string(value)?),
        "federated_content_item_ids" => &["tenant_id", "origin_node_id", "origin_content_item_id"],
        "pod_package_versions" => &["pod_id", "version"],
        "event_log" => &["event_id"],
        "feedback_events" => return Ok(serde_json::to_string(value)?),
        "harness_write_audit" => &["id"],
        "discovery_result_item_learning_links" => &["batch_id", "candidate_id"],
        "discovery_task_source_availability" => &["task_id"],
        "authentication_needed_notices" => &["id"],
        "taste_learning_evidence" => &["id"],
        _ => &["id"],
    };
    let mut key = Vec::with_capacity(fields.len());
    for field in fields {
        key.push(
            value
                .get(*field)
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        );
    }
    Ok(serde_json::to_string(&key)?)
}

impl InMemoryStore {
    /// Stores a Pod Package version once and refuses replacement.
    pub(crate) fn insert_pod_package_version(
        &mut self,
        package: PodPackage,
    ) -> Result<(), StoreError> {
        let version = PackageVersion::new(package.version)
            .map_err(|error| StoreError::Validation(error.to_string()))?;
        let key = (package.pod_id, version);
        if self.pod_package_versions.contains_key(&key) {
            return Err(StoreError::Duplicate(format!(
                "Pod Package version {} for Pod {}",
                version.value(),
                package.pod_id
            )));
        }
        self.pod_package_versions.insert(key, package);
        Ok(())
    }

    pub(crate) fn pod_package_version(
        &self,
        pod_id: PodId,
        version: PackageVersion,
    ) -> Option<&PodPackage> {
        self.pod_package_versions.get(&(pod_id, version))
    }

    pub fn default_node(&self) -> Result<NodeIdentity, StoreError> {
        self.node_identities
            .values()
            .find(|node| node.tenant_id.is_none())
            .or_else(|| self.node_identities.values().next())
            .cloned()
            .ok_or_else(|| StoreError::NotFound("node identity".to_string()))
    }

    pub fn node_for_tenant(&self, tenant_id: Option<TenantId>) -> Result<NodeIdentity, StoreError> {
        self.node_identities
            .values()
            .find(|node| node.tenant_id == tenant_id)
            .or_else(|| {
                self.node_identities
                    .values()
                    .find(|node| node.tenant_id.is_none())
            })
            .cloned()
            .ok_or_else(|| StoreError::NotFound("node identity".to_string()))
    }

    pub fn pod_by_slug(&self, slug: &str, tenant_id: Option<TenantId>) -> Result<Pod, StoreError> {
        self.pods
            .values()
            .find(|pod| pod.slug == slug && pod.tenant_id == tenant_id)
            .or_else(|| {
                self.pods
                    .values()
                    .find(|pod| pod.slug == slug && pod.tenant_id.is_none())
            })
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("pod {slug}")))
    }

    pub fn tenant_by_slug(&self, slug: &str) -> Result<Tenant, StoreError> {
        self.tenants
            .values()
            .find(|tenant| tenant.slug == slug)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("tenant {slug}")))
    }

    pub fn assert_tenant(
        &self,
        actual: Option<TenantId>,
        expected: Option<TenantId>,
    ) -> Result<(), StoreError> {
        if actual == expected || actual.is_none() {
            Ok(())
        } else {
            Err(StoreError::TenantBoundary)
        }
    }

    pub fn submissions_for_pod(&self, pod_id: PodId) -> Vec<&Submission> {
        let ids: HashSet<_> = self
            .submission_pods
            .iter()
            .filter(|link| link.pod_id == pod_id)
            .map(|link| link.submission_id)
            .collect();
        self.submissions
            .values()
            .filter(|submission| ids.contains(&submission.id))
            .collect()
    }

    pub fn public_events_for_pod(&self, pod_slug: &str) -> Vec<EventLog> {
        self.event_log
            .iter()
            .filter(|event| event.pod_slug == pod_slug && is_federated_pod_event(&event.event_type))
            .cloned()
            .collect()
    }

    pub fn portable_package_events_for_pod(&self, pod_slug: &str) -> Vec<EventLog> {
        self.event_log
            .iter()
            .filter(|event| {
                event.pod_slug == pod_slug
                    && matches!(
                        event.event_type.as_str(),
                        "pod_created"
                            | "private_pod_package_created"
                            | "pod_skill_pack_updated"
                            | "pod_package_imported"
                            | "pod_package_forked"
                    )
            })
            .cloned()
            .collect()
    }

    pub fn latest_federated_event_hash(&self, pod_slug: &str) -> Option<String> {
        self.event_log
            .iter()
            .rev()
            .find(|event| event.pod_slug == pod_slug && is_federated_pod_event(&event.event_type))
            .map(|event| event.content_hash.clone())
    }

    pub fn latest_event_hash(&self, pod_slug: &str) -> Option<String> {
        self.event_log
            .iter()
            .rev()
            .find(|event| event.pod_slug == pod_slug)
            .map(|event| event.content_hash.clone())
    }
}

pub fn is_private_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "link_saved_private"
            | "link_dismissed_private"
            | "private_note_added"
            | "user_preference_updated"
            | "source_blocked_private"
            | "topic_blocked_private"
            | "reading_history_recorded"
    )
}

pub fn is_federated_pod_event(event_type: &str) -> bool {
    FederatedPodEventType::from_wire(event_type).is_some_and(FederatedPodEventType::is_federated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store_dir(test_name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("stumble-{test_name}-{}", Uuid::now_v7()))
    }

    fn populated_legacy_store() -> InMemoryStore {
        let store = crate::seeds::seed_store();
        let local_node_id = store
            .node_identities
            .values()
            .find(|node| node.tenant_id.is_none())
            .unwrap()
            .id;
        let user_id = *store.users.keys().next().unwrap();
        let tools = crate::AgentTools::new(store);
        let ctx = AuthContext {
            user_id: Some(user_id),
            tenant_id: None,
            node_id: local_node_id,
            harness_id: None,
        };
        tools
            .create_pod(
                &ctx,
                CreatePodRequest {
                    name: "Legacy Pod".to_string(),
                    slug: "legacy-pod".to_string(),
                    description: "Existing curation".to_string(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();
        let submission = tools
            .submit_link_to_pod(
                &ctx,
                "legacy-pod",
                SubmitLinkRequest {
                    url: "https://example.com/legacy".to_string(),
                    title: Some("Legacy Item".to_string()),
                    description: Some("Existing submission".to_string()),
                    note: Some("Keep this provenance".to_string()),
                    tags: vec!["legacy".to_string()],
                    discovered_by_crawler: false,
                },
            )
            .unwrap();
        tools.save_link(&ctx, submission.id).unwrap();
        tools
            .generate_brief(
                &ctx,
                GenerateBriefRequest {
                    pod_slugs: vec!["legacy-pod".to_string()],
                    query: Some("legacy".to_string()),
                    user_id: Some(user_id),
                },
            )
            .unwrap();
        tools.store().read().unwrap().clone()
    }

    #[test]
    fn sqlite_home_node_initializes_and_restarts() {
        let dir = temp_store_dir("sqlite-restart");
        let database_path = dir.join("stumble.sqlite3");
        let legacy_path = dir.join("store.json");

        let first = load_or_initialize_sqlite_store(&database_path, &legacy_path, || {
            crate::seeds::seed_store()
        })
        .unwrap();
        let first_node_id = first
            .node_identities
            .values()
            .find(|node| node.tenant_id.is_none())
            .unwrap()
            .id;
        let restarted =
            load_or_initialize_sqlite_store(&database_path, &legacy_path, InMemoryStore::default)
                .unwrap();

        assert!(database_path.exists());
        assert_eq!(
            restarted
                .node_identities
                .values()
                .find(|node| node.tenant_id.is_none())
                .unwrap()
                .id,
            first_node_id
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sqlite_transactions_preserve_writes_from_separate_home_node_instances() {
        let dir = temp_store_dir("sqlite-concurrent-writes");
        let database_path = dir.join("stumble.sqlite3");
        let legacy_path = dir.join("store.json");
        let first_store =
            load_or_initialize_sqlite_store(&database_path, &legacy_path, crate::seeds::seed_store)
                .unwrap();
        let second_store = load_sqlite_store(&database_path).unwrap();
        let first = crate::AgentTools::new_sqlite_persistent(first_store, &database_path);
        let second = crate::AgentTools::new_sqlite_persistent(second_store, &database_path);
        let local_node_id = first
            .store()
            .read()
            .unwrap()
            .node_identities
            .values()
            .find(|node| node.tenant_id.is_none())
            .unwrap()
            .id;
        let first_ctx = AuthContext {
            user_id: None,
            tenant_id: None,
            node_id: local_node_id,
            harness_id: None,
        };
        let second_ctx = first_ctx.clone();

        first
            .create_pod(
                &first_ctx,
                CreatePodRequest {
                    name: "First Pod".to_string(),
                    slug: "first-pod".to_string(),
                    description: "Written by the first process".to_string(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();
        second
            .create_pod(
                &second_ctx,
                CreatePodRequest {
                    name: "Second Pod".to_string(),
                    slug: "second-pod".to_string(),
                    description: "Written by the second process".to_string(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();

        let restarted = load_sqlite_store(&database_path).unwrap();
        assert!(restarted.pod_by_slug("first-pod", None).is_ok());
        assert!(restarted.pod_by_slug("second-pod", None).is_ok());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sqlite_rejects_a_stale_write_to_the_same_record() {
        let dir = temp_store_dir("sqlite-conflicting-writes");
        let database_path = dir.join("stumble.sqlite3");
        let legacy_path = dir.join("store.json");
        let first_store =
            load_or_initialize_sqlite_store(&database_path, &legacy_path, crate::seeds::seed_store)
                .unwrap();
        let user_id = *first_store.users.keys().next().unwrap();
        let local_node_id = first_store.default_node().unwrap().id;
        let second_store = load_sqlite_store(&database_path).unwrap();
        let first = crate::AgentTools::new_sqlite_persistent(first_store, &database_path);
        let second = crate::AgentTools::new_sqlite_persistent(second_store, &database_path);
        let ctx = AuthContext {
            user_id: Some(user_id),
            tenant_id: None,
            node_id: local_node_id,
            harness_id: None,
        };

        first
            .update_preferences(
                &ctx,
                UpdatePreferencesRequest {
                    interests: Some(vec!["first writer".to_string()]),
                    blocked_topics: None,
                    blocked_sources: None,
                    preferred_brief_length: None,
                    preferred_discovery_mode: None,
                },
            )
            .unwrap();
        let error = second
            .update_preferences(
                &ctx,
                UpdatePreferencesRequest {
                    interests: Some(vec!["stale writer".to_string()]),
                    blocked_topics: None,
                    blocked_sources: None,
                    preferred_brief_length: None,
                    preferred_discovery_mode: None,
                },
            )
            .unwrap_err();

        assert!(matches!(
            error,
            crate::AgentToolsError::Persistence(
                StorePersistenceError::ConcurrentWriteConflict { .. }
            )
        ));
        assert_eq!(
            second
                .store()
                .read()
                .unwrap()
                .user_preferences
                .get(&(user_id, None))
                .unwrap()
                .interests,
            vec!["first writer"]
        );
        second
            .update_preferences(
                &ctx,
                UpdatePreferencesRequest {
                    interests: Some(vec!["retried writer".to_string()]),
                    blocked_topics: None,
                    blocked_sources: None,
                    preferred_brief_length: None,
                    preferred_discovery_mode: None,
                },
            )
            .unwrap();
        let restarted = load_sqlite_store(&database_path).unwrap();
        assert_eq!(
            restarted
                .user_preferences
                .get(&(user_id, None))
                .unwrap()
                .interests,
            vec!["retried writer"]
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn two_connections_prevent_stale_discovery_task_migration_overwrite() {
        let dir = temp_store_dir("sqlite-discovery-task-migration-race");
        let database_path = dir.join("stumble.sqlite3");
        let legacy_path = dir.join("store.json");
        let mut store = crate::seeds::seed_store();
        let pod_id = Uuid::now_v7();
        let now = chrono::Utc::now();
        let task = DiscoveryTask {
            id: Uuid::now_v7().into(),
            target: DiscoveryTaskTarget::Pod {
                pod_id,
                package_version: PackageVersion::new(1).unwrap(),
            },
            origin: DiscoveryTaskOrigin::Scheduled {
                source_rule_index: 0,
            },
            due_at: now,
            state: DiscoveryTaskState::Pending,
            attempts: Vec::new(),
            created_at: now,
        };
        store.discovery_tasks.insert(task.id, task.clone());
        load_or_initialize_sqlite_store(&database_path, &legacy_path, || store.clone()).unwrap();

        let records = store_records(&store).unwrap();
        let ((_, record_key), canonical_json) = records
            .iter()
            .find(|((collection, _), _)| collection == "discovery_tasks")
            .unwrap();
        let mut legacy_task: serde_json::Value = serde_json::from_str(canonical_json).unwrap();
        legacy_task
            .as_object_mut()
            .unwrap()
            .remove("target")
            .unwrap();
        let legacy_json = serde_json::to_string(&legacy_task).unwrap();
        let mut first = rusqlite::Connection::open(&database_path).unwrap();
        first
            .execute(
                "UPDATE stumble_store_records SET value_json = ?1
                 WHERE collection = 'discovery_tasks' AND record_key = ?2",
                rusqlite::params![legacy_json, record_key],
            )
            .unwrap();

        let mut lifecycle_update = legacy_task;
        lifecycle_update["state"] = serde_json::json!({"status": "completed"});
        let lifecycle_json = serde_json::to_string(&lifecycle_update).unwrap();
        let second = rusqlite::Connection::open(&database_path).unwrap();
        second.busy_timeout(Duration::from_millis(1)).unwrap();
        let transaction = first
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        let competing_write = second.execute(
            "UPDATE stumble_store_records SET value_json = ?1
                 WHERE collection = 'discovery_tasks' AND record_key = ?2",
            rusqlite::params![lifecycle_json, record_key],
        );
        assert!(matches!(
            competing_write,
            Err(rusqlite::Error::SqliteFailure(error, _))
                if matches!(
                    error.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                )
        ));

        persist_migrated_records(
            &transaction,
            &store,
            "discovery_tasks",
            std::slice::from_ref(record_key),
        )
        .unwrap();
        transaction.commit().unwrap();
        second
            .execute(
                "UPDATE stumble_store_records SET value_json = ?1
                 WHERE collection = 'discovery_tasks' AND record_key = ?2",
                rusqlite::params![lifecycle_json, record_key],
            )
            .unwrap();
        let persisted: String = second
            .query_row(
                "SELECT value_json FROM stumble_store_records
                 WHERE collection = 'discovery_tasks' AND record_key = ?1",
                [record_key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted, lifecycle_json);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_json_imports_once_with_a_recoverable_backup() {
        let dir = temp_store_dir("sqlite-legacy-import");
        let database_path = dir.join("stumble.sqlite3");
        let legacy_path = dir.join("store.json");
        let original = populated_legacy_store();
        save_store_snapshot(&original, &legacy_path).unwrap();

        let imported =
            load_or_initialize_sqlite_store(&database_path, &legacy_path, InMemoryStore::default)
                .unwrap();
        assert_eq!(
            store_records(&imported).unwrap(),
            store_records(&original).unwrap()
        );
        assert!(legacy_path.with_extension("json.migrated.bak").exists());

        save_store_snapshot(&InMemoryStore::default(), &legacy_path).unwrap();
        let restarted =
            load_or_initialize_sqlite_store(&database_path, &legacy_path, InMemoryStore::default)
                .unwrap();
        assert_eq!(
            store_records(&restarted).unwrap(),
            store_records(&original).unwrap()
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_sqlite_pod_memberships_rewrite_once_before_restart() {
        let dir = temp_store_dir("sqlite-pod-relationship-migration");
        let database_path = dir.join("stumble.sqlite3");
        std::fs::create_dir_all(&dir).unwrap();
        let original = populated_legacy_store();
        let user_id = *original.users.keys().next().unwrap();
        let pod_id = original.pod_by_slug("legacy-pod", None).unwrap().id;
        let created_at = original
            .pod_roles
            .iter()
            .find(|assignment| assignment.user_id == user_id && assignment.pod_id == pod_id)
            .unwrap()
            .created_at;
        let legacy_membership = LegacyPodMembership {
            user_id,
            pod_id,
            role: LegacyPodRole::Moderator,
            is_priority: true,
            created_at,
        };
        let mut connection = open_sqlite_store(&database_path).unwrap();
        initialize_sqlite_store(&mut connection, &original).unwrap();
        connection
            .execute(
                "DELETE FROM stumble_store_records WHERE collection = 'pod_roles'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO stumble_store_records (collection, record_key, value_json)
                 VALUES ('pod_memberships', ?1, ?2)",
                rusqlite::params![
                    serde_json::to_string(&[user_id, pod_id]).unwrap(),
                    serde_json::to_string(&legacy_membership).unwrap()
                ],
            )
            .unwrap();
        drop(connection);

        let migrated = load_sqlite_store(&database_path).unwrap();
        assert!(migrated.pod_roles.iter().any(|assignment| {
            assignment.user_id == user_id
                && assignment.pod_id == pod_id
                && assignment.role == PodRole::Curator
        }));
        assert!(migrated.subscriptions.values().any(|subscription| {
            subscription.user_id == user_id
                && subscription.local_pod_id == pod_id
                && subscription.is_priority
        }));
        let connection = open_sqlite_store(&database_path).unwrap();
        let legacy_rows: usize = connection
            .query_row(
                "SELECT COUNT(*) FROM stumble_store_records WHERE collection = 'pod_memberships'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_rows, 0);
        drop(connection);

        let restarted = load_sqlite_store(&database_path).unwrap();
        assert_eq!(restarted.pod_roles, migrated.pod_roles);
        assert_eq!(restarted.subscriptions, migrated.subscriptions);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_legacy_json_leaves_sqlite_empty_and_can_be_retried() {
        let dir = temp_store_dir("sqlite-malformed-legacy");
        let database_path = dir.join("stumble.sqlite3");
        let legacy_path = dir.join("store.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&legacy_path, b"{ not valid json").unwrap();

        assert!(matches!(
            load_or_initialize_sqlite_store(&database_path, &legacy_path, InMemoryStore::default),
            Err(StorePersistenceError::Json(_))
        ));
        assert!(store_records(&load_sqlite_store(&database_path).unwrap())
            .unwrap()
            .is_empty());

        let recoverable = populated_legacy_store();
        save_store_snapshot(&recoverable, &legacy_path).unwrap();
        let imported =
            load_or_initialize_sqlite_store(&database_path, &legacy_path, InMemoryStore::default)
                .unwrap();
        assert_eq!(
            store_records(&imported).unwrap(),
            store_records(&recoverable).unwrap()
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_import_refuses_a_populated_database_without_migration_metadata() {
        let dir = temp_store_dir("sqlite-uninitialized-populated");
        let database_path = dir.join("stumble.sqlite3");
        let legacy_path = dir.join("store.json");
        std::fs::create_dir_all(&dir).unwrap();
        save_store_snapshot(&populated_legacy_store(), &legacy_path).unwrap();
        let connection = open_sqlite_store(&database_path).unwrap();
        connection
            .execute(
                "INSERT INTO stumble_store_records (collection, record_key, value_json)
                 VALUES ('unknown', 'existing', '{\"preserve\":true}')",
                [],
            )
            .unwrap();

        assert!(matches!(
            load_or_initialize_sqlite_store(&database_path, &legacy_path, InMemoryStore::default),
            Err(StorePersistenceError::PopulatedUninitializedDatabase)
        ));
        let existing: String = connection
            .query_row(
                "SELECT value_json FROM stumble_store_records WHERE record_key = 'existing'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(existing, "{\"preserve\":true}");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn snapshot_round_trips_seeded_store() {
        let store = crate::seeds::seed_store();
        let dir = std::env::temp_dir().join(format!("stumble-store-test-{}", Uuid::now_v7()));
        let path = dir.join("store.json");

        save_store_snapshot(&store, &path).unwrap();
        let loaded = load_store_snapshot(&path).unwrap();

        assert_eq!(loaded.pods.len(), store.pods.len());
        assert_eq!(loaded.node_identities.len(), store.node_identities.len());
        assert_eq!(loaded.user_preferences.len(), store.user_preferences.len());
        assert!(loaded.pods.is_empty());

        let _ = std::fs::remove_dir_all(dir);
    }
}
