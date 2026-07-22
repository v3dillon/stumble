//! Deterministic local Pod Similarity for passive and intentional discovery.
//!
//! Scores verified public metadata already on the Home Node against private
//! matching inputs held only in memory. Never issues interest-derived remote
//! queries. Endorsements strengthen evidence but are never required and never
//! become transferable trust or global reputation. Authorized local agent
//! evidence may adjust ordering under Core policy but never leaves the Home
//! Node and never creates trust, Subscription, placement, or Feed eligibility.

mod agent_evidence;
mod caps;
mod rank;
mod samples;
mod score;

pub use agent_evidence::{
    agent_evidence_alone_grants_eligibility, agent_evidence_harness_active,
    agent_evidence_idempotency_matches, agent_evidence_is_active, agent_evidence_is_fresh,
    announcement_ref, build_agent_evidence_record, collect_active_agent_evidence_for_candidate,
    find_bounded_agent_evidence_for_pair, find_idempotent_agent_evidence,
    layer_agent_similarity_evidence, resolve_agent_evidence_freshness_hours,
    validate_agent_evidence_request_shape, validate_agent_evidence_submission,
    validate_announcement_for_agent_evidence, AgentEvidenceError, AgentEvidencePodPair,
    DEFAULT_AGENT_EVIDENCE_FRESHNESS_HOURS, MAX_AGENT_EVIDENCE_BOOST,
    MAX_AGENT_EVIDENCE_EXPLANATION_CHARS, MAX_AGENT_EVIDENCE_FRESHNESS_HOURS,
    MAX_AGENT_EVIDENCE_IDEMPOTENCY_CHARS, MAX_AGENT_EVIDENCE_PROVENANCE_CHARS,
    MAX_AGENT_EVIDENCE_PUBLIC_INPUTS,
};
pub use caps::{
    ExplorationCapTracker, ExplorationCaps, MAX_ORIGIN_EXPLORE_SAMPLES, MAX_RESULTS_PER_ORIGIN,
    MAX_TRIAL_ITEMS_PER_ORIGIN, TRIAL_SIMILARITY_THRESHOLD,
};
pub use rank::{
    announcement_scoring_eligible, collect_endorsements_for_announcement,
    collect_policy_endorsements, endorser_allowed, feedback_affects_future_exposure,
    filter_samples_by_policy, rank_similar_pods, rank_similar_pods_with_agent_evidence,
    OwnedCandidateEvidence, RankedSimilarPod,
};
pub use samples::{
    fetch_verified_origin_explore_samples, sample_request_is_public_only,
    verify_explore_samples_for_announcement, CapturedSampleRequest, OriginExploreSampleClient,
    SampleFetchError, ScriptedOriginExploreSampleClient,
};
pub use score::{
    append_trial_exposure_label, score_exploration_item, score_pod_similarity,
    CandidatePodEvidence, LocalSimilarityContext, PodSimilarityScore, SimilarityEvidenceKind,
    SimilarityReason, TRIAL_EXPOSURE_REASON,
};
