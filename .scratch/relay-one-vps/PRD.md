# Relay on the same VPS as Bootstrap and Index

Status: ready-for-agent

## Goal

One `stumble-api` process on one VPS can enable Bootstrap, Index, and Relay as independent flags. A Home Node with no public address can push Origin-signed Pod artifacts to the Relay. Friends subscribe to the Relay URL. The Relay never re-signs and never becomes the Origin.

## Non-goals

- Hosted Home Nodes (Taste Profile, User Context, snapshots, Owner credentials stay off the VPS)
- Changing Bootstrap or Index authority
- PostgreSQL hosted-mode adapter
- Making Relay mandatory

## Domain

Follow `CONTEXT.md`. Relay Node: caches and serves signed Pod Events for an Origin Node without authority to alter them. ADR-0031.

Write `docs/adr/0055-serve-signed-pod-events-from-an-optional-relay.md` (Accepted). Decision: Relay is a third independent process capability; Origin identity stays in the snapshot; public URL is `/relay/pods/<origin_node_id>/<slug>`.

## Capability flag (copy Bootstrap/Index)

1. `AgentTools` gets `relay: RelayCapability { enabled: bool }` (default false).
2. `with_relay_capability(self, enabled: bool) -> Self`
3. `relay_enabled(&self) -> bool`
4. `stumble-api` CLI: `--relay` / `STUMBLE_RELAY`, same wiring as `--index`.
5. Log `stumble-api serving the signed Pod Event Relay role` when on.

## Public URL and subscribe

Accept two canonical public Pod URL shapes in `canonical_public_pod_url` / `validate_public_pod_url` (`crates/stumble-core/src/pod_announcement.rs` and the helper wrapper):

- Origin: `/federation/pods/<slug>` (unchanged)
- Relay: `/relay/pods/<origin_node_id>/<slug>` where `origin_node_id` is the Origin Node UUID

`validate_public_pod_url(url, pod_slug)` accepts either shape. For the Relay shape the last segment must match `pod_slug`.

`fetch_pod_snapshot` in `crates/stumble-sync/src/lib.rs`:

- If the path is `/relay/pods/<origin>/<slug>`, GET that URL and deserialize `FederationPodSnapshot`. Use `snapshot.node` as Origin. Do **not** pin `well_known.node` (that is the Relay).
- Else keep today’s Origin fetch (`well-known` + `/manifest` + `/events`).

OriginProbe (`ReqwestOriginProbe`): if the URL is Relay-shaped, GET `{url}/manifest` (or the snapshot URL) and build `OriginPublicManifestView` from the Origin manifest + `snapshot.node`. A Relay-backed URL must be reachable without the private Origin being on the internet.

## HTTP surface (only when `relay_enabled`)

Register in `crates/stumble-api/src/lib.rs` and document in `docs.rs`:

| Method | Path | Behavior |
|---|---|---|
| POST | `/relay/pods/:origin_node_id/:slug` | Admit a `FederationPodSnapshot`. Verify Origin signatures. Store unchanged. |
| GET | `/relay/pods/:origin_node_id/:slug` | Return the stored snapshot (Origin `node` + manifest + events). |
| GET | `/relay/pods/:origin_node_id/:slug/manifest` | Return stored Origin manifest. |
| GET | `/relay/pods/:origin_node_id/:slug/events` | Return stored Origin events. |

When Relay is off, these routes return the same disabled pattern as Bootstrap (`relay_disabled`, 404).

Well-known (`well_known_node`) adds only when enabled:

- `relay_publications` = `{base}/relay/pods/{origin_node_id}/{slug}`
- `relay_pod_snapshot_template` = `{base}/relay/pods/{origin_node_id}/{slug}`

Never advertise Relay when the flag is off. Update tests that currently assert the word `relay` is absent: they must keep passing for Bootstrap/Index-only processes. Add a Relay-on case that **does** advertise the keys above.

`GET /federation/pods/:slug` stays Origin-local only (`federation_pod_manifest` already hides non-local origins). Do not serve relayed pods on that path.

## Admit rules

`AgentTools::admit_relay_snapshot(snapshot: FederationPodSnapshot) -> Result<...>`:

1. Fail if Relay is disabled (`relay_disabled`).
2. `snapshot.node.node_id` must match URL `origin_node_id`.
3. `snapshot.manifest.pod.slug` must match URL slug.
4. Reuse `validate_federation_snapshot` (or the same signature/chain checks subscribe uses). Invalid signature is rejected. Relay does not re-sign.
5. Upsert by `(origin_node_id, slug)`. Newer snapshot replaces older when the event chain is a valid extension or identical replay (idempotent).
6. Persist in SQLite via a new store collection `relay_publications`.

