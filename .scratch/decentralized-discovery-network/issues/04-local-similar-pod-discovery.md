# Discover and preview similar public Pods locally

Status: ready-for-agent
Blocked by: 03
Source: ../PRD.md

## What to build

Turn synchronized public announcements into useful local discovery. A Home Node should retrieve bounded Origin-signed Explore samples, calculate inspectable deterministic Pod Similarity, and give relevant endorsed or unendorsed public Pods tightly bounded exposure under the existing Trust Policy, Explore, and Feed rules.

## Acceptance criteria

- [x] The Home Node retrieves bounded Explore samples directly from the canonical Origin and accepts them only when their signature and current announcement binding verify.
  - Evidence: `OriginExploreSampleClient` + `fetch_origin_explore_samples` / `verify_explore_samples_for_announcement`; tests `home_node_fetches_bounded_origin_samples_only_when_signature_and_binding_verify`, `verified_samples_require_signature_and_binding`
- [x] Deterministic Pod Similarity uses verified public subject and Pod Context text, source neighborhoods, Explore samples, and valid Pod Endorsements.
  - Evidence: `score_pod_similarity` / `rank_similar_pods` in `pod_similarity/`; tests `subject_match_is_deterministic_and_inspectable`, `sample_and_source_evidence_raise_score_with_reasons`, `deterministic_similarity_exposes_subject_source_sample_and_endorsement_reasons`
- [x] Similarity is calculated locally from synchronized metadata and private evidence without issuing background interest-derived remote queries.
  - Evidence: `explore_public_pods` scores only local store state; test `explore_similarity_is_local_without_remote_interest_queries` (scripted client captures zero fetches)
- [x] Explore results and Exploration Items provide inspectable reasons identifying subject, source, sample, or endorsement evidence.
  - Evidence: `SimilarityReason::display` prefixes; Explore `reasons` + Feed exploration reason attachment via `exploration_similarity_for_item`
- [x] Pod Endorsements strengthen evidence but are neither mandatory nor treated as transferable trust or global reputation.
  - Evidence: endorsement boost only when base_score > 0; reason text "not transferable trust"; tests `endorsements_strengthen_but_are_not_required_for_trial`, `endorsement_alone_does_not_surface_unrelated_pod`
- [x] A strongly relevant unendorsed Pod can receive limited labeled trial exposure after identity, reachability, manifest, announcement, and sample verification.
  - Evidence: `trial_exposure` when `samples_verified` + base score ≥ threshold + zero endorsements; test `unendorsed_pod_receives_limited_labeled_trial_exposure_after_verification`
- [x] Per-Origin, per-Pod, per-source, and existing Feed Mix exploration caps prevent open-admission flooding.
  - Evidence: `ExplorationCaps` / `ExplorationCapTracker`; Explore `MAX_RESULTS_PER_ORIGIN`; Feed `apply_exploration_origin_caps` + Feed Mix caps; tests `per_origin_caps_limit_results`, `per_origin_caps_bound_explore_results_from_one_origin`
- [x] Local Pod, Origin, source, and topic blocks exclude matching Pods and samples before ranking or delivery.
  - Evidence: Trust Policy applied before `rank_similar_pods`; sample filter; tests `blocks_exclude_before_ranking`, `local_blocks_exclude_before_similarity_ranking`, existing policy-filtered sample tests
- [x] Explicit Feedback Signals affect future local exposure while ignores and passive delivery do not create durable preference by themselves.
  - Evidence: private taste learning remains the durable path; `feedback_affects_future_exposure`; tests `ignore_and_passive_do_not_count_as_durable_preference`, existing feed feedback tests
- [x] Deterministic discovery remains functional with no active Agent Harness or model service.
  - Evidence: pure `score_pod_similarity` unit test `works_without_agent_harness_or_model_service`

## Comments

- Implemented in `crates/stumble-core/src/pod_similarity/` (`mod.rs`, `score.rs`, `samples.rs`, `caps.rs`, `rank.rs`) with thin integration in `explore_public_pods`, `fetch_origin_explore_samples`, and Feed exploration selection.
- Docs: `docs/discovery.md` (Local Pod Similarity and trial exposure).
- Thermo-nuclear remediation: honest Feed similarity (no synthetic announcements / real samples_verified), typed `trial_exposure` on `RankedFeedCandidate`, endorsement only boosts when base_score > 0, shared OwnedCandidateEvidence + endorsement/local-context builders, trial labeled only at DTO boundary.
