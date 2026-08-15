# Operating a Bootstrap / Index Node

This runbook covers operators who run open Bootstrap admission, Announcement Streams, optional Index search, and the optional signed Pod Event Relay—whether that is the sponsored first deployment or an independent replacement. Bootstrap, Index, and Relay are **independent capabilities**: enable Relay with `--relay`, and never advertise a role whose flag is off. The Relay stores and serves Origin-signed snapshots unchanged; it never re-signs, never becomes the Origin, and never holds Home Node private state (ADR-0055).

## Open admission verification

`POST /bootstrap/announcements` accepts a signed `PodAnnouncement` **without** a User account, API token, payment, proof-of-work, or prior Trusted Peer relationship.

Admission verifies, in order relevant to failures:

1. Bootstrap capability enabled on this process (`bootstrap_disabled` otherwise)
2. Payload size bounds
3. Origin identity consistency
4. Protocol compatibility (`stumble/1.0`)
5. Idempotent replay of an already-admitted announcement (short-circuits before any limits)
6. Per-network and per-Origin submission rate limits
7. Origin reachability and current public manifest (via the injectable `OriginProbe` port), including manifest agreement with the signed announcement
8. Per-Origin active-admission quota (bounds material duplicates; not a quality score)
9. Ed25519 signature, Announcement Lease integrity, canonical public Pod URL shape (`/federation/pods/<slug>`; HTTPS, or loopback HTTP for lab use), and covering-withdrawal checks—the single retain/verification primitive runs **last**

Because signature, lease, and URL-shape verification run last, an unsigned, expired, or malformed payload from a rate-limited, over-quota, or unreachable Origin surfaces as `rate_limited`, `origin_quota_exceeded`, or `unreachable_origin`—not `invalid_signature`, `stale_lease`, or `malformed`.

Idempotent resubmission requires full canonical equality with the already-admitted announcement—not merely the same announcement identity—and returns `idempotent` without a second stream effect. A same-identity payload with any changed field falls through to renewal or rejection. Preferable renewals append a `renewed` stream entry.

Open withdrawal admission: `POST /bootstrap/withdrawals`.  
Open Discovery Peer Advertisement admission: `POST /bootstrap/peer-advertisements`.

## Narrow moderation

Admission establishes **discoverability only**. It does not endorse, trust, rank, authorize, or globally block Pods. Operator filtering is **local to each Bootstrap/Index instance**—never a network-wide blocklist or reputation system.

When you reject or quarantine:

- Persist only a minimal `BootstrapRejectionAudit` (reason, optional Origin key material, Pod slug, timestamp)
- Never store User identifiers, Taste Profiles, Subscriptions, feedback, or product analytics
- Publish your filtering policy so operators remain replaceable and transparent

## Rejection reasons (stable wire codes)

Public responses use typed `code` values:

| Code | Meaning |
|------|---------|
| `malformed` | Structural or canonical-field validation failed |
| `invalid_identity` | Origin identity fields inconsistent or unusable |
| `invalid_signature` | Signature verification failed |
| `unreachable_origin` | Origin endpoint not reachable |
| `manifest_unavailable` | Origin reachable but no usable public manifest |
| `incompatible_protocol` | Protocol version not supported |
| `stale_lease` | Announcement Lease expired or not current |
| `announcement_withdrawn` | Covering withdrawal already ends discovery |
| `rate_limited` | Per-network or per-Origin rate limit exceeded |
| `payload_too_large` | Signed payload exceeds admission bounds |
| `manifest_mismatch` | Live manifest disagrees with signed announcement |
| `origin_quota_exceeded` | Origin at active-admission quota |
| `bootstrap_disabled` | This process has Bootstrap capability off |

Index search failures use: `malformed`, `query_too_large`, `rate_limited`, `incompatible_protocol`, `index_disabled`, `transport`, `protocol`.

## Rate limits

