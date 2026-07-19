Status: ready-for-agent
Blocked by: 05

# Schedule Personal Discovery with backpressure

## Parent

Personal Discovery from User evidence PRD.

## What to build

Support multiple named opt-in Personal Discovery schedules that materialize the same tasks whether a capable Agent Harness wakes itself or Stumble's local Scheduler Adapter performs the wake-up, while preventing unreviewed result accumulation.

## Acceptance criteria

- [ ] A User may create, inspect, update, disable, and remove multiple named private schedules.
- [ ] Each schedule defines cadence, optional temporary focus and avoidance, finite batch size, and notify-when-supported or queue-only delivery.
- [ ] A schedule remains dormant when Personal Discovery readiness is below the cold-start threshold.
- [ ] Due materialization is deterministic and idempotent for a schedule and period across retries, restart, concurrent wakeups, and scheduler changes.
- [ ] Harness-owned scheduling and the local Scheduler Adapter list and claim the same canonical ready tasks without controlling browser behavior.
- [ ] Each schedule defers while it owns an unreviewed Discovery Result Batch and resumes after that batch is reviewed or dismissed.
- [ ] Explicit on-demand Personal Discovery remains available while a schedule is backpressured.
- [ ] Successful scheduled completion emits one private Discovery-results-ready Event; delivery does not mark the batch reviewed.
- [ ] Notify-when-supported emits at most one notification attempt per completed batch, while queue-only retains the batch silently.
- [ ] Schedule configuration, events, tasks, and batch relationships persist across restart and never federate.
- [ ] Interactive management and unattended execution use separate authorization; a worker cannot change its schedule or delivery policy.
- [ ] Supported adapters and the shipped local wake-up integration expose equivalent task identities and inspectable backpressure state.

## Comments

Preserve the existing separation between Discovery Task semantics and replaceable scheduling mechanisms.
