# PRD: Decentralized Personal Feed

Status: ready-for-agent

## Problem Statement

People currently discover useful material through social feeds whose ranking objectives, source access, and behavioral data are controlled by platform companies. The User wants a personal agent to find worthwhile material across the internet, organize it into subscribable Pods, and deliver a finite decentralized Feed without requiring a dedicated Stumble interface or an infinite-scroll social network.

The existing Stumble MVP centers product behavior on individual Pods and stored link discovery. It has useful primitives for Pod curation, signed events, federation, preferences, feedback, ranking, and Agent Harness adapters, but it does not yet provide the complete autonomous loop: harness-driven discovery, local synchronization of subscribed Pods, a cross-Pod Feed, private learning, portable Pod Packages, or transactional local persistence.

## Solution

Stumble becomes a headless, harness-neutral, decentralized personal discovery system. A User operates a Home Node through an Agent Harness such as ChatGPT, OpenClaw, or another compatible runtime. Pods remain independently curated collections, each carrying a signed, versioned Pod Package with subject context, agent instructions, Source Rules, filters, and calibration examples.

Agent Harnesses own external discovery. They use their own browser, search, APIs, credentials, and scheduling, claim due Discovery Tasks, and submit structured provenance-bearing Candidates. Stumble validates and deduplicates those submissions, proposes or evaluates Pod Placements, applies each Pod's Curation Policy, and synchronizes only Accepted Placements.

The Home Node copies subscribed public Pod content locally through verified signed Pod Events and builds finite, stable Feed Batches from local and synchronized items. Feed ranking optimizes Attention Value rather than time spent, using the User's private Taste Profile, a constrained Feed Mix, explicit Feedback Signals, limited Exploration Items, and deliberate Old Gems. The Feed is returned as structured data; each Agent Harness chooses how to present it. Stumble has no dedicated graphical interface and no social-conversation layer.

## User Stories

