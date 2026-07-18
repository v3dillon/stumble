# Deliver Pod lifecycle and Explore

Status: complete

Blocked by: 04, 05

Provide canonical Pod discovery, inspection, creation, visibility, and Explore workflows without encoding privacy in package-specific commands.

## Acceptance criteria

- [x] `pod list` and `pod show` use uniform Pod references, pagination, envelopes, and allowed actions.
- [x] `pod create` requires explicit private, invite-only, or public visibility.
- [x] Creation accepts either an optional initial package directory or mutually exclusive package derivation from another Pod.
- [x] Package-derived creation preserves source-package provenance.
- [x] Public creation returns a Pending Proposal and never exposes a partially initialized Pod.
- [x] `pod visibility set` requires approval when exposure expands and permits authorized restrictions directly.
- [x] `pod explore` returns Trust Policy-filtered public Pods and sample Content Items without creating a Subscription.

## Comments

- 2026-07-18: Implementation started after issues 04 and 05 completed and the issue 06 checkpoint passed full workspace validation.
- 2026-07-18: Delivered the real `stumble` Pod lifecycle and Explore workflows. Creation now uses one atomic core request for default, directory-backed, or provenance-preserving derived packages; public creation and visibility expansion produce Pending Proposals, while authorized restrictions apply directly. Added executable coverage plus core atomicity/provenance coverage and retained the existing Trust Policy-filtered, unsubscribed Explore test.
- 2026-07-18: Focused validation passed: `cargo test -p stumble-cli --test pod_lifecycle_workflows` (4 tests), `cargo test -p stumble-core --test pod_packages` (4 tests), `cargo test -p stumble-core --test pending_proposals` (4 tests), and the focused `discovery_substrate` Trust Policy Explore test (1 test).
- 2026-07-18: Parent review removed `allowed_actions` from Pod list items and added canonical `pod_id` to successful creation results, matching the shared machine contract.
- 2026-07-18: Cross-issue focused validation updated the Owner allowed-actions regression to include the new `visibility_set` lifecycle action.
