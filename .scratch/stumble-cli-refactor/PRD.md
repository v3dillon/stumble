# Stumble CLI workflow refactor

Status: complete

## Problem Statement

Stumble's current `podctl` executable presents a flat collection of operations that mixes canonical User and Agent Harness workflows with transport launchers, hosted-development utilities, low-level protocol diagnostics, duplicate commands, and obsolete compatibility behavior. Command names do not consistently use Stumble's domain language, some operations expose persistence or transport mechanics instead of User intent, and stdout, stderr, identifiers, inputs, pagination, authorization, and retry behavior are inconsistent. The executable also silently creates Home Node state, advertises a nonfunctional remote mode, lets a shell caller omit its Harness credential to inherit owner authority, and conflates Subscription with Pod governance.

## Solution

Replace `podctl` atomically with a local-only `stumble` executable organized into five predictable workflow families: `node`, `pod`, `discover`, `feed`, and `sync`. Expose only canonical User and Agent Harness workflows, use short resource-first command paths, provide a versioned JSON-first machine contract, make initialization and authorization explicit, automatically handle the local Home Node Owner Credential through the operating-system keychain, and preserve scoped Harness authorization for adapters. Separate Subscription from Pod Roles, make sensitive operations return Pending Proposals, move all long-running HTTP and MCP behavior to dedicated executables, and remove obsolete, duplicate, development-only, and protocol-level commands without aliases.

The accepted public command tree is:

```text
stumble
├── node
│   ├── init
│   ├── show
│   ├── harness  list | show | register | revoke
│   └── proposal list | show | approve | reject
├── pod
│   ├── list | show | create | explore | subscribe | unsubscribe
│   ├── subscription set
│   ├── visibility   set
│   ├── role         list | grant | revoke
│   ├── content      list | show | add | remove
│   ├── policy       show | set
│   └── package      show | export | validate | revise
├── discover
│   ├── task      list | show | claim | renew | complete | fail
│   └── candidate list | submit | show | evaluate | route | review
├── feed
│   ├── batch    get | complete
│   ├── feedback record
│   └── taste    show | set | reset
└── sync
    ├── peer list | add | remove
    └── pod  run | status
```

## User Stories