Store:

- Add `RelayPublication { origin_node_id, pod_slug, snapshot: FederationPodSnapshot, received_at }` in domain.
- Register in `store/registry.rs` (`#[serde(default)]`, key `origin_node_id` + `pod_slug`).
- Add `relay_publications: HashMap<(NodeIdentityId, String), RelayPublication>` on `InMemoryStore`.
- Update every `PersistedStore` / `InMemoryStore` conversion the compiler names. Follow `known_pod_announcements`.

Relay stores only the signed public snapshot. No Taste Profile, Subscriptions, Candidates, snapshots/assets, or Owner credentials.

## Origin push (Home Node)

CLI: extend `stumble pod publish`:

```
stumble pod publish <slug> --base-url https://relay.example --via-relay
```

`--via-relay` requires `--base-url`. Share URL becomes `{base}/relay/pods/{this_node_id}/{slug}`. After the Pod is public:

1. Build `FederationPodSnapshot` with `federation_pod_snapshot`.
2. POST it to `{base}/relay/pods/{origin_node_id}/{slug}`.
3. Issue the announcement with that Relay URL (`pod_announcement`).
4. Push the announcement to enabled Bootstraps as today.

Also add `stumble pod relay-push <slug> --relay-url https://relay.example` for later event updates (same POST). `stumble pod announce` should re-push to the Relay when the current announcement URL is Relay-shaped.

Add a small HTTP helper in `stumble-api` clients, same style as `submit_pod_announcement_to_bootstrap`.

## Deploy

`scripts/deploy-bootstrap-vps.sh`:

```
sudo ./scripts/deploy-bootstrap-vps.sh bootstrap.example.com
# flags: --no-index, --no-relay
```

Default ExecStart:

```
stumble-api --bootstrap --index --relay --bind 127.0.0.1:8787 --base-url https://$DOMAIN
```

Update unit description to Bootstrap/Index/Relay. Update `docs/deploy-bootstrap.md`, `docs/sponsored-bootstrap-operators.md` (Relay is now in scope when `--relay` is on; still independent; do not advertise if the flag is off), `docs/sponsored-bootstrap-users.md`, `README.md`, `docs/discovery.md` milestone proof sentence.

## Tests (required)

1. **Capability independence** (`capability_surfaces.rs`): bootstrap-only, index-only, relay-only, all three, none. Well-known includes Relay keys only when enabled. Existing “no relay” assertions stay on non-relay processes.

2. **Admit + subscribe** (new `crates/stumble-api/tests/relay_publication.rs` or extend `sponsored_deployment_acceptance`):
   - Origin node (no public listener after push).
   - Combined sponsor: `--bootstrap --index --relay`.
   - Origin publishes a public Pod, POSTs snapshot to Relay, announces with Relay URL.
   - Bootstrap admit succeeds because OriginProbe hits the Relay URL (not the private Origin).
   - Fresh Home Node `subscribe_pod_from_url` on the Relay URL.
   - Subscription `origin_node_id` is the Origin, **not** the Relay.
   - Events verify with the Origin public key.
   - Forged/re-signed snapshot is rejected.
   - Relay-disabled POST returns `relay_disabled`.

3. Update `canonical_public_pod_url` unit tests for the new Relay shape. Keep old Origin URLs passing.

4. Run:
   - `cargo test -p stumble-core --lib pod_announcement`
   - `cargo test -p stumble-api --test sponsored_deployment_acceptance`
   - `cargo test -p stumble-api --test relay_publication` (or whatever you name it)
   - `cargo test -p stumble-sync`
   - `cargo fmt --check` on touched files (or `cargo fmt`)

## Docs / issue tracker

- ADR-0055 as specified.
- Update operator/user/deploy/README/discovery docs.
- Keep this PRD. Add `.scratch/relay-one-vps/issues/01-relay-capability.md` with Status: ready-for-agent.

## Constraints

- Canonical terms from `CONTEXT.md`.
- Do not put Home Node private state on the Relay.
- Do not make Bootstrap/Index/Relay a single fused role.
- Do not commit unless asked. Do not open a PR. Do not push.
- Work only in this checkout.
- When done, send `worker_done` with outcome succeeded or failed and a 3-sentence summary plus files-modified.
