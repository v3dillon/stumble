# Enrich Pod Similarity with authorized local agent evidence

Status: ready-for-agent
Blocked by: 04
Source: ../PRD.md

## What to build

Allow a local Node Agent or narrowly authorized Agent Harness to add richer semantic evidence to Pod Similarity while Core remains the sole authority over eligibility, provenance, Trust Policy, exploration caps, and durable User learning.

## Acceptance criteria

- [x] A scoped capability permits submitting bounded, confidence-scored, evidence-backed semantic relationships between exact current Pod Announcements.
  - Evidence: `HarnessCapability::PodSimilarityEvidence`; `AgentTools::submit_pod_similarity_agent_evidence`; domain `SubmitPodSimilarityAgentEvidenceRequest` / `PodSimilarityAgentEvidence`; unit + integration tests
- [x] Submissions identify the public inputs used and are rejected when announcements are stale, withdrawn, expired, blocked, mismatched, or unverifiable.
  - Evidence: `validate_agent_evidence_submission` / `validate_announcement_for_agent_evidence`; tests `rejects_expired_withdrawn_blocked_and_unverifiable`, `agent_evidence_rejects_stale_blocked_and_missing_capability`
- [x] Agent evidence can adjust local ordering and produce an inspectable explanation but cannot create trust, Subscription, Accepted Placement, or Feed eligibility by itself.
  - Evidence: `layer_agent_similarity_evidence` only when `base_score > 0`; Explore reasons `agent evidence:`; tests `agent_evidence_adjusts_score_with_inspectable_reason_but_not_zero_base`, `agent_evidence_enriches_ordering_with_inspectable_reason_under_core_authority`
- [x] Deterministic policy applies existing caps and blocks after agent evidence is considered.
  - Evidence: `rank_similar_pods_with_agent_evidence` blocks before score, caps after; test `agent_evidence_respects_blocks_and_caps_after_layering`
- [x] Agent evidence never leaves the Home Node as an Endorsement, global score, announcement field, or remote interest query.
  - Evidence: private store collection only; Explore DTO keeps Endorsements separate; no federation export; tests assert empty endorsements when only agent evidence applies
- [x] Revoking the Harness Grant immediately prevents new evidence and excludes evidence attributable only to that revoked grant from current ranking.
  - Evidence: `agent_evidence_harness_active`; test `revoking_harness_excludes_agent_evidence_from_ranking_and_blocks_new_submissions`
- [x] Duplicate submissions are idempotent and bounded by Pod pair, model or harness provenance, and freshness.
  - Evidence: `find_idempotent_agent_evidence` + `find_bounded_agent_evidence_for_pair`; integration idempotent replay in enrich test
- [x] Local semantic evidence and its audit provenance survive SQLite restart.
  - Evidence: `pod_similarity_agent_evidence` + `HarnessWriteOperation::SubmitPodSimilarityAgentEvidence` in store collections; test `agent_evidence_survives_sqlite_restart_and_baseline_without_evidence_matches`
- [x] With no agent evidence or active harness, deterministic Pod Similarity produces the same externally observable baseline behavior as before.
  - Evidence: `rank_similar_pods` empty map path; tests `without_agent_evidence_score_unchanged`, baseline half of restart test, ticket 04 suite still green

## Comments

- Implemented in `crates/stumble-core/src/pod_similarity/agent_evidence.rs` with thin ranking layer (`rank_similar_pods_with_agent_evidence`), store persistence, and `submit_pod_similarity_agent_evidence` on `AgentTools`.
- Docs: `docs/discovery.md` (Local agent semantic evidence).