1. As a User, I want to operate Stumble through my preferred Agent Harness, so that I am not tied to a dedicated application.
2. As a User, I want to retrieve a structured Feed Batch through a high-level tool, so that my harness can present it conversationally, by voice, or in another format.
3. As a User, I want a finite Feed Batch with a Caught Up state, so that discovery does not become infinite scrolling.
4. As a User, I want a configurable batch size, so that the amount of material matches my available attention.
5. As a User, I want Feed Batches to remain stable after delivery, so that newly arriving content does not reorder the session I am consuming.
6. As a User, I want Delivered Items suppressed for a configurable period, so that the Feed does not repeat itself immediately.
7. As a User, I want durable Old Gems to resurface later, so that valuable older material is not permanently lost.
8. As a User, I want dismissed, blocked, or negatively rated content excluded from automatic resurfacing, so that Stumble respects explicit rejection.
9. As a User, I want Feed ranking to optimize Attention Value rather than dwell time, so that the system serves my interests instead of maximizing usage.
10. As a User, I want ranking explanations and evidence, so that I can understand why each item was selected.
11. As a User, I want Feed composition to limit domination by one Pod or source, so that the batch remains diverse.
12. As a User, I want Priority Subscriptions to receive representation when content is available, so that important Pods are not crowded out.
13. As a User, I want a small quota of clearly labeled Exploration Items, so that I can discover relevant public Pods beyond my existing Subscriptions.
14. As a User, I want an intentional Explore operation, so that I can search and inspect public Pods without changing my Subscriptions.
15. As a User, I want temporary Batch Intent such as focus and avoidance instructions, so that one Feed request can reflect my current mood or task.
16. As a User, I want temporary Batch Intent kept separate from durable preferences, so that one request does not silently retrain my Feed.
17. As a User, I want an explicit and learned Taste Profile, so that the Feed improves while preserving my stated preferences.
18. As a User, I want to inspect, edit, explain, and reset learned taste weights, so that personalization remains under my control.
19. As a User, I want my Taste Profile, feedback, saves, Feed history, Subscriptions, tasks, and grants to remain on my Home Node, so that behavioral data is private.
20. As a User, I want to Save an item, so that I can return to it later.
21. As a User, I want More like this and Less like this actions, so that I can shape future ranking without editing configuration manually.
22. As a User, I want to Dismiss a specific item, so that it leaves my Feed without blocking an entire source or topic.
23. As a User, I want to block a source, Origin Node, Pod, or topic, so that unwanted material is excluded locally.
24. As a User, I want to add a discovered item to a Pod I curate, so that useful material becomes part of my collection.
25. As a User, I want Add to Pod to preserve the original source and Origin Pod, so that curation never becomes false authorship.
26. As a User, I want one canonical Content Item to have placements in multiple Pods, so that overlapping subjects do not create duplicate content.
27. As a User, I want an item appearing through multiple subscribed Pods to appear only once in a Feed Batch, so that agreement improves context rather than duplication.
28. As a User, I want to subscribe directly using a public Pod URL, so that discovery does not require a central directory.
29. As a User, I want a Subscription to make Pod content eligible rather than guarantee every item, so that the Feed can still rank selectively.
30. As a User, I want to inspect a Pod's complete accepted stream, so that items not selected for my Feed remain available through an Agent Harness.
31. As a User, I want remote Pod content copied to my Home Node, so that ranking is private and the Feed remains useful during remote outages.
32. As a User, I want remote Placement Tombstones synchronized, so that withdrawn origin curation stops appearing locally.
33. As a User, I want my independent local placement or Save to survive an Origin Pod withdrawal, so that remote deletion cannot erase my own curation.
34. As a Pod owner, I want to create a Pod Package containing `CONTEXT.md`, `SKILL.md`, Source Rules, filters, and examples, so that any compatible Agent Harness can understand the Pod.
35. As a Pod owner, I want Pod Packages validated, versioned, signed, imported, and exported, so that they are portable and auditable.
36. As a Pod owner, I want Package Revisions shown as structured diffs before public acceptance, so that agent-authored changes remain under my control.
37. As a Pod owner, I want Manual, Assisted, and Autonomous Curation Policies, so that each Pod can choose its appropriate level of automation.
38. As a Pod owner, I want Assisted Curation to accept only trusted high-confidence proposals automatically, so that unattended updates remain controlled.
39. As a Pod owner, I want automated acceptance and rejection history to be auditable and reversible, so that I can correct agent behavior.
40. As a Pod owner, I want Source Rules to describe what an Agent Harness should inspect and seek, so that discovery intent travels with the Pod without platform-specific connector code.
41. As a Pod owner, I want public, invite-only, and private visibility represented in the domain, so that future access models do not require a redesign.
42. As a private Pod owner, I want the Pod excluded from federation and Explore, so that it never leaves my Home Node.
43. As an Agent Harness, I want to retrieve a signed Pod Package before performing discovery or curation, so that I act with the correct context and instructions.
44. As an Agent Harness, I want remote Pod Skills treated as scoped untrusted instructions, so that they cannot override my governing rules or expand permissions.
45. As an Agent Harness, I want to discover external content using my own browser, search, APIs, credentials, and scheduling, so that Stumble does not need platform-specific connectors.
46. As an Agent Harness, I want to claim a lease on a due Discovery Task, so that multiple harnesses do not duplicate scheduled work.
47. As an Agent Harness, I want to submit an idempotent structured Candidate with provenance and placement evidence, so that Stumble can validate and deduplicate it safely.
48. As an Agent Harness, I want Stumble to expose high-level domain tools instead of requiring prompt choreography, so that integrations remain portable.
49. As an Agent Harness, I want allowed next actions returned with Feed and Candidate data, so that I can guide the User without guessing permissions.
50. As a node owner, I want each Agent Harness authorized by a revocable Harness Grant, so that unattended and interactive agents receive different authority.
51. As a node owner, I want every harness write attributed to its identity, so that automated activity is auditable.
52. As a node owner, I want sensitive public, trust, autonomy, and authority changes represented as Pending Proposals, so that unattended agents cannot approve their own expansion.
53. As a node owner, I want harness-native scheduling to drive discovery when available, so that ChatGPT, OpenClaw, and future runtimes can use their own automation systems.
54. As a node owner, I want a local launchd, cron, or equivalent Scheduler Adapter fallback, so that discovery work can still be woken when the harness has no scheduler.
55. As a node owner, I want browser-required work left pending for a capable harness, so that Stumble never silently takes control of Chrome.
56. As a node owner, I want SQLite to store authoritative local state transactionally, so that concurrent tasks, synchronization, curation, and Feed creation are reliable.
57. As a node owner, I want an existing JSON snapshot imported safely into SQLite, so that upgrading does not lose current data.
58. As a public Pod publisher, I want signed append-only Pod Events, so that subscribers can verify origin and synchronize incrementally.
59. As a public Pod publisher, I want compact signed Pod Announcements, so that public metadata can travel through the Stumble Substrate without replicating all content.
60. As a Home Node operator, I want direct Pod addressing to work without a public inbound endpoint, so that private nodes can remain outbound-only.
61. As a Home Node operator, I want to choose peers and Index Nodes under a local Trust Policy, so that no global reputation authority controls discovery.
62. As an Index Node operator, I want to aggregate signed Pod Announcements without becoming authoritative, so that search can scale while remaining replaceable.
63. As a subscriber, I want signatures to prove origin without implying quality, so that trust remains a local decision.
64. As a developer, I want HTTP, MCP, and CLI adapters to expose equivalent domain operations, so that Stumble remains Agent Harness neutral.
65. As a developer, I want one core acceptance seam for domain behavior, so that adapter changes do not duplicate business tests.
66. As a developer, I want two-node tests backed by temporary SQLite databases, so that federation and privacy boundaries are exercised realistically.

