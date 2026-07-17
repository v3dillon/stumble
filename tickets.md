# Tickets: Decentralized Personal Feed

These tickets build Stumble's headless, harness-neutral decentralized Feed. The source specification is [`.scratch/dopamine-drip/PRD.md`](.scratch/dopamine-drip/PRD.md).

Work the **frontier**: any ticket whose blockers are all done. Each ticket is sized for one fresh implementation context and must leave formatting and the full workspace test suite green.

## Boot the Home Node from SQLite

**Status:** ready-for-agent

**What to build:** Make a local Stumble node boot, persist existing behavior, and restart safely using SQLite as its authoritative store. When legacy JSON state exists and the database is empty, import it once while preserving a recoverable backup; never overwrite populated SQLite state.

**Blocked by:** None — can start immediately.

- [x] A new local node initializes a SQLite database and preserves state across restart.
- [x] Existing Pods, packages, submissions, preferences, feedback, events, peers, and briefs round-trip through the database.
- [x] Legacy JSON state imports exactly once into an empty database and remains recoverable.
- [x] Malformed legacy state rolls back without leaving a partially populated database.
- [x] Concurrent writes use transactions rather than whole-store snapshot replacement.
- [x] Existing public domain behavior remains compatible while later tickets add new concepts.

## Authorize Agent Harnesses with scoped grants

**Status:** ready-for-agent

**What to build:** Let a User register multiple Agent Harnesses with revocable Harness Grants, capability scopes, and optional Pod scopes. Make every harness write attributable and expose the same authorization behavior through HTTP, MCP, and CLI.

**Blocked by:** Boot the Home Node from SQLite.

- [ ] A User can register a labeled interactive or unattended Agent Harness and receive a one-time token.
- [ ] Grants can independently allow Feed reads, feedback, Discovery Tasks, Candidate Submission, Pod curation, package management, Subscription management, and administration.
- [ ] Pod-scoped grants cannot access or modify other Pods.
- [ ] Revocation takes effect without restarting the node.
- [ ] Every harness-originated write records the harness identity.
- [ ] Tokens and grants are absent from federation and public export surfaces.
- [ ] Representative HTTP, MCP, and CLI calls return equivalent authorization outcomes.

## Create and exchange portable Pod Packages

**Status:** ready-for-agent

**What to build:** Let an authorized Agent Harness create a private Pod with a validated, versioned Pod Package containing Pod Context, Pod Skill, Source Rules, filters, and calibration examples. Make packages inspectable and portable without exposing local grants.

**Blocked by:** Boot the Home Node from SQLite; Authorize Agent Harnesses with scoped grants.

- [ ] A harness can create a private Pod and its initial complete Pod Package in one flow.
- [ ] Package validation distinguishes subject context from scoped curation instructions.
- [ ] Source Rules express what a harness should inspect, seek, and schedule without executable connector code or credentials.
- [ ] Package import and export round-trip through the portable directory format.
- [ ] Package versions are immutable and attributable to their proposer and owner.
- [ ] Remote or imported packages cannot alter Harness Grants, browser permissions, or other node-local authority.
- [ ] HTTP, MCP, and CLI expose equivalent create, read, validate, import, and export behavior.

## Claim scheduled Discovery Tasks

**Status:** ready-for-agent

**What to build:** Turn due Pod Source Rules into leaseable Discovery Tasks that any authorized Agent Harness can claim, complete, fail, or retry. Support harness-native scheduling and a local scheduler fallback without giving Stumble browser ownership.

**Blocked by:** Authorize Agent Harnesses with scoped grants; Create and exchange portable Pod Packages.

- [ ] Due Source Rules create idempotent Discovery Tasks with the relevant Pod Package version.
- [ ] Authorized harnesses can list, claim, renew, complete, and fail tasks.
- [ ] Leases prevent concurrent duplicate execution and expire safely after abandoned work.
- [ ] Retry history and terminal failure are inspectable through status tools.
- [ ] A local launchd, cron, or equivalent adapter wakes due work when no harness scheduler exists.
- [ ] The local scheduler may invoke an explicitly configured harness command or emit a Discovery-ready Event but never controls a browser itself.
- [ ] Manual conversational discovery can create an immediate task through the same contract.

