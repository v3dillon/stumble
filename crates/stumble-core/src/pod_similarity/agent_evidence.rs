//! Local agent semantic evidence layered on deterministic Pod Similarity.
//!
//! Authorized harnesses may submit confidence-scored, evidence-backed
//! relationships between exact current Pod Announcements. Core remains
//! authoritative: Trust Policy blocks, eligibility, exploration caps, and
//! durable User learning are never granted by agent evidence alone. Evidence
//! never leaves the Home Node as an Endorsement, global score, announcement
//! field, or remote interest query.

use super::score::{PodSimilarityScore, SimilarityEvidenceKind, SimilarityReason};
use crate::domain::{
    AgentHarness, AgentHarnessId, KnownPodAnnouncement, PodAnnouncement,
    PodSimilarityAgentEvidence, PodSimilarityAgentEvidenceAnnouncementRef,
    SubmitPodSimilarityAgentEvidenceRequest, TenantId, TrustPolicy, UserId,
};
use crate::pod_announcement::announcement_is_discovery_eligible;
use crate::store::InMemoryStore;
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeSet;
use thiserror::Error;
use uuid::Uuid;

/// Maximum contribution agent evidence may add when base similarity is already positive.
pub const MAX_AGENT_EVIDENCE_BOOST: f32 = 0.5;

/// Maximum explanation length retained with agent evidence.
pub const MAX_AGENT_EVIDENCE_EXPLANATION_CHARS: usize = 500;

/// Maximum model/harness provenance label length.
pub const MAX_AGENT_EVIDENCE_PROVENANCE_CHARS: usize = 128;

/// Maximum harness idempotency key length.
pub const MAX_AGENT_EVIDENCE_IDEMPOTENCY_CHARS: usize = 128;

/// Maximum public announcement inputs listed on one submission.
pub const MAX_AGENT_EVIDENCE_PUBLIC_INPUTS: usize = 16;

/// Default active lifetime when the submitter omits freshness.
pub const DEFAULT_AGENT_EVIDENCE_FRESHNESS_HOURS: u32 = 24;

/// Hard maximum active lifetime for agent evidence.
pub const MAX_AGENT_EVIDENCE_FRESHNESS_HOURS: u32 = 24 * 7;

/// Stable rejection reasons for agent evidence submissions.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AgentEvidenceError {
    /// Explanation empty or exceeds the bound.
    #[error("agent evidence explanation must be non-empty and at most {MAX_AGENT_EVIDENCE_EXPLANATION_CHARS} characters")]
    ExplanationBound,
    /// Model provenance empty or exceeds the bound.
    #[error("agent evidence model provenance must be non-empty and at most {MAX_AGENT_EVIDENCE_PROVENANCE_CHARS} characters")]
    ProvenanceBound,
    /// Idempotency key empty or exceeds the bound.
    #[error("agent evidence harness idempotency key must be non-empty and at most {MAX_AGENT_EVIDENCE_IDEMPOTENCY_CHARS} characters")]
    IdempotencyBound,
    /// Public inputs empty or exceed the bound.
    #[error("agent evidence public inputs must be non-empty and at most {MAX_AGENT_EVIDENCE_PUBLIC_INPUTS}")]
    PublicInputsBound,
    /// Relationship endpoints are the same announcement.
    #[error("agent evidence requires two distinct Pod Announcements")]
    SameAnnouncement,
    /// Referenced announcement is missing, stale, mismatched, or unverifiable.
    #[error("agent evidence announcement is unverifiable or mismatched: {0}")]
    Unverifiable(String),
    /// Referenced announcement lease is expired.
    #[error("agent evidence announcement is expired: {0}")]
    Expired(String),
    /// Referenced announcement was withdrawn.
    #[error("agent evidence announcement is withdrawn: {0}")]
    Withdrawn(String),
    /// Referenced announcement is blocked by Trust Policy.
    #[error("agent evidence announcement is blocked by Trust Policy: {0}")]
    Blocked(String),
    /// Public inputs omit a required relationship announcement.
    #[error("agent evidence public inputs must include both relationship announcements")]
    PublicInputsMismatch,
    /// Harness grant is missing, revoked, or lacks the scoped capability.
    #[error("agent evidence requires an active PodSimilarityEvidence harness grant")]
    CapabilityDenied,
    /// Submission is not scoped to an authenticated User.
    #[error("agent evidence requires an authenticated User")]
    UserRequired,
}

