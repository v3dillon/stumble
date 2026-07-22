//! Policy filtering, endorsement collection, and ranked selection.

use super::agent_evidence::layer_agent_similarity_evidence;
use super::caps::{ExplorationCapTracker, ExplorationCaps};
use super::score::{
    score_pod_similarity, CandidatePodEvidence, LocalSimilarityContext, PodSimilarityScore,
};
use crate::domain::{
    FeedContentReference, KnownPodAnnouncement, PodAnnouncement, PodEndorsement,
    PodSimilarityAgentEvidence, TrustPolicy,
};
use crate::pod_announcement::{
    announcement_delivery_is_active, announcement_is_discovery_eligible,
};
use crate::store::InMemoryStore;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

/// Owned candidate evidence so Explore / Feed can rank without HashMap dances.
#[derive(Debug, Clone)]
pub struct OwnedCandidateEvidence {
    /// Current verified announcement.
    pub announcement: PodAnnouncement,
    /// Optional local Pod Context text.
    pub context_text: Option<String>,
    /// Policy-filtered samples for scoring / delivery.
    pub samples: Vec<FeedContentReference>,
    /// Valid endorsements binding the current announcement.
    pub endorsements: Vec<PodEndorsement>,
    /// Whether samples passed Origin signature + current announcement binding.
    pub samples_verified: bool,
}

impl OwnedCandidateEvidence {
    /// Borrows as scoring evidence.
    #[must_use]
    pub fn as_evidence(&self) -> CandidatePodEvidence<'_> {
        CandidatePodEvidence {
            announcement: &self.announcement,
            context_text: self.context_text.as_deref(),
            samples: &self.samples,
            endorsements: &self.endorsements,
            samples_verified: self.samples_verified,
        }
    }
}

/// Ranked Explore candidate after policy filtering and similarity scoring.
#[derive(Debug, Clone)]
pub struct RankedSimilarPod {
    /// Announcement under consideration.
    pub announcement: PodAnnouncement,
    /// Similarity outcome.
    pub similarity: PodSimilarityScore,
    /// Policy-filtered samples to return.
    pub samples: Vec<FeedContentReference>,
    /// Valid endorsements attached for inspection.
    pub endorsements: Vec<PodEndorsement>,
}

/// Whether a signed endorsement targets the given current announcement.
#[must_use]
pub fn endorsement_targets_announcement(
    endorsement: &PodEndorsement,
    announcement: &PodAnnouncement,
) -> bool {
    endorsement.endorsed_node_id == announcement.origin_node_id
        && endorsement.endorsed_pod_slug == announcement.pod_slug
        && endorsement.endorsed_announcement_id == announcement.id
        && endorsement.verify().unwrap_or(false)
}

/// Shared Explore/Feed gate: discovery-eligible, actively delivered, and not blocked.
#[must_use]
pub fn announcement_scoring_eligible(
    store: &InMemoryStore,
    known: &KnownPodAnnouncement,
    policy: &TrustPolicy,
    now: DateTime<Utc>,
) -> bool {
    announcement_is_discovery_eligible(store, &known.announcement, now)
        && announcement_delivery_is_active(store, known, Some(policy))
        && !policy.blocks_announcement(&known.announcement)
}

/// Whether an endorser's known announcement is still usable for scoring under policy.
#[must_use]
pub fn endorser_allowed(
    store: &InMemoryStore,
    policy: &TrustPolicy,
    endorsement: &PodEndorsement,
) -> bool {
    store
        .known_pod_announcements
        .get(&(
            endorsement.endorsing_node_id,
            endorsement.endorsing_pod_slug.clone(),
        ))
        .is_some_and(|known| {
            known.announcement.id == endorsement.endorsing_announcement_id
                && !policy.blocks_announcement(&known.announcement)
        })
}

/// Collects policy-aware endorsements for a current announcement from store state.
#[must_use]
pub fn collect_policy_endorsements(
    store: &InMemoryStore,
    announcement: &PodAnnouncement,
    policy: &TrustPolicy,
) -> Vec<PodEndorsement> {
    collect_endorsements_for_announcement(
        announcement,
        store.pod_endorsements.values(),
        |endorsement| endorser_allowed(store, policy, endorsement),
    )
}