## Implementation Decisions

- The Feed becomes Stumble's primary product center; Pods become subscribable curation and discovery units beneath it.
- Stumble remains headless. It does not ship a dedicated graphical User interface.
- Stumble exposes transport-neutral high-level domain capabilities with HTTP, MCP, and CLI adapters.
- The canonical harness operations include Feed retrieval and refresh, feedback, Explore, Subscription management, Add to Pod, Candidate review, Pod and Package management, Discovery Task claim and completion, Candidate Submission, synchronization, and status inspection.
- Feed delivery is pull-first through `get_feed_batch`; optional Feed-ready Events support proactive harness delivery without changing batch semantics.
- A Feed Batch is finite, stable, structured, and records its Batch Intent, Feed Mix, ranking evidence, provenance, exploration labels, feedback state, and allowed next actions.
- Inclusion in a returned Feed Batch makes an item Delivered. Per-item presentation acknowledgement is not required.
- Delivered Items receive a default thirty-day repetition penalty. Strong new evidence, a new independent placement, explicit User intent, or Old Gem selection may permit earlier or later resurfacing. Explicit rejection prevents automatic resurfacing.
- Feed ranking estimates Attention Value from relevance, quality, novelty, diversity, timeliness, explicit preferences, and Feedback Signals. Dwell time, session length, and compulsive usage are not objectives.
- Feed Mix is a constrained blend. Defaults target roughly 70–80% highest-value subscribed content, up to 10% Exploration Items, up to 10% Old Gems, representation for eligible Priority Subscriptions, and per-Pod and per-source caps. Missing categories backfill from the next highest-value items.
- A Taste Profile combines explicit preferences with inspectable learned weights. Explicit preferences win conflicts, learned weights stay local, and weak single events do not become permanent preferences.
- One canonical Content Item may have multiple Pod Placements. Feed generation deduplicates by canonical identity and retains all contributing placement evidence.
- External content uses a reference-first model. Content References carry canonical source location, permitted metadata and excerpts, generated understanding, and provenance; full third-party content or media is retained only when permission allows it.
- All external discovery enters through Agent Harness Candidate Submissions. Stumble ships no dedicated platform, RSS, API, or browser connectors in this release.
- An Agent Harness owns its browser, search, API credentials, external access policy, and scheduling. Stumble never receives those credentials or controls its browser session.
- A Candidate Submission is authenticated and idempotent and includes source identity, known author and publication metadata, permitted excerpt and summary, content type and tags, proposed placements with evidence and confidence, discovery provenance, and relevant task and package versions.
- Stumble remains authoritative for URL normalization, canonical identity, deduplication, authorization, curation state, synchronization, and Feed creation. Agent scores are evidence rather than authority.
- Source Rules are Pod-owned instructions for Agent Harness discovery rather than executable Stumble connector configuration.
- Discovery Tasks are leaseable, retryable, idempotent units derived from Source Rules. Stumble owns due-work state, leases, deduplication, and completion history.
- Scheduler Adapters wake workers. Harness-native schedules are supported, and Stumble ships local launchd, cron, or equivalent fallback setup. Local scheduling may invoke an explicitly configured harness command or emit Discovery-ready Events; it does not perform browser discovery itself.
- A Pod Package contains Pod Context, Pod Skill, Source Rules, filters, and good and bad calibration examples. It is versioned, signed, validated, importable, and exportable.
- Remote Pod Skills are scoped untrusted instructions. They cannot override higher-priority Agent Harness rules, receive Harness Grants, control a browser, expose credentials, or approve sensitive actions.
- SQLite is authoritative for local mode. It stores domain state, tasks, leases, synchronization cursors, Feed history, and private projections transactionally. JSON becomes migration, import, and export format; PostgreSQL remains a later hosted adapter.
- A one-time migration imports existing JSON state without overwriting a populated SQLite database and preserves a recoverable source snapshot.
- Public Pod federation uses signed append-only Pod Events. Home Nodes verify origin and chain integrity before projecting accepted placements, package versions, updates, and tombstones.
- Subscribed remote Pod content synchronizes locally before ranking. Private User state never federates.
- Remote Placement Tombstones withdraw only the origin placement. Independent local placements and saves survive with withdrawal provenance.
- Direct Pod URLs are canonical. The Stumble Substrate also supports signed Pod Announcements, trusted peer exchange, and optional non-authoritative Index Nodes. Full content synchronizes only after Subscription.
- Private Home Nodes may remain outbound-only. Public Origin Nodes use stable HTTPS in the first release; Relay Nodes are a later extension.
- Trust is local. Signatures prove origin but not quality, and each Home Node applies its own Trust Policy. Pod Endorsements may inform local ranking without establishing a universal score.
- The first federation release proves a real direct two-node public-Pod flow. Index-node search may use existing foundations, while broad peer gossip and relays are deferred.
- Public remote Pods and private local Pods are first-release access paths. Invite-only federation remains represented but its authorization, revocation, and encrypted synchronization protocol are deferred.
- Harness Grants are revocable and capability-scoped, including Pod restrictions. Unattended workers receive narrower authority than interactive harnesses.
- Sensitive changes use expiring Pending Proposals and structured diffs. Public exposure, public Package Revisions, Autonomous Curation, powerful grants, public tombstones, and trust changes require independent interactive approval.
- Add to Pod is an authorized one-step human curation action that creates an Accepted Placement and preserves provenance. Automated discovery always begins as a Candidate.
- Pods support Manual, Assisted, and Autonomous Curation. Assisted is the default; Autonomous requires explicit owner approval. Automated decisions remain auditable and reversible.
- Stumble excludes posts, replies, direct messages, follower counts, and other social-conversation features. Interaction with source content occurs at the original source.
- “Stumble” remains the system and protocol name. “Feed” is canonical domain language, and “Drip” is a friendly Agent Harness command for retrieving a Feed Batch.
- The first release is optimized for one User on a local Home Node while retaining User identity and authorization boundaries for later hosted operation.