/// Ordered announcement-id pair used for idempotency and bounding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgentEvidencePodPair {
    /// Smaller announcement identity.
    pub low: Uuid,
    /// Larger announcement identity.
    pub high: Uuid,
}

impl AgentEvidencePodPair {
    /// Builds a direction-independent pair of distinct announcement identities.
    #[must_use]
    pub fn new(left: Uuid, right: Uuid) -> Option<Self> {
        if left == right {
            None
        } else if left < right {
            Some(Self {
                low: left,
                high: right,
            })
        } else {
            Some(Self {
                low: right,
                high: left,
            })
        }
    }

    /// Whether either side matches the candidate announcement.
    #[must_use]
    pub fn involves(self, announcement_id: Uuid) -> bool {
        self.low == announcement_id || self.high == announcement_id
    }
}

/// Resolves clamped freshness for a submission.
#[must_use]
pub fn resolve_agent_evidence_freshness_hours(requested: Option<u32>) -> u32 {
    requested
        .unwrap_or(DEFAULT_AGENT_EVIDENCE_FRESHNESS_HOURS)
        .clamp(1, MAX_AGENT_EVIDENCE_FRESHNESS_HOURS)
}

/// Validates request shape before store lookups.
///
/// # Errors
///
/// Returns [`AgentEvidenceError`] when bounds fail or endpoints are identical.
pub fn validate_agent_evidence_request_shape(
    request: &SubmitPodSimilarityAgentEvidenceRequest,
) -> Result<AgentEvidencePodPair, AgentEvidenceError> {
    let explanation = request.explanation.trim();
    if explanation.is_empty() || explanation.chars().count() > MAX_AGENT_EVIDENCE_EXPLANATION_CHARS
    {
        return Err(AgentEvidenceError::ExplanationBound);
    }
    let provenance = request.model_provenance.trim();
    if provenance.is_empty() || provenance.chars().count() > MAX_AGENT_EVIDENCE_PROVENANCE_CHARS {
        return Err(AgentEvidenceError::ProvenanceBound);
    }
    let idempotency = request.harness_idempotency_key.trim();
    if idempotency.is_empty() || idempotency.chars().count() > MAX_AGENT_EVIDENCE_IDEMPOTENCY_CHARS
    {
        return Err(AgentEvidenceError::IdempotencyBound);
    }
    if request.public_inputs.is_empty()
        || request.public_inputs.len() > MAX_AGENT_EVIDENCE_PUBLIC_INPUTS
    {
        return Err(AgentEvidenceError::PublicInputsBound);
    }
    let pair =
        AgentEvidencePodPair::new(request.left_announcement_id, request.right_announcement_id)
            .ok_or(AgentEvidenceError::SameAnnouncement)?;

    let mut input_ids = BTreeSet::new();
    for input in &request.public_inputs {
        if input.pod_slug.trim().is_empty() {
            return Err(AgentEvidenceError::Unverifiable(
                "public input pod_slug must not be empty".into(),
            ));
        }
        input_ids.insert(input.announcement_id);
    }
    if !input_ids.contains(&request.left_announcement_id)
        || !input_ids.contains(&request.right_announcement_id)
    {
        return Err(AgentEvidenceError::PublicInputsMismatch);
    }
    Ok(pair)
}

/// Looks up a known announcement by exact id and verifies it is current.
fn known_current_announcement<'a>(
    store: &'a InMemoryStore,
    announcement_id: Uuid,
) -> Result<&'a KnownPodAnnouncement, AgentEvidenceError> {
    store
        .known_pod_announcements
        .values()
        .find(|known| known.announcement.id == announcement_id)
        .ok_or_else(|| {
            AgentEvidenceError::Unverifiable(format!(
                "announcement {announcement_id} is not a known current Pod Announcement"
            ))
        })
}

