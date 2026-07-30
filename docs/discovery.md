# Stumble Substrate discovery

Public Origin Nodes advertise Pods with compact `PodAnnouncement` values signed by the node's Ed25519 identity. An announcement contains public Pod identity, subject, canonical direct Pod URL, Package version, latest Pod Event pointer, and a renewable **Announcement Lease**: `expires_at` is exactly `announced_at + 30 days` and is covered by the Origin signature. It does not contain Pod Events, Package contents, Candidates, Content Items, Subscriptions, or other private Home Node state. Full accepted content still synchronizes only after a direct Subscription.

Origins produce announcements through `pod_announcement` (also `pod_announcement_at` for deterministic clocks). Each call reflects current public metadata, Package version, and latest event pointer with a fresh id, issuance time, and lease, and retains the announcement on the Origin so later public-state changes can refresh it. Renewing a lease is the same entry point with a later issuance time. When public Pod metadata, Package version, or the latest federated event pointer changes on a public Origin Pod that already has a retained announcement, the Origin automatically re-issues and retains a refreshed announcement. Consumers retain announcements with deterministic preference: an active lease beats an expired one; among equal lease validity, later `announced_at` (then package version) wins. Invalid signatures, expired leases, and stale renewals are rejected without mutating local discovery state.

An Origin-signed **Pod Withdrawal** immediately ends new discovery for that Pod identity (`origin_node_id` + `pod_slug`). Withdrawals are produced by `withdraw_public_pod` or automatically when a public Pod is made private via `request_set_pod_visibility`. Expired leases and withdrawals exclude the Pod from Index search and `explore_public_pods`, but never delete existing Subscriptions or previously synchronized content. A later Origin-signed announcement issued after a withdrawal may re-admit the Pod.

A Home Node may receive an unchanged signed announcement from an explicitly trusted peer. The immediate peer is recorded only as delivery provenance; verification still resolves to the Origin Node in the announcement. Disabling a peer through an independently approved Pending Proposal retains its audit state while immediately rejecting further discovery exchange. Optional Index Nodes use the same verification and lease contract to aggregate and search announcements. A Home Node accepts search results only from an Index configured in its Trust Policy, discards the Index's score, and recomputes relevance locally. Removing an Index immediately excludes results received only from it, so another Index can replace it. Direct Pod URLs continue to work without any Index Node.

Public HTTP contracts (typed machine-readable `code` on failures):

| Method | Path | Behavior |
|--------|------|----------|
| `POST` | `/discovery/announcements/produce` | Origin produces a signed announcement with a 30-day lease |
| `POST` | `/discovery/announcements` | Verify and index an announcement |
| `POST` | `/discovery/announcements/receive` | Receive a peer-delivered announcement |
| `GET`  | `/discovery/announcements` | Index search of currently eligible announcements (`q`, `limit`; no User id) |
| `POST` | `/discovery/withdrawals/produce` | Origin produces a withdrawal (optionally makes the Pod private) |
| `POST` | `/discovery/withdrawals` | Verify and index a withdrawal |
| `POST` | `/discovery/withdrawals/receive` | Receive a peer-delivered withdrawal |
| `POST` | `/bootstrap/announcements` | Open Bootstrap admission (no User account or Trusted Peer) |
| `GET`  | `/bootstrap/announcements/stream` | Topic-neutral cursor-paginated Announcement Stream |
| `POST` | `/bootstrap/withdrawals` | Open Bootstrap withdrawal admission |
| `POST` | `/bootstrap/peer-advertisements` | Open Bootstrap admission of Discovery Peer Advertisements |
| `GET`  | `/bootstrap/peer-advertisements` | Bootstrap-open unranked sample of admitted peer advertisements |
| `GET`  | `/discovery/peer/announcements/stream` | Opt-in Discovery Peer Announcement Stream pages |
| `GET`  | `/discovery/peer/advertisements` | Opt-in randomized unranked peer-advertisement samples |

