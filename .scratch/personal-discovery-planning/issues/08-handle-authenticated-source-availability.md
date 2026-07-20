Status: complete
Blocked by: 05, 07

# Handle authenticated-source availability safely

## Parent

Personal Discovery from User evidence PRD.

## What to build

Add a privacy-safe source-availability contract so Personal Discovery can use authenticated source neighborhoods when the User-approved Browser Connector is ready, degrade gracefully when it is not, and never move credentials or authentication control into Stumble.

## Acceptance criteria

- [x] A worker can report planned source availability and authentication-required state without submitting credentials, cookies, tokens, or raw browser state.
- [x] Browser Grant eligibility restricts planning and execution but is never broadened by Taste Profile evidence, a Pod Package, or a Discovery Lead.
- [x] An on-demand run may request User-assisted login for a valuable unavailable source while continuing accessible planned work.
- [x] A scheduled run never waits indefinitely or attempts authentication; it skips the source, reallocates within plan policy, and completes with an inspectable explanation.
- [x] Authentication-needed notification is emitted at most once per unavailable source state and becomes eligible again only after availability changes.
- [x] Failure or unavailability of one source cannot discard already valid task-bound results from other sources.
- [x] Source availability and reallocation remain retry-safe, lease-scoped, persisted across restart, and private.
- [x] Remote Pods, Index Nodes, public metadata, and worker-supplied content cannot authorize account mutations or authentication attempts.
- [x] Tests cover expired sessions, restored sessions, inaccessible sources, partial batches, scheduled reallocation, on-demand continuation, and notice suppression.
- [x] Operational documentation explains that the Agent Harness owns login and browser control and that scheduled runs skip unavailable authenticated sources.

## Comments

Follow the existing permitted-source and Browser Grant ADRs. Stumble stores availability facts, not authentication material.

Final implementation SHA: `de1e26210f51cc276b598374924d2c66a603c512`.
