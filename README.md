# Stumble

Rust MVP for an agent-native shared discovery system.

The repository is organized around one rule: HTTP, CLI, MCP, hosted mode, local mode, crawler, federation, and custom hub discovery all call the shared `AgentTools` service in `stumble-core`.

Stumble uses a custom `stumble/0.1` discovery protocol.

## Workspace

- `stumble-core`: domain model, in-memory MVP store, signing, ranking, skill packs, briefs, seed data, AgentTools.
- `stumble-api`: Axum HTTP JSON API, hosted/account endpoints, federation endpoints, route docs.
- `stumble-cli`: `podctl` CLI.
- `stumble-mcp`: MCP adapter boundary and JSON tool-call dispatcher.
- `stumble-crawler`: cautious RSS/Atom/webpage crawler boundary.
- `stumble-sync`: federation peer sync/import/export helpers.
- `migrations/postgres`: hosted PostgreSQL schema.
- `migrations/sqlite`: local SQLite schema.

## Setup

```bash
rustup default stable
cargo fmt
cargo test
cargo run -p stumble-cli -- --help
mkdir -p ~/.stumble/nodes/default
cargo run -p stumble-api -- --mode local --bind 127.0.0.1:8787 --data-dir ~/.stumble/nodes/default
```

Home Nodes use `<data-dir>/stumble.sqlite3` as their authoritative store (`.stumble` when `--data-dir` is omitted). On first boot, an existing `<data-dir>/store.json` is imported only when SQLite is empty and retained alongside `store.json.migrated.bak` for recovery; populated SQLite state is never replaced by the legacy snapshot.

Use `--port <number>` to choose a port without changing the bind host; use `--port 0` to let the operating system assign an available port. Dev-token minting endpoints are enabled on loopback binds and disabled on public binds unless `--allow-public-dev-tokens` is explicitly passed.

## Agent Harness grants

Register each interactive or unattended Agent Harness separately and grant only the capabilities it needs: `feed-read`, `feedback`, `discovery-tasks`, `candidate-submission`, `pod-curation`, `package-management`, `subscription-management`, and `administration`. Add one or more `--pod-id` values to restrict every Pod-facing operation; omit them for all local Pods.

```bash
podctl --data-dir ~/.stumble/nodes/default register-harness \
  --label "Nightly discovery" \
  --kind unattended \
  --capability discovery-tasks \
  --capability candidate-submission \
  --pod-id <pod-uuid>

podctl --data-dir ~/.stumble/nodes/default \
  --token '<one-time-token>' \
  submit --pod beautiful-interfaces --url https://example.com/item

podctl --data-dir ~/.stumble/nodes/default revoke-harness <harness-uuid>
```

The registration response is the only place the plaintext bearer token appears; the Home Node stores its hash. Revocation affects existing HTTP, MCP, and CLI contexts immediately. Harness identity is recorded with successful writes. Tokens, grants, write audits, and private User state remain node-local and are not included in federation Pod lists, manifests, events, or package exports.

Only a direct local owner context may bootstrap an interactive `administration` harness; unattended and delegated administrative grants are forbidden. Later authority expansion remains reserved for the Pending Proposal approval flow. A harness may otherwise delegate only an unattended subset of its own capabilities and Pod scope. On a public bind, every non-public API operation requires a bearer token; unauthenticated owner bootstrap is loopback-only.

Legacy dev tokens remain available for compatibility but are linked to a Harness identity and receive no implicit capabilities. Register a scoped Harness for agent work.

HTTP exposes `POST /harnesses` and `DELETE /harnesses/:id`. MCP exposes `register_agent_harness` and `revoke_agent_harness`; construct an authenticated router with the one-time token. All three adapters return the same core authorization reason, with HTTP mapping denials to `403 Forbidden` and CLI returning a non-zero exit status.

## Local Mode Examples

```bash
podctl init-node
podctl list-pods
podctl create-pod --name "Beautiful Interfaces" --slug beautiful-interfaces
podctl submit --pod beautiful-interfaces --url https://worrydream.com/MagicInk/ --title "Magic Ink" --note "Foundational UI thinking"
podctl discover --pod beautiful-interfaces --query "weird practical UI inspiration" --avoid politics --avoid "generic AI hype"
podctl brief --pod beautiful-interfaces
podctl export-skill-pack --pod beautiful-interfaces --out ./pods/beautiful-interfaces
```

## Portable Pod Packages

A portable Pod Package directory contains exactly `CONTEXT.md`, `SKILL.md`,
`sources.yaml`, `filters.yaml`, `examples.good.md`, `examples.bad.md`, and the
signed `events.jsonl` history.
`CONTEXT.md` defines subject scope and boundaries; `SKILL.md` contains scoped,
untrusted curation instructions. Source Rules are declarative `inspect`, `seek`,
and `schedule` suggestions and cannot contain connector commands or credentials.