Node operations live in the CLI: `stumble sync discovery serve show|enable|disable` (inbound serving), `stumble sync discovery peers|gossip|run` (outbound peer set), and `stumble sync discovery status` (readiness including Bootstrap-outage degraded mode).

Failure codes include `invalid_signature`, `announcement_expired`, `announcement_withdrawn`, `announcement_stale`, `withdrawal_stale`, and `validation_error`. Bootstrap open admission additionally returns stable codes: `malformed`, `invalid_identity`, `invalid_signature`, `unreachable_origin`, `incompatible_protocol`, `stale_lease`, `rate_limited`, `payload_too_large`, `manifest_mismatch`, `origin_quota_exceeded`, and `bootstrap_disabled`. Public Index search returns stable codes: `malformed`, `query_too_large`, `rate_limited`, `incompatible_protocol`, `index_disabled`, `transport`, and `protocol`.

## Open Bootstrap admission and Announcement Streams

Bootstrap is an independent node capability (enabled via `AgentTools::with_bootstrap_capability`), not an Origin proxy for private state. When enabled, a public Origin may submit a signed `PodAnnouncement` to `POST /bootstrap/announcements` without a User account or pre-existing Trusted Peer relationship.

Admission verifies, in order relevant to the failure: payload size bounds; Origin identity consistency; Ed25519 signature and lease integrity; protocol compatibility (`stumble/1.0`); canonical public Pod URL; per-network and per-Origin submission rate limits; Origin reachability and the current public manifest through an injectable `OriginProbe` port; and a per-Origin active-admission quota that bounds materially duplicate public Pods without inventing a global quality score. Canonically identical resubmissions (same announcement identity and payload) are idempotent and do not append a second stream effect. Preferable renewals of an already admitted Pod append a `renewed` stream entry.

Rejected attempts persist a minimal operator audit (`BootstrapRejectionAudit`) with reason, optional Origin key material, Pod slug, and timestamp only—no User identifiers, Taste Profiles, or product analytics.

The **Announcement Stream** at `GET /bootstrap/announcements/stream?cursor=&limit=` is topic-neutral and cursor-paginated (bounded page size). It emits lifecycle entries `admitted`, `renewed`, `withdrawn`, and `expired`. Empty cursor starts at the beginning; numeric cursors resume strictly after the last consumed sequence without gaps or duplicate effects across SQLite restart; unknown or future positions are rejected safely as `malformed`. Serving a page advances expiry transitions for leases that are no longer active under the injectable clock. Open withdrawal admission is `POST /bootstrap/withdrawals`.

Well-known node metadata advertises `bootstrap_announcements`, `bootstrap_announcement_stream`, and `bootstrap_withdrawals` only when Bootstrap is enabled. The public protocol never carries Taste Profile, Subscription, feedback, popularity, endorsement-derived authority, or personalized ranking data.

Admission, stream entries, rejection audits, leases, and rate-limit bookkeeping persist transactionally in the authoritative SQLite store (`known_pod_announcements`, `known_pod_withdrawals`, `announcement_stream_entries`, `bootstrap_rejection_audits`, `bootstrap_runtime`). Focused temporary-SQLite coverage is in `crates/stumble-core/tests/bootstrap_admission_stream.rs` and `crates/stumble-api/tests/bootstrap_admission_stream.rs`.

## Home Node outbound Bootstrap configuration and sync

A newly initialized Home Node receives the sponsored Bootstrap base URL (`DEFAULT_SPONSORED_BOOTSTRAP_URL`, currently `https://bootstrap.stumble.network`) as an **ordinary removable** list entry (`enabled: true`, `is_sponsored_default: true`). The URL is configuration, not a protocol constant or authority for Pods, trust, or ranking.

