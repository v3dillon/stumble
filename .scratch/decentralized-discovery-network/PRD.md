# PRD: Decentralized Discovery Network

Status: ready-for-agent

## Problem Statement

Stumble has the foundations of signed Pod federation and local discovery, but a new Home Node cannot yet join a useful decentralized discovery network automatically. The public runtime still exposes a legacy centralized Hub registration and refresh model, while the newer Stumble Substrate exists mainly as Core behavior without the Bootstrap, neutral synchronization, lifecycle, and automatic Discovery Peer contracts needed for real network effects.

Users should be able to install Stumble and discover relevant content from independently operated public Pods without supplying a Pod URL, exposing their private Taste Profile to network infrastructure, or depending permanently on a Stumble-operated server. Agent Harnesses should continue complementing that network discovery by browsing the wider web under User-controlled Browser Grants, while browser-found Candidates retain a stricter review boundary than Pod-curated Exploration Items.

## Solution

Ship a sponsored but removable Bootstrap Node as the initial contact for a decentralized Stumble Substrate. Public Origin Nodes openly submit compact signed Pod Announcements; Bootstrap and Index Nodes verify them, publish a topic-neutral cursor-paginated Announcement Stream, and support explicit User-authored searches without becoming authoritative for Pods or ranking.

Home Nodes synchronize public metadata outbound, compute personalized Pod Similarity locally, and automatically maintain a small rotating set of capability-limited Discovery Peers. Opted-in reachable nodes serve unchanged Origin-signed announcements and bounded randomized peer samples, allowing discovery to continue after the sponsored node becomes unavailable. Pod Announcements use renewable leases and signed withdrawals so stale Pods disappear from new discovery.

Public-Pod content may receive small, labeled trial exposure through the existing Explore and Feed constraints. Fresh browser findings remain Candidates in finite Discovery Result Batches until the User acts. The legacy Hub implementation is removed completely, including its APIs, daemon, types, persistence, fixtures, and tests; its non-authoritative cache data is discarded during migration.

## User Stories