/// Validates one announcement reference against store state and policy.
///
/// # Errors
///
/// Rejects stale, withdrawn, expired, blocked, mismatched, or unverifiable
/// announcements.
pub fn validate_announcement_for_agent_evidence(
    store: &InMemoryStore,
    announcement_id: Uuid,
    expected: Option<&PodSimilarityAgentEvidenceAnnouncementRef>,
    policy: &TrustPolicy,
    now: DateTime<Utc>,
) -> Result<PodSimilarityAgentEvidenceAnnouncementRef, AgentEvidenceError> {
    let known = known_current_announcement(store, announcement_id)?;
    let announcement = &known.announcement;
    if !announcement.verify().unwrap_or(false) {
        return Err(AgentEvidenceError::Unverifiable(format!(
            "announcement {announcement_id} signature is invalid"
        )));
    }
    if !announcement.lease_is_active(now) {
        return Err(AgentEvidenceError::Expired(announcement.pod_slug.clone()));
    }
    if !announcement_is_discovery_eligible(store, announcement, now) {
        return Err(AgentEvidenceError::Withdrawn(announcement.pod_slug.clone()));
    }
    if policy.blocks_announcement(announcement) {
        return Err(AgentEvidenceError::Blocked(announcement.pod_slug.clone()));
    }
    if let Some(expected) = expected {
        if expected.announcement_id != announcement.id
            || expected.origin_node_id != announcement.origin_node_id
            || expected.pod_slug != announcement.pod_slug
        {
            return Err(AgentEvidenceError::Unverifiable(format!(
                "public input for {} does not match known announcement",
                announcement.pod_slug
            )));
        }
    }
    // Current known map is keyed by (origin, slug); ensure this id is still the current one.
    let current_key = (announcement.origin_node_id, announcement.pod_slug.clone());
    if store
        .known_pod_announcements
        .get(&current_key)
        .is_none_or(|current| current.announcement.id != announcement.id)
    {
        return Err(AgentEvidenceError::Unverifiable(format!(
            "announcement {announcement_id} is stale relative to the current Pod Announcement"
        )));
    }
    Ok(PodSimilarityAgentEvidenceAnnouncementRef {
        announcement_id: announcement.id,
        origin_node_id: announcement.origin_node_id,
        pod_slug: announcement.pod_slug.clone(),
    })
}

/// Validates a full submission against store state, policy, and grant.
///
/// # Errors
///
/// Returns structured [`AgentEvidenceError`] for any acceptance failure.
pub fn validate_agent_evidence_submission(
    store: &InMemoryStore,
    request: &SubmitPodSimilarityAgentEvidenceRequest,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    harness: Option<&AgentHarness>,
    policy: &TrustPolicy,
    now: DateTime<Utc>,
) -> Result<
    (
        AgentEvidencePodPair,
        PodSimilarityAgentEvidenceAnnouncementRef,
        PodSimilarityAgentEvidenceAnnouncementRef,
        Vec<PodSimilarityAgentEvidenceAnnouncementRef>,
        Duration,
    ),
    AgentEvidenceError,
> {
    let pair = validate_agent_evidence_request_shape(request)?;
    let Some(harness) = harness else {
        return Err(AgentEvidenceError::CapabilityDenied);
    };
    if harness.revoked_at.is_some()
        || harness.user_id != user_id
        || harness.tenant_id != tenant_id
        || !harness
            .grant
            .capabilities
            .contains(&crate::domain::HarnessCapability::PodSimilarityEvidence)
    {
        return Err(AgentEvidenceError::CapabilityDenied);
    }

    let left_input = request
        .public_inputs
        .iter()
        .find(|input| input.announcement_id == request.left_announcement_id);
    let right_input = request
        .public_inputs
        .iter()
        .find(|input| input.announcement_id == request.right_announcement_id);
    let left = validate_announcement_for_agent_evidence(
        store,
        request.left_announcement_id,
        left_input,
        policy,
        now,
    )?;
    let right = validate_announcement_for_agent_evidence(
        store,
        request.right_announcement_id,
        right_input,
        policy,
        now,
    )?;

    let mut public_inputs = Vec::with_capacity(request.public_inputs.len());
    for input in &request.public_inputs {
        public_inputs.push(validate_announcement_for_agent_evidence(
            store,
            input.announcement_id,
            Some(input),
            policy,
            now,
        )?);
    }

    let hours = resolve_agent_evidence_freshness_hours(request.freshness_hours);
    let freshness = Duration::hours(i64::from(hours));
    Ok((pair, left, right, public_inputs, freshness))
}

