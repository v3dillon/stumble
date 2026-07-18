# Derive and operate Discovery Tasks

Status: complete

Blocked by: 09

Make authoritative Source Rules the only source of scheduled discovery work and expose the complete lease-based Agent Harness workflow.

## Acceptance criteria

- [x] Due Discovery Tasks materialize automatically from current Source Rules.
- [x] No public command manually creates or materializes scheduled tasks.
- [x] `discover task list` combines all and ready views through state filters and cursor pagination.
- [x] `show`, `claim`, `renew`, `complete`, and `fail` enforce valid state transitions and Harness scope.
- [x] Claims and renewals preserve lease ownership and reject stale or competing actors deterministically.
- [x] Repeated scheduler or listing execution does not duplicate task instances.
- [x] Scheduler Adapter tests use the canonical task surface without browser control.

## Comments

- 2026-07-18: Implementation started after issue 09 completed and passed focused plus full workspace validation.
- 2026-07-18: Implemented the canonical executable Discovery Task workflow with automatic Source Rule materialization, state and Pod filters, opaque cursor pagination, scoped lease transitions, stable task errors, and state-aware allowed actions.
- 2026-07-18: Migrated the local scheduler and launchd adapter to `stumble discover task list --state ready`; focused executable, scheduler, core lease, and full `stumble-cli` package tests pass.
- 2026-07-18: Self-review found and added explicit foreign and stale renewal coverage, bounded the scheduler's canonical ready page at the public maximum, and confirmed manual task creation/materialization remain ordinary usage errors.
