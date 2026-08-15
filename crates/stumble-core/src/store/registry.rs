//! Single declaration of every persisted collection.
//!
//! The `store_collections!` invocation below is the one place a collection is
//! registered: it generates the `PersistedStore` wire struct, the
//! `STORE_COLLECTIONS` name list, and the typed record-key specs together, so
//! none of them can drift apart. The `From`/`TryFrom` conversions stay
//! hand-written because the compiler already forces both struct literals to
//! name every field.

use super::migrations::{migrate_legacy_pod_memberships, LegacyPodMembership};
use super::{FederatedContentItemKey, InMemoryStore, StorePersistenceError};
use crate::domain::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// How one collection derives the SQLite record key from a serialized value.
pub(super) enum KeySpec {
    /// JSON array of the named top-level fields.
    Fields(&'static [&'static str]),
    /// JSON array of named fields drawn from a nested container object.
    NestedFields {
        container: &'static str,
        fields: &'static [&'static str],
    },
    /// Single JSON value drawn from a nested container object.
    NestedValue {
        container: &'static str,
        field: &'static str,
    },
    /// Fixed key for a zero-or-one bookkeeping record; upserts replace in place.
    Fixed(&'static str),
    /// Zero-padded element position. Order-preserving and duplicate-preserving
    /// for append-mostly log collections without their own identity.
    Positional,
}