## Testing Decisions

- Test external domain behavior rather than internal tables, private helper functions, score implementation details, or SQL statement shape.
- The primary acceptance seam is a pair of real AgentTools instances backed by temporary SQLite databases.
- End-to-end tests drive Harness Grant authorization, Pod creation and packages, Discovery Task leases, Candidate Submission idempotency, deduplication, routing, curation, Feed creation, Feedback Signals, Add to Pod, signed synchronization, and Placement Tombstones through public domain operations.
- The canonical two-node acceptance test creates a public Pod on an Origin Node, synchronizes it to a subscribed Home Node, verifies Feed inclusion, preserves private feedback locally, and applies a later tombstone without deleting an independent local placement.
- Privacy tests inspect only exported protocol artifacts and public adapter responses to prove that Taste Profiles, feedback, saves, Feed history, tasks, and grants are absent.
- Feed tests assert observable composition constraints, stability, delivery history, recurrence behavior, exploration labeling, and explanations without locking down one exact floating-point score formula.
- Authorization tests prove least-authority Harness Grants, Pod scoping, revocation, Pending Proposal separation, and the inability of unattended workers to self-approve.
- Package tests validate import/export round trips, signature verification, versioning, structured diffs, and the inability of synchronized packages to alter local grants.
- Discovery Task tests cover leases, expiry, retry, idempotency, duplicate harness workers, and local scheduler wake-up behavior.
- SQLite migration tests cover empty-node initialization, one-time JSON import, rollback on malformed state, idempotent restart, and prevention of accidental overwrite.
- A thin adapter contract suite exercises representative high-level operations through HTTP, MCP, and CLI and compares structured behavior. Domain semantics are not duplicated separately inside every adapter.
- Existing AgentTools behavior tests are prior art and should be migrated toward focused module-specific integration fixtures as the monolithic seed test file is decomposed.
- The full workspace test suite and formatting checks must remain green after every ticket.