/// Collects and sorts valid endorsements for a current announcement.
///
/// `endorser_allowed` encodes Trust Policy / known-announcement checks that
/// require store access; pure binding + signature checks live here.
#[must_use]
pub fn collect_endorsements_for_announcement<'a, I>(
    announcement: &PodAnnouncement,
    endorsements: I,
    endorser_allowed: impl Fn(&PodEndorsement) -> bool,
) -> Vec<PodEndorsement>
where
    I: IntoIterator<Item = &'a PodEndorsement>,
{
    let mut collected = endorsements
        .into_iter()
        .filter(|endorsement| {
            endorsement_targets_announcement(endorsement, announcement)
                && endorser_allowed(endorsement)
        })
        .cloned()
        .collect::<Vec<_>>();
    collected.sort_by(|left, right| {
        left.endorsing_pod_slug
            .cmp(&right.endorsing_pod_slug)
            .then_with(|| left.id.cmp(&right.id))
    });
    collected
}

/// Filters Explore samples by Trust Policy before scoring or delivery.
#[must_use]
pub fn filter_samples_by_policy(
    policy: &TrustPolicy,
    samples: &[FeedContentReference],
) -> Vec<FeedContentReference> {
    samples
        .iter()
        .filter(|sample| !policy.blocks_content_reference(sample))
        .cloned()
        .collect()
}

/// Scores and ranks candidates, applying blocks before ranking and Origin caps after.
///
/// Optional agent evidence is layered after deterministic scoring and before
/// caps. Blocks still exclude candidates first, so agent evidence cannot
/// override Trust Policy. Without agent evidence, behavior matches the pure
/// deterministic baseline.
#[must_use]
pub fn rank_similar_pods(
    local: &LocalSimilarityContext,
    candidates: Vec<CandidatePodEvidence<'_>>,
    policy: &TrustPolicy,
    caps: ExplorationCaps,
    limit: usize,
) -> Vec<RankedSimilarPod> {
    rank_similar_pods_with_agent_evidence(local, candidates, policy, caps, limit, &HashMap::new())
}

/// Like [`rank_similar_pods`], with optional active agent evidence keyed by
/// candidate announcement id.
#[must_use]
pub fn rank_similar_pods_with_agent_evidence(
    local: &LocalSimilarityContext,
    candidates: Vec<CandidatePodEvidence<'_>>,
    policy: &TrustPolicy,
    caps: ExplorationCaps,
    limit: usize,
    agent_evidence_by_announcement: &HashMap<Uuid, Vec<&PodSimilarityAgentEvidence>>,
) -> Vec<RankedSimilarPod> {
    let mut scored = Vec::new();
    for candidate in candidates {
        if policy.blocks_announcement(candidate.announcement) {
            continue;
        }
        let filtered = filter_samples_by_policy(policy, candidate.samples);
        let evidence = CandidatePodEvidence {
            announcement: candidate.announcement,
            context_text: candidate.context_text,
            samples: &filtered,
            endorsements: candidate.endorsements,
            samples_verified: candidate.samples_verified,
        };
        let mut similarity = score_pod_similarity(local, &evidence);
        if let Some(agent_evidence) = agent_evidence_by_announcement.get(&candidate.announcement.id)
        {
            similarity = layer_agent_similarity_evidence(similarity, agent_evidence);
        }
        // Zero-signal empty-query baseline always admits; otherwise require positive score.
        // Agent evidence cannot create eligibility from a zero deterministic base.
        if !local.is_empty() && similarity.score <= 0.0 {
            continue;
        }
        scored.push(RankedSimilarPod {
            announcement: candidate.announcement.clone(),
            similarity,
            samples: filtered,
            endorsements: candidate.endorsements.to_vec(),
        });
    }
    scored.sort_by(|left, right| {
        right
            .similarity
            .score
            .total_cmp(&left.similarity.score)
            .then_with(|| left.announcement.pod_slug.cmp(&right.announcement.pod_slug))
    });

    // Deterministic caps apply after agent evidence is considered.
    let mut tracker = ExplorationCapTracker::new();
    let mut selected = Vec::new();
    for candidate in scored {
        let origin = candidate.announcement.origin_node_id;
        if !tracker.can_admit_origin(origin, caps) {
            continue;
        }
        if !tracker.can_admit_pod(origin, &candidate.announcement.pod_slug, caps) {
            continue;
        }
        if candidate.similarity.trial_exposure && !tracker.can_admit_trial(origin, caps) {
            continue;
        }
        tracker.record(
            origin,
            &candidate.announcement.pod_slug,
            None,
            candidate.similarity.trial_exposure,
        );
        selected.push(candidate);
        if selected.len() >= limit {
            break;
        }
    }
    selected
}

