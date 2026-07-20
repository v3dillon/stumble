//! Deliberate User review of private Discovery Result Batch items.

use crate::domain::*;
use crate::interest_seeds::candidate_submission_taste_signals;
use crate::skill_pack::default_skill_pack;
use crate::store::InMemoryStore;
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use uuid::Uuid;

/// Allowed interactive actions for one result item given the caller's grants.
pub(crate) fn discovery_result_allowed_actions(
    store: &InMemoryStore,
    ctx: &AuthContext,
) -> Vec<DiscoveryResultAllowedAction> {
    let mut actions = vec![
        DiscoveryResultAllowedAction::Save,
        DiscoveryResultAllowedAction::MoreLikeThis,
        DiscoveryResultAllowedAction::NotForMe,
        DiscoveryResultAllowedAction::Ignore,
    ];
    if caller_may_curate_any_local_pod(store, ctx) {
        actions.insert(1, DiscoveryResultAllowedAction::AddToPod);
    }
    actions
}

fn caller_may_curate_any_local_pod(store: &InMemoryStore, ctx: &AuthContext) -> bool {
    let Some(user_id) = ctx.user_id else {
        return false;
    };
    let Ok(local_node) = store.node_for_tenant(ctx.tenant_id) else {
        return false;
    };
    let has_role = store.pod_roles.iter().any(|assignment| {
        assignment.user_id == user_id
            && matches!(assignment.role, PodRole::Owner | PodRole::Curator)
            && store.pods.get(&assignment.pod_id).is_some_and(|pod| {
                pod.tenant_id == ctx.tenant_id
                    && pod
                        .origin_node_id
                        .is_none_or(|origin| origin == local_node.id)
            })
    });
    if !has_role {
        return false;
    }
    let Some(harness) = ctx.harness_id.and_then(|id| store.agent_harnesses.get(&id)) else {
        return true;
    };
    harness
        .grant
        .capabilities
        .contains(&HarnessCapability::PodCuration)
}

/// Ensures the User's private Inbox Pod exists and is owned by them.
pub(crate) fn ensure_private_inbox(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    user_id: UserId,
    now: DateTime<Utc>,
) -> Result<Pod, String> {
    let slug = private_inbox_slug(user_id);
    if let Some(existing) = store.pods.values().find(|pod| {
        pod.slug == slug
            && pod.tenant_id == ctx.tenant_id
            && pod.visibility == Visibility::Private
            && pod.created_by == Some(user_id)
    }) {
        return Ok(existing.clone());
    }

    let node = store
        .node_for_tenant(ctx.tenant_id)
        .map_err(|error| error.to_string())?;
    let pod_id = PodId::from(stable_uuid(
        "user-private-inbox",
        &[
            &user_id.to_string(),
            &ctx.tenant_id
                .map_or_else(|| "local".into(), |id| id.to_string()),
        ],
    ));
    if let Some(existing) = store.pods.get(&pod_id) {
        return Ok(existing.clone());
    }

    let pod = Pod {
        id: pod_id,
        tenant_id: ctx.tenant_id,
        name: "Inbox".into(),
        slug,
        description: "Private Inbox for saved Personal Discovery results".into(),
        visibility: Visibility::Private,
        created_by: Some(user_id),
        created_at: now,
        origin_node_id: Some(node.id),
    };
    let package = default_skill_pack(&pod);
    store.pods.insert(pod.id, pod.clone());
    store.pod_rules.insert(
        pod.id,
        PodRules {
            pod_id: pod.id,
            blocked_topics: Vec::new(),
            blocked_domains: Vec::new(),
            auto_promote_crawler_candidates: false,
            federate_sources: false,
        },
    );
    store.pod_roles.push(PodRoleAssignment {
        user_id,
        pod_id: pod.id,
        role: PodRole::Owner,
        created_at: now,
    });
    store
        .insert_pod_package_version(package.clone())
        .map_err(|error| error.to_string())?;
    store.pod_skill_packs.insert(pod.id, package);
    Ok(pod)
}

fn private_inbox_slug(user_id: UserId) -> String {
    format!("inbox-{user_id}")
}

/// Parameters for replaceable discovery-result learning evidence.
pub(crate) struct DiscoveryResultLearningInput<'a> {
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub candidate: &'a Candidate,
    pub submission: &'a CandidateSubmission,
    pub kind: LearnedTasteEvidenceKind,
    pub direction: TasteEvidenceDirection,
    pub now: DateTime<Utc>,
}

/// Records replaceable learning evidence from a deliberate result action.
pub(crate) fn record_discovery_result_learning(
    store: &mut InMemoryStore,
    input: DiscoveryResultLearningInput<'_>,
) -> Vec<Uuid> {
    let signals = candidate_submission_taste_signals(input.candidate, input.submission);
    let mut ids = Vec::with_capacity(signals.len());
    for signal in signals {
        let id = Uuid::now_v7();
        ids.push(id);
        store.taste_learning_evidence.push(TasteLearningEvidence {
            id,
            user_id: input.user_id,
            tenant_id: input.tenant_id,
            signal,
            kind: input.kind,
            direction: input.direction,
            created_at: input.now,
        });
    }
    ids
}

/// Removes prior evidence produced by a previous action on the same item.
pub(crate) fn clear_discovery_result_learning(
    store: &mut InMemoryStore,
    batch_id: DiscoveryResultBatchId,
    candidate_id: CandidateId,
) {
    let Some(index) = store
        .discovery_result_item_learning_links
        .iter()
        .position(|link| link.batch_id == batch_id && link.candidate_id == candidate_id)
    else {
        return;
    };
    let link = store.discovery_result_item_learning_links.remove(index);
    let ids: HashSet<_> = link.evidence_ids.into_iter().collect();
    store
        .taste_learning_evidence
        .retain(|evidence| !ids.contains(&evidence.id));
}

/// Stores the latest replaceable evidence id set for one item.
pub(crate) fn set_discovery_result_learning_link(
    store: &mut InMemoryStore,
    batch_id: DiscoveryResultBatchId,
    candidate_id: CandidateId,
    evidence_ids: Vec<Uuid>,
) {
    store
        .discovery_result_item_learning_links
        .retain(|link| !(link.batch_id == batch_id && link.candidate_id == candidate_id));
    if !evidence_ids.is_empty() {
        store
            .discovery_result_item_learning_links
            .push(DiscoveryResultItemLearningLink {
                batch_id,
                candidate_id,
                evidence_ids,
            });
    }
}

/// Counts taste evidence rows currently linked to one reviewed item.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn linked_evidence_count(
    store: &InMemoryStore,
    batch_id: DiscoveryResultBatchId,
    candidate_id: CandidateId,
) -> usize {
    store
        .discovery_result_item_learning_links
        .iter()
        .find(|link| link.batch_id == batch_id && link.candidate_id == candidate_id)
        .map(|link| link.evidence_ids.len())
        .unwrap_or(0)
}

fn stable_uuid(namespace: &str, parts: &[&str]) -> Uuid {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(namespace.as_bytes());
    for part in parts {
        hasher.update([0]);
        hasher.update(part.as_bytes());
    }
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}
