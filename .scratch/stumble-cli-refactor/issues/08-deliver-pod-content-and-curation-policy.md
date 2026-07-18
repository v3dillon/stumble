# Deliver Pod content and Curation Policy

Status: complete

Blocked by: 06, 07

Expose complete accepted Pod streams, direct placement actions, and Pod-owned curation autonomy through canonical workflows.

## Acceptance criteria

- [x] `pod content list` returns the complete accepted stream with cursor pagination independently of Feed selection.
- [x] `pod content show` returns canonical Content Item and Accepted Placement evidence with allowed actions.
- [x] `pod content add` preserves provenance and immediately creates an Accepted Placement under existing authority.
- [x] Private `pod content remove` applies directly without deleting the Content Item or unrelated placements.
- [x] Public removal returns a Pending Proposal and emits a Placement Tombstone only after approval.
- [x] `pod policy show` exposes the canonical Manual, Assisted, or Autonomous Curation Policy.
- [x] `pod policy set` applies Manual and Assisted changes normally while Autonomous enablement requires approval.

## Comments

- 2026-07-18: Implementation started after issues 06 and 07 completed and passed full workspace validation.
- 2026-07-18: Completed the real `stumble` content and policy workflows with uniform Pod references, cursor pagination, placement evidence, provenance-preserving Add to Pod, visibility-aware removal, and approval-gated Autonomous Curation. Focused validation passed: `cargo test -p stumble-cli --test pod_content_policy_workflows` (3 tests) and `cargo test -p stumble-core --test content_curation --test placement_tombstones` (11 tests).