## Submit provenance-bearing Candidates

**Status:** ready-for-agent

**What to build:** Let a task-owning or interactive Agent Harness submit structured external discoveries that Stumble authenticates, canonicalizes, deduplicates, and records as private Candidates with proposed multi-Pod placements.

**Blocked by:** Authorize Agent Harnesses with scoped grants; Create and exchange portable Pod Packages; Claim scheduled Discovery Tasks.

- [ ] Candidate Submission accepts source URL, known source metadata, permitted excerpt and summary, content type, tags, provenance, and placement evidence.
- [ ] Task-driven submissions carry the task and Pod Package versions used during discovery.
- [ ] Harness and client idempotency keys make retries safe.
- [ ] Canonical identity deduplicates repeated discoveries without losing independent placement evidence.
- [ ] One submission may propose several authorized local Pod Placements with separate reasons and confidence.
- [ ] Harness confidence is retained as evidence but does not directly create authoritative placements.
- [ ] Candidates and review state never appear in federation exports.
- [ ] HTTP, MCP, and CLI expose equivalent submission and inspection behavior.

## Require approval for sensitive changes

**Status:** ready-for-agent

**What to build:** Represent sensitive public, authority, trust, autonomy, and removal changes as expiring Pending Proposals with structured diffs and independent approval.

**Blocked by:** Authorize Agent Harnesses with scoped grants; Create and exchange portable Pod Packages.

- [ ] Sensitive operations create a Pending Proposal rather than applying immediately.
- [ ] Proposals state the requested change, affected resources, proposer, expiry, and expected consequences.
- [ ] An interactive harness with approval permission can approve or reject a proposal.
- [ ] An unattended harness cannot approve a proposal it created or expand its own grant.
- [ ] Expired, rejected, and accepted proposals remain auditable.
- [ ] Routine Feed, feedback, synchronization, Candidate Submission, and already-authorized curation operations remain one step.
- [ ] Approval behavior is consistent across HTTP, MCP, and CLI.

## Curate and route Content Items across Pods

**Status:** ready-for-agent

**What to build:** Convert Candidates into canonical Content Items and independently accepted Pod Placements under Manual, Assisted, or Autonomous Curation. Support local cross-Pod routing and provenance-preserving Add to Pod.

**Blocked by:** Submit provenance-bearing Candidates; Require approval for sensitive changes.

- [ ] Manual Curation requires authorized review for every proposed placement.
- [ ] Assisted Curation may accept trusted high-confidence proposals and queues uncertainty.
- [ ] Autonomous Curation requires an approved sensitive-change proposal before activation.
- [ ] Each placement records its evidence, curation path, actor, and audit history.
- [ ] A Routing Agent may propose additional placements only in Pods the node is authorized to curate.
- [ ] One canonical Content Item can hold Accepted Placements in multiple Pods.
- [ ] An authorized Add to Pod action immediately creates an Accepted Placement and optional curation note.
- [ ] Rejections and reversals affect future local routing without leaking private feedback.

## Deliver a finite local Feed

**Status:** ready-for-agent

**What to build:** Let an Agent Harness request a stable finite Feed Batch from accepted content on the Home Node, receive structured explanations and allowed actions, and record the complete initial Feedback Signal vocabulary.

**Blocked by:** Boot the Home Node from SQLite; Authorize Agent Harnesses with scoped grants; Curate and route Content Items across Pods.

