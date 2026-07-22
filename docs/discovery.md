# Stumble Substrate discovery

Public Origin Nodes advertise Pods with compact `PodAnnouncement` values signed by the node's Ed25519 identity. An announcement contains public Pod identity, subject, canonical direct Pod URL, Package version, latest Pod Event pointer, and a renewable **Announcement Lease**: `expires_at` is exactly `announced_at + 30 days` and is covered by the Origin signature. It does not contain Pod Events, Package contents, Candidates, Content Items, Subscriptions, or other private Home Node state. Full accepted content still synchronizes only after a direct Subscription.

Origins produce announcements through `pod_announcement` (also `pod_announcement_at` for deterministic clocks). Each call reflects current public metadata, Package version, and latest event pointer with a fresh id, issuance time, and lease, and retains the announcement on the Origin so later public-state changes can refresh it. Renewing a lease is the same entry point with a later issuance time. When public Pod metadata, Package version, or the latest federated event pointer changes on a public Origin Pod that already has a retained announcement, the Origin automatically re-issues and retains a refreshed announcement. Consumers retain announcements with deterministic preference: an active lease beats an expired one; among equal lease validity, later `announced_at` (then package version) wins. Invalid signatures, expired leases, and stale renewals are rejected without mutating local discovery state.

An Origin-signed **Pod Withdrawal** immediately ends new discovery and relaying for that Pod identity (`origin_node_id` + `pod_slug`). Withdrawals are produced by `withdraw_public_pod` or automatically when a public Pod is made private via `request_set_pod_visibility`. Expired leases and withdrawals exclude the Pod from `relay_pod_announcements`, Index search, and `explore_public_pods`, but never delete existing Subscriptions or previously synchronized content. A later Origin-signed announcement issued after a withdrawal may re-admit the Pod.

A Home Node may receive an unchanged signed announcement from an explicitly trusted peer, and `relay_pod_announcements` serves only currently eligible Origin signatures unchanged to another trusted peer. The immediate peer is recorded only as delivery provenance; verification still resolves to the Origin Node in the announcement. Disabling a peer through an independently approved Pending Proposal retains its audit state while immediately rejecting further discovery exchange. Optional Index Nodes use the same verification and lease contract to aggregate and search announcements. A Home Node accepts search results only from an Index configured in its Trust Policy, discards the Index's score, and recomputes relevance locally. Removing an Index immediately excludes results received only from it, so another Index can replace it. Direct Pod URLs continue to work without any Index Node.

Public HTTP contracts (typed machine-readable `code` on failures):

| Method | Path | Behavior |
|--------|------|----------|
| `POST` | `/discovery/announcements/produce` | Origin produces a signed announcement with a 30-day lease |
| `POST` | `/discovery/announcements` | Verify and index an announcement |
| `POST` | `/discovery/announcements/receive` | Receive a peer-delivered announcement |
| `GET`  | `/discovery/announcements` | Search currently eligible announcements |
| `POST` | `/discovery/withdrawals/produce` | Origin produces a withdrawal (optionally makes the Pod private) |
| `POST` | `/discovery/withdrawals` | Verify and index a withdrawal |
| `POST` | `/discovery/withdrawals/receive` | Receive a peer-delivered withdrawal |
| `POST` | `/bootstrap/announcements` | Open Bootstrap admission (no User account or Trusted Peer) |
| `GET`  | `/bootstrap/announcements/stream` | Topic-neutral cursor-paginated Announcement Stream |
| `POST` | `/bootstrap/withdrawals` | Open Bootstrap withdrawal admission |

Failure codes include `invalid_signature`, `announcement_expired`, `announcement_withdrawn`, `announcement_stale`, `withdrawal_stale`, and `validation_error`. Bootstrap open admission additionally returns stable codes: `malformed`, `invalid_identity`, `invalid_signature`, `unreachable_origin`, `incompatible_protocol`, `stale_lease`, `rate_limited`, `payload_too_large`, `manifest_mismatch`, `origin_quota_exceeded`, and `bootstrap_disabled`.

## Open Bootstrap admission and Announcement Streams

Bootstrap is an independent node capability (enabled via `AgentTools::with_bootstrap_capability`), not a Hub role and not an Origin proxy for private state. When enabled, a public Origin may submit a signed `PodAnnouncement` to `POST /bootstrap/announcements` without a User account or pre-existing Trusted Peer relationship.

Admission verifies, in order relevant to the failure: payload size bounds; Origin identity consistency; Ed25519 signature and lease integrity; protocol compatibility (`stumble/1.0`); canonical public Pod URL; per-network and per-Origin submission rate limits; Origin reachability and the current public manifest through an injectable `OriginProbe` port; and a per-Origin active-admission quota that bounds materially duplicate public Pods without inventing a global quality score. Canonically identical resubmissions (same announcement identity and payload) are idempotent and do not append a second stream effect. Preferable renewals of an already admitted Pod append a `renewed` stream entry.

Rejected attempts persist a minimal operator audit (`BootstrapRejectionAudit`) with reason, optional Origin key material, Pod slug, and timestamp only—no User identifiers, Taste Profiles, or product analytics.

The **Announcement Stream** at `GET /bootstrap/announcements/stream?cursor=&limit=` is topic-neutral and cursor-paginated (bounded page size). It emits lifecycle entries `admitted`, `renewed`, `withdrawn`, and `expired`. Empty cursor starts at the beginning; numeric cursors resume strictly after the last consumed sequence without gaps or duplicate effects across SQLite restart; unknown or future positions are rejected safely as `malformed`. Serving a page advances expiry transitions for leases that are no longer active under the injectable clock. Open withdrawal admission is `POST /bootstrap/withdrawals`.

Well-known node metadata advertises `bootstrap_announcements`, `bootstrap_announcement_stream`, and `bootstrap_withdrawals` only when Bootstrap is enabled. The public protocol never carries Taste Profile, Subscription, feedback, popularity, endorsement-derived authority, or personalized ranking data.

Admission, stream entries, rejection audits, leases, and rate-limit bookkeeping persist transactionally in the authoritative SQLite store (`known_pod_announcements`, `known_pod_withdrawals`, `announcement_stream_entries`, `bootstrap_rejection_audits`, `bootstrap_runtime`). Focused temporary-SQLite coverage is in `crates/stumble-core/tests/bootstrap_admission_stream.rs` and `crates/stumble-api/tests/bootstrap_admission_stream.rs`.

Changes to trusted peers, configured Index Nodes, and local Pod, node, source, or topic blocks pass through Pending Proposal approval. `explore_public_pods` applies that User-owned Trust Policy and does not create a Subscription. For an unsubscribed remote Pod, the Origin may separately produce a bounded signed `PodExploreSamples` artifact; the Home Node accepts it only for the exact current announcement and filters its Content References locally. Signed Pod Endorsements likewise bind the exact known current announcements of both public Pods before adding bounded, inspectable local ranking evidence. Neither signatures nor endorsements establish a global quality or reputation score.

Announcement lease and withdrawal state persist in the authoritative SQLite store (`known_pod_announcements`, `known_pod_withdrawals`). The focused temporary-SQLite acceptance coverage is in `crates/stumble-core/tests/discovery_substrate.rs` and `crates/stumble-api/tests/discovery_announcements.rs`. Direct outbound addressing remains covered in `crates/stumble-cli/tests/direct_subscription.rs`.

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