/// Builds a durable evidence record after validation.
#[must_use]
pub fn build_agent_evidence_record(
    request: &SubmitPodSimilarityAgentEvidenceRequest,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    submitted_by: AgentHarnessId,
    left: PodSimilarityAgentEvidenceAnnouncementRef,
    right: PodSimilarityAgentEvidenceAnnouncementRef,
    public_inputs: Vec<PodSimilarityAgentEvidenceAnnouncementRef>,
    now: DateTime<Utc>,
    freshness: Duration,
) -> PodSimilarityAgentEvidence {
    PodSimilarityAgentEvidence {
        id: Uuid::now_v7(),
        user_id,
        tenant_id,
        submitted_by,
        left,
        right,
        confidence: request.confidence,
        explanation: request.explanation.trim().to_string(),
        public_inputs,
        model_provenance: request.model_provenance.trim().to_string(),
        harness_idempotency_key: request.harness_idempotency_key.trim().to_string(),
        submitted_at: now,
        expires_at: now + freshness,
    }
}

/// Whether evidence is still usable for ranking at `now`.
#[must_use]
pub fn agent_evidence_is_fresh(evidence: &PodSimilarityAgentEvidence, now: DateTime<Utc>) -> bool {
    evidence.expires_at > now && evidence.submitted_at <= now
}

/// Whether the submitting harness grant is still active.
#[must_use]
pub fn agent_evidence_harness_active(
    store: &InMemoryStore,
    evidence: &PodSimilarityAgentEvidence,
) -> bool {
    store
        .agent_harnesses
        .get(&evidence.submitted_by)
        .is_some_and(|harness| {
            harness.revoked_at.is_none()
                && harness.user_id == evidence.user_id
                && harness.tenant_id == evidence.tenant_id
                && harness
                    .grant
                    .capabilities
                    .contains(&crate::domain::HarnessCapability::PodSimilarityEvidence)
        })
}

/// Whether both endpoints still verify as current, eligible, and unblocked.
#[must_use]
pub fn agent_evidence_endpoints_usable(
    store: &InMemoryStore,
    evidence: &PodSimilarityAgentEvidence,
    policy: &TrustPolicy,
    now: DateTime<Utc>,
) -> bool {
    for side in [&evidence.left, &evidence.right] {
        if validate_announcement_for_agent_evidence(
            store,
            side.announcement_id,
            Some(side),
            policy,
            now,
        )
        .is_err()
        {
            return false;
        }
    }
    true
}

/// Whether evidence is active for current ranking.
#[must_use]
pub fn agent_evidence_is_active(
    store: &InMemoryStore,
    evidence: &PodSimilarityAgentEvidence,
    policy: &TrustPolicy,
    now: DateTime<Utc>,
) -> bool {
    agent_evidence_is_fresh(evidence, now)
        && agent_evidence_harness_active(store, evidence)
        && agent_evidence_endpoints_usable(store, evidence, policy, now)
}

/// Collects active evidence involving a candidate announcement for one User.
#[must_use]
pub fn collect_active_agent_evidence_for_candidate<'a>(
    store: &'a InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    candidate: &PodAnnouncement,
    policy: &TrustPolicy,
    now: DateTime<Utc>,
) -> Vec<&'a PodSimilarityAgentEvidence> {
    let mut collected = store
        .pod_similarity_agent_evidence
        .values()
        .filter(|evidence| {
            evidence.user_id == user_id
                && evidence.tenant_id == tenant_id
                && (evidence.left.announcement_id == candidate.id
                    || evidence.right.announcement_id == candidate.id)
                && agent_evidence_is_active(store, evidence, policy, now)
        })
        .collect::<Vec<_>>();
    collected.sort_by(|left, right| {
        right
            .submitted_at
            .cmp(&left.submitted_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    collected
}

/// Idempotency lookup key for exact retry-safe replay.
#[must_use]
pub fn agent_evidence_idempotency_matches(
    evidence: &PodSimilarityAgentEvidence,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    submitted_by: AgentHarnessId,
    harness_idempotency_key: &str,
) -> bool {
    evidence.user_id == user_id
        && evidence.tenant_id == tenant_id
        && evidence.submitted_by == submitted_by
        && evidence.harness_idempotency_key == harness_idempotency_key
}

/// Finds an existing idempotent submission.
#[must_use]
pub fn find_idempotent_agent_evidence<'a>(
    store: &'a InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    submitted_by: AgentHarnessId,
    harness_idempotency_key: &str,
) -> Option<&'a PodSimilarityAgentEvidence> {
    store
        .pod_similarity_agent_evidence
        .values()
        .find(|evidence| {
            agent_evidence_idempotency_matches(
                evidence,
                user_id,
                tenant_id,
                submitted_by,
                harness_idempotency_key,
            )
        })
}

