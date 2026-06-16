use crate::domain::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
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
    #[error("validation failed: {0}")]
    Validation(String),
}

#[derive(Debug, Error)]
pub enum StorePersistenceError {
    #[error("storage io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("storage json failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryStore {
    pub tenants: HashMap<TenantId, Tenant>,
    pub users: HashMap<UserId, User>,
    pub tenant_users: Vec<TenantUser>,
    pub api_tokens: HashMap<Uuid, ApiToken>,
    pub node_identities: HashMap<NodeIdentityId, NodeIdentity>,
    pub trusted_peers: HashMap<PeerId, TrustedPeer>,
    pub pods: HashMap<PodId, Pod>,
    pub pod_memberships: Vec<PodMembership>,
    pub pod_rules: HashMap<PodId, PodRules>,
    pub pod_skill_packs: HashMap<PodId, PodSkillPack>,
    pub event_log: Vec<EventLog>,
    pub submissions: HashMap<SubmissionId, Submission>,
    pub submission_pods: Vec<SubmissionPod>,
    pub submission_assets: HashMap<Uuid, SubmissionAsset>,
    pub crawler_sources: HashMap<Uuid, CrawlerSource>,
    pub crawl_candidates: HashMap<Uuid, CrawlCandidate>,
    pub user_preferences: HashMap<(UserId, Option<TenantId>), UserPreferences>,
    pub feedback_events: Vec<FeedbackEvent>,
    pub briefs: HashMap<Uuid, Brief>,
    pub saves: HashSet<(UserId, SubmissionId)>,
    pub private_notes: BTreeMap<(UserId, SubmissionId), String>,
    pub reading_history: HashSet<(UserId, SubmissionId)>,
    pub hub_nodes: HashMap<NodeIdentityId, HubRegisteredNode>,
    pub hub_pods: HashMap<(NodeIdentityId, String), HubRegisteredPod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedStore {
    version: u32,
    tenants: Vec<Tenant>,
    users: Vec<User>,
    tenant_users: Vec<TenantUser>,
    api_tokens: Vec<ApiToken>,
    node_identities: Vec<NodeIdentity>,
    trusted_peers: Vec<TrustedPeer>,
    pods: Vec<Pod>,
    pod_memberships: Vec<PodMembership>,
    pod_rules: Vec<PodRules>,
    pod_skill_packs: Vec<PodSkillPack>,
    event_log: Vec<EventLog>,
    submissions: Vec<Submission>,
    submission_pods: Vec<SubmissionPod>,
    submission_assets: Vec<SubmissionAsset>,
    crawler_sources: Vec<CrawlerSource>,
    crawl_candidates: Vec<CrawlCandidate>,
    user_preferences: Vec<UserPreferences>,
    feedback_events: Vec<FeedbackEvent>,
    briefs: Vec<Brief>,
    saves: Vec<PersistedUserSubmission>,
    private_notes: Vec<PersistedPrivateNote>,
    reading_history: Vec<PersistedUserSubmission>,
    hub_nodes: Vec<HubRegisteredNode>,
    hub_pods: Vec<HubRegisteredPod>,
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

impl From<&InMemoryStore> for PersistedStore {
    fn from(store: &InMemoryStore) -> Self {
        Self {
            version: 1,
            tenants: store.tenants.values().cloned().collect(),
            users: store.users.values().cloned().collect(),
            tenant_users: store.tenant_users.clone(),
            api_tokens: store.api_tokens.values().cloned().collect(),
            node_identities: store.node_identities.values().cloned().collect(),
            trusted_peers: store.trusted_peers.values().cloned().collect(),
            pods: store.pods.values().cloned().collect(),
            pod_memberships: store.pod_memberships.clone(),
            pod_rules: store.pod_rules.values().cloned().collect(),
            pod_skill_packs: store.pod_skill_packs.values().cloned().collect(),
            event_log: store.event_log.clone(),
            submissions: store.submissions.values().cloned().collect(),
            submission_pods: store.submission_pods.clone(),
            submission_assets: store.submission_assets.values().cloned().collect(),
            crawler_sources: store.crawler_sources.values().cloned().collect(),
            crawl_candidates: store.crawl_candidates.values().cloned().collect(),
            user_preferences: store.user_preferences.values().cloned().collect(),
            feedback_events: store.feedback_events.clone(),
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
            hub_nodes: store.hub_nodes.values().cloned().collect(),
            hub_pods: store.hub_pods.values().cloned().collect(),
        }
    }
}

impl From<PersistedStore> for InMemoryStore {
    fn from(snapshot: PersistedStore) -> Self {
        Self {
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
            pods: snapshot.pods.into_iter().map(|pod| (pod.id, pod)).collect(),
            pod_memberships: snapshot.pod_memberships,
            pod_rules: snapshot
                .pod_rules
                .into_iter()
                .map(|rules| (rules.pod_id, rules))
                .collect(),
            pod_skill_packs: snapshot
                .pod_skill_packs
                .into_iter()
                .map(|pack| (pack.pod_id, pack))
                .collect(),
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
            hub_nodes: snapshot
                .hub_nodes
                .into_iter()
                .map(|node| (node.node_id, node))
                .collect(),
            hub_pods: snapshot
                .hub_pods
                .into_iter()
                .map(|pod| ((pod.node_id, pod.pod_slug.clone()), pod))
                .collect(),
        }
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
    let snapshot: PersistedStore = serde_json::from_slice(&bytes)?;
    Ok(snapshot.into())
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

impl InMemoryStore {
    pub fn default_node(&self) -> Result<NodeIdentity, StoreError> {
        self.node_identities
            .values()
            .next()
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
            .filter(|event| event.pod_slug == pod_slug && !is_private_event(&event.event_type))
            .cloned()
            .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(loaded
            .pods
            .values()
            .any(|pod| pod.slug == "beautiful-interfaces"));

        let _ = std::fs::remove_dir_all(dir);
    }
}