/// Whether explicit feedback should adjust future local exposure for a Pod/source.
///
/// Passive delivery (no feedback recorded) never creates durable preference.
/// Explicit feed feedback kinds—including dismiss as a soft negative—do.
/// Personal Discovery batch dismiss/ignore remains a separate path that does not
/// call this helper.
#[must_use]
pub fn feedback_affects_future_exposure(kind: crate::domain::FeedbackKind) -> bool {
    matches!(
        kind,
        crate::domain::FeedbackKind::Saved
            | crate::domain::FeedbackKind::Interesting
            | crate::domain::FeedbackKind::NotForMe
            | crate::domain::FeedbackKind::Dismissed
            | crate::domain::FeedbackKind::BlockSource
            | crate::domain::FeedbackKind::BlockTopic
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        announcement_lease_duration, NodeInfo, PackageVersion, CURRENT_PROTOCOL_VERSION,
    };
    use crate::signing::{create_node_identity, sign_pod_announcement};
    use chrono::Utc;
    use uuid::Uuid;

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

    #[test]
    fn blocks_exclude_before_ranking() {
        let (_node, blocked) = announcement("systems research", "blocked-pod");
        let (_node2, allowed) = announcement("systems research", "allowed-pod");
        let mut policy = TrustPolicy::new(Uuid::now_v7(), None);
        policy.blocked_pods.insert(crate::domain::BlockedPod::new(
            blocked.origin_node_id,
            "blocked-pod",
        ));
        let local = LocalSimilarityContext::from_query("systems");
        let caps = ExplorationCaps::explore_defaults();
        let ranked = rank_similar_pods(
            &local,
            vec![
                CandidatePodEvidence {
                    announcement: &blocked,
                    context_text: None,
                    samples: &[],
                    endorsements: &[],
                    samples_verified: false,
                },
                CandidatePodEvidence {
                    announcement: &allowed,
                    context_text: None,
                    samples: &[],
                    endorsements: &[],
                    samples_verified: false,
                },
            ],
            &policy,
            caps,
            10,
        );
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].announcement.pod_slug, "allowed-pod");
    }

    #[test]
    fn endorsement_alone_does_not_rank_unrelated_pod() {
        let (node, announcement) = announcement("Cooking recipes and baking tips", "food-blog");
        let endorsement = PodEndorsement {
            id: Uuid::now_v7(),
            endorsing_node_id: node.id,
            signer: NodeInfo {
                node_id: node.id,
                display_name: node.display_name.clone(),
                public_key: node.public_key.clone(),
                supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
            },
            endorsing_pod_slug: "curators".into(),
            endorsing_announcement_id: Uuid::now_v7(),
            endorsed_node_id: announcement.origin_node_id,
            endorsed_pod_slug: announcement.pod_slug.clone(),
            endorsed_announcement_id: announcement.id,
            reason: "Friendly shout-out".into(),
            endorsed_at: Utc::now(),
            signature: "sig".into(),
        };
        let local = LocalSimilarityContext::from_query("distributed systems research");
        let policy = TrustPolicy::new(Uuid::now_v7(), None);
        let ranked = rank_similar_pods(
            &local,
            vec![CandidatePodEvidence {
                announcement: &announcement,
                context_text: None,
                samples: &[],
                endorsements: &[endorsement],
                samples_verified: false,
            }],
            &policy,
            ExplorationCaps::explore_defaults(),
            10,
        );
        assert!(ranked.is_empty());
    }

    #[test]
    fn per_origin_caps_limit_results() {
        let (node, a) = announcement("systems one research topic", "pod-a");
        let b = {
            let now = Utc::now();
            sign_pod_announcement(
                &node,
                PodAnnouncement {
                    id: Uuid::now_v7(),
                    origin_node_id: node.id,
                    signer: a.signer.clone(),
                    pod_slug: "pod-b".into(),
                    pod_name: "pod b".into(),
                    subject: "systems two research topic".into(),
                    public_pod_url: "https://origin.example/federation/pods/pod-b".into(),
                    package_version: PackageVersion::new(1).unwrap(),
                    latest_event_hash: None,
                    announced_at: now,
                    expires_at: now + announcement_lease_duration(),
                    signature: String::new(),
                },
            )
            .unwrap()
        };
        let c = {
            let now = Utc::now();
            sign_pod_announcement(
                &node,
                PodAnnouncement {
                    id: Uuid::now_v7(),
                    origin_node_id: node.id,
                    signer: a.signer.clone(),
                    pod_slug: "pod-c".into(),
                    pod_name: "pod c".into(),
                    subject: "systems three research topic".into(),
                    public_pod_url: "https://origin.example/federation/pods/pod-c".into(),
                    package_version: PackageVersion::new(1).unwrap(),
                    latest_event_hash: None,
                    announced_at: now,
                    expires_at: now + announcement_lease_duration(),
                    signature: String::new(),
                },
            )
            .unwrap()
        };
        let d = {
            let now = Utc::now();
            sign_pod_announcement(
                &node,
                PodAnnouncement {
                    id: Uuid::now_v7(),
                    origin_node_id: node.id,
                    signer: a.signer.clone(),
                    pod_slug: "pod-d".into(),
                    pod_name: "pod d".into(),
                    subject: "systems four research topic".into(),
                    public_pod_url: "https://origin.example/federation/pods/pod-d".into(),
                    package_version: PackageVersion::new(1).unwrap(),
                    latest_event_hash: None,
                    announced_at: now,
                    expires_at: now + announcement_lease_duration(),
                    signature: String::new(),
                },
            )
            .unwrap()
        };
        let local = LocalSimilarityContext::from_query("systems research");
        let policy = TrustPolicy::new(Uuid::now_v7(), None);
        let caps = ExplorationCaps {
            per_origin: 2,
            per_pod: 1,
            per_source: 5,
            per_origin_trial: 2,
        };
        let ranked = rank_similar_pods(
            &local,
            vec![
                CandidatePodEvidence {
                    announcement: &a,
                    context_text: None,
                    samples: &[],
                    endorsements: &[],
                    samples_verified: false,
                },
                CandidatePodEvidence {
                    announcement: &b,
                    context_text: None,
                    samples: &[],
                    endorsements: &[],
                    samples_verified: false,
                },
                CandidatePodEvidence {
                    announcement: &c,
                    context_text: None,
                    samples: &[],
                    endorsements: &[],
                    samples_verified: false,
                },
                CandidatePodEvidence {
                    announcement: &d,
                    context_text: None,
                    samples: &[],
                    endorsements: &[],
                    samples_verified: false,
                },
            ],
            &policy,
            caps,
            10,
        );
        assert_eq!(ranked.len(), 2);
        assert!(ranked
            .iter()
            .all(|r| r.announcement.origin_node_id == node.id));
    }

    #[test]
    fn explicit_feed_feedback_kinds_adjust_future_exposure() {
        assert!(feedback_affects_future_exposure(
            crate::domain::FeedbackKind::Dismissed
        ));
        assert!(feedback_affects_future_exposure(
            crate::domain::FeedbackKind::Interesting
        ));
        assert!(feedback_affects_future_exposure(
            crate::domain::FeedbackKind::Saved
        ));
        assert!(feedback_affects_future_exposure(
            crate::domain::FeedbackKind::NotForMe
        ));
        assert!(feedback_affects_future_exposure(
            crate::domain::FeedbackKind::BlockSource
        ));
    }

    #[test]
    fn owned_candidate_evidence_borrows_cleanly() {
        let (_node, announcement) = announcement("systems", "systems");
        let owned = OwnedCandidateEvidence {
            announcement: announcement.clone(),
            context_text: Some("context".into()),
            samples: vec![],
            endorsements: vec![],
            samples_verified: false,
        };
        let evidence = owned.as_evidence();
        assert_eq!(evidence.announcement.pod_slug, "systems");
        assert_eq!(evidence.context_text, Some("context"));
    }
}
