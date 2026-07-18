# Deliver Node Harness and Proposal workflows

Status: complete

Blocked by: 02

Let the Home Node Owner manage scoped Agent Harness access and let authorized actors review sensitive Pending Proposals through canonical Node workflows.

## Acceptance criteria

- [x] `node harness list`, `show`, `register`, and `revoke` are implemented through the shared machine contract.
- [x] Only the automatically authenticated Home Node Owner may register or revoke a Harness directly.
- [x] Registration activates the Harness Grant and returns its plaintext credential exactly once.
- [x] Later Harness reads expose only metadata, Pod scope, capabilities, status, and credential fingerprint.
- [x] `node proposal list`, `show`, `approve`, and `reject` enforce User identity, approval capability, Pod scope, expiry, and independent-actor rules.
- [x] Agent-initiated authority expansion returns a Pending Proposal rather than applying directly.
- [x] Generic proposal-document creation and standalone tenant or raw token management are absent.

## Comments

- 2026-07-18: Implementation started after issue 02 completed and the repository passed the issue 03 full validation checkpoint.
- 2026-07-18: Implemented the canonical Node Harness and Pending Proposal workflows at the real `stumble` executable seam. Added safe Harness metadata views with credential fingerprints, Owner-only direct bootstrap, scoped Harness authentication, explicit authority-expansion proposals, Owner and independent interactive-Harness decisions, and targeted operator documentation.
- 2026-07-18: Focused validation passed: `cargo test -p stumble-cli --test node_authority_workflows`, `cargo test -p stumble-core --test harness_grants --test pending_proposals`, `cargo test -p stumble-cli --test stumble_shell --test home_node_lifecycle`, and `cargo check -p stumble-cli`.
- 2026-07-18: Parent review added bounded opaque cursor pagination and state/authority-derived `allowed_actions` to the real Node list/show workflows, with executable regressions.