1. As a User, I want one `stumble` executable, so that the CLI name matches the complete product rather than only Pods.
2. As a User, I want commands grouped by workflow family, so that I can predict where an operation belongs.
3. As an Agent Harness, I want resource-first command paths, so that I can infer command grammar without memorizing flat compound names.
4. As a User, I want short command words without hyphenated command tokens, so that common operations remain easy to type.
5. As a maintainer, I want the old executable and names removed atomically, so that the repository has one canonical contract.
6. As a User, I want removed commands to fail as ordinary usage errors, so that hidden aliases do not prolong obsolete behavior.
7. As a User, I want the CLI to default to my standard Home Node location, so that it does not depend on my current directory.
8. As a User, I want to select another Home Node through an environment variable or flag, so that local multi-node workflows remain possible.
9. As a User, I want only `node init` to create state, so that a path typo cannot silently create a second Home Node.
10. As a User, I want initialization to fail when a Home Node already exists, so that initialization cannot overwrite or ambiguously reuse state.
11. As a User, I want uninitialized paths reported with a stable error code, so that recovery guidance is deterministic.
12. As a User, I want initialization to store my Owner credential in the operating-system keychain, so that routine local commands authenticate automatically.
13. As a User, I want Node results to identify the resolved data directory, so that I can verify which Home Node a command used.
14. As a Home Node Owner, I want to register a scoped Agent Harness directly, so that I can bootstrap trusted integrations without an inactive-credential handshake.
15. As a Home Node Owner, I want registration to reveal a Harness credential once, so that long-lived secrets are not retrievable later.
16. As a Home Node Owner, I want Harness reads to show only credential fingerprints and metadata, so that inspection cannot leak secrets.
17. As a Home Node Owner, I want to revoke a Harness and its credential immediately, so that compromised or obsolete integrations lose access.
18. As an Agent Harness, I want my credential checked on every adapter operation, so that my Harness Grant cannot be bypassed.
19. As a User, I want unrestricted same-user shell access treated explicitly as owner authority, so that the local security boundary is understandable.
20. As a User, I want sensitive changes represented as Pending Proposals, so that publication, autonomy, trust, authority, and public removal remain reviewable.
21. As a Home Node Owner, I want to list, inspect, approve, and reject Pending Proposals, so that I can manage the approval inbox without constructing internal change documents.
22. As an interactive Agent Harness, I want proposal actions constrained by my approval capability and Pod scope, so that approval remains independently authorized.
23. As an unattended Agent Harness, I want to be prevented from approving my own request, so that two-step protection cannot be self-authorized.
24. As an Agent Harness, I want every successful CLI invocation to return one versioned JSON document, so that output parsing is uniform.
25. As an Agent Harness, I want failures returned as versioned JSON on stderr, so that I never parse human prose.
26. As a User, I want an explicit text format, so that direct terminal use can remain readable without weakening the JSON contract.
27. As an Agent Harness, I want stable domain error codes and broad process exit categories, so that recovery behavior is portable.
28. As an Agent Harness, I want structured requests accepted from a file or stdin, so that complex payloads do not require fragile flag choreography.
29. As a User, I want portable Pod Packages to remain directory-based, so that their documented files remain inspectable and transferable.
30. As an Agent Harness, I want every collection paginated with an opaque cursor, so that responses remain bounded and resumable.
31. As an Agent Harness, I want detailed resources to include allowed next actions, so that I do not guess permissions or state transitions.
32. As an Agent Harness, I want creation and submission retries to be idempotent, so that a lost response cannot create duplicate state.
33. As an Agent Harness, I want idempotency-key reuse with changed input rejected, so that accidental key collisions are visible.
34. As a User, I want Pod commands to accept a slug or immutable Pod ID consistently, so that interactive and automated callers use one locator model.
35. As an Agent Harness, I want Pod results to include both ID and slug, so that I can retain immutable identity while presenting readable names.
36. As a Pod Owner, I want one Pod creation command with explicit visibility, so that package presence does not silently determine privacy.
37. As a Pod Owner, I want to create a Pod with an initial package atomically, so that partially initialized Pods are not exposed.
38. As a Pod Owner, I want to derive a new Pod from another Pod's package with preserved provenance, so that forking is a creation workflow rather than an overwrite.
39. As a Pod Owner, I want visibility changes expressed explicitly, so that publication and restriction are auditable lifecycle transitions.
40. As a User, I want Subscription separate from Pod authority, so that subscribing affects my Feed without granting curation rights.
41. As a User, I want to subscribe and unsubscribe symmetrically, so that Feed eligibility is reversible.
42. As a User, I want Priority Subscription to be a Subscription property, so that representation preferences do not alter Pod governance.
43. As a Pod Owner, I want only Owner and Curator Pod Roles, so that authority is not confused with social membership or generic administration.
44. As a Pod Owner, I want to list, grant, and revoke Pod Roles, so that governance is explicit and auditable.
45. As a User, I want to Explore public Pods under my Trust Policy, so that I can discover subscriptions without knowing URLs in advance.
46. As a User, I want Explore to show sample Content Items without subscribing, so that discovery has no hidden side effect.
47. As a User, I want to list a Pod's complete accepted stream, so that items omitted from my Feed remain available.
48. As a User, I want to inspect one Content Item in its Pod context, so that placement evidence and allowed actions are visible.
49. As an authorized curator, I want to Add to Pod directly, so that an existing Content Item can receive an Accepted Placement without Candidate review.
50. As an authorized curator, I want private placement removal to apply directly, so that local curation is not needlessly delayed.
51. As a Pod Owner, I want public placement removal to require approval and emit a Placement Tombstone, so that federation withdrawal does not erase unrelated placements.
52. As a Pod Owner, I want to inspect and set Curation Policy, so that Manual, Assisted, and Autonomous behavior is explicit.
53. As a User, I want Autonomous Curation enablement to require approval, so that automation authority cannot expand silently.
54. As a Pod Owner, I want package changes represented as Package Revisions, so that authoritative state is versioned rather than overwritten by imports.
55. As a Pod Owner, I want to inspect historical package versions, so that prior instructions and Source Rules remain auditable.
56. As a Pod Owner, I want to validate a portable package directory before revision, so that invalid artifacts fail before changing authoritative state.
57. As a Pod Owner, I want to export a Pod Package, so that it remains portable across compatible Stumble nodes.
58. As an Agent Harness, I want Source Rules to be the only origin of scheduled Discovery Tasks, so that manual task injection cannot bypass Pod intent.
59. As an Agent Harness, I want due tasks materialized automatically, so that I do not call scheduler internals.
60. As an Agent Harness, I want one filterable Discovery Task list, so that ready and historical work share one collection contract.
61. As an Agent Harness, I want to inspect, claim, renew, complete, and fail Discovery Tasks, so that lease-based work is complete and recoverable.
62. As an Agent Harness, I want to list and submit Candidates with provenance and idempotency, so that discovery inputs are trustworthy and retry-safe.
63. As an authorized curator, I want Candidate evaluation to apply each Pod's Curation Policy, so that automated and pending outcomes are deterministic.
64. As a Routing Agent, I want to record evidence-backed Pod Placement proposals, so that cross-Pod routing remains scoped and auditable.
65. As an authorized curator, I want to accept or reject one pending Pod Placement, so that manual decisions do not conflate a Candidate's other placements.
66. As a User, I want `feed batch get` to return the current stable Feed Batch, so that retrieval uses canonical transport language.
67. As a User, I want Drip to remain conversational intent rather than a CLI alias, so that friendly language does not fragment the machine contract.
68. As a User, I want to complete a Feed Batch explicitly, so that Caught Up remains a deliberate state transition.
69. As a User, I want Feedback Signals recorded through one operation, so that saves, preference signals, dismissals, and item-driven blocks share one contract.
70. As a User, I want to inspect, set, and reset my Taste Profile, so that durable preferences and learned weights remain controllable.
71. As a Home Node Owner, I want to list, add, and remove trusted peers through domain workflows, so that Trust Policy is not manipulated as raw configuration.
72. As a User, I want synchronization to remain automatic by default, so that normal subscriptions do not require manual protocol commands.
73. As an operator, I want an explicit Pod synchronization run and status, so that I can diagnose and recover stalled synchronization.
74. As a maintainer, I want signed-event file manipulation removed from the public parser, so that protocol diagnostics do not masquerade as User workflows.
75. As an operator, I want HTTP serving owned by `stumble-api`, so that process lifecycle is separate from one-shot CLI workflows.
76. As an Agent Harness integrator, I want streamable HTTP and stdio MCP owned by `stumble-mcp`, so that MCP transports share one dedicated executable.
77. As a maintainer, I want repository callers, documentation, tests, and scheduler adapters migrated in the same change sequence, so that no internal dependency relies on removed names.
78. As a maintainer, I want the complete first-release scenario to pass through the new contracts, so that the refactor preserves the product rather than only parser structure.

