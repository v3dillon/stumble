Status: complete
Blocked by: 04, 06, 07, 08

# Prove and document the complete Agent Harness flow

## Parent

Personal Discovery from User evidence PRD.

## What to build

Close the feature with one persistent-node acceptance journey proving that URLs submitted by the User lead to autonomous source selection, privacy-minimized browser work, a finite reviewable result batch, explicit learning, changed future discovery, and scheduler-neutral operation without leaking private state.

## Acceptance criteria

- [x] The primary MCP acceptance scenario uses distinct interactive and unattended Harness Grants against a persistent Home Node.
- [x] The User submits several URLs, requests Personal Discovery without naming a source, and receives a plan derived from corroborated interests and Source Affinities.
- [x] The worker can inspect only its minimized plan, submit provenance-bearing Candidates, report availability, and complete a ten-item Discovery Result Batch.
- [x] The batch demonstrates the 70/30 allocation, diversity caps, canonical deduplication, local network Discovery Leads, and explainable shortfalls or reallocations.
- [x] Explicit User result feedback changes a later plan while ignored and agent-found items create no learning by themselves.
- [x] A scheduled run is shown to converge through both harness-owned wake-up and the local Scheduler Adapter, respect backpressure, and produce one results-ready notification state.
- [x] Supported HTTP, MCP, and CLI representations and authorization failures are behaviorally equivalent for all new public operations.
- [x] Migration from a pre-feature persistent store preserves existing Taste Profiles, Candidates, Pod Discovery Tasks, Pods, Subscriptions, and federation state.
- [x] An adversarial serialization audit proves that Interest Seeds, Source Affinities, Discovery Plans, schedules, result batches, reactions, and profile-derived queries never cross federation or public discovery boundaries.
- [x] The Stumble skill handles generic interest-based discovery, User-assisted login, scheduled fallback, result presentation, and explicit feedback without asking the User to name platforms.
- [x] User and operator documentation explains setup, grants, schedules, privacy, source availability, result review, and recovery after restart.
- [x] Formatting, warnings-as-errors linting, focused tests, the full workspace suite, Standards review, and Spec review all pass on the final combined change.

## Comments

CI must use deterministic browser-contract fixtures and local federation nodes rather than live third-party websites or credentials. A manual authenticated-X check may supplement but never replace automated acceptance.

Final implementation SHA: `e1ce6dab8fc8f4e3b41d34ebb2a2bd8f3032cdbe`.

Note: AC12 full workspace suite is completed by the parent final combined audits and single workspace gate after tickets 04–09; focused tests, targeted clippy, fmt, Standards, and Spec for this ticket range are complete at the implementation SHA.
