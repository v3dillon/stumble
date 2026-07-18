# Deliver synchronization workflows

Status: complete

Blocked by: 03, 06, 07

Expose high-level peer trust and Pod synchronization recovery without leaking signed-event file mechanics into the public parser.

## Acceptance criteria

- [x] `sync peer list`, `add`, and `remove` use canonical Node identities and Trust Policy behavior.
- [x] Peer trust additions and removals return Pending Proposals where required.
- [x] `sync pod run` performs real high-level synchronization for the selected Subscription and peer.
- [x] `sync pod status` reports cursor, verification, latest event, last success, and actionable failure state.
- [x] Normal Subscription synchronization remains automatic without manual runs.
- [x] Event-file export, import, and verification are absent from the public parser.
- [x] Signature, protocol negotiation, tombstone, and outbound-only Home Node tests continue to pass.

## Comments

- 2026-07-18: Implementation started after issues 03, 06, and 07 completed and the issue 12 checkpoint passed full workspace validation.
- 2026-07-18: Added canonical Node identity peer proposals, selected-peer Subscription recovery, persisted actionable synchronization status, and executable coverage for approval, pagination, network synchronization, failure recovery, removal, and retired event-file commands. Focused CLI, direct Subscription, protocol, signature, Pending Proposal, tombstone, discovery substrate, and first-release tests pass.