- [ ] `get_feed_batch` returns a stable structured batch with configurable size and a real Caught Up state.
- [ ] Batch retrieval marks included items Delivered without requiring per-item presentation acknowledgement.
- [ ] Repeated retrieval of the current batch does not create a different batch or duplicate delivery history.
- [ ] Recently Delivered Items receive a configurable recurrence penalty instead of permanent exclusion.
- [ ] Items include Content References, placements, provenance, ranking evidence, exploration state, feedback state, and allowed next actions.
- [ ] Save, More like this, Less like this, Dismiss, source block, topic block, and Add to Pod affect observable subsequent behavior.
- [ ] Dwell time and session duration are not recorded as ranking objectives.
- [ ] HTTP, MCP, and CLI return equivalent structured Feed and feedback behavior.

## Learn a private Taste Profile

**Status:** ready-for-agent

**What to build:** Improve subsequent Feed Batches using a private Taste Profile that combines explicit preferences with explainable learned weights while keeping the User in control.

**Blocked by:** Deliver a finite local Feed.

- [ ] Users can inspect and edit explicit interests, blocks, and recurrence preferences.
- [ ] Feedback Signals and Add to Pod actions update learned weights locally.
- [ ] Explicit settings override learned inference when they conflict.
- [ ] A single weak signal cannot create a permanent preference.
- [ ] Users can inspect evidence for learned weights and reset some or all of them.
- [ ] Feed explanations identify relevant explicit and learned signals without exposing sensitive raw history unnecessarily.
- [ ] Taste Profiles and their evidence are absent from every federation and public export surface.

## Subscribe and synchronize across two nodes

**Status:** ready-for-agent

**What to build:** Let a private outbound-only Home Node subscribe directly to a public Pod on another reachable Origin Node, verify its signed package and events, synchronize accepted content into SQLite, and include it in the local Feed.

**Blocked by:** Create and exchange portable Pod Packages; Curate and route Content Items across Pods; Deliver a finite local Feed.

- [ ] An Origin Node can publish a public Pod through an approved visibility change.
- [ ] A Home Node can subscribe using the public Pod URL without becoming publicly reachable.
- [ ] Signed Pod Package versions and append-only Pod Events are verified before projection.
- [ ] Synchronization resumes incrementally from a stored cursor and is idempotent.
- [ ] Only Accepted Placements and permitted Content References synchronize.
- [ ] Remote content becomes Feed-eligible while local Taste Profile and Feedback Signals remain private.
- [ ] An unavailable Origin Node does not make already synchronized Feed content unusable.
- [ ] The two-node behavior is covered at the primary temporary-SQLite acceptance seam.

## Apply tombstones without erasing local curation

**Status:** ready-for-agent

**What to build:** Synchronize Origin Pod withdrawals as Placement Tombstones while preserving independent local Saves and Accepted Placements with accurate provenance.

**Blocked by:** Subscribe and synchronize across two nodes.

- [ ] An Origin Node can propose and approve withdrawal of a public placement.
- [ ] Subscribers verify and apply the signed Placement Tombstone incrementally.
- [ ] The withdrawn origin placement stops contributing Feed eligibility.
- [ ] A local Save survives and records the origin withdrawal.
- [ ] A local Add to Pod placement survives and retains its original provenance chain.
- [ ] Orphaned Content References are purged only when no placement, Save, or required audit record retains them.
- [ ] Synchronization never silently rewrites or deletes origin history.

## Discover Pods through the Stumble Substrate

**Status:** ready-for-agent

**What to build:** Let Users discover public Pods through direct addresses, signed Pod Announcements, optional Index Nodes, endorsements, and local Trust Policies without introducing a mandatory central registry.

**Blocked by:** Subscribe and synchronize across two nodes.

- [ ] Public Origin Nodes produce compact signed Pod Announcements without exporting full Pod content.
- [ ] Trusted peers can exchange and relay announcements without becoming authoritative.
- [ ] Optional Index Nodes aggregate announcements and expose replaceable search results.
- [ ] Direct Pod URLs continue to work when Index Nodes are absent.
- [ ] Users can configure trusted peers and Index Nodes and locally block Pods, nodes, sources, and topics.
- [ ] Signatures prove origin without assigning a global quality score.
- [ ] Pod Endorsements are optional local ranking evidence rather than universal reputation.
- [ ] Explore can return public Pods and sample Content References beyond current Subscriptions.