- Per-network and per-Origin submission windows bound open admission
- Per-Origin active-admission quotas bound catalog size without inventing quality scores
- Index search rate-limit bookkeeping stores **timestamps only** in `index_runtime`—never query text
- Responses return `429` / `rate_limited` with inspectable reasons when limits apply

Exact windows and quotas are implementation constants in Core (`bootstrap` / `index` modules); publish the values you deploy so Origins can plan renewals.

## No-account behavior

Public discovery surfaces require **no User account** and **no stable User identifier**:

- Bootstrap admit / stream / withdrawals / peer-ad samples
- Index search (`GET /discovery/announcements?q=&limit=`)

Do not introduce login, OAuth, or persistent client IDs for these paths. Home Nodes perform their own node operations locally through the CLI (`stumble sync ...`).

## Minimized security logging

Retain only short-lived operational security logs required for abuse response, for example:

- Timestamp
- Rejection reason code
- Optional Origin public key fingerprint / Pod slug
- Rate-limit counters

Do **not** log:

- Taste Profiles, interests, or feedback
- Subscriptions or private Home Node state
- Full request bodies beyond what verification already stores in rejection audits
- Product analytics events for explicit searches

## Configurable retention

Publish and honor a retention policy for:

| Data | Guidance |
|------|----------|
| Rejection audits | Short window; delete after operational need ends |
| Rate-limit timestamps | Rolling window only |
| Admitted announcements / stream entries | Bound by lease, withdrawal, and storage quotas; not permanent marketing history |
| Security logs | Configurable; default short-lived |

Document retention in your deployment notes so alternative operators can match or improve on it.

## No product analytics or global ranking

- Explicit Index searches must not feed product analytics pipelines
- Bootstrap and Index responses carry retrieval evidence only—**never** quality, trust, popularity, or personalized authority fields
- Peer advertisement samples are **unranked and randomized** (server entropy; no client-supplied seed on production sample paths)
- Home Nodes discard remote scores and recompute eligibility under local Trust Policy

## Independent capability configuration

Enable Bootstrap and Index separately on a process:

```rust
let tools = AgentTools::open_initialized_home_node(data_dir)?
    .with_bootstrap_capability(true, origin_probe)
    .with_index_capability(true); // optional
```

Well-known metadata (`GET /.well-known/stumble-node`) advertises only enabled endpoints:

- Bootstrap: `bootstrap_announcements`, `bootstrap_announcement_stream`, `bootstrap_withdrawals`, `bootstrap_peer_advertisements`, `bootstrap_peer_advertisement_sample`
- Index: `index_search_announcements`
- Relay (only with `--relay`): `relay_publications`, `relay_pod_snapshot_template`
- Discovery Peer serving (opt-in on that node): `discovery_peer_announcement_stream`, `discovery_peer_advertisement_sample`

**Do not** advertise a disabled Relay, Hub, or retired discovery routes. A Bootstrap/Index-only process must not mention Relay.

## Topic-neutral Announcement Stream

`GET /bootstrap/announcements/stream?cursor=&limit=` is cursor-paginated and topic-neutral. Entries: `admitted`, `renewed`, `withdrawn`, `expired`. Empty cursor starts at the beginning; numeric cursors resume strictly after the last consumed sequence. Unknown or future cursors fail safely as `malformed`.

## Acceptance proof

Multi-node HTTP acceptance (real temporary SQLite, public contracts) lives at:

```bash
cargo test -p stumble-api --test sponsored_deployment_acceptance
```

That scenario covers publish → admit → cursor-sync → local match/explain/preview/subscribe, sponsor outage with peer delivery, renewal/expiry/withdrawal/rejections, Trust Policy over Index scores, browser Candidates vs Feed, capability independence, and outbound-only defaults.

## Related

- User-facing sponsored Bootstrap guide: [`docs/sponsored-bootstrap-users.md`](sponsored-bootstrap-users.md)
- Substrate detail: [`docs/discovery.md`](discovery.md)
- ADR-0037 (removable sponsored Bootstrap), ADR-0038 (open admission), ADR-0046 (minimize observation), ADR-0048 (abuse limits without identity gates)
