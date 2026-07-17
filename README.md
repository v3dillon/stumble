# Stumble

Stumble is a headless, decentralized personal discovery system. Agent Harnesses
use their own browser, search, APIs, credentials, and schedulers; Stumble owns
structured Candidate ingestion, curation, synchronization, and finite Feed
Batches.

HTTP, MCP, and `podctl` call the shared `AgentTools` service in `stumble-core`.
The first-release federation contract is `stumble/1.0`.

For a local MCP client, run the authenticated Streamable HTTP bridge against
the same Home Node directory:

```bash
cargo run -p stumble-mcp -- --data-dir ~/.stumble/nodes/home
```

The endpoint is `http://127.0.0.1:8790/mcp` by default and requires a current
Stumble Harness bearer token. Keep it on loopback; expose it to a remote client
only through authenticated HTTPS.

## Workspace

- `stumble-core`: domain, SQLite persistence, curation, Feed, signing, and tools.
- `stumble-api`: HTTP adapter and federation endpoints.
- `stumble-cli`: local `podctl` adapter.
- `stumble-mcp`: transport-neutral MCP tool dispatcher.
- `stumble-sync`: direct-address Subscription synchronization.

Stumble does not ship a crawler or dedicated source connector.

## Run a Home Node

```bash
cargo run -p stumble-cli -- --data-dir ~/.stumble/nodes/default init-node
cargo run -p stumble-api -- \
  --mode local \
  --bind 127.0.0.1:8787 \
  --data-dir ~/.stumble/nodes/default
```

`<data-dir>/stumble.sqlite3` is authoritative. On first boot only, a legacy
`store.json` is transactionally imported into an empty database and retained as
`store.json.migrated.bak`. Canonical Content Item IDs, Pod Placements, Feedback
Signals, Saves, briefs, and Pod Events are preserved; populated SQLite state is
never overwritten.

## Agent Harness workflow

Register each interactive or unattended harness with only the capabilities and
optional Pod scope it needs:

```bash
podctl --data-dir ~/.stumble/nodes/default register-harness \
  --label "Nightly discovery" \
  --kind unattended \
  --capability discovery-tasks \
  --capability candidate-submission \
  --pod-id <pod-uuid>
```

The plaintext bearer token is returned once. Grants, tokens, Taste Profiles,
Feedback Signals, Feed history, and Candidates remain Home-Node private.

The normal discovery flow is:

1. Read the Pod Package.
2. List and claim a due Discovery Task.
3. Discover externally using harness-owned capabilities.
4. Submit a structured, provenance-bearing Candidate.
5. Complete or fail the task.
6. Retrieve a stable finite Feed Batch and record explicit feedback.

```bash
podctl --token "$TOKEN" get-pod-package my-pod
podctl --token "$TOKEN" list-ready-discovery-tasks
podctl --token "$TOKEN" claim-discovery-task <task-id>
podctl --token "$TOKEN" submit-candidate --from candidate.json
podctl --token "$TOKEN" complete-discovery-task <task-id>
podctl --token "$TOKEN" feed --size 7
```

Pod Packages use `CONTEXT.md`, `SKILL.md`, `sources.yaml`, `filters.yaml`,
calibration examples, and signed history. Source Rules describe what to inspect,
seek, and schedule; they never contain executable connectors or credentials.

```bash
podctl create-pod-package --name "Rust Systems" --slug rust-systems --from ./rust-systems
podctl get-pod-package rust-systems
podctl validate-pod-package rust-systems
podctl export-pod-package rust-systems ./rust-systems-export
podctl import-pod-package rust-systems ./rust-systems-export
```

HTTP uses `POST /pod-packages` and `/pods/:slug/package` routes. MCP uses
`create_private_pod_with_package`, `get_pod_package`, `validate_pod_package`,
`export_pod_package`, and `import_pod_package`.

## Canonical adapter operations

First-release catalogs expose high-level operations including:

- `submit_candidate` and `inspect_candidate`
- Discovery Task materialization, claim, renew, complete, and fail
- `get_feed_batch`, `complete_feed_batch`, and `record_feed_feedback`
- Pod Package creation, validation, import, and export
- Harness registration/revocation and Pending Proposal approval
- signed Pod Event export and direct Subscription synchronization

HTTP exposes the equivalent `/candidates`, `/discovery-tasks`, `/feed`,
`/pod-packages`, `/harnesses`, and `/pending-proposals` resources. Adapter
contract tests verify stable IDs, provenance, allowed actions, and errors across
HTTP, MCP, and CLI.

## Retired pre-release contracts

Crawler/source-connector operations, direct link submissions, in-node discovery,
and brief generation are not first-release workflows. Hidden CLI compatibility
commands and legacy HTTP/MCP calls fail explicitly with HTTP `410 Gone` or the
equivalent non-zero adapter error:

```json
{
  "code": "legacy_contract_retired",
  "contract": "crawler_source_connector",
  "protocol_version": "stumble/1.0",
  "replacement": "discovery_tasks+submit_candidate"
}
```

Persisted legacy briefs remain readable migration data only. Agent Harnesses
present a Feed Batch in whatever conversational, voice, or visual format the
User prefers; Stumble does not create a separate brief-centered feed.

## Federation compatibility

Every Origin Node advertises `stumble/1.0` through
`/.well-known/stumble-node`. A Home Node verifies the advertised version before
projecting any signed event. Incompatible nodes fail negotiation, so an older
node cannot interpret first-release Content Item, Accepted Placement, Package,
or tombstone event shapes as its pre-release event contract.

See [synchronization](docs/synchronization.md) for direct-address behavior.

For a complete operator and Agent Harness setup, scheduling, recovery, and
reproducible two-node release proof, see the
[first-release runbook](docs/first-release.md).

## Validation

```bash
cargo fmt --check
cargo test --workspace
```