## Implementation Decisions

- The executable is renamed from `podctl` to `stumble`; no deprecated binary, hidden command, or compatibility alias remains.
- The CLI is local-only. Remote operation is provided by the dedicated HTTP and MCP adapters, not by a CLI HTTP-client mode.
- The parser has exactly five top-level workflow families: Node, Pod, Discovery, Feed, and Sync. Authorization remains attached to operations and resources rather than inferred from family membership.
- Command paths use resource-first grammar with short, unhyphenated command words. Flags use conventional kebab case when needed.
- The default Home Node is `~/.stumble/nodes/home`, with environment and flag overrides. Path resolution is deterministic and observable in Node output.
- Home Node opening is split from initialization. Only explicit initialization may create state; normal commands never seed missing state.
- Owner authentication uses a credential-store boundary backed by the operating-system keychain. Owner credential material is not stored in the Home Node database or emitted by normal commands.
- Owner-authenticated Harness registration and revocation are the only direct bootstrap exception to sensitive-change approval. Registration returns the Harness credential once; persisted and returned Harness data contains only the hash or fingerprint required for authentication and identification.
- All other sensitive changes return a Pending Proposal as the outcome of the canonical domain command. Generic proposal creation is not public.
- Success and error output use version-1 envelopes. Domain result types remain transport-neutral data inside the envelope.
- Text output is a renderer over the same result and error data, not a separate behavioral contract.
- Exit status is intentionally coarse; stable domain error codes carry the actionable meaning.
- Collection results share one cursor-pagination shape and bounded default and maximum limits.
- Detailed resource results compute allowed actions from current state, User identity, Harness Grant, and Pod scope.
- Structured request input uses one file-or-stdin convention. Portable Pod Package directories remain the exception because their files are the canonical artifact.
- Idempotency is a core-owned mutation behavior shared across CLI, HTTP, and MCP rather than a parser-only cache.
- Pod references use one resolver for local slugs and immutable IDs. Public Pod URLs are accepted only where direct Subscription requires addressing an Origin Node.
- Subscription becomes a distinct persistence and domain relationship from Pod Role. Existing combined membership state is migrated without losing Priority Subscription or Owner/Curator authority.
- Pod Role has only Owner and Curator variants. Existing Owner and Admin authority maps to the appropriate canonical role; a passive Member maps to Subscription only when it represented Feed eligibility.
- Pod creation combines bare creation, initial package creation, and package-derived creation without making package presence determine visibility.
- Explore is a Pod-family read workflow filtered by the User's local Trust Policy and never creates a Subscription implicitly.
- Pod content reads return Accepted Placement context. Add to Pod creates an Accepted Placement immediately under existing authority; public removal follows the Pending Proposal and Placement Tombstone rules.
- Package mutation creates a validated Package Revision based on an explicit current version. Public revision approval and stale-base detection remain core-owned.
- Source Rules in the authoritative Pod Package are the sole scheduled Discovery Task source. Automatic materialization occurs at scheduler/listing seams without exposing infrastructure commands.
- Candidate curation actions preserve independent Pod Placements and existing Manual, Assisted, and Autonomous policy semantics.
- Feed operations preserve stable finite Feed Batches, Caught Up, Feedback Signals, private Taste Profiles, and allowed actions.
- Sync commands call high-level synchronization operations. Event export, import, and verification remain internal test or recovery APIs rather than parser commands.
- `stumble-api` remains the HTTP process. `stumble-mcp` gains or retains both streamable HTTP and stdio MCP. `stumble` contains no long-running server loop.
- The migration is delivered through dependency-ordered, green vertical slices. The old parser contracts are removed only after repository callers and dedicated transport entry points have moved.

