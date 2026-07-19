Status: ready-for-agent
Blocked by: 04, 06, 07, 08

# Prove and document the complete Agent Harness flow

## Parent

Personal Discovery from User evidence PRD.

## What to build

Close the feature with one persistent-node acceptance journey proving that URLs submitted by the User lead to autonomous source selection, privacy-minimized browser work, a finite reviewable result batch, explicit learning, changed future discovery, and scheduler-neutral operation without leaking private state.

## Acceptance criteria

- [ ] The primary MCP acceptance scenario uses distinct interactive and unattended Harness Grants against a persistent Home Node.
- [ ] The User submits several URLs, requests Personal Discovery without naming a source, and receives a plan derived from corroborated interests and Source Affinities.
- [ ] The worker can inspect only its minimized plan, submit provenance-bearing Candidates, report availability, and complete a ten-item Discovery Result Batch.
- [ ] The batch demonstrates the 70/30 allocation, diversity caps, canonical deduplication, local network Discovery Leads, and explainable shortfalls or reallocations.
- [ ] Explicit User result feedback changes a later plan while ignored and agent-found items create no learning by themselves.
- [ ] A scheduled run is shown to converge through both harness-owned wake-up and the local Scheduler Adapter, respect backpressure, and produce one results-ready notification state.
- [ ] Supported HTTP, MCP, and CLI representations and authorization failures are behaviorally equivalent for all new public operations.
- [ ] Migration from a pre-feature persistent store preserves existing Taste Profiles, Candidates, Pod Discovery Tasks, Pods, Subscriptions, and federation state.
- [ ] An adversarial serialization audit proves that Interest Seeds, Source Affinities, Discovery Plans, schedules, result batches, reactions, and profile-derived queries never cross federation or public discovery boundaries.
- [ ] The Stumble skill handles generic interest-based discovery, User-assisted login, scheduled fallback, result presentation, and explicit feedback without asking the User to name platforms.
- [ ] User and operator documentation explains setup, grants, schedules, privacy, source availability, result review, and recovery after restart.
- [ ] Formatting, warnings-as-errors linting, focused tests, the full workspace suite, Standards review, and Spec review all pass on the final combined change.

## Comments

CI must use deterministic browser-contract fixtures and local federation nodes rather than live third-party websites or credentials. A manual authenticated-X check may supplement but never replace automated acceptance.