```bash
podctl create-pod-package \
  --name "Rust Systems" \
  --slug rust-systems \
  --from ./pods/rust-systems
podctl get-skill-pack rust-systems
podctl validate-skill-pack rust-systems
podctl export-skill-pack rust-systems ./pods/rust-systems-exported
podctl import-skill-pack rust-systems ./pods/rust-systems-exported
```

Creation is atomic and always creates a private Pod. Accepted package versions
are immutable, owner/proposer-attributed, and recorded in signed Pod Events.
Imports ignore no extra files: unsupported files—including grants, permissions,
or credentials—are rejected, and package exports never include node-local
Harness Grants. HTTP uses `POST /pod-packages` plus the existing
`/pods/:slug/skill-pack` routes; MCP exposes
`create_private_pod_with_package` and the package read/validate/import/export
tools.

## Dillon Interest Agent

Run the local node, then run the HTTP agent that knows the interests `tech`, `ai`, and `aliens`. It creates or reuses the `dillon-tech-ai-aliens` pod, stores those interests as user preferences, runs discovery, and generates a private brief.

Agent submission policy: AI/harness agents do not submit links by default. They may submit a link only when the user provides/approves the URL, or when an explicit seed flag is used for demo data.

Pod skill policy: agent harnesses must read the target pod skill pack before submitting, discovering, or generating a brief. MCP tool responses include a `pod_skill_read` receipt, and `interest-agent` fetches `/pods/<slug>/skill-pack` before each submit/discover/brief action.

```bash
mkdir -p ~/.stumble/nodes/default
cargo run -p stumble-api -- --mode local --bind 127.0.0.1:8787 --data-dir ~/.stumble/nodes/default
cargo run -p stumble-agent -- --api http://127.0.0.1:8787 --label "Dillon Tech AI Aliens Agent"
cargo run -p stumble-agent -- --api http://127.0.0.1:8787 --label "Dillon Tech AI Aliens Agent" --keep-alive
cargo run -p stumble-agent -- --api http://127.0.0.1:8787 --label "Dillon Tech AI Aliens Agent" --seed-starter-links
cargo run -p stumble-agent -- --api http://127.0.0.1:8787 --pod-slug dillon-tech-ai-aliens --submit-link-url https://www.seti.org/ --submit-link-title "SETI Institute" --submit-link-tags aliens,seti,signals
```

## Hosted Mode Examples

```bash
podctl --data-dir ~/.stumble/nodes/hosted serve --mode hosted --bind 0.0.0.0:8787
podctl create-tenant --slug acme --name "Acme Research"
podctl create-api-token --user demo-user --tenant acme --label "phone agent"
podctl discover --api http://localhost:8787 --token st_dev_token --pod beautiful-interfaces --query "agent UI patterns"
```

## HTTP Examples

```bash
curl http://localhost:8787/health
curl http://localhost:8787/pods
curl -X POST http://localhost:8787/pods \
  -H 'content-type: application/json' \
  -d '{"name":"Beautiful Interfaces","slug":"beautiful-interfaces","description":"Thoughtful, strange, useful interface design.","visibility":"public"}'
curl -X POST http://localhost:8787/pods/beautiful-interfaces/discover \
  -H 'content-type: application/json' \
  -d '{"query":"weird practical UI inspiration","avoid":["politics","generic AI hype"],"limit":7,"mode":"deep_match"}'
curl -X POST http://localhost:8787/pods/design-stuff/intake-link \
  -H 'content-type: application/json' \
  -d '{"url":"https://example.com/article","note":"User-approved link intake.","tags":["design"]}'
curl -X POST http://localhost:8787/route-link \
  -H 'content-type: application/json' \
  -d '{"url":"https://x.com/example/status/1","tags":["uap","signals"]}'
curl -X POST http://localhost:8787/intake-link \
  -H 'content-type: application/json' \
  -d '{"url":"https://x.com/example/status/1","tags":["uap","signals"],"min_confidence":5}'
curl -X POST http://localhost:8787/briefs/generate \
  -H 'content-type: application/json' \
  -d '{"pod_slugs":["beautiful-interfaces"],"query":"UI inspiration"}'
```

`/pods/:slug/intake-link` fetches page metadata, creates a heuristic summary, extracts a representative Open Graph/Twitter image when present, submits the link, and stores the image in `submission_assets`. AI-generated images can be stored through the same asset model with source `ai_generated` when an agent harness generates or receives an approved image.

`/route-link` scores a fetched link against existing pod names, descriptions, and skill packs without storing it. `/intake-link` uses the same router and stores only when one pod clears the confidence threshold and is clearly ahead of the next candidate; otherwise it returns candidates plus a draft `suggested_new_pod` with `needs_confirmation: true`.

## Remote/Hosted HTTP Examples

