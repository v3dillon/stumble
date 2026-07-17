# Deliver a finite local Feed

Status: complete

Blocked by: 01, 02, 07

Implement the `Deliver a finite local Feed` ticket in `tickets.md` through the
temporary-SQLite `AgentTools` seam and equivalent HTTP, MCP, and CLI adapters.

## Acceptance

- [x] `get_feed_batch` returns a stable finite configurable batch and Caught Up.
- [x] Retrieval marks items Delivered and repeated reads reuse the current batch.
- [x] A validated configurable recurrence penalty permits strong early resurfacing.
- [x] Items expose references, scoped placements and provenance, ranking evidence,
  exploration and feedback state, and permission-derived allowed actions.
- [x] The complete initial Feedback Signal vocabulary changes subsequent behavior.
- [x] No dwell-time or session-duration ranking objective is stored.
- [x] HTTP, MCP, and CLI contracts are equivalent.

## Comments

- 2026-07-17: Implementation started with strict red-green TDD.
- 2026-07-17: Finite delivery, recurrence, feedback, and adapter parity completed.