1. As a new User, I want Stumble to contact a sponsored Bootstrap Node automatically, so that I can discover public Pods without already knowing a Pod URL.
2. As a privacy-conscious User, I want the sponsored Bootstrap Node to be removable, so that it never becomes a permanent dependency.
3. As a User, I want to configure multiple Bootstrap Nodes, so that I can choose independently operated entry points.
4. As a User, I want direct Pod URLs to keep working without any Bootstrap or Index Node, so that decentralized addressing remains canonical.
5. As a public Pod Owner, I want to submit a signed Pod Announcement without prior operator approval, so that the sponsor cannot gate legitimate network participation.
6. As a public Pod Owner, I want a rejected announcement to include an inspectable reason, so that I can correct verification or policy failures.
7. As a Bootstrap operator, I want to verify Origin identity, signature, canonical Pod URL, current manifest, reachability, protocol version, and resource limits, so that admission does not amplify malformed or unreachable Pods.
8. As a Bootstrap operator, I want rate limits and per-Origin caps, so that one actor cannot dominate shared discovery resources.
9. As a Home Node User, I want a neutral stream of public announcements, so that personalized matching happens without disclosing my interests.
10. As a Home Node User, I want announcement synchronization to resume from a cursor, so that routine refreshes transfer only new or changed metadata.
11. As a Home Node User, I want invalid, stale, blocked, and expired announcements rejected locally, so that remote infrastructure cannot bypass my Trust Policy.
12. As a Home Node User, I want background discovery never to send Taste Profile data or inferred-interest queries, so that the discovery network cannot build a behavioral profile about me.
13. As a User performing an explicit search, I want a configured Index Node to search the words I deliberately entered, so that intentional Explore remains efficient.
14. As a User, I want Index-provided scores discarded and relevance recomputed locally, so that an Index cannot determine my ranking.
15. As a User, I want related Pods found through both signed curator Endorsements and semantic similarity, so that discovery works even when the endorsement graph is sparse.
16. As a User, I want a deterministic similarity baseline when no Agent Harness or local AI is available, so that passive discovery remains dependable.
17. As a User with an authorized Agent Harness, I want it to contribute richer local semantic evidence, so that discovery quality can improve without transferring authority to the agent.
18. As a User, I want an explanation for each related Pod or Exploration Item, so that I can inspect whether it matched by subject, source neighborhood, sample content, or endorsement.
19. As the Owner of a new unendorsed Pod, I want a bounded opportunity to reach locally interested Users, so that endorsements do not become an admission gate.
20. As a Home Node User, I want unknown Pods constrained by per-Pod and per-Origin exploration caps, so that open admission does not flood my Feed.
21. As a User, I want to block an Origin Node, Pod, source, or topic locally, so that my Trust Policy controls future discovery.
22. As a User, I want explicit feedback to adjust future local exposure, so that discovery improves without creating a global reputation profile.
23. As a public Pod Owner, I want announcements to renew periodically and when metadata changes, so that the network knows the Pod is current.
24. As a User, I want expired announcements excluded from new discovery and relaying, so that abandoned Pods do not remain visible forever.
25. As a public Pod Owner, I want to issue a signed Pod Withdrawal, so that a deleted or private Pod leaves discovery promptly.
26. As an existing subscriber, I want announcement expiry or withdrawal to preserve my synchronized content and Subscription, so that discovery lifecycle does not erase private local state.
27. As a Home Node User, I want Stumble to learn a small rotating set of Discovery Peers automatically, so that discovery can survive Bootstrap outages.
28. As a private Home Node User, I want all default network activity to be outbound, so that installing Stumble never exposes a public listener.
29. As a network participant, I want to opt into serving announcements explicitly, so that I control whether my node contributes bandwidth and availability.
30. As an opted-in participant, I want Stumble to verify my signed identity and endpoint reachability before advertising me, so that other nodes receive usable peer candidates.
31. As a Home Node User, I want Discovery Peers limited to public announcement and peer-advertisement exchange, so that automatic peering grants no access to Pod Events or private state.
32. As a Home Node User, I want abusive or repeatedly failing Discovery Peers evicted automatically, so that the rotating peer set remains useful.
33. As a network participant, I want peer samples to be bounded and randomized rather than globally ranked, so that peer discovery does not create another central hierarchy.
34. As an existing User, I want cached peers and announcements to remain usable during a sponsored-node outage, so that established nodes continue discovering.
35. As a brand-new User during a sponsored-node outage, I want direct Pod URLs and alternative configured bootstraps to remain available, so that initialization is degraded rather than locked to one operator.
36. As a User, I want a small, clearly labeled amount of accepted content from similar unsubscribed public Pods to appear under existing Feed exploration constraints, so that discovery feels useful without silently subscribing me.
37. As a User, I want browser-found content kept in a finite Discovery Result Batch until I act, so that uncurated web results do not silently enter my Feed.
38. As a User, I want saving, placing, reinforcing, or rejecting a browser result to remain explicit, so that only my evidence changes durable private learning.
39. As an Agent Harness operator, I want existing Browser Grant and minimized Discovery Plan boundaries preserved, so that wider-web discovery never exports credentials or unrestricted browsing authority.
40. As a sponsored-node operator, I want no User accounts or stable User identifiers for public discovery, so that the service does not accumulate unnecessary identity data.
41. As a sponsored-node operator, I want explicit searches processed without retained product analytics and only short-lived security logs, so that operational safety does not become behavioral surveillance.
42. As an independent Bootstrap or Index operator, I want to publish my own filtering and log-retention policy, so that operators remain replaceable and transparent.
43. As a Stumble maintainer, I want the legacy Hub contract removed completely, so that the codebase has one coherent discovery model.
44. As a Stumble maintainer, I want legacy Hub cache tables dropped without conversion, so that non-authoritative obsolete state does not complicate the new substrate.
45. As a Stumble integrator, I want well-known node metadata to advertise only current Bootstrap, Index, discovery-peer, and federation capabilities, so that clients do not depend on retired Hub endpoints.
46. As a subscriber, I want direct Origin Pod synchronization unchanged, so that discovery work does not destabilize signed Pod Event federation.
47. As a Stumble operator, I want Bootstrap, Index, and Discovery Peer capabilities independently configurable, so that one process may combine roles without conflating their authority.
48. As a Stumble operator, I want signed Pod Event relaying excluded from this milestone, so that discovery can ship without taking on content-cache freshness and storage obligations.
49. As a developer, I want network state and synchronization cursors to survive process restarts transactionally, so that discovery behaves consistently on SQLite-backed nodes.
50. As a developer, I want protocol failures to be typed and inspectable, so that operators and Agent Harnesses can distinguish invalid signatures, expiry, policy rejection, rate limiting, incompatibility, and network failure.