Bootstrap configuration is an ordered User-controlled list stored in SQLite (`bootstrap_endpoints`, `bootstrap_sync_states`). Operators may add, disable, enable, remove, and inspect multiple endpoints. Each endpoint persists its own Announcement Stream cursor, last success time, last attempt, and typed failure. Removing an endpoint drops its sync row; announcements remain as audit state. Eligibility requires at least one remaining active delivery source (enabled Bootstrap URL in provenance, configured Index URL, peer delivery, or local/origin retain with no remote sources). Sole-source announcements from a removed or disabled Bootstrap leave Explore eligibility while independently learned copies stay usable.

Outbound sync walks enabled endpoints in order, `GET {base}/bootstrap/announcements/stream?cursor=&limit=`, verifies each entry locally via `retain_verified_*`, records `received_from_bootstrap_urls` provenance, and advances the per-endpoint cursor on success. Transport or protocol failure records a typed `BootstrapSyncFailure` and falls through to the next endpoint without discarding previously verified announcements. Outbound requests carry **only** cursor pagination fields—never Taste Profile, Subscriptions, feedback, Source Affinity, or interest-derived queries.

Direct Pod URL validation and Subscription continue with every Bootstrap disabled or unavailable. Operator surfaces:

| Surface | Behavior |
|---------|----------|
| `stumble sync bootstrap list\|status\|add\|disable\|enable\|remove` | Inspect/mutate config and report cursor + typed failure |
| `stumble sync bootstrap run` | Outbound multi-endpoint stream synchronization |

Focused temporary-SQLite coverage is in `crates/stumble-core/tests/bootstrap_home_node_sync.rs` and unit tests under `bootstrap::client`.

## Replaceable private Index search

Index is an independent node capability (`AgentTools::with_index_capability`), optionally co-located with Bootstrap. When enabled, `GET /discovery/announcements?q=&limit=` searches the node's admitted valid announcement catalog for an **explicit bounded query**. The public search API requires no User account or stable User identifier. Responses contain Origin-signed announcements plus retrieval-only relevance and reasons—never quality, trust, popularity, or personalized authority fields. Processing retains no product analytics: only short-lived rate-limit timestamps in `index_runtime` (no query text).

Query bounds: max UTF-8 length `MAX_INDEX_QUERY_BYTES` (256); limit `1..=MAX_INDEX_SEARCH_LIMIT` (50). Empty queries return a bounded catalog listing. Oversized, malformed, rate-limited, disabled, and incompatible outcomes are typed (`IndexSearchFailure` / wire `code`). Well-known metadata advertises `index_search_announcements` only when Index is enabled.

A Home Node may call configured Trust Policy Index Nodes **only** from an explicit User-authored Explore action (`explore_public_pods_with_indexes` / `import_explicit_index_search`). Outbound requests use the injectable `IndexSearchClient` port and carry only `IndexSearchRequest` fields (`query` + `limit`)—never Taste Profile, Subscriptions, feedback, Source Affinity, or Discovery Plan inference. Empty Explore queries stay local and do not fan out. Personal Discovery planning has no Index client parameter.

Import verifies each announcement, records multi-source `received_from_index_urls` provenance (accumulating Index base URLs across retains of the same signed announcement), discards remote ordering/scores, and recomputes eligibility and order locally via Trust Policy and `explore_public_pods` Pod Similarity. Delivery stays active when any recorded Index URL remains in Trust Policy. Multiple Indexes are supported in configuration order with fallthrough on transport failure. Removing an Index excludes sole-source results while independent copies (other Index, Bootstrap, peer, or local retain) remain eligible. Provenance and rate-limit timestamps survive SQLite restart. Short intentional Index query tokens (`ai`, `go`, `web`, …) are searchable; Index search does not apply Personal Discovery stop/short-token filtering.

Domain contract: `ExploreRequest` / `ExploreResponse` for intentional Explore (CLI `stumble pod explore` and Agent Harness tools share the same Core types). Focused coverage: unit tests in `crates/stumble-core/src/index/`, temporary-SQLite acceptance in `crates/stumble-core/tests/index_search.rs`, and Index aggregation in `crates/stumble-core/tests/discovery_substrate.rs`.

