//! Deterministic local Pod Similarity for passive and intentional discovery.
//!
//! Scores verified public metadata already on the Home Node against private
//! matching inputs held only in memory. Never issues interest-derived remote
//! queries. Endorsements strengthen evidence but are never required and never
//! become transferable trust or global reputation.

mod caps;
mod rank;
mod samples;
mod score;

pub use caps::{
    ExplorationCapTracker, ExplorationCaps, MAX_ORIGIN_EXPLORE_SAMPLES, MAX_RESULTS_PER_ORIGIN,
    MAX_TRIAL_ITEMS_PER_ORIGIN, TRIAL_SIMILARITY_THRESHOLD,
};
pub use rank::{
    announcement_scoring_eligible, collect_endorsements_for_announcement,
    collect_policy_endorsements, endorser_allowed, feedback_affects_future_exposure,
    filter_samples_by_policy, rank_similar_pods, OwnedCandidateEvidence, RankedSimilarPod,
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