## Implementation Decisions

- Bootstrap Node, Index Node, Discovery Peer, Origin Node, Home Node, and Relay Node remain separate capabilities and authority boundaries even when one deployment combines several capabilities.
- The first sponsored deployment enables Bootstrap and Index capabilities. Signed Pod Event relaying is deferred.
- Bootstrap configuration is represented as a User-controlled list from the first release. The distribution initially contains one sponsored default.
- A Bootstrap Node accepts a public Pod Announcement without prior Origin approval when signature, identity, canonical URL, current public manifest, reachability, protocol compatibility, and resource bounds verify.
- Bootstrap admission establishes discoverability only. It does not endorse, trust, rank, or authorize the announced Pod.
- Announcement submission applies per-network and per-Origin rate limits, bounded storage and stream participation, canonical deduplication, and operator-local quarantine. It requires no account, payment, proof-of-work, or centralized identity.
- Sponsored admission policy and rejection reasons are inspectable. Filtering remains local to each Bootstrap or Index operator rather than becoming a network blocklist.
- A Pod Announcement gains a signed expiry and acts as a renewable 30-day Announcement Lease. Origins refresh it periodically and whenever its public metadata or latest event pointer changes.
- A new Origin-signed Pod Withdrawal removes the Pod from new discovery and relaying immediately. Expiry and withdrawal do not delete existing Subscriptions or synchronized content.
- Bootstrap and Index capabilities expose a topic-neutral cursor-paginated Announcement Stream of admitted new, changed, withdrawn, and expired public Pod state.
- A Home Node persists its cursor and verifies every Origin signature, lease, canonical URL, freshness transition, and local Trust Policy before updating its local projection.
- Background discovery uses only locally synchronized public metadata and never sends Taste Profile data, Subscriptions, feedback, Source Affinities, or interest-derived queries to remote infrastructure.
- Explicit User-authored Explore searches may query a configured Index Node. Remote relevance is treated only as retrieval evidence; local policy and ranking recompute the result order.
- Pod Similarity combines deterministic evidence from public subject text, Pod Context, source neighborhoods, bounded signed Explore samples, and signed Pod Endorsements.
- Local Node Agents or authorized Agent Harnesses may submit richer semantic evidence, but Core remains authoritative for policy, provenance, caps, blocks, and final eligibility.
- Similarity and exploration responses expose inspectable reasons rather than a universal opaque score.
- Unendorsed Pods remain eligible for tightly bounded, clearly labeled trial exposure when verification succeeds and local relevance is strong. Endorsements are optional evidence rather than a gate.
- Existing Feed Mix limits, per-Pod and per-source caps, Trust Policy blocks, and explicit Feedback Signals govern network Exploration Items.
- Browser-found content remains a Candidate in a finite Discovery Result Batch until explicit User action. Existing Browser Grants, Discovery Plans, task leases, provenance, and backpressure remain unchanged.
- A Discovery Peer relationship is automatically managed and grants only bounded exchange of signed public Pod Announcements and Discovery Peer Advertisements. It is not a Trusted Peer relationship.
- Ordinary Home Nodes make outbound discovery requests by default and never advertise or bind a public discovery service automatically.
- Serving Announcement Streams is explicit opt-in. An opted-in node publishes a signed expiring Discovery Peer Advertisement only after identity and endpoint reachability verification.
- Bootstrap Nodes and Discovery Peers return small randomized peer samples without global rank. Home Nodes maintain a bounded rotating outbound peer set and evict invalid, flooding, incompatible, or repeatedly unreachable peers.
- Known announcements, stream cursors, Bootstrap configuration, peer advertisements, peer health, withdrawals, and relevant provenance are authoritative local SQLite state and survive restart.
- The sponsored service uses no User account or stable User identifier for public discovery, retains no product analytics for explicit searches, and keeps only short-lived minimized security logs under a published configurable policy.
- The legacy Hub model is removed without compatibility aliases: routes, refresh daemon, command options, domain types, tools, synchronization behavior, endpoint metadata, fixtures, and tests all disappear.
- A forward database migration drops legacy Hub tables without transforming their contents because they contain non-authoritative caches. New substrate state is reacquired through configured Bootstrap Nodes and Discovery Peers.
- Existing direct Pod URL subscription, Origin-signed Pod Events, Placement Tombstones, Package federation, and Subscription synchronization contracts remain compatible.
- Public protocol responses are bounded, versioned, strict about unknown fields where existing conventions require it, and produce typed failures for invalid identity, signature, lease, cursor, policy, rate, protocol, and transport conditions.