Changes to trusted peers, configured Index Nodes, and local Pod, node, source, or topic blocks pass through Pending Proposal approval. `explore_public_pods` applies that User-owned Trust Policy and does not create a Subscription. For an unsubscribed remote Pod, the Origin may separately produce a bounded signed `PodExploreSamples` artifact; the Home Node accepts it only for the exact current announcement and filters its Content References locally. Signed Pod Endorsements likewise bind the exact known current announcements of both public Pods before adding bounded, inspectable local ranking evidence. Neither signatures nor endorsements establish a global quality or reputation score.

## Local Pod Similarity and trial exposure

Home Nodes compute **deterministic Pod Similarity** in-process from verified public subject text, optional local Pod Context (`CONTEXT.md`), source neighborhoods on Explore samples, sample titles/tags, and valid Pod Endorsements (`crates/stumble-core/src/pod_similarity/`). Scoring never requires an Agent Harness or model service and never issues background interest-derived remote queries; private Taste Profile interests and Source Affinities stay on the Home Node as ranking inputs only.

`explore_public_pods` ranks known announcements with that pure scorer (`pod_similarity/` module). Results expose inspectable reason strings keyed by evidence class (`subject`, `source`, `sample`, `endorsement`, and optional local `agent`). Endorsements strengthen only when base similarity is already positive and are never treated as transferable trust. A strongly relevant unendorsed Pod with real retained Origin-signed samples can receive limited labeled **trial exposure** (`trial_exposure: bool` is the sole signal; DTO surfaces append a trial reason once at the boundary). Trust Policy Pod/Origin/source/topic blocks exclude candidates before ranking. Per-Origin caps (`MAX_RESULTS_PER_ORIGIN`) join existing Feed Mix per-Pod and per-source caps so open admission cannot flood Explore or Feed exploration. Feed Exploration Items score only against verified current announcements (no synthetic announcements), reuse the same endorsement collection and local-context builders as Explore, and enforce per-Origin trial caps via the typed `trial_exposure` flag after Feed Mix composition.

### Local agent semantic evidence

A narrowly scoped harness capability (`pod_similarity_evidence`) may submit **bounded, confidence-scored, evidence-backed semantic relationships** between two exact current Pod Announcements via `AgentTools::submit_pod_similarity_agent_evidence` (`pod_similarity/agent_evidence.rs`). Submissions identify the public announcement inputs used and are rejected when either announcement is stale, withdrawn, expired, blocked, mismatched, or unverifiable. Duplicate submissions are idempotent by harness idempotency key and bounded by Pod pair, model/harness provenance, and freshness (default 24h, max 7 days).

Agent evidence is private Home Node state (`pod_similarity_agent_evidence` store collection with harness write audit). It can adjust local Explore ordering and produce inspectable `agent evidence:` reasons, but **cannot** create trust, Subscription, Accepted Placement, or Feed eligibility by itself: Core layers it only onto a positive deterministic base score, then applies blocks and exploration caps. Revoking the Harness Grant immediately rejects new submissions and excludes that grant’s evidence from ranking. Agent evidence never leaves the Home Node as an Endorsement, global score, announcement field, or remote interest query. Without agent evidence or an active harness, deterministic Pod Similarity matches the pre-agent baseline.

Bounded Explore samples are retrieved from the canonical Origin through the injectable `OriginExploreSampleClient` port (`fetch_origin_explore_samples`). Acceptance requires Origin signature verification and binding to the exact current announcement (`verify_explore_samples_for_announcement` / `accept_pod_explore_samples`). Tests assert outbound sample requests are public-only (`sample_request_is_public_only`) and that pure Explore ranking does not call the Origin client. Explicit Feedback Signals continue to adjust private learning and therefore future local exposure; dismiss/ignore alone do not create durable preference (`feedback_affects_future_exposure`).

