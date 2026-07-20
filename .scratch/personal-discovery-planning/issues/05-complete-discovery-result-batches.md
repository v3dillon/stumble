Status: complete
Blocked by: 03

# Complete a private Discovery Result Batch

## Parent

Personal Discovery from User evidence PRD.

## What to build

Allow a scoped worker to submit only the finite, provenance-bearing shortlist for its claimed Personal Discovery Task and atomically complete that work into a private Discovery Result Batch tied to the immutable plan.

## Acceptance criteria

- [x] Only the worker holding the current Personal Discovery Task lease may submit task-bound result Candidates or complete its batch.
- [x] Every result retains canonical URL identity, permitted metadata and media references, discovery method, referrer, source facts, task identity, and plan identity.
- [x] Worker submissions are always agent-discovered and create no Interest Seed or other learning evidence by themselves.
- [x] Batch completion enforces requested size, 70/30 allocation, domain and source-identity caps, blocks, canonical deduplication, and recent-result suppression.
- [x] An underfilled or reallocated batch records inspectable source availability and quota reasons instead of inventing results or weakening policy silently.
- [x] Completion is atomic and retry-safe: one task produces at most one ordered batch, and duplicate submissions cannot inflate it.
- [x] Ready, reviewed, and dismissed batch states are distinct from task completion and notification state.
- [x] A whole-batch dismissal creates no item-level positive or negative learning evidence.
- [x] Batches and their Candidate provenance persist across restart and remain private and non-federated.
- [x] Supported adapters expose equivalent result submission, completion, listing, inspection, dismissal, lease errors, and structured batch representations.

## Comments

The worker may browse broadly outside Stumble, but Stumble receives only the finite shortlist selected under the plan.

Final implementation SHA: `051174f354cf14c971f7873c8f7c6eb54cacfa4b`.