## Testing Decisions

- The primary acceptance seam is a multi-node HTTP scenario backed by real temporary SQLite Home Nodes. Tests exercise public protocol behavior rather than calling internal storage or ranking helpers directly.
- The principal scenario starts an Origin, Bootstrap/Index, serving Discovery Peer, and fresh Home Node; publishes a public Pod; admits and cursor-synchronizes its announcement; obtains and filters signed Explore samples; explains local similarity; learns a peer; then proves continued announcement discovery while the Bootstrap is unavailable.
- The same seam verifies renewal, expiry, signed withdrawal, restart recovery, cursor idempotency, direct Pod URL fallback, replacement Bootstrap configuration, invalid signatures, unreachable Origins, incompatible versions, rate limits, local blocks, and automatic peer eviction.
- Existing Personal Discovery HTTP tests remain the prior art for the separate Agent Harness path. They continue proving that browser-originated Candidates require task-bound provenance and become finite Discovery Result Batches rather than Feed items.
- Existing discovery-substrate Core integration tests are prior art for cryptographic verification, Endorsements, Explore samples, Trust Policy filtering, and SQLite persistence. Focused Core tests should be added only for deterministic similarity scoring and policy combinations that would make HTTP acceptance tests needlessly indirect.
- Legacy-removal tests assert that retired Hub endpoints are absent, retired daemon options are rejected, well-known metadata contains no Hub capabilities, obsolete types and tools have no callers, and database migration removes Hub cache tables while preserving unrelated node and federation state.
- Tests assert observable invariants rather than exact internal scores: a relevant Pod is eligible, a blocked or expired Pod is not, reasons identify the evidence class, caps are respected, remote rank cannot override local policy, and unreviewed browser Candidates never enter Feed batches.
- Network tests use deterministic clocks, seeded random peer selection, and controllable transports so expiry, rotation, retry, and outage behavior are reliable without sleeping or depending on external services.

## Out of Scope

- Caching or serving signed Pod Events through Relay Nodes.
- Requiring ordinary Home Nodes to accept inbound connections.
- Blockchain, DHT, global consensus, universal reputation, proof-of-work, payment, or centralized publisher identity.
- A hosted recommendation or embedding service.
- Sending private Taste Profile data or background interest-derived queries to Bootstrap or Index Nodes.
- Automatically promoting browser-found Candidates into Feed exploration.
- Global moderation or distributed blocklists.
- Perfect Sybil resistance, anonymous transport, private information retrieval, or a full large-network catalog partitioning strategy.
- Changing direct Pod URL subscription or authoritative Origin synchronization semantics.
- Building a graphical client; the milestone exposes headless HTTP, CLI, and Agent Harness contracts consistent with existing architecture.

## Further Notes

- The architecture intentionally permits a single deployment to sponsor Bootstrap and Index capabilities while ensuring neither capability is required after a Home Node has direct addresses and viable Discovery Peers.
- A brand-new installation can still be temporarily unable to discover automatically if every configured Bootstrap Node is unavailable. Multiple bootstrap support and direct URLs make this an availability limitation rather than protocol authority.
- The neutral Announcement Stream is accepted as a first-release scalability trade-off. If the public catalog later becomes too large, privacy-preserving partitioning can be designed without changing the rule that passive personalization stays local.
- The sponsored node's operational deployment should publish its admission policy, security-log retention, availability expectations, and instructions for replacing or removing it.
