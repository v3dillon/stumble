Status: ready-for-agent

# Federate Agent Harness discoveries between Stumble nodes

## Problem Statement

A User can ask an Agent Harness to browse an authenticated source such as X and submit provenance-bearing Candidates, but the current MCP interface stops before those Candidates can be curated into a public Pod and synchronized to a second Home Node. Image references are also represented as separate Candidates, so a post and its attached media do not remain one Content Item.

## Solution

Expose the existing deterministic curation, approval, publication, subscription, synchronization, and Pod-content behavior through capability-scoped MCP tools. Extend Candidate Submissions and synchronized Content References with permitted attached media references. Prove the complete flow with two independently authenticated Stumble nodes: the Origin accepts discovered posts into an isolated public test Pod, and a second Home Node subscribes outbound, verifies signed Pod Events, and reads the synchronized links with their attached images.

## User Stories

1. As a User, I want my interactive Agent Harness to create an isolated Pod for discoveries, so that my private Inbox remains private.
2. As a Pod Curator, I want public Pod creation to remain a Pending Proposal, so that publication cannot bypass independent approval.
3. As an Approver, I want to inspect and approve public exposure through MCP, so that the workflow remains harness-neutral.
4. As a Pod Curator, I want to route a pending Candidate to an authorized local Pod, so that discoveries submitted for Inbox can be selected for publication without duplication.
5. As a Pod Curator, I want to accept or reject a pending Pod Placement through MCP, so that only Accepted Placements federate.
6. As a User, I want to list accepted Pod content through MCP, so that I can verify publication without querying SQLite.
7. As a User on a second Home Node, I want to subscribe to a canonical public Pod URL, so that signed content synchronizes locally through outbound access.
8. As a subscriber, I want to refresh an existing Subscription from its Origin Node, so that later accepted content and tombstones arrive incrementally.
9. As a subscriber, I want synchronization results to report imported-event counts and the verified cursor, so that an Agent Harness can explain what changed.
10. As a node owner, I want subscription tools gated by Subscription Management, so that discovery-only Harnesses cannot change Subscriptions.
11. As a node owner, I want curation tools gated by Pod Curation and Pod scope, so that a Harness cannot modify unrelated Pods.
12. As a node owner, I want approval tools gated by Approval and independent-approver rules, so that a proposer cannot approve its own sensitive change.
13. As a User, I want a discovered post and its permitted image references represented by one Content Item, so that the media remains attached during synchronization.
14. As a subscriber, I want attached media references to survive signed Pod Event export and import, so that the remote Content Reference matches the Origin.
15. As a User, I want media references to remain reference-first, so that Stumble does not imply that third-party image bytes were archived.
16. As an Agent Harness author, I want MCP tool discovery to advertise only operations authorized by the active Harness Grant, so that unavailable tools are not presented.
17. As an operator, I want two separately configured MCP adapters for Origin and subscriber nodes, so that credentials and authority remain node-local.
18. As a developer, I want a two-real-node acceptance test, so that Candidate gating, approval, signed federation, attached media, and subscriber reads are verified through public interfaces.

## Implementation Decisions

- Keep authority, URL normalization, Candidate state, Pod Placement acceptance, signatures, and synchronization in the deterministic core.
- Keep Inbox private. Use a distinct public test Pod for the federation workflow.
- Reuse Pending Proposals for public Pod creation or visibility expansion; do not create an MCP-only shortcut.
- Expose capability-filtered MCP interfaces for Pod creation, proposal inspection/approval, Candidate routing/review, and accepted Pod-content reads.
- Add asynchronous MCP interfaces for direct public-Pod subscription and incremental Origin synchronization, delegating to the existing synchronization module.
- Address a node through its own MCP adapter and Harness Grant rather than adding a caller-selected node identifier to one adapter.
- Model attached media as typed, permitted URL references on Candidate Submission evidence and Content References. Do not store binary media in this release.
- Preserve attached media in accepted-placement projections and signed Pod Events.
- Keep existing submissions compatible by defaulting missing media-reference arrays to empty.
- Prefer batch-shaped results only where the core can preserve atomicity; do not hide sequential partial writes behind a falsely atomic interface.

## Testing Decisions

- Test MCP behavior through tool discovery and tool calls with authenticated Harness tokens.
- Test authorization by verifying that tools are absent without their capability and that direct calls remain denied by the core.
- Test public exposure through the existing Pending Proposal and independent Approval behavior.
- Test Candidate routing/review through the MCP interface and observe accepted content through Pod-content reads.
- Test direct subscription through a real ephemeral Origin HTTP listener and a separate SQLite-backed Home Node.
- Test attached media through Candidate submission, acceptance, signed export/import, and subscriber Content Reference reads.
- Add one complete two-node acceptance test that never reads SQLite directly.
- Run focused adapter tests during each red-green slice and the full workspace suite after integration.

## Out of Scope

- Downloading or archiving third-party image or video bytes.
- Making Inbox public.
- Sharing Harness tokens, private Candidates, Discovery Tasks, Taste Profiles, Feedback Signals, or Feed history.
- Adding an X-specific connector to Stumble.
- Replacing the required independent approval for public exposure.
- Public deployment or stable HTTPS setup beyond the existing operator contract.

## Further Notes

The core direct-subscription workflow already has test coverage. The primary missing behavior is a complete capability-scoped MCP interface plus attached media in the reference-first contract.
