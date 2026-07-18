# Expose Subscription and Pod Role workflows

Status: complete

Blocked by: 04, 05

Expose the separated Subscription and Pod Role relationships as complete User and Owner workflows.

## Acceptance criteria

- [x] `pod subscribe` accepts a local Pod reference or canonical public Pod URL as appropriate.
- [x] `pod unsubscribe` removes Feed eligibility without changing Pod authority.
- [x] `pod subscription set` updates Priority Subscription without changing governance.
- [x] `pod role list`, `grant`, and `revoke` expose only Owner and Curator roles.
- [x] Role changes use Pending Proposals and preserve independent approval.
- [x] Pod results include the resolved Pod ID and slug, and detailed results include allowed actions.
- [x] Authorization tests prove subscribers, Curators, Owners, and scoped Harnesses receive only their intended actions.

## Comments

- 2026-07-18: Implementation started after issues 04 and 05 completed and passed full workspace validation.
- 2026-07-18: Implemented local slug/ID and canonical public URL Subscription entry points, reversible Feed eligibility and Priority updates, and scoped allowed actions without coupling Subscription to Pod authority.
- 2026-07-18: Added Owner/Curator-only role listing and independently approved grant/revoke proposals, including last-Owner protection and Pod-scoped approval checks.
- 2026-07-18: Focused validation passed for the real executable relationship suite (5 tests) and core Subscription/Pod Role authorization suite (5 tests); review and package-level validation completed before commit.
- 2026-07-18: Parent review corrected Pod list/show results to include the canonical `pod_id` alongside `slug`, with executable regression coverage.
- 2026-07-18: Full validation updated the original pagination shell test to cover both a successful empty page and deterministic rejection of a malformed opaque cursor.