macro_rules! store_collections {
    ($( $(#[$attr:meta])* $name:ident : $elem:ty => $spec:expr ),+ $(,)?) => {
        /// Serde wire contract shared by the SQLite record store and the
        /// legacy JSON snapshot format.
        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub(super) struct PersistedStore {
            pub(super) version: u32,
            $( $(#[$attr])* pub(super) $name: Vec<$elem>, )+
        }

        pub(super) const STORE_COLLECTIONS: &[&str] = &[$(stringify!($name)),+];

        pub(super) fn collection_key_spec(collection: &str) -> Option<KeySpec> {
            match collection {
                $( stringify!($name) => Some($spec), )+
                _ => None,
            }
        }
    };
}

store_collections! {
    tenants: Tenant => KeySpec::Fields(&["id"]),
    users: User => KeySpec::Fields(&["id"]),
    tenant_users: TenantUser => KeySpec::Fields(&["tenant_id", "user_id"]),
    api_tokens: ApiToken => KeySpec::Fields(&["id"]),
    #[serde(default)]
    agent_harnesses: AgentHarness => KeySpec::Fields(&["id"]),
    #[serde(default)]
    pending_proposals: PendingProposal => KeySpec::Fields(&["id"]),
    #[serde(default)]
    harness_write_audit: HarnessWriteAudit => KeySpec::Fields(&["id"]),
    #[serde(default)]
    discovery_tasks: DiscoveryTask => KeySpec::Fields(&["id"]),
    #[serde(default)]
    discovery_plans: DiscoveryPlan => KeySpec::Fields(&["id"]),
    #[serde(default)]
    discovery_result_batches: DiscoveryResultBatch => KeySpec::Fields(&["id"]),
    #[serde(default)]
    discovery_result_item_learning_links: DiscoveryResultItemLearningLink =>
        KeySpec::Fields(&["batch_id", "candidate_id"]),
    #[serde(default)]
    personal_discovery_schedules: PersonalDiscoverySchedule => KeySpec::Fields(&["id"]),
    #[serde(default)]
    discovery_results_ready_events: DiscoveryResultsReadyEvent => KeySpec::Fields(&["id"]),
    #[serde(default)]
    discovery_task_source_availability: DiscoveryTaskSourceAvailability =>
        KeySpec::Fields(&["task_id"]),
    #[serde(default)]
    authentication_needed_notices: AuthenticationNeededNotice => KeySpec::Fields(&["id"]),
    #[serde(default)]
    candidates: Candidate => KeySpec::Fields(&["id"]),
    #[serde(default)]
    candidate_submissions: CandidateSubmission => KeySpec::Fields(&["id"]),
    #[serde(default)]
    interest_seeds: InterestSeed => KeySpec::Fields(&["user_id", "candidate_id"]),
    #[serde(default)]
    pod_curation_policies: PersistedPodCurationPolicy => KeySpec::Fields(&["pod_id"]),
    #[serde(default)]
    pod_placements: PodPlacement => KeySpec::Fields(&["candidate_id", "pod_id"]),
    #[serde(default)]
    accepted_placement_projections: AcceptedPlacementProjection =>
        KeySpec::Fields(&["content_item_id", "pod_id"]),
    #[serde(default)]
    placement_tombstones: PlacementTombstone => KeySpec::Positional,
    #[serde(default)]
    federated_content_item_ids: PersistedFederatedContentItemId =>
        KeySpec::Fields(&["tenant_id", "origin_node_id", "origin_content_item_id"]),
    node_identities: NodeIdentity => KeySpec::Fields(&["id"]),
    trusted_peers: TrustedPeer => KeySpec::Fields(&["id"]),
    #[serde(default)]
    known_pod_announcements: KnownPodAnnouncement => KeySpec::NestedFields {
        container: "announcement",
        fields: &["origin_node_id", "pod_slug"],
    },
    #[serde(default)]
    known_pod_withdrawals: KnownPodWithdrawal => KeySpec::NestedFields {
        container: "withdrawal",
        fields: &["origin_node_id", "pod_slug"],
    },
    #[serde(default)]
    announcement_stream_entries: AnnouncementStreamEntry => KeySpec::Fields(&["sequence"]),
    #[serde(default)]
    discovery_peer_stream_entries: AnnouncementStreamEntry => KeySpec::Fields(&["sequence"]),
    #[serde(default)]
    bootstrap_rejection_audits: BootstrapRejectionAudit => KeySpec::Fields(&["id"]),
    /// Zero or one Bootstrap runtime bookkeeping record.
    #[serde(default)]
    bootstrap_runtime: BootstrapRuntimeState => KeySpec::Fixed("bootstrap"),
    /// Zero or one Index runtime bookkeeping record.
    #[serde(default)]
    index_runtime: IndexRuntimeState => KeySpec::Fixed("index"),
    /// Zero or one Discovery Peer service opt-in record.
    #[serde(default)]
    discovery_peer_service: DiscoveryPeerServiceState => KeySpec::Fixed("discovery_peer"),
    #[serde(default)]
    known_discovery_peer_advertisements: KnownDiscoveryPeerAdvertisement =>
        KeySpec::NestedValue { container: "advertisement", field: "node_id" },
    #[serde(default)]
    bootstrap_endpoints: BootstrapEndpointConfig => KeySpec::Fields(&["id"]),
    #[serde(default)]
    bootstrap_sync_states: BootstrapSyncState => KeySpec::Fields(&["endpoint_id"]),
    /// Zero or one Discovery Peer gossip config record.
    #[serde(default)]
    discovery_peer_gossip_config: DiscoveryPeerGossipConfig =>
        KeySpec::Fixed("discovery_peer_gossip"),
    #[serde(default)]
    outbound_discovery_peers: OutboundDiscoveryPeer => KeySpec::Fields(&["node_id"]),
    #[serde(default)]
    discovery_peer_sync_states: DiscoveryPeerSyncState => KeySpec::Fields(&["node_id"]),
    #[serde(default)]
    trust_policies: TrustPolicy => KeySpec::Fields(&["user_id", "tenant_id"]),
    #[serde(default)]
    pod_endorsements: PodEndorsement => KeySpec::Fields(&["id"]),
    #[serde(default)]
    pod_similarity_agent_evidence: PodSimilarityAgentEvidence => KeySpec::Fields(&["id"]),
    #[serde(default)]
    pod_explore_sample_sets: PodExploreSamples => KeySpec::Fields(&["id"]),
    #[serde(default)]
    subscriptions: Subscription => KeySpec::Fields(&["id"]),
    pods: Pod => KeySpec::Fields(&["id"]),
    #[serde(default)]
    pod_roles: PodRoleAssignment => KeySpec::Fields(&["user_id", "pod_id"]),
    /// Legacy read-only input; the load path rewrites it into subscriptions
    /// and pod_roles, and the persist path always writes it empty.
    #[serde(default)]
    pod_memberships: LegacyPodMembership => KeySpec::Fields(&["user_id", "pod_id"]),
    pod_rules: PodRules => KeySpec::Fields(&["pod_id"]),
    pod_skill_packs: PodSkillPack => KeySpec::Fields(&["pod_id"]),
    #[serde(default, alias = "pod_skill_pack_versions")]
    pod_package_versions: PodSkillPack => KeySpec::Fields(&["pod_id", "version"]),
    /// Keyed by event id; ids are UUIDv7, so key order is append order for
    /// locally authored events. Imported peer events keep their origin ids and
    /// may interleave by origin time on reload.
    event_log: EventLog => KeySpec::Fields(&["event_id"]),
    submissions: Submission => KeySpec::Fields(&["id"]),
    submission_pods: SubmissionPod => KeySpec::Fields(&["submission_id", "pod_id"]),
    submission_assets: SubmissionAsset => KeySpec::Fields(&["id"]),
    crawler_sources: CrawlerSource => KeySpec::Fields(&["id"]),
    user_preferences: UserPreferences => KeySpec::Fields(&["user_id", "tenant_id"]),
    feedback_events: FeedbackEvent => KeySpec::Positional,
    #[serde(default)]
    taste_learning_evidence: TasteLearningEvidence => KeySpec::Fields(&["id"]),
    #[serde(default)]
    feed_batches: FeedBatch => KeySpec::Fields(&["id"]),
    briefs: Brief => KeySpec::Fields(&["id"]),
    #[serde(default)]
    user_contexts: UserContext => KeySpec::Fields(&["user_id", "tenant_id"]),
    #[serde(default)]
    user_watches: UserWatch => KeySpec::Fields(&["id"]),
    saves: PersistedUserSubmission => KeySpec::Fields(&["user_id", "submission_id"]),
    private_notes: PersistedPrivateNote => KeySpec::Fields(&["user_id", "submission_id"]),
    reading_history: PersistedUserSubmission => KeySpec::Fields(&["user_id", "submission_id"]),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedUserSubmission {
    user_id: UserId,
    submission_id: SubmissionId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedPrivateNote {
    user_id: UserId,
    submission_id: SubmissionId,
    body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedPodCurationPolicy {
    pod_id: PodId,
    policy: CurationPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedFederatedContentItemId {
    #[serde(default)]
    tenant_id: Option<TenantId>,
    origin_node_id: NodeIdentityId,
    origin_content_item_id: ContentItemId,
    local_content_item_id: ContentItemId,
}

pub(crate) type StoreRecords = BTreeMap<(String, String), String>;

/// Serializes the store into `(collection, record_key) -> value_json` rows.
pub(crate) fn store_records(store: &InMemoryStore) -> Result<StoreRecords, StorePersistenceError> {
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
        for (index, value) in values.iter().enumerate() {
            records.insert(
                (
                    (*collection).to_string(),
                    record_key(collection, value, index)?,
                ),
                serde_json::to_string(value)?,
            );
        }
    }
    Ok(records)
}

/// Rebuilds a store from persisted rows, used to recover an authoritative
/// baseline when the database itself cannot be reloaded.
pub(super) fn store_from_rows<'a>(
    rows: impl IntoIterator<Item = (&'a str, serde_json::Value)>,
) -> Result<InMemoryStore, StorePersistenceError> {
    let mut collections = serde_json::Map::new();
    for collection in STORE_COLLECTIONS {
        collections.insert(
            (*collection).to_string(),
            serde_json::Value::Array(Vec::new()),
        );
    }
    collections.insert("version".to_string(), serde_json::json!(1));
    for (collection, value) in rows {
        if let Some(serde_json::Value::Array(values)) = collections.get_mut(collection) {
            values.push(value);
        }
    }
    let snapshot: PersistedStore = serde_json::from_value(serde_json::Value::Object(collections))?;
    snapshot.try_into()
}

pub(super) fn record_key(
    collection: &str,
    value: &serde_json::Value,
    index: usize,
) -> Result<String, StorePersistenceError> {
    let spec = collection_key_spec(collection).ok_or_else(|| {
        StorePersistenceError::MissingRecordKeyField {
            collection: collection.to_string(),
            field: "<unregistered collection>".to_string(),
        }
    })?;
    let key_field = |container: &serde_json::Value, field: &'static str| {
        container
            .get(field)
            .cloned()
            .ok_or_else(|| StorePersistenceError::MissingRecordKeyField {
                collection: collection.to_string(),
                field: field.to_string(),
            })
    };
    match spec {
        KeySpec::Fields(fields) => {
            let key = fields
                .iter()
                .map(|field| key_field(value, field))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(serde_json::to_string(&key)?)
        }
        KeySpec::NestedFields { container, fields } => {
            let nested = key_field(value, container)?;
            let key = fields
                .iter()
                .map(|field| key_field(&nested, field))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(serde_json::to_string(&key)?)
        }
        KeySpec::NestedValue { container, field } => {
            let nested = key_field(value, container)?;
            Ok(serde_json::to_string(&key_field(&nested, field)?)?)
        }
        KeySpec::Fixed(key) => Ok(key.to_string()),
        KeySpec::Positional => Ok(format!("{index:020}")),
    }
}

/// Whether a persisted record key is a canonical positional key.
pub(super) fn is_positional_key(record_key: &str) -> bool {
    record_key.len() == 20 && record_key.bytes().all(|byte| byte.is_ascii_digit())
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
            user_preferences: store.user_preferences.values().cloned().collect(),
            feedback_events: store.feedback_events.clone(),
            taste_learning_evidence: store.taste_learning_evidence.clone(),
            feed_batches: store.feed_batches.values().cloned().collect(),
            briefs: store.briefs.values().cloned().collect(),
            user_contexts: store.user_contexts.values().cloned().collect(),
            user_watches: store.user_watches.values().cloned().collect(),
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
            user_contexts: snapshot
                .user_contexts
                .into_iter()
                .map(|context| ((context.user_id, context.tenant_id), context))
                .collect(),
            user_watches: snapshot
                .user_watches
                .into_iter()
                .map(|watch| (watch.id, watch))
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