Focused coverage: unit tests in `pod_similarity` (including `agent_evidence`) and temporary-SQLite integration tests in `crates/stumble-core/tests/discovery_substrate.rs` (sample fetch, trial exposure, caps, blocks, local-only ranking, agent evidence, revocation, restart).

## Opt-in Discovery Peer service

Ordinary Home Nodes remain **outbound-only** for discovery by default (ADR-0044): they do not bind, advertise, or accept an inbound discovery service, and well-known metadata omits peer-serving endpoints. An authorized User enables announcement serving through operator surfaces (`stumble sync discovery serve enable|disable`, backed by `AgentTools::enable_discovery_peer_service` / `disable_discovery_peer_service`).

Enabling requires a declared public endpoint and successful verification of node identity, protocol compatibility (`stumble/1.0`), HTTPS policy outside loopback (HTTP allowed only on loopback), and external reachability via the injectable `DiscoveryPeerProbe` port. Private/reserved literal IP hosts are rejected. A verified node produces a signed, renewable **Discovery Peer Advertisement** (7-day lease) containing only identity, endpoint, protocol version, `announcement_serving` capability, and expiry—never private state or rank assertions.

Bootstrap-capable nodes openly admit peer advertisements at `POST /bootstrap/peer-advertisements` after the same identity/signature/lease/protocol/endpoint/reachability checks and reject forged, stale, incompatible, private, insecure, or unreachable advertisements. Admitted ads are retained for unranked sampling.

An enabled Discovery Peer serves:

| Method | Path | Behavior |
|--------|------|----------|
| `GET` | `/discovery/peer/announcements/stream` | Bounded Announcement Stream pages; Origin announcement bytes and signatures unchanged |
| `GET` | `/discovery/peer/advertisements` | Small randomized bounded samples of current peer advertisements (no rank/trust) |
| `POST` | `/bootstrap/peer-advertisements` | Open Bootstrap admission of peer advertisements (Bootstrap capability) |

Opt-in serving is inspected and toggled locally with `stumble sync discovery serve show|enable|disable`.

Peer endpoints expose no Pod Events, Subscriptions, Taste Profiles, feedback, credentials, private projections, or administrative capability. Disabling service clears the renewable advertisement and stops inbound serving without affecting outbound Bootstrap configuration, Index Explore, or direct Pod synchronization. Opt-in state, advertisement lease, and peer serving stream sequence high-water (`next_stream_sequence`) persist in SQLite (`discovery_peer_service`, `known_discovery_peer_advertisements`, peer-local `discovery_peer_stream_entries`). Peer and Bootstrap streams use independent sequence allocators so combined-role nodes never overwrite each other's stream entries. Peer advertisement samples use server entropy (no client-supplied seed). Bootstrap peer-ad admission applies rate limits and identity-bound reachability probes.

Domain module: `crates/stumble-core/src/discovery_peer/`. Focused coverage: unit tests in that module and temporary-SQLite acceptance in `crates/stumble-core/tests/discovery_peer_service.rs`.

## Outbound Discovery Peer rotation and Bootstrap outages

Home Nodes automatically maintain a **small rotating outbound Discovery Peer set** (default 4, hard max 8) learned from Bootstrap peer-advertisement samples (`GET /bootstrap/peer-advertisements`) and existing peer samples (`GET /discovery/peer/advertisements`). Learning verifies identity, `announcement_serving` capability, protocol version (`stumble/1.0`), public endpoint policy, renewable lease, and signature locally. Reachability via the injectable `DiscoveryPeerProbe` is **optional** for outbound learning (production learn path uses signed-ad verification alone; live reachability remains required when enabling a node as a Discovery Peer). Sample fetches run outside the store write lock; verify/retain/select run under a short write. Multi-source sample provenance accumulates on known advertisements (`learned_from`) and is copied into the outbound set at selection. A fresh verified learn **un-evicts** a previously transport-evicted peer so it may re-enter the outbound set. Selection is bounded and randomized under an injectable seed for deterministic tests; it **never** creates a Trusted Peer relationship or grants Pod Event / private / administrative access.