/// Finds active evidence already bounding the same pair + provenance for a harness.
#[must_use]
pub fn find_bounded_agent_evidence_for_pair<'a>(
    store: &'a InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    submitted_by: AgentHarnessId,
    model_provenance: &str,
    pair: AgentEvidencePodPair,
    now: DateTime<Utc>,
) -> Option<&'a PodSimilarityAgentEvidence> {
    store
        .pod_similarity_agent_evidence
        .values()
        .find(|evidence| {
            evidence.user_id == user_id
                && evidence.tenant_id == tenant_id
                && evidence.submitted_by == submitted_by
                && evidence.model_provenance == model_provenance
                && agent_evidence_is_fresh(evidence, now)
                && AgentEvidencePodPair::new(
                    evidence.left.announcement_id,
                    evidence.right.announcement_id,
                )
                .is_some_and(|existing| existing == pair)
        })
}

/// Applies agent evidence as a local ordering boost with inspectable reasons.
///
/// Agent evidence only strengthens an already-positive deterministic base score
/// and never creates eligibility, trust, Subscription, or placement by itself.
/// Caps and blocks are applied by callers after this adjustment.
#[must_use]
pub fn layer_agent_similarity_evidence(
    mut score: PodSimilarityScore,
    evidence: &[&PodSimilarityAgentEvidence],
) -> PodSimilarityScore {
    if score.base_score <= 0.0 || evidence.is_empty() {
        return score;
    }
    let prior = score.score;
    let mut boost = 0.0_f32;
    for item in evidence {
        let unit = (item.confidence.value() * 0.25).min(0.25);
        boost = (boost + unit).min(MAX_AGENT_EVIDENCE_BOOST);
        let related_slug = related_pod_slug(item);
        score.reasons.push(SimilarityReason {
            kind: SimilarityEvidenceKind::Agent,
            detail: format!(
                "local agent semantic relationship involving {related_slug} (confidence {:.2}; not transferable trust, not an Endorsement): {}",
                item.confidence.value(),
                item.explanation
            ),
        });
    }
    score.score = prior + boost;
    score
}

fn related_pod_slug(evidence: &PodSimilarityAgentEvidence) -> &str {
    // Prefer lexicographically second slug for a stable display of the pair.
    if evidence.left.pod_slug <= evidence.right.pod_slug {
        &evidence.right.pod_slug
    } else {
        &evidence.left.pod_slug
    }
}

/// Agent evidence never grants Feed eligibility or placement on its own.
#[must_use]
pub fn agent_evidence_alone_grants_eligibility(base_score: f32) -> bool {
    base_score > 0.0
}

