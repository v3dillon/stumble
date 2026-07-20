# Stumble Substrate discovery

Public Origin Nodes advertise Pods with compact `PodAnnouncement` values signed by the node's Ed25519 identity. An announcement contains public Pod identity, subject, canonical direct Pod URL, Package version, and latest Pod Event pointer. It does not contain Pod Events, Package contents, Candidates, Content Items, Subscriptions, or other private Home Node state. Full accepted content still synchronizes only after a direct Subscription.

A Home Node may receive an unchanged signed announcement from an explicitly trusted peer, and `relay_pod_announcements` serves retained Origin signatures unchanged to another trusted peer. The immediate peer is recorded only as delivery provenance; verification still resolves to the Origin Node in the announcement. Disabling a peer through an independently approved Pending Proposal retains its audit state while immediately rejecting further discovery exchange. Optional Index Nodes use the same verification contract to aggregate and search announcements. A Home Node accepts search results only from an Index configured in its Trust Policy, discards the Index's score, and recomputes relevance locally. Removing an Index immediately excludes results received only from it, so another Index can replace it. Direct Pod URLs continue to work without any Index Node.

Changes to trusted peers, configured Index Nodes, and local Pod, node, source, or topic blocks pass through Pending Proposal approval. `explore_public_pods` applies that User-owned Trust Policy and does not create a Subscription. For an unsubscribed remote Pod, the Origin may separately produce a bounded signed `PodExploreSamples` artifact; the Home Node accepts it only for the exact current announcement and filters its Content References locally. Signed Pod Endorsements likewise bind the exact known current announcements of both public Pods before adding bounded, inspectable local ranking evidence. Neither signatures nor endorsements establish a global quality or reputation score.

The focused temporary-SQLite acceptance coverage is in `crates/stumble-core/tests/discovery_substrate.rs`. Direct outbound addressing remains covered in `crates/stumble-cli/tests/direct_subscription.rs`.

## Personal Discovery

Personal Discovery is User-scoped work governed by a private Discovery Plan. It
does not require a Pod, does not modify Pod Packages or Source Rules, and never
federates Interest Seeds, Source Affinities, plans, schedules, result batches,
or result reactions.

The Home Node builds each plan from explicit Taste Profile settings, corroborated
User evidence (Interest Seeds and Feedback Signals), Source Affinities, blocks,
recent result history, optional schedule intent, Browser Grant eligibility, and
locally matched Discovery Leads from verified public Stumble metadata. Matching
is local; autonomous planning must not issue profile-derived queries to a remote
Index Node. Default batches contain ten results with a 70/30 proven-to-adjacent
allocation and diversity caps (three per domain; two per author, publisher, or
community), plus canonical deduplication.

Workers receive only the minimized plan for a claimed task. They submit
provenance-bearing Candidates, report source availability, and complete one
finite Discovery Result Batch. Explicit User feedback (Save, Add to Pod, More
like this, Not for me) updates private learning; ignore and batch dismiss do not.
Agent-found content never trains the Taste Profile by itself.

Multiple named private schedules share the same Discovery Task contract whether
a harness wakes itself or the local Scheduler Adapter wakes workers. Each
schedule enforces cold-start dormancy and one-unreviewed-batch backpressure;
on-demand runs remain available. Results-ready notification is one-shot and does
not mark a batch reviewed.

### Authenticated sources

Personal Discovery may plan authenticated source neighborhoods, but authentication
remains harness-owned. The Agent Harness reports privacy-safe availability facts
and Browser Grant eligibility; Stumble stores those facts privately, never
credentials. Scheduled Personal Discovery skips unavailable authenticated sources
and reallocates within plan policy without waiting for login. On-demand runs may
emit a one-shot authentication-needed notice while continuing accessible work.

### Operator surfaces

HTTP, MCP, and `stumble discover personal …` expose equivalent domain contracts
for readiness, request, plans, batches, reviews, schedules, availability, and
notifications. See `docs/first-release.md` for the Agent Harness skill loop,
grants, schedules, privacy, and recovery after restart. Authoritative decisions:
ADR-0035, ADR-0036, ADR-0017, ADR-0025, ADR-0012.