Each selected peer persists a stream cursor, sample provenance (`learned_from`), health (`healthy` / `backed_off` / `evicted`), consecutive failure count, backoff deadline, and last-success time (`outbound_discovery_peers`, `discovery_peer_sync_states`). Outbound peer sync fetches `{endpoint}/discovery/peer/announcements/stream?cursor=&limit=` outside the store write lock, then applies Origin-signed lifecycle entries only (`admitted` / `renewed` / `withdrawn` / `expired`) through `retain_verified_*` with multi-source `received_from_discovery_peer_endpoints` provenance. Requests carry only public pagination fields.

Invalid signatures, flooding (too many invalid entries on one page), incompatible protocol, expired advertisements, or repeated transport failures cause bounded exponential backoff and automatic local eviction. Evicting a peer excludes sole-source announcements from Explore eligibility while independently learned copies (other peer, Bootstrap, Index, or local retain) stay usable; audit rows remain.

When every configured Bootstrap is unavailable, an established Home Node continues receiving new announcements through viable outbound peers. A fresh node with no viable Bootstrap reports **degraded discovery** (`stumble sync discovery status`) while direct Pod URL subscription continues. Users may disable automatic peer gossip (`stumble sync discovery gossip --enabled false`) without deleting cached advertisements, outbound audit state, cursors, Bootstrap config, or direct-address paths. Rotation, eviction, cursor resume, and outage behavior survive SQLite restart; network I/O never holds the store write lock across awaits.

| Surface | Behavior |
|---------|----------|
| `GET /bootstrap/peer-advertisements` | Bootstrap-open unranked sample of admitted peer advertisements |
| `stumble sync discovery peers` | Outbound set with cursor, health, last-success |
| `stumble sync discovery gossip --enabled <bool>` | Enable/disable automatic peer gossip (audit retained) |
| `stumble sync discovery run [--learn] [--no-sync]` | Learn samples and/or sync outbound peer streams |
| `stumble sync discovery status` | Degraded discovery readiness (Bootstrap outage messaging) |

Domain module: `crates/stumble-core/src/discovery_peer/client.rs`. Focused coverage: unit tests in that module and temporary-SQLite acceptance in `crates/stumble-core/tests/discovery_peer_rotation.rs`.

Announcement lease and withdrawal state persist in the authoritative SQLite store (`known_pod_announcements`, `known_pod_withdrawals`). The focused temporary-SQLite acceptance coverage is in `crates/stumble-core/tests/discovery_substrate.rs` and `crates/stumble-api/tests/discovery_announcements.rs`. Direct outbound addressing remains covered in `crates/stumble-cli/tests/direct_subscription.rs`.

## Sponsored multi-node acceptance and operator docs

The milestone proof is a deterministic multi-node HTTP scenario against real temporary SQLite stores: separate Origin, sponsored Bootstrap/Index, Discovery Peer, and fresh Home Node, exercising public contracts for publish, open admit, cursor-sync, local match/explain/preview, subscribe, sponsor outage with peer delivery, renewal/expiry/withdrawal, malformed signature and protocol rejection, local Trust Policy blocks over Index scores, peer eviction, restart/cursor idempotency, direct Pod URL fallback, unendorsed trial exposure, browser Candidates vs Feed, and independent Bootstrap/Index enablement without Relay.

```bash
cargo test -p stumble-api --test sponsored_deployment_acceptance
```

User guide (sponsored default, multi Bootstrap, outbound-only, serving opt-in, direct address, sponsor outages): [`docs/sponsored-bootstrap-users.md`](sponsored-bootstrap-users.md).

Operator runbook (open admission, rejection codes, rate limits, no-account, security logs, retention, no analytics/ranking): [`docs/sponsored-bootstrap-operators.md`](sponsored-bootstrap-operators.md).

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
