Status: complete
Blocked by: 05

# Schedule Personal Discovery with backpressure

## Parent

Personal Discovery from User evidence PRD.

## What to build

Support multiple named opt-in Personal Discovery schedules that materialize the same tasks whether a capable Agent Harness wakes itself or Stumble's local Scheduler Adapter performs the wake-up, while preventing unreviewed result accumulation.

## Acceptance criteria

- [x] A User may create, inspect, update, disable, and remove multiple named private schedules.
- [x] Each schedule defines cadence, optional temporary focus and avoidance, finite batch size, and notify-when-supported or queue-only delivery.
- [x] A schedule remains dormant when Personal Discovery readiness is below the cold-start threshold.
- [x] Due materialization is deterministic and idempotent for a schedule and period across retries, restart, concurrent wakeups, and scheduler changes.
- [x] Harness-owned scheduling and the local Scheduler Adapter list and claim the same canonical ready tasks without controlling browser behavior.
- [x] Each schedule defers while it owns an unreviewed Discovery Result Batch and resumes after that batch is reviewed or dismissed.
- [x] Explicit on-demand Personal Discovery remains available while a schedule is backpressured.
- [x] Successful scheduled completion emits one private Discovery-results-ready Event; delivery does not mark the batch reviewed.
- [x] Notify-when-supported emits at most one notification attempt per completed batch, while queue-only retains the batch silently.
- [x] Schedule configuration, events, tasks, and batch relationships persist across restart and never federate.
- [x] Interactive management and unattended execution use separate authorization; a worker cannot change its schedule or delivery policy.
- [x] Supported adapters and the shipped local wake-up integration expose equivalent task identities and inspectable backpressure state.

## Comments

Preserve the existing separation between Discovery Task semantics and replaceable scheduling mechanisms.

Final implementation SHA: `a9c411e3db938fab03556e1e3fade5623866574c`.