## Out of Scope

- A dedicated web, desktop, or mobile User interface.
- Social posts, replies, quote-posts, direct messages, comments, follower counts, or conversation hosting.
- Dedicated X, Reddit, Pinterest, Hacker News, RSS, browser, or other external-source connectors inside Stumble.
- Stumble-owned browser automation or storage of external platform credentials.
- Optimization for dwell time, retention, session length, or infinite scrolling.
- Full third-party content mirroring when rights or source policy do not permit it.
- Invite-only cross-node authorization, encrypted Pod synchronization, and membership revocation in the first release.
- A global reputation score, centralized mandatory registry, blockchain, global consensus, or DHT.
- Broad gossip, Relay Node publishing, and production-scale Index Node operation in the first release.
- Hosted multi-tenant administration and full PostgreSQL runtime parity in the first release.
- Native proactive delivery integrations for every Agent Harness; the event contract is included, while harness-specific setup may follow separately.
- Fully autonomous acceptance without explicit per-Pod owner configuration.

## Further Notes

- The first release is complete when the agreed fourteen-step definition of done works end to end, including harness authorization, package creation, task-driven Candidate Submission, direct two-node synchronization, a finite blended Feed Batch, private feedback, provenance-preserving Add to Pod, tombstones, adapter parity, privacy checks, and local scheduling fallback.
- Existing Pod, signing, federation, ranking, feedback, and adapter foundations should be evolved rather than replaced wholesale.
- The current crawler boundary and crawler-oriented naming are legacy concepts. New domain APIs should prefer Candidate Submission, Discovery Task, Pod Package, Content Item, and Pod Placement. Compatibility may require an expand-migrate-contract sequence rather than a repository-wide breaking rename.
- The first useful demonstration should use two locally running nodes and a real Agent Harness submission; it does not need a platform-specific scraper to prove the product.
- The official OpenAI documentation MCP source was added globally during planning and may require a new Codex task before it appears as an available documentation connector.
