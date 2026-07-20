Status: complete
Blocked by: 01, 02

# Request an on-demand Personal Discovery Plan

## Parent

Personal Discovery from User evidence PRD.

## What to build

Let an interactive User request Personal Discovery without selecting a Pod or naming websites. The Home Node must evaluate readiness, create an immutable minimized Discovery Plan, and materialize a User-scoped task that an independently authorized worker can claim without reading the complete Taste Profile.

## Acceptance criteria

- [x] Personal Discovery creates a first-class User-scoped Discovery Task and never creates or requires a Pod.
- [x] A generic request requires an explicit interest or two corroborating User actions; a specific link or topic may supply temporary intent for that run.
- [x] The task pins an immutable Discovery Plan identity and remains retry-safe across transport retries and restart.
- [x] The default plan requests ten results with a 70% proven-neighborhood and 30% adjacent-exploration allocation.
- [x] The plan applies a maximum of three results per domain and two per author, account, publisher, or community.
- [x] Explicit source and topic blocks, recent-result suppression, canonical deduplication, and requested finite size are represented as enforceable plan constraints.
- [x] The plan explains selected topics and source neighborhoods without revealing raw Feedback history, raw Interest Seeds, unrelated private URLs, the complete Taste Profile, or credentials.
- [x] An interactive management grant may request and inspect Personal Discovery while an unattended worker grant may only list, claim, and read plans for its assigned tasks.
- [x] Pod Discovery Tasks and their Package-governed authorization remain unchanged.
- [x] Supported adapters expose equivalent readiness, request, plan inspection, task lifecycle, and authorization behavior.

## Comments

Follow ADR-0036. Keep Personal Discovery and Pod discovery as separate target semantics behind the shared task lifecycle.

Implemented in `669a8fbffa1bdaa5106ca6a686c35b72580841a3`.