```bash
curl -X POST https://pods.example.com/auth/dev-token \
  -H 'content-type: application/json' \
  -d '{"user_id":"demo-user","tenant_slug":"acme","label":"chatgpt"}'
curl https://pods.example.com/me -H 'authorization: Bearer st_dev_token'
curl -X POST https://pods.example.com/pods/beautiful-interfaces/submit \
  -H 'authorization: Bearer st_dev_token' \
  -H 'content-type: application/json' \
  -d '{"url":"https://example.com/demo","title":"Odd UI Demo","note":"Tactile interaction pattern"}'
```

## Federation Examples

```bash
curl http://localhost:8787/federation/node
curl http://localhost:8787/federation/pods
curl http://localhost:8787/federation/pods/beautiful-interfaces/manifest
curl http://localhost:8787/federation/pods/beautiful-interfaces/events
curl -X POST http://localhost:8787/federation/sync/peer_default_hosted
```

## Custom Discovery Hub Examples

The discovery hub indexes public node and pod metadata so a user's home node can find public pods that match explicit interests. It does not export private preferences, saved links, notes, reading history, or private briefs.

```bash
curl http://localhost:8787/.well-known/stumble-node
curl 'http://localhost:8787/discovery/pods?limit=10'
curl 'http://localhost:8787/discovery/pods?q=aliens%20signals&limit=5'
curl 'http://localhost:8787/home/discover-public-pods?topics=aliens,uap,signals&limit=5'
curl 'http://localhost:8787/hub/search-pods?q=aliens%20signals&limit=5'
curl -X POST http://localhost:8787/hub/refresh
```

`GET /discovery/pods` is the agent-facing discovery feed. It returns this node's local public pods separately from global public pods known through the hub index. With no `q`, it behaves like a feed; with `q`, it behaves like a ranked search.

`GET /hub/search-pods` automatically refreshes the local hub index with this node's public pods before searching. Private and invite-only pods are excluded from that index.

For the full federated discovery loop, remote nodes register or announce their reachable base URL, then hubs pull `/.well-known/stumble-node`, `/federation/pods`, public pod manifests, and public pod events to keep the global feed fresh. Only public pod metadata and public events should be indexed; private preferences, private pods, notes, saved links, reading history, and private briefs must stay local.

The API process runs a cancellable in-process hub refresh daemon every 24 hours by default. Configure it with `STUMBLE_HUB_REFRESH_INTERVAL_SECONDS` or disable it with `STUMBLE_DISABLE_HUB_REFRESH=true`. `POST /hub/refresh` runs the same pull immediately. Public event import and signature verification are offloaded to Tokio's blocking pool so CPU-heavy verification does not occupy async worker threads.

For real federation, bind the server on a reachable interface and advertise the public HTTPS base URL that other nodes should use:

```bash
STUMBLE_BASE_URL=https://pods.example.com \
cargo run -p stumble-api -- --bind 0.0.0.0:8787
```

Register another public node and public pod with this hub:

```bash
curl http://localhost:8787/federation/node
curl -X POST http://localhost:8787/hub/register-node \
  -H 'content-type: application/json' \
  -d '{"node_id":"<node_id_from_federation_node>","base_url":"http://localhost:8787","public_key":"<public_key_from_federation_node>","protocol_version":"stumble/0.1","display_name":"Local node"}'
curl -X POST http://localhost:8787/hub/register-pod \
  -H 'content-type: application/json' \
  -d '{"node_id":"<node_id_from_federation_node>","node_base_url":"http://localhost:8787","pod_slug":"public-uap-research","pod_name":"Public UAP Research","description":"Public pod about UAP and signals.","tags":["uap","signals"],"skill_pack_version":1,"latest_event_hash":null,"manifest_url":"http://localhost:8787/federation/pods/public-uap-research/manifest","events_url":"http://localhost:8787/federation/pods/public-uap-research/events"}'
```

`/.well-known/stumble-node` advertises the `stumble/0.1` protocol and the node's discovery endpoints.

## MCP Tool Examples

The MVP includes a clean MCP adapter boundary and tool dispatcher in `stumble-mcp`. It exposes the intended tool names and calls the same `AgentTools` implementation as HTTP/CLI:

```json
{"tool":"list_pods","arguments":{}}
{"tool":"discover_in_pod","arguments":{"pod_slug":"beautiful-interfaces","query":"weird practical UI inspiration","avoid":["politics","generic AI hype"],"limit":7}}
{"tool":"submit_link_to_pod","arguments":{"pod_slug":"beautiful-interfaces","url":"https://example.com","title":"Example","note":"Why it belongs"}}
{"tool":"get_pod_brief","arguments":{"pod_slugs":["beautiful-interfaces"],"query":"daily brief"}}
{"tool":"export_pod_events","arguments":{"pod_slug":"beautiful-interfaces"}}
```

MCP limitation: this MVP does not bind to a full third-party MCP transport crate yet. The `McpToolRouter` isolates the adapter and can be mounted into a concrete stdio/HTTP MCP server without duplicating business logic.
