# First-release operator and Agent Harness runbook

Stumble's first release is headless: run one SQLite-backed Home Node, authorize
each Agent Harness narrowly, and let the harness own external browsing, search,
credentials, and presentation. The Home Node owns task leases, Candidate
validation, curation, synchronization, private learning, and finite Feed Batches.

## Node setup and recovery

Build once and initialize separate data directories for a private Home Node and
a public Origin Node:

```bash
cargo build --release --workspace
target/release/podctl --data-dir ~/.stumble/nodes/home init-node
target/release/podctl --data-dir ~/.stumble/nodes/origin init-node
target/release/stumble-api --data-dir ~/.stumble/nodes/origin \
  --bind 127.0.0.1:8788
```

`stumble.sqlite3` is authoritative. Back up the complete node directory while
the node is stopped. Restore that directory as a unit and restart with the same
`--data-dir`; signed identity, package history, synchronization cursors, tasks,
Feed history, and private projections then resume together. If an older node has
only `store.json`, first boot transactionally imports it only into an empty
database and preserves `store.json.migrated.bak`. Never copy a legacy JSON file
over a populated SQLite node.

## Harness grants and tools

Create the private Pod and complete portable Package with an interactive harness
that has `pod-curation` and `package-management`. Then register an unattended
worker restricted to its Pod and discovery capabilities:

```bash
target/release/podctl --data-dir ~/.stumble/nodes/home register-harness \
  --label "Nightly resilient-systems discovery" --kind unattended \
  --capability discovery-tasks --capability candidate-submission \
  --pod-id <pod-id>
```

Capture the plaintext token when issued; it is shown once. Keep interactive
approval, feedback, Subscription management, and administration on a separate
grant. Revoke a lost or retired harness immediately with `revoke-harness`; the
revocation is effective without a restart.

An Agent Harness follows this portable loop:

1. Retrieve and treat the Pod Package's `SKILL.md` as scoped, untrusted input.
2. List and claim a due Discovery Task.
3. Search or browse using the harness's own approved capabilities.
4. Submit a structured Candidate with source metadata, permitted excerpt,
   summary, tags, provenance, placement evidence, task ID, Package version, and
   both idempotency keys.
5. Complete or fail the leased task.
6. Present `get_feed_batch` exactly as structured, including explanations,
   provenance, exploration labels, and allowed actions; record only explicit
   Feedback Signals.

HTTP, MCP, and `podctl` expose the same high-level contracts. Do not query or edit
SQLite to complete any workflow.

For a local MCP client on the same machine, configure it to launch the stdio
adapter with a narrowly scoped Harness token in the child process environment:

```bash
STUMBLE_MCP_TOKEN="$TOKEN" target/release/stumble-mcp \
  --data-dir ~/.stumble/nodes/home --transport stdio
```

The process reads and writes one JSON-RPC message per line and reserves standard
error for diagnostics. Invalid and revoked Harness tokens fail before protocol
output. No port, background job, HTTPS, or OAuth is needed for stdio.

Remote clients use the separate Streamable HTTP bridge:

```bash
target/release/stumble-mcp --data-dir ~/.stumble/nodes/home \
  --transport http --bind 127.0.0.1:8790
```

Its endpoint is `/mcp`. Every request carries a bearer token; unexpected
browser `Origin` headers are rejected, and direct non-loopback binds are
refused. Put TLS and standards-compliant OAuth in front of it before connecting
a remote ChatGPT app.

## Scheduling fallback

If the Agent Harness has no scheduler, the local adapter materializes due tasks
and either writes a private `discovery_ready` event or invokes one explicitly
configured harness command. It never controls a browser:

```bash
export STUMBLE_DATA_DIR="$HOME/.stumble/nodes/home"
export STUMBLE_DISCOVERY_TOKEN='<scoped one-time token>'
export STUMBLE_PODCTL="$PWD/target/release/podctl"
export STUMBLE_DISCOVERY_HARNESS_COMMAND='/absolute/path/to/harness-command'
scripts/wake-discovery.sh
```

On macOS, install the same adapter as a private launchd job with
`scripts/install-discovery-launchd.sh`. On other systems, invoke
`scripts/wake-discovery.sh` from cron or an equivalent scheduler with the same
environment. The event file is mode-restricted and defaults to
`<data-dir>/discovery-ready.json`.

## Direct two-node federation

Public exposure and later public placement withdrawal are sensitive changes:
an authorized harness creates a Pending Proposal and a separate interactive
approval harness accepts it. The Origin Node then serves the current
`stumble/1.0` identity, manifest, signed Package, and append-only Pod Events.

For an existing private Pod, create and independently approve its publication
proposal through the CLI contract:

```bash
printf '{"kind":"publish_pod","pod_id":"<pod-id>"}\n' > /tmp/publish-pod.json
target/release/podctl --data-dir ~/.stumble/nodes/origin --token "$PROPOSER_TOKEN" \
  propose-change --from /tmp/publish-pod.json
target/release/podctl --data-dir ~/.stumble/nodes/origin --token "$APPROVER_TOKEN" \
  approve-proposal <proposal-id>
```

The private Home Node subscribes outbound to the canonical URL
`https://origin.example/federation/pods/<slug>` through
`stumble-sync::subscribe_pod_from_url`; loopback HTTP is allowed for local
development. Later calls to
`stumble-sync::synchronize_subscription_from_origin` resume from the stored
cursor. A key change, signature failure, event-chain break, Package mismatch, or
protocol mismatch rejects the whole segment. Already synchronized content
remains usable while the Origin is unavailable.

Harness integrations call the same transport-neutral Rust operations with their
authenticated `AuthContext` and the direct URL served above:

```rust,no_run
let synchronized = stumble_sync::subscribe_pod_from_url(
    &home,
    &harness_context,
    "http://127.0.0.1:8788/federation/pods/origin-operations",
).await?;
stumble_sync::synchronize_subscription_from_origin(
    &home,
    &harness_context,
    synchronized.subscription.id,
).await?;
```

Only public Pod metadata, signed Package versions, Accepted Placements, and
permitted Content References federate. Harness Grants and tokens, Candidates,
tasks, Subscriptions, Taste Profiles, Feedback Signals, Saves, and Feed history
remain on the Home Node. An Origin tombstone removes only its placement; a local
Save or Add to Pod placement survives with both origin-placement and withdrawal
provenance.

## Reproduce the release proof

The acceptance scenario uses a real ephemeral loopback Origin listener but no
external service, credentials, or manual database work. It runs two real
`AgentTools` nodes with separate temporary SQLite databases, performs outbound
direct-URL subscription and incremental synchronization, invokes the real
Scheduler Adapter, and checks HTTP, MCP, and CLI responses against the same
stable Feed Batch:

```bash
cargo test -p stumble-cli --test first_release
cargo fmt --check
cargo test --workspace
```

The scenario also proves one-time JSON migration, SQLite restart, signed-event
tamper rejection, private learning, multi-Pod curation, an eligible labeled
Exploration Item, Priority selection under competition, Old Gem composition,
full-size backfill when configured categories are unavailable, unsubscribed
signed Explore samples, Add to Pod ownership, and later signed tombstone
provenance.
