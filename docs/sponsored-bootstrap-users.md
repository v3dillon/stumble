# Using the sponsored Bootstrap (Users)

Stumble ships a **removable sponsored Bootstrap Node** so a fresh Home Node can discover public Pods without already knowing a Pod URL. The sponsor accelerates reachability. It is **not** authoritative for Pods, trust, quality, ranking, or your private Taste Profile.

## Sponsored default

A newly initialized Home Node receives one ordinary Bootstrap list entry:

| Field | Value |
|-------|--------|
| Label | `Sponsored Bootstrap` |
| Base URL | `https://bootstrap.stumble.network` (constant `DEFAULT_SPONSORED_BOOTSTRAP_URL`) |
| Enabled | `true` |
| Sponsored default | `true` |

You may disable or remove this entry at any time. Removing it does not delete announcements you already verified, Subscriptions, or synchronized content.

```bash
# Inspect configured Bootstraps and cursors
stumble sync bootstrap list
stumble sync bootstrap status

# Disable without deleting the row
stumble sync bootstrap disable <endpoint-id>

# Remove completely
stumble sync bootstrap remove <endpoint-id>
```


## Multiple replacement Bootstraps

Bootstrap configuration is an **ordered User-controlled list** from day one. Add independently operated entry points; outbound sync walks enabled endpoints in order and falls through on transport or protocol failure without discarding previously verified announcements.

```bash
stumble sync bootstrap add --label "community" --base-url https://bootstrap.example.org
stumble sync bootstrap enable <endpoint-id>
stumble sync bootstrap run
```

Each endpoint keeps its own Announcement Stream cursor, last success time, and typed failure.

## Outbound-only Home Node default

Ordinary Home Nodes make **outbound-only** discovery requests by default:

- No public discovery listener is bound or advertised on install
- Well-known node metadata omits Discovery Peer serving endpoints until you opt in
- Background discovery never sends Taste Profile data, Subscriptions, feedback, Source Affinities, or interest-derived queries to Bootstrap or Index Nodes

Automatic Discovery Peer **gossip** (learning peer samples and syncing their streams) is outbound configuration only; it does not expose your node.

## Serving opt-in (Discovery Peer)

If you want your node to help others after a Bootstrap outage, **explicitly** enable announcement serving:

```bash
stumble sync discovery serve enable --public-endpoint https://your-node.example
```

Enablement verifies identity, protocol, endpoint policy, and reachability, then issues a signed renewable Discovery Peer Advertisement (7-day lease). Disable with `stumble sync discovery serve disable` without affecting outbound Bootstrap, Index Explore, or direct Pod subscriptions.

Serving exposes only:

- `GET /discovery/peer/announcements/stream` — Origin-signed lifecycle entries unchanged
- `GET /discovery/peer/advertisements` — small unranked peer samples

Never Pod Events, private state, credentials, or administrative APIs.

## Direct-address fallback

Canonical Pod addressing remains the direct public Pod URL:

```text
https://origin.example/federation/pods/<slug>
```

Subscribe and synchronize through that URL with **no** Bootstrap or Index required:

```bash
# CLI / harness paths use the same Core contract as
# stumble_sync::subscribe_pod_from_url(...)
```

Loopback HTTP is allowed for local development; production Origins use HTTPS.

## Behavior during sponsor outages

| Situation | Behavior |
|-----------|----------|
| Established Home with viable Discovery Peers | Continues receiving new Origin-signed announcements through peer streams; previously verified catalog remains usable |
| Established Home, peers and Bootstraps all down | Cached announcements and Subscriptions remain local; new automatic discovery pauses until a Bootstrap or peer recovers |
| Fresh install, only sponsored Bootstrap configured and down | Discovery is **degraded** (`stumble sync discovery status`); direct Pod URLs and any additional configured Bootstraps still work |
| Index-only outage | Intentional Explore can still rank locally known announcements; remote Index search fails through without poisoning local Trust Policy |

Check readiness:

```bash
stumble sync discovery status
```

Degraded messages preserve direct Pod URL guidance. They never claim the sponsor is protocol authority.

## Relay-backed Pod URLs

Some public Pods share a Relay URL of the shape `https://<relay>/relay/pods/<origin-node-id>/<slug>` instead of a direct Origin URL. Subscribe to it the same way (`stumble pod subscribe <url>`). Your Home Node reads the Origin-signed snapshot from the Relay, pins the **Origin** identity and key from the snapshot, and verifies every event with the Origin public key. The Relay host never becomes the Origin and cannot alter the content. Explore samples at `{public_pod_url}/explore-samples` are Origin-signed; the Relay stores that artifact and returns it without change. A Subscription still uses the snapshot.

## What never leaves your Home Node

Background discovery and Bootstrap stream sync carry only public cursor pagination fields. Explicit Explore may send the words you typed to a configured Index Node—never your Taste Profile or Subscriptions. Remote Index scores are discarded; local Trust Policy and Pod Similarity recompute order. Browser-found Candidates stay in finite Discovery Result Batches until you act; they do not enter Feed exploration by themselves.

## Related

- Technical substrate: [`docs/discovery.md`](discovery.md)
- Operator runbook for running a Bootstrap/Index: [`docs/sponsored-bootstrap-operators.md`](sponsored-bootstrap-operators.md)
- First-release harness runbook: [`docs/first-release.md`](first-release.md)
