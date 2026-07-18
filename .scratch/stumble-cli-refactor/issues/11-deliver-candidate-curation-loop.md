# Deliver the complete Candidate curation loop

Status: complete

Blocked by: 08, 10

Expose provenance-bearing Candidate submission and independent Pod Placement evaluation, routing, and review as one complete discovery-to-curation workflow.

## Acceptance criteria

- [x] `discover candidate list` supports status filters and cursor pagination.
- [x] `submit` accepts structured stdin or file input and requires an idempotency key.
- [x] Identical submission retries return the original result; changed input under the same key is rejected.
- [x] `show` returns provenance, placement evidence, state, and allowed actions.
- [x] `evaluate` applies each target Pod's current Curation Policy independently.
- [x] `route` records evidence-backed Routing Agent placement proposals only within authorized local Pod scope.
- [x] `review` accepts or rejects one pending Pod Placement without changing the Candidate's other placements.
- [x] Accepted placements preserve one canonical Content Item identity across Pods.

## Comments

- 2026-07-18: Implementation started after issues 08 and 10 completed and the issue 10 checkpoint passed full workspace validation.
- 2026-07-18: Delivered the Candidate executable workflows with a single required CLI idempotency key, scoped status pagination, detailed placement inspection and permission-derived actions, independent policy evaluation, evidence-backed routing, isolated review, and canonical Content Item identity across accepted Pods. Real-process coverage exercises file and stdin submission, retry conflicts, split submitter/curator grants, and out-of-scope routing; focused Candidate, shell, and adapter suites pass.
