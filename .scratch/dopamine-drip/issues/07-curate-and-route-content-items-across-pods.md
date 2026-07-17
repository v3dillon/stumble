# Curate and route Content Items across Pods

Status: complete

Blocked by: 05, 06

## Acceptance

- [x] Manual Curation requires authorized review for every proposed placement.
- [x] Assisted Curation may accept trusted high-confidence proposals and queues uncertainty.
- [x] Autonomous Curation requires an approved sensitive-change proposal before activation.
- [x] Each placement records its evidence, curation path, actor, and audit history.
- [x] A Routing Agent may propose additional placements only in Pods the node is authorized to curate.
- [x] One canonical Content Item can hold Accepted Placements in multiple Pods.
- [x] An authorized Add to Pod action immediately creates an Accepted Placement and optional curation note.
- [x] Rejections and reversals affect future local routing without leaking private feedback.

## Comments

- Implemented and verified at the primary `AgentTools` temporary-SQLite seam.
- Assisted trust is established by a valid leased Discovery Task and its pinned Pod Package version; harness confidence remains evidence rather than authority.
- Public placement reversal remains subject to the existing two-step sensitive-change policy.