## Testing Decisions

- The primary acceptance seam is the real `stumble` executable running against temporary Home Nodes. Tests assert command discovery, parsing, versioned stdout and stderr, exit status, state transitions, idempotent retries, pagination, allowed actions, and absence of retired commands.
- Executable tests use an isolated credential-store implementation so Owner keychain behavior is exercised without touching a developer's real keychain. A small credential-store contract test proves the production backend stores, reads, and removes the expected secret.
- Core `AgentTools` tests remain the authority seam for Subscription and Pod Role separation, migration, Harness Grants, Pending Proposals, Package Revisions, Curation Policy, Candidate and placement transitions, Feed behavior, synchronization, idempotency, and error codes.
- Existing CLI integration-test style using real child processes and temporary data directories is prior art and should be extended rather than replaced with parser-only unit tests.
- Existing cross-adapter tests are prior art for semantic equivalence. CLI, HTTP, and MCP should return equivalent domain IDs, authorization failures, Pending Proposal outcomes, and allowed actions even though their envelopes differ.
- Dedicated `stumble-mcp` process tests prove both stdio and streamable HTTP operation, authentication, tool filtering, and revocation without relying on the removed CLI transport mode.
- Existing package, Candidate, Feed, Harness authorization, Pending Proposal, direct Subscription, Scheduler Adapter, and legacy-contract suites are migrated to the canonical commands and vocabulary.
- A final first-release integration test exercises two nodes, scoped Harnesses, Pod creation and package work, Source Rule-derived Discovery Tasks, Candidate submission and curation, Subscription and synchronization, Feed and feedback, Explore, Add to Pod, Placement Tombstones, approval, restart, signatures, and privacy through the new executable and dedicated adapters.
- Good tests assert observable domain behavior and stable contracts, not Clap enum structure, private helper calls, SQL layout, or formatting implementation.
- No test may be deleted, skipped, weakened, or narrowed solely to make the migration pass. Tests for retired behavior are converted to assert ordinary usage errors or moved to the correct dedicated adapter contract.
- Focused package and integration tests run during each vertical slice. The completed feature requires `cargo fmt --check` and `cargo test --workspace` to pass.

## Out of Scope

- A graphical Stumble client or any social conversation surface.
- A remote HTTP-client mode in the `stumble` executable.
- Backward-compatible `podctl` binaries, deprecated aliases, or hidden legacy commands.
- Public tenant administration, raw API-token administration, hosted-development token minting, or server lifecycle commands.
- Public signed-event file import, export, or verification commands.
- Manual Discovery Task creation or materialization outside Source Rule scheduling.
- Restoring legacy crawler, Source Connector, Submission, Stumble, Brief, or standalone block workflows.
- Changing the signed federation protocol or making Home Nodes publicly reachable by default.
- Adding Pod social membership, Admin, or Member roles.
- Changing the meaning of Feed Batch stability, Attention Value, Candidate provenance, Package Revision approval, or Placement Tombstones beyond what is required to expose the accepted workflows.
- Creating new ADRs during implementation without explicit User approval.

## Further Notes

- The governing decisions are ADR 0033 and ADR 0034, and canonical vocabulary is defined in the root domain glossary.
- This is a wide refactor with several domain corrections. Ticketing should use an expand–migrate–contract sequence where a mechanical rename or shared type migration cannot remain green as one vertical slice.
- Implementation tickets must be small enough for one fresh agent context and must declare blocking edges. A parent goal should work only the ready frontier, using one implementation subagent at a time because all agents share one worktree.
- Completed on 2026-07-18: issues 01–16 are complete, the final two-node executable and cross-adapter journey is covered, and `cargo fmt --check && cargo test --workspace` passes.