## Complete the constrained Feed Mix

**Status:** ready-for-agent

**What to build:** Produce the intended Personal Feed by blending high-value subscribed content, controlled exploration, Priority Subscriptions, and deliberate Old Gems under configurable diversity constraints and temporary Batch Intent.

**Blocked by:** Learn a private Taste Profile; Apply tombstones without erasing local curation; Discover Pods through the Stumble Substrate.

- [ ] Default batches target roughly 70–80% highest-value subscribed content, up to 10% Exploration Items, and up to 10% Old Gems.
- [ ] Per-Pod and per-source caps prevent domination while unavailable categories backfill cleanly.
- [ ] Eligible Priority Subscriptions receive representation without overwhelming the batch.
- [ ] The same canonical Content Item appearing through several Pods is delivered once with all contributing placement evidence.
- [ ] Exploration Items are clearly labeled and do not silently create a Subscription.
- [ ] Batch Intent changes only the requested batch and remains visible in its explanation.
- [ ] Old Gems become eligible after the recurrence penalty decays or strong new evidence appears.
- [ ] Dismissed, blocked, and Less like this items do not automatically resurface.
- [ ] Observable composition tests avoid locking down one exact floating-point scoring formula.

## Retire legacy crawler and submission contracts

**Status:** ready-for-agent

**What to build:** Complete the expand–migrate–contract transition from crawler, source-connector, submission, and brief-centered behavior to the approved headless Candidate, Content Item, Pod Package, Discovery Task, and Feed contracts.

**Blocked by:** Claim scheduled Discovery Tasks; Curate and route Content Items across Pods; Complete the constrained Feed Mix.

- [ ] All first-release workflows use the canonical domain vocabulary and high-level harness operations.
- [ ] Legacy persisted data migrates without losing canonical identity, placements, feedback, or events.
- [ ] Obsolete crawler and dedicated connector operations are removed or return an explicit versioned compatibility error rather than a placeholder success.
- [ ] Brief behavior is either expressed as Agent Harness presentation of a Feed Batch or clearly retained as a compatibility adapter.
- [ ] HTTP, MCP, and CLI tool catalogs contain no reserved placeholder operations for first-release behavior.
- [ ] Adapter contract tests prove equivalent IDs, provenance, errors, and allowed actions across transports.
- [ ] Protocol version negotiation prevents new event shapes from being misread by older nodes.

## Prove the complete first release

**Status:** ready-for-agent

**What to build:** Verify and document the complete headless decentralized Feed as one reproducible two-node scenario that an Agent Harness can operate without hidden manual database work.

**Blocked by:** Require approval for sensitive changes; Apply tombstones without erasing local curation; Complete the constrained Feed Mix; Retire legacy crawler and submission contracts.

- [ ] A scoped Agent Harness creates a private Pod and valid Pod Package.
- [ ] The harness claims a due Discovery Task and submits a structured Candidate using its own external capabilities.
- [ ] Curation produces accepted multi-Pod content under the configured policy.
- [ ] A second node publishes a public Pod that the Home Node subscribes to and synchronizes.
- [ ] A finite Feed Batch blends local, remote, exploratory, priority, and Old Gem content as available.
- [ ] Feedback changes later ranking without appearing in federation artifacts.
- [ ] Add to Pod and a later remote Placement Tombstone preserve correct provenance and local ownership.
- [ ] A local Scheduler Adapter can wake or invoke discovery work when the harness lacks scheduling.
- [ ] HTTP, MCP, and CLI pass the shared adapter contract suite.
- [ ] JSON migration, SQLite restart, signed-event verification, privacy exports, and the complete workspace test suite pass.
- [ ] Operator and Agent Harness documentation describe setup, grants, tools, scheduling, two-node federation, and recovery.