/// Helper for tests and callers that need announcement refs from a live announcement.
#[must_use]
pub fn announcement_ref(
    announcement: &PodAnnouncement,
) -> PodSimilarityAgentEvidenceAnnouncementRef {
    PodSimilarityAgentEvidenceAnnouncementRef {
        announcement_id: announcement.id,
        origin_node_id: announcement.origin_node_id,
        pod_slug: announcement.pod_slug.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        announcement_lease_duration, AgentHarnessKind, CandidateConfidence, HarnessCapability,
        HarnessGrant, NodeInfo, PackageVersion, CURRENT_PROTOCOL_VERSION,
    };
    use crate::pod_similarity::score::{
        score_pod_similarity, CandidatePodEvidence, LocalSimilarityContext,
    };
    use crate::signing::{create_node_identity, sign_pod_announcement};
    use crate::store::InMemoryStore;

    fn announcement(subject: &str, slug: &str) -> (crate::domain::NodeIdentity, PodAnnouncement) {
        let node = create_node_identity("origin", None);
        let now = Utc::now();
        let announcement = sign_pod_announcement(
            &node,
            PodAnnouncement {
                id: Uuid::now_v7(),
                origin_node_id: node.id,
                signer: NodeInfo {
                    node_id: node.id,
                    display_name: node.display_name.clone(),
                    public_key: node.public_key.clone(),
                    supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
                },
                pod_slug: slug.into(),
                pod_name: slug.replace('-', " "),
                subject: subject.into(),
                public_pod_url: format!("https://origin.example/federation/pods/{slug}"),
                package_version: PackageVersion::new(1).unwrap(),
                latest_event_hash: None,
                announced_at: now,
                expires_at: now + announcement_lease_duration(),
                signature: String::new(),
            },
        )
        .unwrap();
        (node, announcement)
    }

    fn sample_request(
        left: &PodAnnouncement,
        right: &PodAnnouncement,
    ) -> SubmitPodSimilarityAgentEvidenceRequest {
        SubmitPodSimilarityAgentEvidenceRequest {
            left_announcement_id: left.id,
            right_announcement_id: right.id,
            confidence: CandidateConfidence::new(0.8).unwrap(),
            explanation: "Shared careful systems research subject".into(),
            public_inputs: vec![announcement_ref(left), announcement_ref(right)],
            model_provenance: "test-model-v1".into(),
            harness_idempotency_key: "idem-1".into(),
            freshness_hours: Some(24),
        }
    }

    fn active_harness(user_id: UserId) -> AgentHarness {
        AgentHarness {
            id: Uuid::now_v7().into(),
            user_id,
            tenant_id: None,
            label: "similarity agent".into(),
            kind: AgentHarnessKind::Unattended,
            grant: HarnessGrant {
                capabilities: vec![HarnessCapability::PodSimilarityEvidence],
                pod_ids: None,
            },
            created_at: Utc::now(),
            revoked_at: None,
        }
    }

    #[test]
    fn request_shape_rejects_same_announcement_and_bounds() {
        let (_n, a) = announcement("systems", "sys");
        let mut request = sample_request(&a, &a);
        assert_eq!(
            validate_agent_evidence_request_shape(&request),
            Err(AgentEvidenceError::SameAnnouncement)
        );
        let (_n2, b) = announcement("systems", "sys-b");
        request = sample_request(&a, &b);
        request.explanation = " ".into();
        assert_eq!(
            validate_agent_evidence_request_shape(&request),
            Err(AgentEvidenceError::ExplanationBound)
        );
        request = sample_request(&a, &b);
        request.public_inputs = vec![announcement_ref(&a)];
        assert_eq!(
            validate_agent_evidence_request_shape(&request),
            Err(AgentEvidenceError::PublicInputsMismatch)
        );
    }

    #[test]
    fn rejects_expired_withdrawn_blocked_and_unverifiable() {
        let mut store = InMemoryStore::default();
        let (_node, left) = announcement("Distributed systems research", "left-sys");
        let (_node2, right) = announcement("Distributed systems notes", "right-sys");
        store.known_pod_announcements.insert(
            (left.origin_node_id, left.pod_slug.clone()),
            KnownPodAnnouncement {
                announcement: left.clone(),
                received_from_peer_id: None,
                received_from_index_urls: BTreeSet::new(),
                received_from_bootstrap_urls: BTreeSet::new(),
                received_from_discovery_peer_endpoints: BTreeSet::new(),
                received_at: Utc::now(),
            },
        );
        store.known_pod_announcements.insert(
            (right.origin_node_id, right.pod_slug.clone()),
            KnownPodAnnouncement {
                announcement: right.clone(),
                received_from_peer_id: None,
                received_from_index_urls: BTreeSet::new(),
                received_from_bootstrap_urls: BTreeSet::new(),
                received_from_discovery_peer_endpoints: BTreeSet::new(),
                received_at: Utc::now(),
            },
        );
        let user_id = Uuid::now_v7();
        let policy = TrustPolicy::new(user_id, None);
        let harness = active_harness(user_id);
        let now = Utc::now();

        // Happy path validates.
        let request = sample_request(&left, &right);
        assert!(validate_agent_evidence_submission(
            &store,
            &request,
            user_id,
            None,
            Some(&harness),
            &policy,
            now,
        )
        .is_ok());

        // Expired: lease end must equal announced_at + lease duration for a valid signature.
        let origin_right = create_node_identity("origin-right-expired", None);
        let announced_at = now - announcement_lease_duration() - Duration::hours(1);
        let expired = sign_pod_announcement(
            &origin_right,
            PodAnnouncement {
                id: Uuid::now_v7(),
                origin_node_id: origin_right.id,
                signer: NodeInfo {
                    node_id: origin_right.id,
                    display_name: origin_right.display_name.clone(),
                    public_key: origin_right.public_key.clone(),
                    supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
                },
                pod_slug: "expired-sys".into(),
                pod_name: "expired sys".into(),
                subject: "Distributed systems notes".into(),
                public_pod_url: "https://origin.example/federation/pods/expired-sys".into(),
                package_version: PackageVersion::new(1).unwrap(),
                latest_event_hash: None,
                announced_at,
                expires_at: announced_at + announcement_lease_duration(),
                signature: String::new(),
            },
        )
        .unwrap();
        store.known_pod_announcements.insert(
            (expired.origin_node_id, expired.pod_slug.clone()),
            KnownPodAnnouncement {
                announcement: expired.clone(),
                received_from_peer_id: None,
                received_from_index_urls: BTreeSet::new(),
                received_from_bootstrap_urls: BTreeSet::new(),
                received_from_discovery_peer_endpoints: BTreeSet::new(),
                received_at: now,
            },
        );
        let expired_request = sample_request(&left, &expired);
        let err = validate_agent_evidence_submission(
            &store,
            &expired_request,
            user_id,
            None,
            Some(&harness),
            &policy,
            now,
        )
        .unwrap_err();
        assert!(
            matches!(err, AgentEvidenceError::Expired(_)),
            "expected Expired, got {err:?}"
        );

        // Blocked right announcement.
        let mut blocked_policy = TrustPolicy::new(user_id, None);
        blocked_policy
            .blocked_pods
            .insert(crate::domain::BlockedPod::new(
                right.origin_node_id,
                right.pod_slug.clone(),
            ));
        let err = validate_agent_evidence_submission(
            &store,
            &request,
            user_id,
            None,
            Some(&harness),
            &blocked_policy,
            now,
        )
        .unwrap_err();
        assert!(matches!(err, AgentEvidenceError::Blocked(_)));

        // Unverifiable unknown id
        let mut unknown = request.clone();
        unknown.right_announcement_id = Uuid::now_v7();
        unknown.public_inputs = vec![
            announcement_ref(&left),
            PodSimilarityAgentEvidenceAnnouncementRef {
                announcement_id: unknown.right_announcement_id,
                origin_node_id: right.origin_node_id,
                pod_slug: right.pod_slug.clone(),
            },
        ];
        let err = validate_agent_evidence_submission(
            &store,
            &unknown,
            user_id,
            None,
            Some(&harness),
            &policy,
            now,
        )
        .unwrap_err();
        assert!(matches!(err, AgentEvidenceError::Unverifiable(_)));

        // Capability denied when harness revoked
        let mut revoked = harness.clone();
        revoked.revoked_at = Some(now);
        let err = validate_agent_evidence_submission(
            &store,
            &request,
            user_id,
            None,
            Some(&revoked),
            &policy,
            now,
        )
        .unwrap_err();
        assert_eq!(err, AgentEvidenceError::CapabilityDenied);
    }

    #[test]
    fn agent_evidence_adjusts_score_with_inspectable_reason_but_not_zero_base() {
        let (_n, food) = announcement("Cooking recipes", "food");
        let local = LocalSimilarityContext::from_query("distributed systems");
        let base = score_pod_similarity(
            &local,
            &CandidatePodEvidence {
                announcement: &food,
                context_text: None,
                samples: &[],
                endorsements: &[],
                samples_verified: false,
            },
        );
        assert_eq!(base.base_score, 0.0);
        let evidence = PodSimilarityAgentEvidence {
            id: Uuid::now_v7(),
            user_id: Uuid::now_v7(),
            tenant_id: None,
            submitted_by: Uuid::now_v7().into(),
            left: announcement_ref(&food),
            right: announcement_ref(&food),
            confidence: CandidateConfidence::new(1.0).unwrap(),
            explanation: "Forced relationship".into(),
            public_inputs: vec![announcement_ref(&food)],
            model_provenance: "m".into(),
            harness_idempotency_key: "k".into(),
            submitted_at: Utc::now(),
            expires_at: Utc::now() + Duration::hours(1),
        };
        let layered = layer_agent_similarity_evidence(base.clone(), &[&evidence]);
        assert_eq!(layered.score, base.score);
        assert_eq!(layered.base_score, 0.0);
        assert!(!agent_evidence_alone_grants_eligibility(layered.base_score));

        let (_n2, relevant) = announcement("Distributed systems research", "systems");
        let local = LocalSimilarityContext::from_query("distributed systems");
        let base = score_pod_similarity(
            &local,
            &CandidatePodEvidence {
                announcement: &relevant,
                context_text: None,
                samples: &[],
                endorsements: &[],
                samples_verified: false,
            },
        );
        assert!(base.base_score > 0.0);
        let (_n3, related) = announcement("Related systems lab notes", "related-sys");
        let evidence = PodSimilarityAgentEvidence {
            id: Uuid::now_v7(),
            user_id: Uuid::now_v7(),
            tenant_id: None,
            submitted_by: Uuid::now_v7().into(),
            left: announcement_ref(&relevant),
            right: announcement_ref(&related),
            confidence: CandidateConfidence::new(0.8).unwrap(),
            explanation: "Strong semantic overlap in systems research".into(),
            public_inputs: vec![announcement_ref(&relevant), announcement_ref(&related)],
            model_provenance: "m".into(),
            harness_idempotency_key: "k".into(),
            submitted_at: Utc::now(),
            expires_at: Utc::now() + Duration::hours(1),
        };
        let layered = layer_agent_similarity_evidence(base.clone(), &[&evidence]);
        assert!(layered.score > base.score);
        assert_eq!(layered.base_score, base.base_score);
        assert!(layered
            .reasons
            .iter()
            .any(|r| r.kind == SimilarityEvidenceKind::Agent
                && r.detail.contains("not transferable trust")
                && r.detail.contains("not an Endorsement")));
    }

    #[test]
    fn revoked_harness_evidence_excluded_from_active_set() {
        let mut store = InMemoryStore::default();
        let user_id = Uuid::now_v7();
        let (_n, left) = announcement("Distributed systems research", "left");
        let (_n2, right) = announcement("Distributed systems notes", "right");
        for ann in [&left, &right] {
            store.known_pod_announcements.insert(
                (ann.origin_node_id, ann.pod_slug.clone()),
                KnownPodAnnouncement {
                    announcement: ann.clone(),
                    received_from_peer_id: None,
                    received_from_index_urls: BTreeSet::new(),
                    received_from_bootstrap_urls: BTreeSet::new(),
                    received_from_discovery_peer_endpoints: BTreeSet::new(),
                    received_at: Utc::now(),
                },
            );
        }
        let mut harness = active_harness(user_id);
        let harness_id = harness.id;
        store.agent_harnesses.insert(harness_id, harness.clone());
        let evidence = PodSimilarityAgentEvidence {
            id: Uuid::now_v7(),
            user_id,
            tenant_id: None,
            submitted_by: harness_id,
            left: announcement_ref(&left),
            right: announcement_ref(&right),
            confidence: CandidateConfidence::new(0.9).unwrap(),
            explanation: "overlap".into(),
            public_inputs: vec![announcement_ref(&left), announcement_ref(&right)],
            model_provenance: "m".into(),
            harness_idempotency_key: "k".into(),
            submitted_at: Utc::now(),
            expires_at: Utc::now() + Duration::hours(2),
        };
        store
            .pod_similarity_agent_evidence
            .insert(evidence.id, evidence.clone());
        let policy = TrustPolicy::new(user_id, None);
        let now = Utc::now();
        assert_eq!(
            collect_active_agent_evidence_for_candidate(
                &store, user_id, None, &right, &policy, now
            )
            .len(),
            1
        );
        harness.revoked_at = Some(now);
        store.agent_harnesses.insert(harness_id, harness);
        assert!(collect_active_agent_evidence_for_candidate(
            &store, user_id, None, &right, &policy, now
        )
        .is_empty());
    }

    #[test]
    fn freshness_clamp_and_pair_ordering() {
        assert_eq!(
            resolve_agent_evidence_freshness_hours(None),
            DEFAULT_AGENT_EVIDENCE_FRESHNESS_HOURS
        );
        assert_eq!(
            resolve_agent_evidence_freshness_hours(Some(9999)),
            MAX_AGENT_EVIDENCE_FRESHNESS_HOURS
        );
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        assert_eq!(
            AgentEvidencePodPair::new(a, b),
            AgentEvidencePodPair::new(b, a)
        );
        assert!(AgentEvidencePodPair::new(a, a).is_none());
    }

    #[test]
    fn without_agent_evidence_score_unchanged() {
        let (_n, announcement) = announcement("machine learning systems", "ml-sys");
        let local = LocalSimilarityContext::from_query("machine learning");
        let base = score_pod_similarity(
            &local,
            &CandidatePodEvidence {
                announcement: &announcement,
                context_text: None,
                samples: &[],
                endorsements: &[],
                samples_verified: false,
            },
        );
        let layered = layer_agent_similarity_evidence(base.clone(), &[]);
        assert_eq!(layered, base);
    }
}
